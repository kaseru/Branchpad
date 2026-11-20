//! Blame, tìm chữ trong repo (`git grep`), danh sách file, kiểm tra .gitignore.

use std::collections::{HashMap, HashSet};

use crate::runner::{Repo, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameLine {
    pub commit: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
    /// Số dòng (từ 1) trong file.
    pub line_no: u32,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
struct BlameInfo {
    author: String,
    time: i64,
    summary: String,
}

pub fn parse_blame(text: &str) -> Vec<BlameLine> {
    let mut infos: HashMap<String, BlameInfo> = HashMap::new();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_line = 0u32;
    for line in text.lines() {
        if let Some(content) = line.strip_prefix('\t') {
            let info = infos.get(&cur).cloned().unwrap_or_default();
            out.push(BlameLine {
                commit: cur.clone(),
                author: info.author,
                time: info.time,
                summary: info.summary,
                line_no: cur_line,
                text: content.to_string(),
            });
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let key = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        if key.len() >= 40 && key.chars().all(|c| c.is_ascii_hexdigit()) {
            cur = key.to_string();
            cur_line = value
                .split(' ')
                .nth(1)
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            infos.entry(cur.clone()).or_default();
            continue;
        }
        let Some(info) = infos.get_mut(&cur) else { continue };
        match key {
            "author" => info.author = value.to_string(),
            "author-time" => info.time = value.parse().unwrap_or(0),
            "summary" => info.summary = value.to_string(),
            _ => {}
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrepMatch {
    pub path: String,
    pub line_no: u32,
    pub text: String,
}

pub fn parse_grep(text: &str, limit: usize) -> Vec<GrepMatch> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\0');
            let path = f.next()?;
            let line_no = f.next()?.parse().ok()?;
            let text = f.next().unwrap_or_default();
            Some(GrepMatch {
                path: path.to_string(),
                line_no,
                text: text.to_string(),
            })
        })
        .take(limit)
        .collect()
}

impl Repo {
    /// Blame file ở thư mục làm việc (`rev = None`) hoặc ở một commit.
    pub fn blame(&self, path: &str, rev: Option<&str>) -> Result<Vec<BlameLine>> {
        let mut args = vec!["blame", "--porcelain"];
        if let Some(r) = rev {
            args.push(r);
        }
        args.extend(["--", path]);
        Ok(parse_blame(&self.run(&args)?))
    }

    /// Tìm chữ (không phân biệt hoa thường) trong file đã theo dõi và file mới,
    /// bỏ qua file trong .gitignore và file nhị phân.
    pub fn grep(&self, pattern: &str, limit: usize) -> Result<Vec<GrepMatch>> {
        if pattern.is_empty() {
            return Ok(Vec::new());
        }
        // Mã 1 = không có kết quả.
        let out = self.run_bytes(
            &[
                "grep",
                "-n",
                "-I",
                "-z",
                "-i",
                "--untracked",
                "--fixed-strings",
                "--no-color",
                "-e",
                pattern,
            ],
            &[0, 1],
        )?;
        Ok(parse_grep(&String::from_utf8_lossy(&out), limit))
    }

    /// Mọi file của repo (đã theo dõi + mới, trừ file bị .gitignore).
    pub fn list_files(&self) -> Result<Vec<String>> {
        let out = self.run_bytes(
            &["ls-files", "-z", "--cached", "--others", "--exclude-standard"],
            &[0],
        )?;
        let text = String::from_utf8_lossy(&out);
        let mut seen = HashSet::new();
        Ok(text
            .split('\0')
            .filter(|p| !p.is_empty() && seen.insert(*p))
            .map(str::to_string)
            .collect())
    }

    /// Trong các đường dẫn (tương đối) cho trước, đường dẫn nào bị .gitignore.
    pub fn ignored(&self, paths: &[String]) -> Result<HashSet<String>> {
        if paths.is_empty() {
            return Ok(HashSet::new());
        }
        let mut input = paths.join("\0");
        input.push('\0');
        // check-ignore trả 1 khi không có file nào bị bỏ qua — dùng run_bytes để
        // chấp nhận mã 1 khi có stdin.
        let out = self.check_ignore_stdin(input.as_bytes())?;
        Ok(out
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect())
    }

    fn check_ignore_stdin(&self, input: &[u8]) -> Result<String> {
        match self.run_input(&["check-ignore", "-z", "--stdin"], input) {
            Ok(s) => Ok(s),
            Err(crate::GitError::Failed { code: Some(1), .. }) => Ok(String::new()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_blame() {
        let sha = "a".repeat(40);
        let text = format!(
            "{sha} 1 1 2\nauthor An\nauthor-time 100\nsummary first\nfilename x\n\tline one\n{sha} 2 2\n\tline two\n"
        );
        let b = parse_blame(&text);
        assert_eq!(b.len(), 2);
        assert_eq!(b[1].line_no, 2);
        assert_eq!(b[1].author, "An");
        assert_eq!(b[0].text, "line one");
    }

    #[test]
    fn parses_grep() {
        let g = parse_grep("src/a.rs\x0012\0let x = 1;\nb.md\x003\0hi\n", 10);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].line_no, 12);
        assert_eq!(g[1].text, "hi");
    }
}
