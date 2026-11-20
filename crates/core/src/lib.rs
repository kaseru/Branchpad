//! Phần lõi của Branchpad, không phụ thuộc giao diện.

pub mod file_tree;
pub mod search;
pub mod settings;
pub mod text;
pub mod watcher;

pub use branchpad_git as git;
pub use file_tree::{Entry, FileTree, TreeRow};
pub use search::{fuzzy_filter, fuzzy_score};
pub use settings::Settings;
pub use text::{language_for, line_comment, line_marks, toggle_comment, LineMark, LineMarkKind};
pub use watcher::{WatchBatch, Watcher};

/// Thời gian tương đối dễ đọc ("3 phút trước").
pub fn relative_time(unix: i64, now: i64) -> String {
    let d = (now - unix).max(0);
    match d {
        0..=59 => "vừa xong".into(),
        60..=3599 => format!("{} phút trước", d / 60),
        3600..=86_399 => format!("{} giờ trước", d / 3600),
        86_400..=2_591_999 => format!("{} ngày trước", d / 86_400),
        2_592_000..=31_535_999 => format!("{} tháng trước", d / 2_592_000),
        _ => format!("{} năm trước", d / 31_536_000),
    }
}

/// Giờ Unix hiện tại (giây).
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
