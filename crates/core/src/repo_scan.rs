//! Quét một thư mục để tìm các git repo bên trong.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// Độ sâu tối đa khi quét (thư mục gốc là 0).
const MAX_DEPTH: usize = 3;
/// Số repo tối đa trả về, để chọn nhầm thư mục lớn không mở hàng trăm tab.
pub const MAX_REPOS: usize = 30;

/// Các thư mục nặng, không chứa repo cần mở.
const SKIP_DIRS: &[&str] = &["node_modules", "target", "Library", "Pods"];

/// Tìm các git repo "đầu tiên" dưới `root`: duyệt theo tầng, gặp repo thì lấy và
/// không đi sâu vào trong nó. Kết quả sắp theo tên.
pub fn find_git_repos(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    while let Some((dir, depth)) = queue.pop_front() {
        if dir.join(".git").exists() {
            found.push(dir);
            if found.len() >= MAX_REPOS {
                break;
            }
            continue;
        }
        if depth >= MAX_DEPTH {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("branchpad: cannot read {}: {e}", dir.display());
                continue;
            }
        };
        let mut subs: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_ref())
            })
            .map(|e| e.path())
            .collect();
        subs.sort();
        queue.extend(subs.into_iter().map(|p| (p, depth + 1)));
    }
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mkrepo(p: &Path) {
        std::fs::create_dir_all(p.join(".git")).unwrap();
    }

    #[test]
    fn finds_outermost_repos_only() {
        let tmp = std::env::temp_dir().join(format!("bp-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkrepo(&tmp.join("a"));
        mkrepo(&tmp.join("a/inner"));
        mkrepo(&tmp.join("group/b"));
        mkrepo(&tmp.join("node_modules/c"));
        mkrepo(&tmp.join(".hidden/d"));
        mkrepo(&tmp.join("w/x/y/z"));
        let got = find_git_repos(&tmp);
        assert_eq!(got, vec![tmp.join("a"), tmp.join("group/b")]);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
