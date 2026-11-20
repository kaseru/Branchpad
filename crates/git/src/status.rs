//! `git status --porcelain=v2`: file thay đổi, nhánh hiện tại, số commit trước/sau.

use crate::runner::{Repo, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Tracked,
    Renamed,
    Unmerged,
    Untracked,
    Ignored,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusEntry {
    /// Đường dẫn tương đối so với gốc repo.
    pub path: String,
    pub orig_path: Option<String>,
    /// Trạng thái trong index (`.` = không đổi).
    pub index: char,
    /// Trạng thái trong thư mục làm việc (`.` = không đổi).
    pub worktree: char,
    pub kind: EntryKind,
}

impl StatusEntry {
    pub fn is_staged(&self) -> bool {
        matches!(self.kind, EntryKind::Tracked | EntryKind::Renamed) && self.index != '.'
    }

    pub fn is_unstaged(&self) -> bool {
        matches!(self.kind, EntryKind::Tracked | EntryKind::Renamed) && self.worktree != '.'
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// Nhánh hiện tại; `None` khi detached HEAD.
    pub branch: Option<String>,
    /// Commit HEAD; `None` khi repo chưa có commit.
    pub head_oid: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub entries: Vec<StatusEntry>,
}

impl Status {
    pub fn staged(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries.iter().filter(|e| e.is_staged())
    }

    pub fn unstaged(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries.iter().filter(|e| e.is_unstaged())
    }

    pub fn untracked(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == EntryKind::Untracked)
    }

    pub fn conflicted(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries.iter().filter(|e| e.kind == EntryKind::Unmerged)
    }

    /// Ký hiệu thay đổi của một file (để tô màu cây file):
    /// `U` xung đột, `?` file mới chưa theo dõi, còn lại là ký hiệu git (M, A, D, R...).
    pub fn mark_of(&self, path: &str) -> Option<char> {
        let e = self.entries.iter().find(|e| e.path == path)?;
        Some(match e.kind {
            EntryKind::Unmerged => 'U',
            EntryKind::Untracked => '?',
            EntryKind::Ignored => return None,
            _ if e.worktree != '.' => e.worktree,
            _ => e.index,
        })
    }

    /// Thư mục `dir` (tương đối, có `/` ở cuối hoặc rỗng = gốc) có chứa file đổi không.
    pub fn dir_has_changes(&self, dir: &str) -> bool {
        self.entries
            .iter()
            .any(|e| e.kind != EntryKind::Ignored && e.path.starts_with(dir))
    }
}

pub fn parse_status(text: &str) -> Status {
    let mut st = Status::default();
    let mut fields = text.split('\0');
    while let Some(rec) = fields.next() {
        if rec.is_empty() {
            continue;
        }
        if let Some(h) = rec.strip_prefix("# ") {
            if let Some(v) = h.strip_prefix("branch.oid ") {
                if v != "(initial)" {
                    st.head_oid = Some(v.to_string());
                }
            } else if let Some(v) = h.strip_prefix("branch.head ") {
                if v != "(detached)" {
                    st.branch = Some(v.to_string());
                }
            } else if let Some(v) = h.strip_prefix("branch.upstream ") {
                st.upstream = Some(v.to_string());
            } else if let Some(v) = h.strip_prefix("branch.ab ") {
                for part in v.split(' ') {
                    if let Some(n) = part.strip_prefix('+') {
                        st.ahead = n.parse().unwrap_or(0);
                    } else if let Some(n) = part.strip_prefix('-') {
                        st.behind = n.parse().unwrap_or(0);
                    }
                }
            }
            continue;
        }
        let xy = |s: &str| {
            let mut c = s.chars();
            (c.next().unwrap_or('.'), c.next().unwrap_or('.'))
        };
        match rec.as_bytes()[0] {
            b'1' => {
                let parts: Vec<&str> = rec.splitn(9, ' ').collect();
                if parts.len() == 9 {
                    let (index, worktree) = xy(parts[1]);
                    st.entries.push(StatusEntry {
                        path: parts[8].to_string(),
                        orig_path: None,
                        index,
                        worktree,
                        kind: EntryKind::Tracked,
                    });
                }
            }
            b'2' => {
                let parts: Vec<&str> = rec.splitn(10, ' ').collect();
                let orig = fields.next().map(str::to_string);
                if parts.len() == 10 {
                    let (index, worktree) = xy(parts[1]);
                    st.entries.push(StatusEntry {
                        path: parts[9].to_string(),
                        orig_path: orig,
                        index,
                        worktree,
                        kind: EntryKind::Renamed,
                    });
                }
            }
            b'u' => {
                let parts: Vec<&str> = rec.splitn(11, ' ').collect();
                if parts.len() == 11 {
                    let (index, worktree) = xy(parts[1]);
                    st.entries.push(StatusEntry {
                        path: parts[10].to_string(),
                        orig_path: None,
                        index,
                        worktree,
                        kind: EntryKind::Unmerged,
                    });
                }
            }
            b'?' | b'!' if rec.len() > 2 => {
                let kind = if rec.starts_with('?') {
                    EntryKind::Untracked
                } else {
                    EntryKind::Ignored
                };
                st.entries.push(StatusEntry {
                    path: rec[2..].to_string(),
                    orig_path: None,
                    index: '.',
                    worktree: '.',
                    kind,
                });
            }
            _ => {}
        }
    }
    st
}

impl Repo {
    pub fn status(&self) -> Result<Status> {
        let out = self.run_bytes(
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=all",
            ],
            &[0],
        )?;
        Ok(parse_status(&String::from_utf8_lossy(&out)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2() {
        let text = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 h1 h2 src/a b.rs\0\
2 R. N... 100644 100644 100644 h1 h2 R100 new.rs\0old.rs\0\
u UU N... 100644 100644 100644 100644 h1 h2 h3 conflict.rs\0\
? new file.txt\0";
        let st = parse_status(text);
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!((st.ahead, st.behind), (2, 1));
        assert_eq!(st.entries.len(), 4);
        assert_eq!(st.entries[0].path, "src/a b.rs");
        assert!(st.entries[0].is_unstaged() && !st.entries[0].is_staged());
        assert_eq!(st.entries[1].orig_path.as_deref(), Some("old.rs"));
        assert!(st.entries[1].is_staged());
        assert_eq!(st.conflicted().count(), 1);
        assert_eq!(st.untracked().next().unwrap().path, "new file.txt");
        assert_eq!(st.mark_of("conflict.rs"), Some('U'));
    }
}
