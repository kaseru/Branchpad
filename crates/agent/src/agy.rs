//! Antigravity CLI: `agy -p <prompt> --output-format stream-json`.
//!
//! Mỗi dòng là một object có trường `event`: `init` (có `conversation_id`), `step_update`
//! (`step_update.step_type` = `tool` với `state` ACTIVE / DONE, hoặc `agent_response` với `text_delta`),
//! `result` (`result.status` SUCCESS / ERROR, `response`, `error`, `usage.total_tokens`).
//! Định dạng lấy theo runner Antigravity của DevDesk (đã chạy thật với agy 1.5.x).

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use crate::{first_line, last_line, AgentEvent, LineParser, RunOptions, ToolKind};

/// Lệnh chạy agy. Prompt đi qua tham số `-p` (agy không đọc prompt từ stdin ở chế độ text).
pub(crate) fn command(opts: &RunOptions) -> Command {
    let mut cmd = Command::new(&opts.binary);
    cmd.arg("-p").arg(&opts.prompt);
    cmd.arg("--add-dir").arg(&opts.workdir);
    cmd.args(["--output-format", "stream-json"]);
    // Không cho chạy lệnh tự do: chế độ sửa file được duyệt sẵn. Cho phép thì bỏ qua mọi hỏi duyệt.
    if opts.allow_shell {
        cmd.arg("--dangerously-skip-permissions");
    } else {
        cmd.args(["--mode", "accept-edits"]);
    }
    if let Some(m) = opts.model.as_deref().filter(|m| !m.trim().is_empty()) {
        cmd.args(["--model", m.trim()]);
    }
    if let Some(id) = &opts.resume {
        cmd.args(["--conversation", id]);
    }
    cmd
}

fn tool_kind(name: &str) -> ToolKind {
    match name {
        "view_file" | "read_file" | "list_dir" | "view_file_outline" | "view_code_item" => ToolKind::Read,
        "write_to_file" | "replace_file_content" | "multi_replace_file_content" => ToolKind::Edit,
        "run_command" | "send_command_input" | "command_status" => ToolKind::Command,
        "find_by_name" | "grep_search" | "codebase_search" => ToolKind::Search,
        "search_web" | "read_url_content" | "open_browser_url" => ToolKind::Web,
        _ => ToolKind::Other,
    }
}

/// Tham số đáng hiện của công cụ: lệnh, file, mẫu tìm, URL...
fn tool_detail(params: &Value) -> String {
    let s = |k: &str| params.get(k).and_then(Value::as_str);
    let detail = s("CommandLine")
        .or_else(|| s("TargetFile"))
        .or_else(|| s("AbsolutePath"))
        .or_else(|| s("DirectoryPath"))
        .or_else(|| s("SearchPath"))
        .or_else(|| s("Query"))
        .or_else(|| s("Pattern"))
        .or_else(|| s("Url"))
        .or_else(|| s("query"))
        .unwrap_or("");
    first_line(detail, 200)
}

#[derive(Default)]
pub struct AgyParser {
    /// Đoạn trả lời đang nhận dần (text_delta), báo một lần khi sang bước khác.
    text: String,
    /// Đã báo đoạn trả lời nào chưa (để biết có cần dùng `result.response` không).
    sent_text: bool,
    /// Công cụ đang chạy: (id, tên công cụ, file sẽ sửa).
    running: Vec<(String, String, Option<PathBuf>)>,
    next_id: u64,
}

impl AgyParser {
    fn flush_text(&mut self, out: &mut Vec<AgentEvent>) {
        let text = std::mem::take(&mut self.text);
        if !text.trim().is_empty() {
            self.sent_text = true;
            out.push(AgentEvent::Text(text.trim().to_string()));
        }
    }

    fn step(&mut self, step: &Value, out: &mut Vec<AgentEvent>) {
        let s = |k: &str| step.get(k).and_then(Value::as_str).unwrap_or_default();
        match s("step_type") {
            "agent_response" => {
                self.text.push_str(s("text_delta"));
                if s("state") == "DONE" {
                    self.flush_text(out);
                }
            }
            "tool" => {
                self.flush_text(out);
                let name = s("tool_name").to_string();
                let params = step.pointer("/tool_info/parameters").cloned().unwrap_or(Value::Null);
                match s("state") {
                    "ACTIVE" => {
                        // Một công cụ có thể báo ACTIVE nhiều lần khi đang chạy: chỉ tính lần đầu.
                        if self.running.iter().any(|(_, n, _)| *n == name) {
                            return;
                        }
                        let id = format!("agy-{}", self.next_id);
                        self.next_id += 1;
                        let kind = tool_kind(&name);
                        let file = (kind == ToolKind::Edit)
                            .then(|| params.get("TargetFile").and_then(Value::as_str).map(PathBuf::from))
                            .flatten();
                        self.running.push((id.clone(), name.clone(), file));
                        out.push(AgentEvent::ToolStart { id, kind, name, detail: tool_detail(&params) });
                    }
                    state @ ("DONE" | "ERROR" | "CANCELED" | "FAILED") => {
                        let Some(pos) = self.running.iter().rposition(|(_, n, _)| *n == name) else { return };
                        let (id, _, file) = self.running.remove(pos);
                        let ok = state == "DONE";
                        // Lý do lỗi nếu agy có gửi; không có thì ghi trạng thái (CANCELED thường là bị chặn quyền).
                        let text = |path: &str| step.pointer(path).and_then(Value::as_str).filter(|t| !t.trim().is_empty());
                        let reason = text("/tool_info/error")
                            .or_else(|| text("/error"))
                            .or_else(|| text("/error_message"))
                            .or_else(|| text("/tool_info/output"));
                        let summary = match (ok, reason) {
                            (_, Some(r)) => last_line(r, 200),
                            (true, None) => String::new(),
                            (false, None) => state.to_string(),
                        };
                        out.push(AgentEvent::ToolDone { id, ok, summary });
                        if let (true, Some(f)) = (ok, file) {
                            out.push(AgentEvent::FileChanged(f));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

impl LineParser for AgyParser {
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match v.get("event").and_then(Value::as_str) {
            Some("init") => {
                if let Some(id) = v.get("conversation_id").and_then(Value::as_str) {
                    out.push(AgentEvent::Started { session_id: id.to_string() });
                }
            }
            Some("step_update") => {
                if let Some(step) = v.get("step_update") {
                    self.step(step, &mut out);
                }
            }
            Some("result") => {
                self.flush_text(&mut out);
                let res = v.get("result").cloned().unwrap_or(Value::Null);
                let s = |k: &str| res.get(k).and_then(Value::as_str).unwrap_or_default().trim().to_string();
                let response = s("response");
                if !self.sent_text && !response.is_empty() {
                    out.push(AgentEvent::Text(response.clone()));
                }
                let error = s("error");
                // agy có thể giữ lỗi của một lượt cũ trong conversation và báo lại dù lượt này đã trả lời xong.
                let failed = (s("status") == "ERROR" || !error.is_empty()) && response.is_empty() && !self.sent_text;
                let tokens = res.pointer("/usage/total_tokens").and_then(Value::as_u64);
                out.push(AgentEvent::Finished {
                    ok: !failed,
                    summary: if failed { if error.is_empty() { "ERROR".into() } else { error } } else { response },
                    cost_usd: None,
                    tokens,
                });
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(text: &str) -> Vec<AgentEvent> {
        let mut p = AgyParser::default();
        text.lines().flat_map(|l| p.parse_line(l)).collect()
    }

    #[test]
    fn edit_session() {
        let ev = parse_all(include_str!("../tests/fixtures/agy_edit.jsonl"));
        assert_eq!(ev[0], AgentEvent::Started { session_id: "c0ffee00-0000-4000-8000-000000000001".into() });
        assert!(matches!(&ev[1], AgentEvent::Text(t) if t == "I'll fix the typo."));
        assert!(matches!(&ev[2], AgentEvent::ToolStart { kind: ToolKind::Read, detail, .. } if detail == "/tmp/demo/README.md"));
        assert!(matches!(&ev[3], AgentEvent::ToolDone { ok: true, .. }));
        assert!(matches!(&ev[4], AgentEvent::ToolStart { kind: ToolKind::Edit, .. }));
        assert!(matches!(&ev[5], AgentEvent::ToolDone { ok: true, .. }));
        assert_eq!(ev[6], AgentEvent::FileChanged(PathBuf::from("/tmp/demo/README.md")));
        assert!(matches!(&ev[7], AgentEvent::ToolStart { kind: ToolKind::Command, detail, .. } if detail == "cargo test"));
        assert!(matches!(&ev[8], AgentEvent::ToolDone { ok: true, .. }));
        assert!(matches!(&ev[9], AgentEvent::Text(t) if t == "Fixed the typo in README.md."));
        assert!(matches!(&ev[10], AgentEvent::Finished { ok: true, tokens: Some(5120), .. }));
        assert_eq!(ev.len(), 11);
    }

    #[test]
    fn error_result() {
        let ev = parse_all(include_str!("../tests/fixtures/agy_error.jsonl"));
        assert!(matches!(&ev[1], AgentEvent::Finished { ok: false, summary, .. } if summary.contains("quota")));
    }

    #[test]
    fn prompt_is_an_argument() {
        let opts = RunOptions {
            kind: crate::CliKind::Agy,
            binary: "/bin/agy".into(),
            workdir: "/tmp".into(),
            prompt: "hi".into(),
            resume: Some("abc".into()),
            model: None,
            effort: None,
            allow_shell: false,
            ask_permissions: false,
            oauth_token: None,
        };
        let args: Vec<String> = command(&opts).get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(&args[..2], ["-p", "hi"]);
        assert!(args.windows(2).any(|w| w == ["--conversation", "abc"]));
        assert!(args.windows(2).any(|w| w == ["--mode", "accept-edits"]));
    }
}
