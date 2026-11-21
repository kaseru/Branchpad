//! Chạy lệnh `git` có sẵn trên máy.
//!
//! Mọi lệnh đều chạy không tương tác: không mở trình soạn, không hỏi mật khẩu
//! (SSH dùng key/keychain/ssh-agent sẵn có, chạy ở chế độ `BatchMode`).

use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

#[derive(Debug)]
pub enum GitError {
    /// Không chạy được tiến trình git (chưa cài git, sai PATH...).
    Spawn(std::io::Error),
    /// git chạy xong nhưng trả mã lỗi.
    Failed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },
    /// Kết quả git không đúng định dạng mong đợi.
    Parse(String),
    /// Lỗi đọc/ghi file.
    Io(std::io::Error),
    /// Thao tác không hợp lệ (kiểm tra trước khi gọi git).
    Invalid(String),
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitError::Spawn(e) => write!(f, "Cannot run git: {e}"),
            GitError::Failed {
                command,
                code,
                stderr,
            } => {
                let msg = stderr.trim();
                if msg.is_empty() {
                    let code = code.map_or_else(|| "?".to_string(), |c| c.to_string());
                    write!(f, "`git {command}` failed (exit code {code})")
                } else {
                    write!(f, "{msg}")
                }
            }
            GitError::Parse(m) => write!(f, "Cannot parse git output: {m}"),
            GitError::Io(e) => write!(f, "{e}"),
            GitError::Invalid(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for GitError {}

impl From<std::io::Error> for GitError {
    fn from(e: std::io::Error) -> Self {
        GitError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, GitError>;

/// Đường dẫn tới git. App mở từ Finder có PATH rất ngắn nên ưu tiên git của
/// Homebrew nếu có, rồi tới `/usr/bin/git` (Xcode). Có thể ép bằng `BRANCHPAD_GIT`.
pub fn git_binary() -> &'static Path {
    static GIT: OnceLock<PathBuf> = OnceLock::new();
    GIT.get_or_init(|| {
        if let Some(p) = std::env::var_os("BRANCHPAD_GIT") {
            return PathBuf::from(p);
        }
        let mut candidates: Vec<PathBuf> = Vec::new();
        if cfg!(windows) {
            // Git for Windows cài cho cả máy hoặc riêng người dùng.
            for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
                if let Some(dir) = std::env::var_os(var) {
                    candidates.push(Path::new(&dir).join("Git").join("cmd").join("git.exe"));
                }
            }
            if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
                candidates.push(Path::new(&dir).join("Programs").join("Git").join("cmd").join("git.exe"));
            }
        } else {
            for c in ["/opt/homebrew/bin/git", "/usr/local/bin/git", "/usr/bin/git"] {
                candidates.push(PathBuf::from(c));
            }
        }
        if let Some(found) = candidates.into_iter().find(|p| p.exists()) {
            return found;
        }
        PathBuf::from("git")
    })
}

fn base_command(dir: &Path) -> Command {
    let mut cmd = Command::new(git_binary());
    cmd.current_dir(dir)
        .args([
            "-c",
            "core.quotepath=false",
            "-c",
            "color.ui=false",
            "-c",
            "core.pager=cat",
        ])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .env("GIT_PAGER", "cat")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .stdin(Stdio::null());
    cmd
}

/// Một repo git trên máy.
#[derive(Clone, Debug)]
pub struct Repo {
    workdir: PathBuf,
    git_dir: PathBuf,
    ssh_command: Option<String>,
}

impl Repo {
    /// Mở repo chứa `path` (có thể là thư mục con).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let out = base_command(path)
            .args(["rev-parse", "--show-toplevel", "--absolute-git-dir"])
            .output()
            .map_err(GitError::Spawn)?;
        if !out.status.success() {
            return Err(GitError::Failed {
                command: "rev-parse".into(),
                code: out.status.code(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            });
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut lines = text.lines();
        let workdir = lines
            .next()
            .filter(|l| !l.is_empty())
            .ok_or_else(|| GitError::Parse("missing working directory".into()))?;
        let git_dir = lines
            .next()
            .filter(|l| !l.is_empty())
            .ok_or_else(|| GitError::Parse("missing .git directory".into()))?;
        let mut repo = Repo {
            workdir: PathBuf::from(workdir),
            git_dir: PathBuf::from(git_dir),
            ssh_command: None,
        };
        repo.ssh_command = repo.batch_ssh_command();
        Ok(repo)
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// Tên hiển thị (tên thư mục).
    pub fn name(&self) -> String {
        self.workdir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.workdir.to_string_lossy().into_owned())
    }

    /// Nếu người dùng chưa tự đặt lệnh ssh thì dùng `BatchMode` để ssh không
    /// bao giờ hỏi mật khẩu (key trong ssh-agent/keychain vẫn dùng được).
    fn batch_ssh_command(&self) -> Option<String> {
        if std::env::var_os("GIT_SSH_COMMAND").is_some() || std::env::var_os("GIT_SSH").is_some()
        {
            return None;
        }
        let configured = self
            .run_codes(&["config", "--get", "core.sshCommand"], &[0, 1])
            .unwrap_or_default();
        if configured.trim().is_empty() {
            Some("ssh -o BatchMode=yes".into())
        } else {
            None
        }
    }

    fn command(&self, args: &[&str], envs: &[(&str, &str)]) -> Command {
        let mut cmd = base_command(&self.workdir);
        cmd.args(args);
        if let Some(ssh) = &self.ssh_command {
            cmd.env("GIT_SSH_COMMAND", ssh);
        }
        for (k, v) in envs {
            cmd.env(k, v);
        }
        cmd
    }

    fn exec(
        &self,
        args: &[&str],
        envs: &[(&str, &str)],
        input: Option<&[u8]>,
        ok_codes: &[i32],
    ) -> Result<Vec<u8>> {
        let mut cmd = self.command(args, envs);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        if input.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn().map_err(GitError::Spawn)?;
        if let Some(input) = input {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(input)?;
            }
        }
        let out = child.wait_with_output().map_err(GitError::Spawn)?;
        let code = out.status.code();
        if code.is_some_and(|c| ok_codes.contains(&c)) {
            Ok(out.stdout)
        } else {
            Err(GitError::Failed {
                command: args.first().copied().unwrap_or_default().to_string(),
                code,
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            })
        }
    }

    /// Chạy git, lỗi nếu mã thoát khác 0.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        self.run_codes(args, &[0])
    }

    /// Chạy git, chấp nhận các mã thoát trong `ok_codes`.
    pub fn run_codes(&self, args: &[&str], ok_codes: &[i32]) -> Result<String> {
        let out = self.exec(args, &[], None, ok_codes)?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// Chạy git, trả về byte gốc.
    pub fn run_bytes(&self, args: &[&str], ok_codes: &[i32]) -> Result<Vec<u8>> {
        self.exec(args, &[], None, ok_codes)
    }

    /// Chạy git với dữ liệu đưa vào stdin.
    pub fn run_input(&self, args: &[&str], input: &[u8]) -> Result<String> {
        let out = self.exec(args, &[], Some(input), &[0])?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// Chạy git với biến môi trường thêm.
    pub fn run_env(&self, args: &[&str], envs: &[(&str, &str)]) -> Result<String> {
        let out = self.exec(args, envs, None, &[0])?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// Thư mục riêng của Branchpad trong `.git` (file tạm cho rebase...).
    pub fn scratch_dir(&self) -> Result<PathBuf> {
        let dir = self.git_dir.join("branchpad");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

/// Bọc chuỗi trong nháy đơn cho shell (`sh -c`).
pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote() {
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }
}
