//! Chạy CLI ở tiến trình con, đọc stdout theo dòng bằng một thread (không polling).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{parser_for, path_env, truncate, AgentEvent, CliKind};

/// Tham số một lần chạy.
#[derive(Clone, Debug)]
pub struct RunOptions {
    pub kind: CliKind,
    /// File chạy (từ [`crate::find_cli`]).
    pub binary: PathBuf,
    /// Thư mục làm việc (repo hoặc worktree).
    pub workdir: PathBuf,
    pub prompt: String,
    /// Phiên cũ để hỏi tiếp.
    pub resume: Option<String>,
    pub model: Option<String>,
    /// Mức suy luận (một giá trị trong [`CliKind::efforts`]); `None` = mặc định của CLI.
    pub effort: Option<String>,
    /// Cho Claude chạy lệnh shell không hỏi (Codex luôn chạy lệnh trong sandbox).
    pub allow_shell: bool,
    /// Claude: hỏi quyền trong chat khi cần (thay vì tự từ chối). Không áp cho Codex / agy.
    pub ask_permissions: bool,
    /// Ảnh đính kèm (đường dẫn tuyệt đối). Codex nhận qua `-i`; Claude / agy đọc theo đường dẫn ghi trong
    /// prompt, thư mục chứa ảnh được thêm bằng `--add-dir`.
    pub images: Vec<PathBuf>,
    /// Token đăng nhập Claude (`CLAUDE_CODE_OAUTH_TOKEN`) từ Keychain; `None` = dùng đăng nhập sẵn của CLI.
    pub oauth_token: Option<crate::Secret>,
}

/// Một lần chạy đang diễn ra. Drop là huỷ (đóng tab repo thì AI dừng theo).
pub struct AgentRun {
    pid: u32,
    done: Arc<AtomicBool>,
    kind: CliKind,
    /// stdin còn mở khi chờ trả lời xin quyền (Claude hỏi quyền trong chat); đóng khi hết lượt.
    stdin: Arc<Mutex<Option<ChildStdin>>>,
}

/// Giới hạn stderr giữ lại để báo lỗi.
const STDERR_KEEP: usize = 8 * 1024;
/// Chờ sau SIGTERM rồi mới SIGKILL.
const KILL_GRACE: Duration = Duration::from_secs(3);

impl AgentRun {
    /// Bắt đầu chạy. `on_event` được gọi trên một thread nền, lần cuối luôn là
    /// [`AgentEvent::Finished`] (tự tạo nếu CLI không gửi, ví dụ khi bị huỷ hoặc lỗi khởi động).
    pub fn start(
        opts: RunOptions,
        mut on_event: impl FnMut(AgentEvent) + Send + 'static,
    ) -> std::io::Result<AgentRun> {
        let mut cmd: Command = match opts.kind {
            CliKind::Claude => crate::claude::command(&opts),
            CliKind::Codex => crate::codex::command(&opts),
            CliKind::Agy => crate::agy::command(&opts),
        };
        // Claude / Codex đọc prompt từ stdin; agy nhận prompt qua tham số.
        let prompt_on_stdin = opts.kind != CliKind::Agy;
        // Claude hỏi quyền trong chat: giữ stdin mở để gửi câu trả lời, đóng khi hết lượt.
        let keep_stdin = opts.kind == CliKind::Claude && crate::claude::interactive(&opts);
        let payload = match opts.kind {
            CliKind::Claude => crate::claude::stdin_payload(&opts),
            _ => opts.prompt.clone(),
        };
        cmd.current_dir(&opts.workdir)
            .env("PATH", path_env(&opts.binary))
            .env("NO_COLOR", "1")
            .stdin(if prompt_on_stdin { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Nhóm tiến trình riêng để huỷ được cả các tiến trình con của CLI.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn()?;
        let pid = child.id();
        let done = Arc::new(AtomicBool::new(false));

        // Ghi prompt ở thread riêng để prompt dài không chặn luồng gọi; đóng stdin khi xong
        // (trừ khi còn phải gửi câu trả lời xin quyền).
        let shared_stdin: Arc<Mutex<Option<ChildStdin>>> = Arc::new(Mutex::new(None));
        if let Some(mut stdin) = child.stdin.take() {
            let shared = shared_stdin.clone();
            std::thread::spawn(move || {
                if let Err(e) = stdin.write_all(payload.as_bytes()).and_then(|()| stdin.flush()) {
                    eprintln!("branchpad-agent: cannot write prompt: {e}");
                }
                if keep_stdin {
                    if let Ok(mut slot) = shared.lock() {
                        *slot = Some(stdin);
                    }
                }
            });
        }
        let stderr = child.stderr.take().map(|mut err| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Err(e) = err.read_to_end(&mut buf) {
                    eprintln!("branchpad-agent: cannot read stderr: {e}");
                }
                let text = String::from_utf8_lossy(&buf).into_owned();
                // Giữ phần cuối: thông báo lỗi thường nằm ở cuối.
                let skip = text.chars().count().saturating_sub(STDERR_KEEP);
                text.chars().skip(skip).collect::<String>()
            })
        });
        let stdout = child.stdout.take();
        let done_flag = done.clone();
        let kind = opts.kind;
        let stdin_slot = shared_stdin.clone();
        let done_for_reader = done.clone();
        std::thread::spawn(move || {
            let mut parser = parser_for(kind);
            let mut finished = false;
            if let Some(out) = stdout {
                for line in BufReader::new(out).lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(e) => {
                            eprintln!("branchpad-agent: cannot read output: {e}");
                            break;
                        }
                    };
                    for ev in parser.parse_line(&line) {
                        if matches!(ev, AgentEvent::Finished { .. }) {
                            finished = true;
                            // agy có thể còn treo chờ tác vụ nền sau khi đã trả kết quả: dọn nhóm tiến trình.
                            if kind == CliKind::Agy {
                                stop_group(pid, done_for_reader.clone());
                            }
                            // Hết lượt: đóng stdin để CLI (chế độ nhận JSON) thoát.
                            if let Ok(mut slot) = stdin_slot.lock() {
                                slot.take();
                            }
                        }
                        on_event(ev);
                    }
                }
            }
            let status = child.wait();
            done_flag.store(true, Ordering::SeqCst);
            let err_text = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
            let success = status.as_ref().is_ok_and(|s| s.success());
            // Đã nhận kết quả mà tiến trình bị dọn (tín hiệu) thì không báo lỗi.
            if !success && !finished {
                let msg = err_text.trim();
                let msg = if msg.is_empty() {
                    match &status {
                        Ok(s) => format!("{} exited: {s}", kind.label()),
                        Err(e) => e.to_string(),
                    }
                } else {
                    truncate(msg, 2000)
                };
                on_event(AgentEvent::Error(msg));
            }
            if !finished {
                on_event(AgentEvent::Finished { ok: false, summary: String::new(), cost_usd: None, tokens: None });
            }
        });
        Ok(AgentRun { pid, done, kind: opts.kind, stdin: shared_stdin })
    }

    /// Trả lời một yêu cầu xin quyền ([`AgentEvent::PermissionRequest`]).
    pub fn respond_permission(&self, request_id: &str, allow: bool, input: &serde_json::Value) -> std::io::Result<()> {
        if self.kind != CliKind::Claude {
            return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "this CLI does not ask for permissions"));
        }
        let line = crate::claude::permission_response(request_id, allow, input);
        let mut slot = self.stdin.lock().map_err(|_| std::io::Error::other("stdin lock poisoned"))?;
        let stdin = slot
            .as_mut()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the CLI is no longer waiting for an answer"))?;
        stdin.write_all(line.as_bytes())?;
        stdin.flush()
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    /// Huỷ: SIGTERM cả nhóm tiến trình, sau [`KILL_GRACE`] còn sống thì SIGKILL.
    pub fn cancel(&self) {
        if self.is_done() {
            return;
        }
        stop_group(self.pid, self.done.clone());
    }
}

/// SIGTERM cả nhóm, sau [`KILL_GRACE`] còn sống thì SIGKILL.
fn stop_group(pid: u32, done: Arc<AtomicBool>) {
    signal_group(pid, false);
    std::thread::spawn(move || {
        std::thread::sleep(KILL_GRACE);
        if !done.load(Ordering::SeqCst) {
            signal_group(pid, true);
        }
    });
}

impl Drop for AgentRun {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(unix)]
fn signal_group(pid: u32, force: bool) {
    let sig = if force { "KILL" } else { "TERM" };
    let result = Command::new("/bin/kill")
        .args(["-s", sig, "--", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if let Err(e) = result {
        eprintln!("branchpad-agent: cannot stop process group {pid}: {e}");
    }
}

#[cfg(windows)]
fn signal_group(pid: u32, _force: bool) {
    let result = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if let Err(e) = result {
        eprintln!("branchpad-agent: cannot stop process {pid}: {e}");
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn fake_cli(dir: &std::path::Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("claude");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn opts(binary: PathBuf, dir: &std::path::Path) -> RunOptions {
        RunOptions {
            kind: CliKind::Claude,
            binary,
            workdir: dir.to_path_buf(),
            prompt: "hello".into(),
            resume: None,
            model: None,
            effort: None,
            allow_shell: false,
            ask_permissions: false,
            images: Vec::new(),
            oauth_token: None,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("branchpad-agent-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn streams_events_and_reads_prompt_from_stdin() {
        let dir = temp_dir("stream");
        // Echo the prompt back inside a result line.
        let bin = fake_cli(
            &dir,
            r#"read p; echo '{"type":"system","subtype":"init","session_id":"s1"}'; echo "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"$p\"}""#,
        );
        let (tx, rx) = mpsc::channel();
        let _run = AgentRun::start(opts(bin, &dir), move |ev| tx.send(ev).unwrap()).unwrap();
        let events: Vec<AgentEvent> = rx.iter().collect();
        assert_eq!(events[0], AgentEvent::Started { session_id: "s1".into() });
        assert!(matches!(&events[1], AgentEvent::Finished { ok: true, summary, .. } if summary == "hello"));
        assert_eq!(events.len(), 2);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn failing_cli_reports_stderr_and_finishes() {
        let dir = temp_dir("fail");
        let bin = fake_cli(&dir, "echo 'not logged in' >&2; exit 1");
        let (tx, rx) = mpsc::channel();
        let _run = AgentRun::start(opts(bin, &dir), move |ev| tx.send(ev).unwrap()).unwrap();
        let events: Vec<AgentEvent> = rx.iter().collect();
        assert!(matches!(&events[0], AgentEvent::Error(m) if m == "not logged in"));
        assert!(matches!(&events[1], AgentEvent::Finished { ok: false, .. }));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn cancel_stops_the_process() {
        let dir = temp_dir("cancel");
        let bin = fake_cli(&dir, "sleep 30");
        let (tx, rx) = mpsc::channel();
        let run = AgentRun::start(opts(bin, &dir), move |ev| tx.send(ev).unwrap()).unwrap();
        run.cancel();
        let events: Vec<AgentEvent> = rx.iter().collect();
        assert!(matches!(events.last(), Some(AgentEvent::Finished { ok: false, .. })));
        assert!(run.is_done());
        std::fs::remove_dir_all(dir).ok();
    }
}
