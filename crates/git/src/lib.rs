//! Lớp git của Branchpad. Gọi lệnh `git` có sẵn trên máy (dùng lại cấu hình,
//! SSH key, keychain của người dùng) và đọc kết quả dạng máy đọc được.
//! Không phụ thuộc thư viện ngoài.

mod blame;
mod conflict;
mod diff;
mod graph;
mod log;
mod ops;
mod rebase;
mod refs;
mod runner;
mod stash;
mod status;

pub use blame::{parse_blame, parse_grep, BlameLine, GrepMatch};
pub use conflict::{
    conflict_count, parse_conflicts, render_markers, resolve, ConflictBlock, ConflictPart,
    Resolution,
};
pub use diff::{parse_diff, DiffLine, FileDiff, Hunk, LineKind, WorkingSide};
pub use graph::{Anchor, GraphBuilder, GraphLine, GraphRow};
pub use log::{
    parse_log, parse_name_status, ChangedFile, Commit, CommitDetail, LogFilter, LogQuery,
};
pub use ops::{is_mailbox_patch, PullMode, RepoState, ResetMode, SequenceStep};
pub use rebase::{build_todo, TodoAction, TodoItem};
pub use refs::{GitRef, RefKind, Remote};
pub use runner::{git_binary, GitError, Repo, Result};
pub use stash::{StashEntry, Worktree};
pub use status::{parse_status, EntryKind, Status, StatusEntry, StatusIndex};
