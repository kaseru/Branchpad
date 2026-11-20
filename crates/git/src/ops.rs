//! Commit, đồng bộ remote, merge/rebase/cherry-pick/revert/reset, patch,
//! và trạng thái thao tác đang dở.

use std::path::{Path, PathBuf};

use crate::runner::{shell_quote, GitError, Repo, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoState {
    Clean,
    Merging,
    Rebasing { interactive: bool },
    /// `git am` đang áp patch dạng mail.
    ApplyingMailbox,
    CherryPicking,
    Reverting,
}

impl RepoState {
    pub fn label(self) -> &'static str {
        match self {
            RepoState::Clean => "",
            RepoState::Merging => "Đang merge",
            RepoState::Rebasing { .. } => "Đang rebase",
            RepoState::ApplyingMailbox => "Đang áp patch",
            RepoState::CherryPicking => "Đang cherry-pick",
            RepoState::Reverting => "Đang revert",
        }
    }

    /// Có hỗ trợ "bỏ qua" commit hiện tại không.
    pub fn can_skip(self) -> bool {
        !matches!(self, RepoState::Clean | RepoState::Merging)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullMode {
    Merge,
    Rebase,
    FastForwardOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequenceStep {
    Continue,
    Skip,
    Abort,
}

impl Repo {
    pub fn state(&self) -> RepoState {
        let g = self.git_dir();
        if g.join("rebase-merge").is_dir() {
            RepoState::Rebasing {
                interactive: g.join("rebase-merge/interactive").exists(),
            }
        } else if g.join("rebase-apply").is_dir() {
            if g.join("rebase-apply/applying").exists() {
                RepoState::ApplyingMailbox
            } else {
                RepoState::Rebasing { interactive: false }
            }
        } else if g.join("MERGE_HEAD").exists() {
            RepoState::Merging
        } else if g.join("CHERRY_PICK_HEAD").exists() {
            RepoState::CherryPicking
        } else if g.join("REVERT_HEAD").exists() {
            RepoState::Reverting
        } else {
            RepoState::Clean
        }
    }

    /// Tiếp tục / bỏ qua / huỷ thao tác đang dở.
    pub fn sequence(&self, step: SequenceStep) -> Result<()> {
        let flag = match step {
            SequenceStep::Continue => "--continue",
            SequenceStep::Skip => "--skip",
            SequenceStep::Abort => "--abort",
        };
        let cmd = match self.state() {
            RepoState::Clean => return Err(GitError::Invalid("Không có thao tác nào đang dở".into())),
            RepoState::Merging => {
                return match step {
                    SequenceStep::Continue => self.run(&["commit", "--no-edit"]).map(drop),
                    SequenceStep::Abort => self.run(&["merge", "--abort"]).map(drop),
                    SequenceStep::Skip => Err(GitError::Invalid("Merge không bỏ qua được".into())),
                }
            }
            RepoState::Rebasing { .. } => "rebase",
            RepoState::ApplyingMailbox => "am",
            RepoState::CherryPicking => "cherry-pick",
            RepoState::Reverting => "revert",
        };
        self.run(&[cmd, flag]).map(drop)
    }

    // ---- Commit ----

    pub fn commit(&self, message: &str, amend: bool) -> Result<()> {
        if message.trim().is_empty() {
            return Err(GitError::Invalid("Chưa nhập nội dung commit".into()));
        }
        let mut args = vec!["commit", "-F", "-"];
        if amend {
            args.push("--amend");
        }
        self.run_input(&args, message.as_bytes()).map(drop)
    }

    // ---- Remote ----

    pub fn fetch_all(&self) -> Result<()> {
        self.run(&["fetch", "--all", "--prune", "--tags"]).map(drop)
    }

    pub fn fetch_remote(&self, remote: &str) -> Result<()> {
        self.run(&["fetch", "--prune", "--tags", remote]).map(drop)
    }

    pub fn pull(&self, mode: PullMode) -> Result<()> {
        let flag = match mode {
            PullMode::Merge => "--no-rebase",
            PullMode::Rebase => "--rebase",
            PullMode::FastForwardOnly => "--ff-only",
        };
        self.run(&["pull", flag]).map(drop)
    }

    /// Push nhánh local `local` lên `remote_branch` của `remote`.
    /// `set_upstream`: đặt nhánh remote làm upstream (lần push đầu).
    pub fn push(
        &self,
        remote: &str,
        local: &str,
        remote_branch: &str,
        set_upstream: bool,
        force: bool,
    ) -> Result<()> {
        let refspec = format!("refs/heads/{local}:refs/heads/{remote_branch}");
        let mut args = vec!["push"];
        if force {
            args.push("--force-with-lease");
        }
        if set_upstream {
            args.push("-u");
        }
        args.extend([remote, refspec.as_str()]);
        self.run(&args).map(drop)
    }

    // ---- Merge / rebase / cherry-pick / revert / reset ----

    pub fn merge(&self, rev: &str, no_ff: bool) -> Result<()> {
        let mut args = vec!["merge", "--no-edit"];
        if no_ff {
            args.push("--no-ff");
        }
        args.push(rev);
        self.run(&args).map(drop)
    }

    pub fn rebase_onto(&self, upstream: &str) -> Result<()> {
        self.run(&["rebase", upstream]).map(drop)
    }

    pub fn cherry_pick(&self, id: &str) -> Result<()> {
        self.run(&["cherry-pick", id]).map(drop)
    }

    /// Revert; commit merge thì revert theo cha đầu tiên.
    pub fn revert(&self, id: &str, is_merge: bool) -> Result<()> {
        let mut args = vec!["revert", "--no-edit"];
        if is_merge {
            args.extend(["-m", "1"]);
        }
        args.push(id);
        self.run(&args).map(drop)
    }

    pub fn reset(&self, rev: &str, mode: ResetMode) -> Result<()> {
        let flag = match mode {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => "--hard",
        };
        self.run(&["reset", flag, rev]).map(drop)
    }

    // ---- Patch ----

    /// Xuất commit thành file `.patch` trong `out_dir`, trả về đường dẫn file.
    pub fn format_patch(&self, id: &str, out_dir: &Path) -> Result<PathBuf> {
        let dir = out_dir.to_string_lossy();
        let out = self.run(&["format-patch", "-1", id, "-o", &dir])?;
        out.lines()
            .last()
            .map(|l| PathBuf::from(l.trim()))
            .ok_or_else(|| GitError::Parse("format-patch không trả về file".into()))
    }

    /// Áp file patch. Patch dạng mail (`git format-patch`) dùng `git am`
    /// (giữ tác giả, message); diff thường dùng `git apply --3way`.
    pub fn apply_patch_file(&self, file: &Path) -> Result<()> {
        let head = std::fs::read(file)?;
        let text = String::from_utf8_lossy(&head[..head.len().min(4096)]);
        let path = file.to_string_lossy();
        if is_mailbox_patch(&text) {
            self.run(&["am", "--3way", &path]).map(drop)
        } else {
            self.run(&["apply", "--3way", "--whitespace=nowarn", &path])
                .map(drop)
        }
    }

    /// Chạy `git rebase -i` với danh sách việc soạn sẵn.
    pub(crate) fn rebase_with_todo(&self, base: Option<&str>, todo: &str) -> Result<()> {
        let dir = self.scratch_dir()?;
        let todo_path = dir.join("rebase-todo");
        std::fs::write(&todo_path, todo)?;
        let editor = format!("cp {}", shell_quote(&todo_path.to_string_lossy()));
        let mut args = vec!["rebase", "-i"];
        match base {
            Some(b) => args.push(b),
            None => args.push("--root"),
        }
        self.run_env(&args, &[("GIT_SEQUENCE_EDITOR", &editor)])
            .map(drop)
    }
}

pub fn is_mailbox_patch(text: &str) -> bool {
    text.starts_with("From ") && (text.contains("\nSubject:") || text.contains("\nFrom:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_mailbox() {
        assert!(is_mailbox_patch("From abc Mon Sep 17 00:00:00 2001\nFrom: A <a@x>\nSubject: [PATCH] x\n"));
        assert!(!is_mailbox_patch("diff --git a/x b/x\n"));
    }
}
