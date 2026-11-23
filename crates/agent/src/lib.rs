//! Giao việc cho Claude Code CLI / Codex CLI ở chế độ không tương tác.
//!
//! CLI chạy trong thư mục repo, ghi sự kiện JSON theo từng dòng ra stdout; mỗi dòng
//! được đổi sang [`AgentEvent`] chung cho cả hai CLI. Không phụ thuộc giao diện.
//! CLI dùng tài khoản người dùng đã đăng nhập sẵn (`claude login`, `codex login`).

mod binary;
mod claude;
mod codex;
mod event;
mod run;
mod secret;

pub use binary::{cli_version, find_cli, path_env};
pub use claude::ClaudeParser;
pub use codex::CodexParser;
pub use event::{AgentEvent, ToolKind};
pub use run::{AgentRun, RunOptions};
pub use secret::{delete_claude_token, load_claude_token, save_claude_token, Secret, CLAUDE_TOKEN_ENV};

/// CLI được hỗ trợ.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CliKind {
    #[default]
    Claude,
    Codex,
}

impl CliKind {
    pub const ALL: [CliKind; 2] = [CliKind::Claude, CliKind::Codex];

    /// Tên lưu trong cài đặt.
    pub fn id(self) -> &'static str {
        match self {
            CliKind::Claude => "claude",
            CliKind::Codex => "codex",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    /// Tên hiển thị.
    pub fn label(self) -> &'static str {
        match self {
            CliKind::Claude => "Claude Code",
            CliKind::Codex => "Codex",
        }
    }

    /// Tên file chạy.
    pub fn binary_name(self) -> &'static str {
        self.id()
    }

    /// Biến môi trường ép đường dẫn CLI.
    pub fn env_var(self) -> &'static str {
        match self {
            CliKind::Claude => "BRANCHPAD_CLAUDE",
            CliKind::Codex => "BRANCHPAD_CODEX",
        }
    }

    /// Lệnh cài, hiện khi chưa tìm thấy CLI.
    pub fn install_hint(self) -> &'static str {
        match self {
            CliKind::Claude => "curl -fsSL https://claude.ai/install.sh | bash && claude login",
            CliKind::Codex => "brew install codex && codex login",
        }
    }
}

/// Bộ đọc output của một CLI: nhận từng dòng stdout, trả về các sự kiện.
/// Dòng hỏng hoặc loại sự kiện lạ thì trả về rỗng (CLI đổi phiên bản không làm hỏng app).
pub trait LineParser: Send {
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent>;
}

/// Bộ đọc phù hợp với `kind`.
pub fn parser_for(kind: CliKind) -> Box<dyn LineParser> {
    match kind {
        CliKind::Claude => Box::new(ClaudeParser::default()),
        CliKind::Codex => Box::new(CodexParser::default()),
    }
}

/// Dòng đầu tiên không rỗng, cắt còn tối đa `max` ký tự (tóm tắt kết quả công cụ).
pub(crate) fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    truncate(line, max)
}

/// Dòng cuối không rỗng (output lệnh: dòng cuối thường là kết luận).
pub(crate) fn last_line(text: &str, max: usize) -> String {
    let line = text.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("");
    truncate(line, max)
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_ids_round_trip() {
        for k in CliKind::ALL {
            assert_eq!(CliKind::from_id(k.id()), Some(k));
        }
        assert_eq!(CliKind::from_id("gemini"), None);
    }

    #[test]
    fn lines() {
        assert_eq!(first_line("\n  a \nb", 10), "a");
        assert_eq!(last_line("a\nb\n\n", 10), "b");
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
    }
}
