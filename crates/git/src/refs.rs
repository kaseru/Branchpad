//! Nhánh local/remote, tag, remote.

use crate::runner::{Repo, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RefKind {
    Local,
    Remote,
    Tag,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitRef {
    pub kind: RefKind,
    /// Tên ngắn: `main`, `origin/main`, `v1.0`.
    pub name: String,
    /// Tên đầy đủ: `refs/heads/main`.
    pub full: String,
    /// Commit mà ref trỏ tới (tag có chú thích đã bóc ra commit).
    pub oid: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Nhánh upstream đã bị xoá trên remote.
    pub gone: bool,
    pub is_head: bool,
}

impl GitRef {
    /// Với nhánh remote `origin/feature/x`: (`origin`, `feature/x`).
    pub fn remote_parts(&self) -> Option<(&str, &str)> {
        if self.kind != RefKind::Remote {
            return None;
        }
        self.name.split_once('/')
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

fn parse_track(s: &str) -> (u32, u32, bool) {
    if s == "gone" {
        return (0, 0, true);
    }
    let (mut ahead, mut behind) = (0, 0);
    for part in s.split(", ") {
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.parse().unwrap_or(0);
        }
    }
    (ahead, behind, false)
}

pub fn parse_refs(text: &str) -> Vec<GitRef> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\0').collect();
            if f.len() < 7 {
                return None;
            }
            let full = f[0];
            let kind = if full.starts_with("refs/heads/") {
                RefKind::Local
            } else if full.starts_with("refs/remotes/") {
                if full.ends_with("/HEAD") {
                    return None;
                }
                RefKind::Remote
            } else if full.starts_with("refs/tags/") {
                RefKind::Tag
            } else {
                return None;
            };
            let (ahead, behind, gone) = parse_track(f[5]);
            Some(GitRef {
                kind,
                name: f[1].to_string(),
                full: full.to_string(),
                oid: if f[3].is_empty() { f[2] } else { f[3] }.to_string(),
                upstream: Some(f[4]).filter(|s| !s.is_empty()).map(str::to_string),
                ahead,
                behind,
                gone,
                is_head: f[6] == "*",
            })
        })
        .collect()
}

pub fn parse_remotes(text: &str) -> Vec<Remote> {
    let mut out: Vec<Remote> = Vec::new();
    for line in text.lines() {
        let mut it = line.split('\t');
        let (Some(name), Some(rest)) = (it.next(), it.next()) else {
            continue;
        };
        let url = rest.trim_end_matches(" (fetch)").trim_end_matches(" (push)");
        if !out.iter().any(|r| r.name == name) {
            out.push(Remote {
                name: name.to_string(),
                url: url.to_string(),
            });
        }
    }
    out
}

impl Repo {
    pub fn refs(&self) -> Result<Vec<GitRef>> {
        let text = self.run(&[
            "for-each-ref",
            "--format=%(refname)%00%(refname:short)%00%(objectname)%00%(*objectname)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(HEAD)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ])?;
        Ok(parse_refs(&text))
    }

    pub fn remotes(&self) -> Result<Vec<Remote>> {
        Ok(parse_remotes(&self.run(&["remote", "-v"])?))
    }

    // ---- Nhánh ----

    pub fn checkout(&self, name: &str) -> Result<()> {
        self.run(&["switch", name]).map(drop)
    }

    /// Checkout một commit (detached HEAD).
    pub fn checkout_detached(&self, rev: &str) -> Result<()> {
        self.run(&["switch", "--detach", rev]).map(drop)
    }

    pub fn create_branch(&self, name: &str, start: Option<&str>, checkout: bool) -> Result<()> {
        let mut args = if checkout {
            vec!["switch", "-c", name]
        } else {
            vec!["branch", name]
        };
        if let Some(s) = start {
            args.push(s);
        }
        self.run(&args).map(drop)
    }

    /// Tạo nhánh local theo dõi nhánh remote (`origin/x` → `x`) rồi checkout.
    pub fn track_remote_branch(&self, remote_branch: &str, local_name: &str) -> Result<()> {
        self.run(&["switch", "-c", local_name, "--track", remote_branch])
            .map(drop)
    }

    pub fn set_upstream(&self, branch: &str, upstream: &str) -> Result<()> {
        self.run(&["branch", &format!("--set-upstream-to={upstream}"), branch])
            .map(drop)
    }

    pub fn delete_branch(&self, name: &str, force: bool) -> Result<()> {
        // Git không xoá nhánh đang được một worktree khác dùng: gỡ worktree đó trước
        // (không `force` thì git từ chối nếu worktree còn thay đổi chưa commit).
        let used_by: Vec<String> = self
            .worktrees()?
            .into_iter()
            .filter(|w| !w.is_main && w.branch.as_deref() == Some(name))
            .map(|w| w.path.to_string_lossy().into_owned())
            .collect();
        for path in used_by {
            self.remove_worktree(&path, force)?;
        }
        self.run(&["branch", if force { "-D" } else { "-d" }, name])
            .map(drop)
    }

    pub fn delete_remote_branch(&self, remote: &str, branch: &str) -> Result<()> {
        self.run(&["push", remote, "--delete", branch]).map(drop)
    }

    pub fn rename_branch(&self, old: &str, new: &str) -> Result<()> {
        self.run(&["branch", "-m", old, new]).map(drop)
    }

    // ---- Remote ----

    pub fn add_remote(&self, name: &str, url: &str) -> Result<()> {
        self.run(&["remote", "add", name, url]).map(drop)
    }

    pub fn remove_remote(&self, name: &str) -> Result<()> {
        self.run(&["remote", "remove", name]).map(drop)
    }

    // ---- Tag ----

    /// Tạo tag; có `message` thì là tag có chú thích.
    pub fn create_tag(&self, name: &str, rev: &str, message: Option<&str>) -> Result<()> {
        match message.filter(|m| !m.trim().is_empty()) {
            Some(m) => self.run(&["tag", "-a", name, "-m", m, rev]).map(drop),
            None => self.run(&["tag", name, rev]).map(drop),
        }
    }

    pub fn delete_tag(&self, name: &str) -> Result<()> {
        self.run(&["tag", "-d", name]).map(drop)
    }

    pub fn push_tag(&self, remote: &str, name: &str) -> Result<()> {
        self.run(&["push", remote, &format!("refs/tags/{name}")])
            .map(drop)
    }

    pub fn delete_remote_tag(&self, remote: &str, name: &str) -> Result<()> {
        self.run(&["push", remote, &format!(":refs/tags/{name}")])
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_refs() {
        let text = "refs/heads/main\0main\0aaa\0\0origin/main\0ahead 1, behind 2\0*\n\
refs/remotes/origin/HEAD\0origin\0aaa\0\0\0\0 \n\
refs/remotes/origin/main\0origin/main\0bbb\0\0\0\0 \n\
refs/tags/v1\0v1\0ttt\0ccc\0\0\0 \n";
        let r = parse_refs(text);
        assert_eq!(r.len(), 3);
        assert!(r[0].is_head);
        assert_eq!((r[0].ahead, r[0].behind), (1, 2));
        assert_eq!(r[1].remote_parts(), Some(("origin", "main")));
        assert_eq!(r[2].oid, "ccc");
    }

    #[test]
    fn parses_remotes() {
        let r = parse_remotes("origin\tgit@x:a.git (fetch)\norigin\tgit@x:a.git (push)\n");
        assert_eq!(r, vec![Remote { name: "origin".into(), url: "git@x:a.git".into() }]);
    }
}
