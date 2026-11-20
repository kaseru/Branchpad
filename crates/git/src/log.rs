//! Lịch sử commit, chi tiết commit, file của commit, nội dung file ở một commit.

use crate::runner::{GitError, Repo, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    /// Giây Unix.
    pub time: i64,
    pub summary: String,
    /// Nhãn ref, ví dụ `HEAD -> main`, `origin/main`, `tag: v1.0`.
    pub refs: Vec<String>,
}

impl Commit {
    pub fn short_id(&self) -> &str {
        &self.id[..self.id.len().min(8)]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitDetail {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub author_time: i64,
    pub committer: String,
    pub committer_time: i64,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    /// `A`, `M`, `D`, `R`, `C`, `T`...
    pub status: char,
    pub path: String,
    pub old_path: Option<String>,
}

/// Bộ lọc danh sách commit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LogFilter {
    #[default]
    None,
    Message(String),
    Author(String),
    Hash(String),
}

impl LogFilter {
    /// Hiểu chuỗi tìm kiếm: `author:xxx`, hash hex (>= 4 ký tự), còn lại tìm theo message.
    pub fn parse(query: &str) -> Self {
        let q = query.trim();
        if q.is_empty() {
            LogFilter::None
        } else if let Some(a) = q.strip_prefix("author:") {
            LogFilter::Author(a.trim().to_string())
        } else if q.len() >= 4 && q.len() <= 64 && q.chars().all(|c| c.is_ascii_hexdigit()) {
            LogFilter::Hash(q.to_lowercase())
        } else {
            LogFilter::Message(q.to_string())
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LogQuery {
    pub skip: usize,
    pub limit: usize,
    pub filter: LogFilter,
    /// Chỉ lấy commit chạm vào file này (theo dõi cả đổi tên).
    pub path: Option<String>,
}

const LOG_FORMAT: &str = "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%s%x00%D%x1e";

pub fn parse_log(text: &str) -> Vec<Commit> {
    text.split('\x1e')
        .filter_map(|rec| {
            let rec = rec.trim_start_matches('\n');
            if rec.is_empty() {
                return None;
            }
            let f: Vec<&str> = rec.splitn(7, '\0').collect();
            if f.len() < 7 {
                return None;
            }
            Some(Commit {
                id: f[0].to_string(),
                parents: f[1].split_whitespace().map(str::to_string).collect(),
                author: f[2].to_string(),
                email: f[3].to_string(),
                time: f[4].parse().unwrap_or(0),
                summary: f[5].to_string(),
                refs: f[6]
                    .split(", ")
                    .filter(|r| !r.is_empty())
                    .map(str::to_string)
                    .collect(),
            })
        })
        .collect()
}

/// Đọc kết quả `--name-status -z`.
pub fn parse_name_status(text: &str) -> Vec<ChangedFile> {
    let mut out = Vec::new();
    let mut it = text.split('\0').filter(|s| !s.is_empty());
    while let Some(code) = it.next() {
        let status = code.chars().next().unwrap_or('M');
        let Some(first) = it.next() else { break };
        if matches!(status, 'R' | 'C') {
            let Some(second) = it.next() else { break };
            out.push(ChangedFile {
                status,
                path: second.to_string(),
                old_path: Some(first.to_string()),
            });
        } else {
            out.push(ChangedFile {
                status,
                path: first.to_string(),
                old_path: None,
            });
        }
    }
    out
}

fn no_commits_yet(e: &GitError) -> bool {
    matches!(e, GitError::Failed { stderr, .. }
        if stderr.contains("does not have any commits") || stderr.contains("unknown revision"))
}

impl Repo {
    /// Danh sách commit của mọi nhánh/tag (thứ tự ngày, con trước cha).
    pub fn log(&self, q: &LogQuery) -> Result<Vec<Commit>> {
        let skip = format!("--skip={}", q.skip);
        let limit = format!("--max-count={}", q.limit.max(1));
        let mut args: Vec<String> = vec![
            "log".into(),
            LOG_FORMAT.into(),
            skip,
            limit,
        ];
        match &q.filter {
            LogFilter::None => {}
            LogFilter::Message(m) => {
                args.push("-i".into());
                args.push("--fixed-strings".into());
                args.push(format!("--grep={m}"));
            }
            LogFilter::Author(a) => {
                args.push("-i".into());
                args.push(format!("--author={a}"));
            }
            LogFilter::Hash(h) => {
                // Hash: lấy đúng commit đó (nếu có).
                let full = self
                    .run_codes(&["rev-parse", "--verify", "-q", &format!("{h}^{{commit}}")], &[0, 1])?;
                let full = full.trim();
                if full.is_empty() || q.skip > 0 {
                    return Ok(Vec::new());
                }
                let refs: Vec<&str> = vec!["log", LOG_FORMAT, "--max-count=1", full];
                return Ok(parse_log(&self.run(&refs)?));
            }
        }
        if let Some(path) = &q.path {
            args.push("--follow".into());
            args.push("--".into());
            args.push(path.clone());
        } else {
            args.push("--date-order".into());
            args.extend(["--branches", "--remotes", "--tags", "HEAD"].map(String::from));
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.run(&refs) {
            Ok(text) => Ok(parse_log(&text)),
            Err(e) if no_commits_yet(&e) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    pub fn commit_detail(&self, id: &str) -> Result<CommitDetail> {
        let text = self.run(&[
            "show",
            "-s",
            "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%cn%x00%ct%x00%B",
            id,
        ])?;
        let f: Vec<&str> = text.splitn(8, '\0').collect();
        if f.len() < 8 {
            return Err(GitError::Parse(format!("commit {id}")));
        }
        Ok(CommitDetail {
            id: f[0].to_string(),
            parents: f[1].split_whitespace().map(str::to_string).collect(),
            author: f[2].to_string(),
            email: f[3].to_string(),
            author_time: f[4].parse().unwrap_or(0),
            committer: f[5].to_string(),
            committer_time: f[6].parse().unwrap_or(0),
            message: f[7].trim_end().to_string(),
        })
    }

    /// Hash của cây rỗng (dùng để so commit gốc).
    pub fn empty_tree(&self) -> Result<String> {
        Ok(self
            .run_input(&["hash-object", "-t", "tree", "--stdin"], b"")?
            .trim()
            .to_string())
    }

    /// File thay đổi trong commit `id`, so với cha đầu tiên (hoặc cây rỗng).
    pub fn commit_files(&self, id: &str, parent: Option<&str>) -> Result<Vec<ChangedFile>> {
        let base = match parent {
            Some(p) => p.to_string(),
            None => self.empty_tree()?,
        };
        let out = self.run_bytes(
            &["diff", "--no-ext-diff", "--name-status", "-z", "-M", &base, id],
            &[0],
        )?;
        Ok(parse_name_status(&String::from_utf8_lossy(&out)))
    }

    /// Nội dung file ở một commit (`rev` có thể là `HEAD`, hash, `:0`...).
    pub fn show_file(&self, rev: &str, path: &str) -> Result<Vec<u8>> {
        self.run_bytes(&["show", &format!("{rev}:{path}")], &[0])
    }

    /// Message của commit cuối (để sửa commit cuối).
    pub fn last_commit_message(&self) -> Result<String> {
        Ok(self
            .run(&["log", "-1", "--format=%B"])?
            .trim_end()
            .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_log_records() {
        let text = "a1\0p1 p2\0An\0a@x\0100\0Merge x\0HEAD -> main, tag: v1\x1e\nb2\0\0Bo\0b@x\x00200\0init\0\x1e\n";
        let c = parse_log(text);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].parents, vec!["p1", "p2"]);
        assert_eq!(c[0].refs, vec!["HEAD -> main", "tag: v1"]);
        assert!(c[1].parents.is_empty());
        assert!(c[1].refs.is_empty());
    }

    #[test]
    fn parses_name_status() {
        let f = parse_name_status("M\0a.rs\0R090\0old.rs\0new.rs\0D\0gone.rs\0");
        assert_eq!(f.len(), 3);
        assert_eq!(f[1].path, "new.rs");
        assert_eq!(f[1].old_path.as_deref(), Some("old.rs"));
        assert_eq!(f[2].status, 'D');
    }

    #[test]
    fn filter_parse() {
        assert_eq!(LogFilter::parse("abc123"), LogFilter::Hash("abc123".into()));
        assert_eq!(LogFilter::parse("author: An"), LogFilter::Author("An".into()));
        assert_eq!(LogFilter::parse("fix bug"), LogFilter::Message("fix bug".into()));
    }
}
