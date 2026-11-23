//! Lịch sử phiên AI của từng repo, lưu ở `<thư mục cài đặt>/Branchpad/agent-history/<mã repo>/<id>.json`
//! để mở lại và hỏi tiếp (resume) sau khi tắt app. Mỗi phiên một file, ghi file tạm rồi đổi tên.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Số phiên tối đa trả về trong danh sách lịch sử (mới nhất trước).
pub const HISTORY_LIMIT: usize = 200;
/// Số mục tối đa lưu cho một phiên (giữ phần cuối).
const ITEMS_KEEP: usize = 1000;

/// Một mục trong dòng hội thoại.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum HistoryItem {
    User { text: String },
    Text { text: String },
    Thinking { text: String },
    /// `kind`: read / edit / command / search / web / other. `ok`: `None` = chưa xong khi lưu.
    Tool { kind: String, name: String, detail: String, ok: Option<bool>, summary: String },
    /// Đường dẫn tương đối với thư mục chạy.
    File { path: String },
    Warning { text: String },
    Error { text: String },
    /// Mốc hết lượt (xong / lỗi / dừng, kèm token, chi phí).
    TurnEnd { ok: bool, text: String },
}

/// Một phiên đã lưu.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionRecord {
    pub id: String,
    /// "claude" / "codex" / "agy".
    pub cli: String,
    pub title: String,
    /// Mã phiên của CLI để hỏi tiếp.
    pub cli_session: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// Thư mục CLI đã chạy (repo hoặc worktree).
    pub run_dir: PathBuf,
    pub created: i64,
    pub updated: i64,
    pub items: Vec<HistoryItem>,
}

impl Default for SessionRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            cli: String::new(),
            title: String::new(),
            cli_session: None,
            model: None,
            effort: None,
            run_dir: PathBuf::new(),
            created: 0,
            updated: 0,
            items: Vec::new(),
        }
    }
}

/// Dòng trong danh sách lịch sử (không kèm nội dung hội thoại).
#[derive(Clone, Debug, PartialEq)]
pub struct SessionSummary {
    pub id: String,
    pub cli: String,
    pub title: String,
    pub updated: i64,
    pub messages: usize,
    pub can_resume: bool,
}

/// FNV-1a 64 bit: mã thư mục ổn định cho đường dẫn repo (không phụ thuộc phiên bản Rust).
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// Thư mục lịch sử của một repo: `<tên repo>-<mã>` cho dễ nhận ra.
pub fn history_dir(base: &Path, repo: &Path) -> PathBuf {
    let name: String = repo
        .file_name()
        .map(|n| n.to_string_lossy().chars().filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c)).collect())
        .unwrap_or_default();
    base.join(format!("{name}-{:016x}", fnv1a(&repo.to_string_lossy())))
}

/// Thư mục gốc chứa lịch sử của mọi repo.
pub fn default_base() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("Branchpad").join("agent-history"))
}

/// Tên file hợp lệ cho id (chặn `..`, `/`).
fn file_for(dir: &Path, id: &str) -> io::Result<PathBuf> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("invalid session id {id:?}")));
    }
    Ok(dir.join(format!("{id}.json")))
}

/// Lưu (ghi đè) một phiên.
pub fn save_session(dir: &Path, record: &SessionRecord) -> io::Result<()> {
    let path = file_for(dir, &record.id)?;
    std::fs::create_dir_all(dir)?;
    let mut record = record.clone();
    if record.items.len() > ITEMS_KEEP {
        record.items.drain(..record.items.len() - ITEMS_KEEP);
    }
    let json = serde_json::to_vec(&record).map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(tmp, path)
}

pub fn load_session(dir: &Path, id: &str) -> io::Result<SessionRecord> {
    let bytes = std::fs::read(file_for(dir, id)?)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Xoá một phiên (chưa có file cũng không lỗi).
pub fn delete_session(dir: &Path, id: &str) -> io::Result<()> {
    match std::fs::remove_file(file_for(dir, id)?) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Danh sách phiên, mới nhất trước, tối đa [`HISTORY_LIMIT`]. File hỏng thì bỏ qua (ghi log).
pub fn list_sessions(dir: &Path) -> io::Result<Vec<SessionSummary>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    // Chỉ đọc các file sửa gần nhất.
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| (std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.truncate(HISTORY_LIMIT);
    let mut out: Vec<SessionSummary> = files
        .into_iter()
        .filter_map(|(_, path)| {
            let parsed = std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|b| serde_json::from_slice::<SessionRecord>(&b).map_err(|e| e.to_string()));
            match parsed {
                Ok(r) => Some(SessionSummary {
                    messages: r.items.iter().filter(|i| matches!(i, HistoryItem::User { .. })).count(),
                    can_resume: r.cli_session.is_some(),
                    id: r.id,
                    cli: r.cli,
                    title: r.title,
                    updated: r.updated,
                }),
                Err(e) => {
                    eprintln!("branchpad: skipping AI history file {}: {e}", path.display());
                    None
                }
            }
        })
        .collect();
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("branchpad-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn record(id: &str, updated: i64) -> SessionRecord {
        SessionRecord {
            id: id.into(),
            cli: "claude".into(),
            title: format!("task {id}"),
            cli_session: Some("s1".into()),
            updated,
            items: vec![
                HistoryItem::User { text: "fix it".into() },
                HistoryItem::Tool { kind: "edit".into(), name: "Edit".into(), detail: "a.rs".into(), ok: Some(true), summary: String::new() },
                HistoryItem::File { path: "a.rs".into() },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn save_list_load_delete() {
        let dir = temp("roundtrip");
        save_session(&dir, &record("a1", 10)).unwrap();
        save_session(&dir, &record("b2", 20)).unwrap();
        let list = list_sessions(&dir).unwrap();
        assert_eq!(list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["b2", "a1"]);
        assert_eq!(list[0].messages, 1);
        assert!(list[0].can_resume);
        assert_eq!(load_session(&dir, "a1").unwrap(), record("a1", 10));
        delete_session(&dir, "a1").unwrap();
        delete_session(&dir, "a1").unwrap();
        assert_eq!(list_sessions(&dir).unwrap().len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rejects_bad_ids_and_missing_dir() {
        let dir = temp("bad");
        assert!(load_session(&dir, "../x").is_err());
        assert!(list_sessions(&dir).unwrap().is_empty());
    }

    #[test]
    fn dir_is_stable_and_readable() {
        let a = history_dir(Path::new("/h"), Path::new("/src/My App"));
        assert_eq!(a, history_dir(Path::new("/h"), Path::new("/src/My App")));
        assert!(a.file_name().unwrap().to_string_lossy().starts_with("MyApp-"));
        assert_ne!(a, history_dir(Path::new("/h"), Path::new("/other/My App")));
    }
}
