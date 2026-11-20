//! Tiện ích cho trình soạn: ngôn ngữ theo đuôi file, chú thích dòng,
//! đánh dấu dòng thêm/sửa/xoá so với git.

use std::path::Path;

/// Tên ngôn ngữ cho bộ tô màu tree-sitter của gpui-component.
pub fn language_for(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "rs" => "rust",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "json" | "jsonc" => "json",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "py" => "python",
        "md" | "markdown" => "markdown",
        "yml" | "yaml" => "yaml",
        "html" | "htm" => "html",
        "css" | "scss" => "css",
        "toml" => "toml",
        _ if name == "cargo.lock" => "toml",
        _ => "text",
    }
}

/// Ký hiệu chú thích dòng của ngôn ngữ (None = không hỗ trợ).
pub fn line_comment(language: &str) -> Option<&'static str> {
    match language {
        "rust" | "javascript" | "typescript" | "tsx" | "swift" | "kotlin" | "json" | "css" => {
            Some("//")
        }
        "python" | "yaml" | "toml" => Some("#"),
        _ => None,
    }
}

/// Bật/tắt chú thích cho các dòng `[first, last]` (đếm từ 0) của `text`.
/// Nếu mọi dòng có chữ đều đã chú thích thì bỏ, ngược lại thêm.
pub fn toggle_comment(text: &str, first: usize, last: usize, prefix: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = last.min(lines.len().saturating_sub(1));
    let target = |i: usize| i >= first && i <= last;
    let non_blank: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| target(*i) && !l.trim().is_empty())
        .map(|(_, l)| *l)
        .collect();
    if non_blank.is_empty() {
        return text.to_string();
    }
    let all_commented = non_blank
        .iter()
        .all(|l| l.trim_start().starts_with(prefix));
    let indent = non_blank
        .iter()
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if !target(i) || l.trim().is_empty() {
                return l.to_string();
            }
            if all_commented {
                let ws = l.len() - l.trim_start().len();
                let rest = &l[ws + prefix.len()..];
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                format!("{}{rest}", &l[..ws])
            } else {
                format!("{}{prefix} {}", &l[..indent], &l[indent..])
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineMarkKind {
    Added,
    Modified,
    /// Có dòng bị xoá ngay trước dòng này.
    Deleted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineMark {
    /// Dòng (từ 0) trong nội dung hiện tại.
    pub line: usize,
    pub kind: LineMarkKind,
}

/// So nội dung hiện tại (kể cả chưa lưu) với bản trong git.
pub fn line_marks(base: &str, current: &str) -> Vec<LineMark> {
    use similar::{DiffOp, TextDiff};
    let diff = TextDiff::from_lines(base, current);
    let mut marks = Vec::new();
    for op in diff.ops() {
        match *op {
            DiffOp::Equal { .. } => {}
            DiffOp::Insert {
                new_index, new_len, ..
            } => marks.extend((new_index..new_index + new_len).map(|line| LineMark {
                line,
                kind: LineMarkKind::Added,
            })),
            DiffOp::Delete { new_index, .. } => marks.push(LineMark {
                line: new_index,
                kind: LineMarkKind::Deleted,
            }),
            DiffOp::Replace {
                new_index, new_len, ..
            } => marks.extend((new_index..new_index + new_len).map(|line| LineMark {
                line,
                kind: LineMarkKind::Modified,
            })),
        }
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_comments() {
        let t = "fn a() {\n    x();\n\n    y();\n}";
        let c = toggle_comment(t, 1, 3, "//");
        assert_eq!(c, "fn a() {\n    // x();\n\n    // y();\n}");
        assert_eq!(toggle_comment(&c, 1, 3, "//"), t);
    }

    #[test]
    fn marks_lines() {
        let m = line_marks("a\nb\nc\n", "a\nB\nc\nd\n");
        assert_eq!(
            m,
            vec![
                LineMark { line: 1, kind: LineMarkKind::Modified },
                LineMark { line: 3, kind: LineMarkKind::Added },
            ]
        );
    }

    #[test]
    fn languages() {
        assert_eq!(language_for(Path::new("a/b.tsx")), "tsx");
        assert_eq!(language_for(Path::new("x.unknown")), "text");
    }
}
