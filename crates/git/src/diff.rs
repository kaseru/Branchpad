//! Diff: đọc unified diff, stage/unstage/bỏ thay đổi theo file và theo đoạn (hunk).

use crate::runner::{Repo, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
    /// Dòng `\ No newline at end of file`.
    NoNewline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Nội dung dòng, không có ký tự đầu `+`/`-`/` `.
    pub text: String,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    /// Các dòng đầu (`diff --git`, `index`, `---`, `+++`...) để dựng lại patch.
    pub header: Vec<String>,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    pub fn path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or_default()
    }

    /// Patch chỉ gồm một hunk (để `git apply --cached`).
    pub fn hunk_patch(&self, hunk: usize) -> Option<String> {
        let h = self.hunks.get(hunk)?;
        let mut s = String::new();
        for line in &self.header {
            s.push_str(line);
            s.push('\n');
        }
        s.push_str(&h.header);
        s.push('\n');
        for l in &h.lines {
            match l.kind {
                LineKind::Context => s.push(' '),
                LineKind::Added => s.push('+'),
                LineKind::Removed => s.push('-'),
                LineKind::NoNewline => {}
            }
            s.push_str(&l.text);
            s.push('\n');
        }
        Some(s)
    }
}

fn strip_side(p: &str) -> Option<String> {
    let p = p.trim_end_matches('\t');
    if p == "/dev/null" {
        return None;
    }
    let p = p
        .strip_prefix("a/")
        .or_else(|| p.strip_prefix("b/"))
        .unwrap_or(p);
    Some(p.to_string())
}

/// `@@ -a,b +c,d @@ ...`
fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    let rest = line.strip_prefix("@@ ")?;
    let end = rest.find(" @@")?;
    let mut parts = rest[..end].split(' ');
    let range = |s: &str| -> Option<(u32, u32)> {
        let mut it = s.splitn(2, ',');
        let start = it.next()?.parse().ok()?;
        let len = it.next().map_or(Some(1), |n| n.parse().ok())?;
        Some((start, len))
    };
    let (os, ol) = range(parts.next()?.strip_prefix('-')?)?;
    let (ns, nl) = range(parts.next()?.strip_prefix('+')?)?;
    Some((os, ol, ns, nl))
}

pub fn parse_diff(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut cur: Option<FileDiff> = None;
    let (mut old_no, mut new_no) = (0u32, 0u32);

    for line in text.split('\n') {
        if line.starts_with("diff --git ") || line.starts_with("diff --cc ") {
            if let Some(f) = cur.take() {
                files.push(f);
            }
            let mut f = FileDiff {
                header: vec![line.to_string()],
                ..Default::default()
            };
            // Dự phòng khi không có dòng ---/+++ (đổi tên thuần, file nhị phân).
            if let Some(rest) = line.strip_prefix("diff --git ") {
                if let Some(idx) = rest.rfind(" b/") {
                    f.old_path = strip_side(&rest[..idx]);
                    f.new_path = strip_side(&rest[idx + 1..]);
                }
            }
            cur = Some(f);
            continue;
        }
        let Some(f) = cur.as_mut() else { continue };

        if line.starts_with("@@ ") {
            if let Some((os, ol, ns, nl)) = parse_hunk_header(line) {
                old_no = os;
                new_no = ns;
                f.hunks.push(Hunk {
                    header: line.to_string(),
                    old_start: os,
                    old_lines: ol,
                    new_start: ns,
                    new_lines: nl,
                    lines: Vec::new(),
                });
            }
            continue;
        }

        if let Some(h) = f.hunks.last_mut() {
            let (kind, text) = match line.as_bytes().first() {
                Some(b'+') => (LineKind::Added, &line[1..]),
                Some(b'-') => (LineKind::Removed, &line[1..]),
                Some(b' ') => (LineKind::Context, &line[1..]),
                Some(b'\\') => (LineKind::NoNewline, line),
                _ => continue,
            };
            let (o, n) = match kind {
                LineKind::Added => {
                    new_no += 1;
                    (None, Some(new_no - 1))
                }
                LineKind::Removed => {
                    old_no += 1;
                    (Some(old_no - 1), None)
                }
                LineKind::Context => {
                    old_no += 1;
                    new_no += 1;
                    (Some(old_no - 1), Some(new_no - 1))
                }
                LineKind::NoNewline => (None, None),
            };
            h.lines.push(DiffLine {
                kind,
                text: text.to_string(),
                old_no: o,
                new_no: n,
            });
            continue;
        }

        if let Some(p) = line.strip_prefix("--- ") {
            f.old_path = strip_side(p);
        } else if let Some(p) = line.strip_prefix("+++ ") {
            f.new_path = strip_side(p);
        } else if let Some(p) = line.strip_prefix("rename from ") {
            f.old_path = Some(p.to_string());
        } else if let Some(p) = line.strip_prefix("rename to ") {
            f.new_path = Some(p.to_string());
        } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            f.binary = true;
        }
        if line.starts_with("new file mode") {
            f.old_path = None;
        } else if line.starts_with("deleted file mode") {
            f.new_path = None;
        }
        if !line.is_empty() {
            f.header.push(line.to_string());
        }
    }
    if let Some(f) = cur {
        files.push(f);
    }
    files
}

/// Diff của file đang làm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkingSide {
    /// Thư mục làm việc so với index.
    Unstaged,
    /// Index so với HEAD.
    Staged,
    /// File mới chưa theo dõi (so với rỗng).
    Untracked,
}

impl Repo {
    pub fn working_diff(&self, path: &str, side: WorkingSide) -> Result<Vec<FileDiff>> {
        let out = match side {
            WorkingSide::Unstaged => {
                self.run_bytes(&["diff", "--no-ext-diff", "--no-color", "-M", "--", path], &[0])?
            }
            WorkingSide::Staged => self.run_bytes(
                &["diff", "--no-ext-diff", "--no-color", "-M", "--cached", "--", path],
                &[0],
            )?,
            // --no-index trả mã 1 khi có khác biệt.
            WorkingSide::Untracked => self.run_bytes(
                &["diff", "--no-ext-diff", "--no-color", "--no-index", "--", "/dev/null", path],
                &[0, 1],
            )?,
        };
        Ok(parse_diff(&String::from_utf8_lossy(&out)))
    }

    /// Diff của một file trong commit `id` so với `parent` (hoặc cây rỗng).
    pub fn commit_diff(&self, id: &str, parent: Option<&str>, path: &str) -> Result<Vec<FileDiff>> {
        let base = match parent {
            Some(p) => p.to_string(),
            None => self.empty_tree()?,
        };
        let out = self.run_bytes(
            &["diff", "--no-ext-diff", "--no-color", "-M", &base, id, "--", path],
            &[0],
        )?;
        Ok(parse_diff(&String::from_utf8_lossy(&out)))
    }

    pub fn stage(&self, paths: &[&str]) -> Result<()> {
        let mut args = vec!["add", "-A", "--"];
        args.extend_from_slice(paths);
        self.run(&args).map(drop)
    }

    pub fn unstage(&self, paths: &[&str], head_exists: bool) -> Result<()> {
        let mut args = if head_exists {
            vec!["restore", "--staged", "--"]
        } else {
            // Repo chưa có commit: chỉ có thể bỏ khỏi index.
            vec!["rm", "--cached", "-r", "-q", "--"]
        };
        args.extend_from_slice(paths);
        self.run(&args).map(drop)
    }

    /// Bỏ thay đổi trong thư mục làm việc. File chưa theo dõi thì xoá.
    pub fn discard(&self, paths: &[&str], untracked: bool) -> Result<()> {
        let mut args = if untracked {
            vec!["clean", "-f", "-q", "--"]
        } else {
            vec!["restore", "--worktree", "--"]
        };
        args.extend_from_slice(paths);
        self.run(&args).map(drop)
    }

    /// Stage một hunk của diff chưa stage (hoặc file mới).
    pub fn stage_hunk(&self, file: &FileDiff, hunk: usize) -> Result<()> {
        self.apply_hunk(file, hunk, &["apply", "--cached", "--recount", "-"])
    }

    /// Bỏ stage một hunk của diff đã stage.
    pub fn unstage_hunk(&self, file: &FileDiff, hunk: usize) -> Result<()> {
        self.apply_hunk(file, hunk, &["apply", "--cached", "--reverse", "--recount", "-"])
    }

    /// Bỏ một hunk khỏi thư mục làm việc (diff chưa stage).
    pub fn discard_hunk(&self, file: &FileDiff, hunk: usize) -> Result<()> {
        self.apply_hunk(file, hunk, &["apply", "--reverse", "--recount", "-"])
    }

    fn apply_hunk(&self, file: &FileDiff, hunk: usize, args: &[&str]) -> Result<()> {
        let patch = file
            .hunk_patch(hunk)
            .ok_or_else(|| crate::GitError::Invalid("Hunk not found".into()))?;
        self.run_input(args, patch.as_bytes()).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,3 @@ fn main
 one
-two
+TWO
 three
@@ -10 +10,2 @@
 ten
+eleven
\\ No newline at end of file
diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1 @@
+hello
";

    #[test]
    fn parses_files_and_hunks() {
        let files = parse_diff(SAMPLE);
        assert_eq!(files.len(), 2);
        let a = &files[0];
        assert_eq!(a.path(), "src/a.rs");
        assert_eq!(a.hunks.len(), 2);
        assert_eq!(a.hunks[0].lines[1].kind, LineKind::Removed);
        assert_eq!(a.hunks[0].lines[1].old_no, Some(2));
        assert_eq!(a.hunks[0].lines[2].new_no, Some(2));
        assert_eq!(a.hunks[1].old_lines, 1);
        assert_eq!(a.hunks[1].lines[2].kind, LineKind::NoNewline);
        assert_eq!(files[1].old_path, None);
        assert_eq!(files[1].path(), "new.txt");
    }

    #[test]
    fn rebuilds_single_hunk_patch() {
        let files = parse_diff(SAMPLE);
        let p = files[0].hunk_patch(1).unwrap();
        assert!(p.starts_with("diff --git a/src/a.rs b/src/a.rs\nindex 111..222 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -10 +10,2 @@\n"));
        assert!(p.ends_with(" ten\n+eleven\n\\ No newline at end of file\n"));
        assert!(!p.contains("TWO"));
    }
}
