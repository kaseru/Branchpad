//! Cài đặt lưu giữa các lần mở app (`~/Library/Application Support/Branchpad`).

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Repo đang mở (theo thứ tự tab).
    pub repos: Vec<PathBuf>,
    /// Tab đang chọn.
    pub active: usize,
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
                eprintln!("branchpad: bỏ qua settings hỏng {}: {e}", path.display());
                Self::default()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                eprintln!("branchpad: không đọc được {}: {e}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::path()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "không có thư mục cấu hình"))?;
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
            self.repos.remove(index);
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
}
