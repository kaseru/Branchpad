//! Phần lõi của Branchpad, không phụ thuộc giao diện.

pub mod file_tree;
pub mod repo_scan;
pub mod search;
pub mod settings;
pub mod text;
pub mod watcher;

pub use branchpad_git as git;
pub use file_tree::{Entry, FileTree, TreeRow};
pub use search::{fuzzy_filter, fuzzy_score};
pub use settings::Settings;
pub use text::{compare_lines, CompareKind, CompareLine, delete_line, duplicate_line, minimal_edit, move_line, language_for, line_comment, line_marks, toggle_comment, LineMark, LineMarkKind};
pub use watcher::{WatchBatch, Watcher};

/// Unit of a relative time (see [`relative_time_parts`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelativeUnit {
    JustNow,
    Minutes,
    Hours,
    Days,
    Months,
    Years,
}

/// Relative time as (unit, count); count is 0 for `JustNow`. UI code turns it into text.
pub fn relative_time_parts(unix: i64, now: i64) -> (RelativeUnit, i64) {
    let d = (now - unix).max(0);
    match d {
        0..=59 => (RelativeUnit::JustNow, 0),
        60..=3599 => (RelativeUnit::Minutes, d / 60),
        3600..=86_399 => (RelativeUnit::Hours, d / 3600),
        86_400..=2_591_999 => (RelativeUnit::Days, d / 86_400),
        2_592_000..=31_535_999 => (RelativeUnit::Months, d / 2_592_000),
        _ => (RelativeUnit::Years, d / 31_536_000),
    }
}

/// Giờ Unix hiện tại (giây).
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
