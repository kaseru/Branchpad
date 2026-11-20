//! Stash và worktree.

use std::path::PathBuf;

use crate::diff::{parse_diff, FileDiff};
use crate::log::{parse_name_status, ChangedFile};
use crate::runner::{Repo, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashEntry {
    pub index: usize,
    /// `stash@{0}`
    pub name: String,
    pub oid: String,
    pub time: i64,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    /// Tên nhánh ngắn; `None` khi detached.
    pub branch: Option<String>,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
    /// Worktree chính (đầu tiên trong danh sách).
    pub is_main: bool,
}

pub fn parse_stash_list(text: &str) -> Vec<StashEntry> {
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let f: Vec<&str> = line.splitn(4, '\0').collect();
            (f.len() == 4).then(|| StashEntry {
                index: i,
                name: f[0].to_string(),
                oid: f[1].to_string(),
                time: f[2].parse().unwrap_or(0),
                message: f[3].to_string(),
            })
        })
        .collect()
}

pub fn parse_worktrees(text: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let mut wt = Worktree {
            path: PathBuf::new(),
            head: None,
            branch: None,
            bare: false,
            locked: false,
            prunable: false,
            is_main: out.is_empty(),
        };
        let mut has_path = false;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                wt.path = PathBuf::from(p);
                has_path = true;
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                wt.head = Some(h.to_string());
            } else if let Some(b) = line.strip_prefix("branch ") {
                wt.branch = Some(b.trim_start_matches("refs/heads/").to_string());
            } else if line == "bare" {
                wt.bare = true;
            } else if line.starts_with("locked") {
                wt.locked = true;
            } else if line.starts_with("prunable") {
                wt.prunable = true;
            }
        }
        if has_path {
            out.push(wt);
        }
    }
    out
}

impl Repo {
    pub fn stashes(&self) -> Result<Vec<StashEntry>> {
        let text = self.run(&["stash", "list", "--format=%gd%x00%H%x00%at%x00%gs"])?;
        Ok(parse_stash_list(&text))
    }

    /// Cất thay đổi (kể cả file chưa theo dõi).
    pub fn stash_push(&self, message: &str) -> Result<()> {
        let mut args = vec!["stash", "push", "--include-untracked"];
        if !message.trim().is_empty() {
            args.extend(["-m", message]);
        }
        self.run(&args).map(drop)
    }

    pub fn stash_apply(&self, name: &str) -> Result<()> {
        self.run(&["stash", "apply", "--index", name]).map(drop)
    }

    pub fn stash_pop(&self, name: &str) -> Result<()> {
        self.run(&["stash", "pop", "--index", name]).map(drop)
    }

    pub fn stash_drop(&self, name: &str) -> Result<()> {
        self.run(&["stash", "drop", name]).map(drop)
    }

    pub fn stash_files(&self, name: &str) -> Result<Vec<ChangedFile>> {
        let out = self.run_bytes(
            &["diff", "--name-status", "-z", "-M", &format!("{name}^1"), name],
            &[0],
        )?;
        Ok(parse_name_status(&String::from_utf8_lossy(&out)))
    }

    pub fn stash_diff(&self, name: &str, path: &str) -> Result<Vec<FileDiff>> {
        let out = self.run_bytes(
            &["diff", "--no-ext-diff", "--no-color", "-M", &format!("{name}^1"), name, "--", path],
            &[0],
        )?;
        Ok(parse_diff(&String::from_utf8_lossy(&out)))
    }

    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        Ok(parse_worktrees(&self.run(&["worktree", "list", "--porcelain"])?))
    }

    /// Thêm worktree. `new_branch` = tạo nhánh mới từ `commitish`.
    pub fn add_worktree(
        &self,
        path: &str,
        commitish: &str,
        new_branch: Option<&str>,
    ) -> Result<()> {
        let mut args = vec!["worktree", "add"];
        if let Some(b) = new_branch {
            args.extend(["-b", b]);
        }
        args.extend([path, commitish]);
        self.run(&args).map(drop)
    }

    pub fn remove_worktree(&self, path: &str, force: bool) -> Result<()> {
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(path);
        self.run(&args).map(drop)
    }

    pub fn prune_worktrees(&self) -> Result<()> {
        self.run(&["worktree", "prune"]).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stash() {
        let s = parse_stash_list("stash@{0}\0abc\x00100\0On main: wip\nstash@{1}\0def\x0090\0WIP on x\n");
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].index, 1);
        assert_eq!(s[0].message, "On main: wip");
    }

    #[test]
    fn parses_worktrees() {
        let w = parse_worktrees(
            "worktree /a\nHEAD 111\nbranch refs/heads/main\n\nworktree /b\nHEAD 222\ndetached\nlocked\n",
        );
        assert_eq!(w.len(), 2);
        assert!(w[0].is_main);
        assert_eq!(w[0].branch.as_deref(), Some("main"));
        assert!(w[1].branch.is_none() && w[1].locked && !w[1].is_main);
    }
}
