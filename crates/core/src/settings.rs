//! Cài đặt lưu giữa các lần mở app (`~/Library/Application Support/Branchpad`).

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Font chữ đều mặc định theo hệ điều hành.
pub const DEFAULT_MONO_FONT: &str = if cfg!(target_os = "windows") { "Consolas" } else { "Menlo" };

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Repo đang mở (theo thứ tự tab).
    pub repos: Vec<PathBuf>,
    /// Tab đang chọn.
    pub active: usize,
    /// Giao diện đang chọn: "system", "modern-dark", "one-dark", "nord", "tokyo-night", "modern-light", "warm-paper".
    pub theme: String,
    /// Cỡ chữ giao diện UI (px). Mặc định 15.0 (lớn hơn bản cũ 14.0).
    pub ui_font_size: f32,
    /// Cỡ chữ trình soạn thảo code (px).
    pub editor_font_size: f32,
    /// Chiều cao dòng danh sách (px). Mặc định 26.0 (thoáng hơn bản cũ 22.0).
    pub row_height: f32,
    /// Phông chữ monospace cho diff / blame / đồ thị.
    pub mono_font: String,
    /// Tự lưu file sau khi ngừng gõ (mặc định tắt).
    pub auto_save: bool,
    /// CLI AI mặc định: "claude", "codex", "agy", hoặc rỗng = cái nào tìm thấy trước.
    pub agent_cli: String,
    /// Ép đường dẫn CLI (rỗng = tự tìm).
    pub claude_path: Option<PathBuf>,
    pub codex_path: Option<PathBuf>,
    pub agy_path: Option<PathBuf>,
    /// Model cho từng CLI (rỗng = mặc định của CLI).
    pub claude_model: Option<String>,
    pub codex_model: Option<String>,
    pub agy_model: Option<String>,
    /// Phiên AI mới chạy trong worktree mới (mặc định tắt: AI sửa thẳng thư mục đang làm).
    /// Tên mới (bản cũ là `agent_worktree`, mặc định bật) để file cài đặt cũ không giữ giá trị bật.
    pub agent_use_worktree: bool,
    /// Cho Claude chạy lệnh shell không hỏi (mặc định tắt).
    pub agent_allow_shell: bool,
    /// Bề rộng panel AI cạnh trình soạn (px), nhớ giữa các lần mở app.
    pub agent_panel_width: f32,
    /// Repo đang chọn theo đường dẫn (chỉ số `active` lệch khi có repo không mở được).
    pub active_repo: Option<PathBuf>,
    /// Trạng thái từng repo để mở lại đúng như lúc tắt app.
    pub repo_state: BTreeMap<PathBuf, RepoState>,
}

/// File đang mở và phiên AI đang mở của một repo.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoState {
    pub open_files: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
    pub show_agent: bool,
    /// Mã phiên AI trong lịch sử (theo thứ tự tab).
    pub agent_sessions: Vec<String>,
    pub agent_active: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            repos: Vec::new(),
            active: 0,
            theme: "system".to_string(),
            ui_font_size: 15.0,
            editor_font_size: 14.0,
            row_height: 26.0,
            mono_font: DEFAULT_MONO_FONT.to_string(),
            auto_save: false,
            agent_cli: String::new(),
            claude_path: None,
            codex_path: None,
            agy_path: None,
            claude_model: None,
            codex_model: None,
            agy_model: None,
            agent_use_worktree: false,
            agent_allow_shell: false,
            agent_panel_width: 360.0,
            active_repo: None,
            repo_state: BTreeMap::new(),
        }
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("Branchpad").join("settings.json"))
    }

    /// Đọc cài đặt; file hỏng hoặc chưa có thì dùng mặc định.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                eprintln!("branchpad: ignoring corrupt settings {}: {e}", path.display());
                Self::default()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                eprintln!("branchpad: cannot read {}: {e}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::path()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no config directory"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        // Ghi file tạm rồi đổi tên để không bao giờ để lại file hỏng.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, path)
    }

    /// Thêm repo (nếu chưa có) và chọn nó.
    pub fn open_repo(&mut self, path: PathBuf) {
        match self.repos.iter().position(|p| *p == path) {
            Some(i) => self.active = i,
            None => {
                self.repos.push(path);
                self.active = self.repos.len() - 1;
            }
        }
    }

    pub fn close_repo(&mut self, index: usize) {
        if index < self.repos.len() {
            let path = self.repos.remove(index);
            self.repo_state.remove(&path);
        }
        self.active = self.active.min(self.repos.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_close() {
        let mut s = Settings::default();
        s.open_repo("/a".into());
        s.open_repo("/b".into());
        s.open_repo("/a".into());
        assert_eq!(s.repos.len(), 2);
        assert_eq!(s.active, 0);
        s.close_repo(1);
        s.close_repo(0);
        assert_eq!(s.active, 0);
        assert!(s.repos.is_empty());
    }

    #[test]
    fn default_values() {
        let s = Settings::default();
        assert_eq!(s.theme, "system");
        assert_eq!(s.ui_font_size, 15.0);
        assert_eq!(s.editor_font_size, 14.0);
        assert_eq!(s.row_height, 26.0);
        assert_eq!(s.mono_font, DEFAULT_MONO_FONT);
        assert!(!s.agent_use_worktree);
        assert!(!s.agent_allow_shell);
        assert_eq!(s.agent_panel_width, 360.0);
    }

    #[test]
    fn old_file_without_agent_fields_still_loads() {
        let old = r#"{"repos":["/a"],"active":0,"theme":"nord","ui_font_size":15.0,"editor_font_size":14.0,"row_height":26.0,"mono_font":"Menlo","auto_save":true}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.theme, "nord");
        assert!(s.auto_save);
        assert_eq!(s.agent_cli, "");
        assert_eq!(s.claude_path, None);
        assert!(!s.agent_use_worktree);
    }
}
