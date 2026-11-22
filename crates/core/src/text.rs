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
    // Tên file đặc biệt (không đuôi hoặc đuôi không đại diện cho loại file).
    match name.as_str() {
        "gemfile" | "podfile" | "rakefile" | "fastfile" | "appfile" | "brewfile" | "vagrantfile"
        | "guardfile" | "cartfile" => return "ruby",
        "dockerfile" | ".bashrc" | ".zshrc" | ".bash_profile" | ".zprofile" | ".profile"
        | ".env" | "gradlew" => return "bash",
        "makefile" | "gnumakefile" => return "make",
        "cmakelists.txt" => return "cmake",
        "cargo.lock" | "pipfile" => return "toml",
        ".babelrc" | ".eslintrc" | ".prettierrc" => return "json",
        ".gitignore" | ".gitattributes" | ".dockerignore" | ".eslintignore" | ".npmignore"
        | ".editorconfig" => return "bash",
        _ => {}
    }
    if name.starts_with(".env.") {
        return "bash";
    }
    match ext.as_str() {
        "rs" => "rust",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "json" | "jsonc" | "json5" => "json",
        "swift" => "swift",
        // Chưa có ngữ pháp Kotlin/Groovy: dùng Java cho gần đúng (chú thích, chuỗi, từ khoá).
        "kt" | "kts" | "gradle" | "groovy" | "java" => "java",
        "py" => "python",
        "md" | "markdown" | "mdx" => "markdown",
        "yml" | "yaml" => "yaml",
        "html" | "htm" | "xml" | "plist" | "svg" | "vue" => "html",
        "css" | "scss" => "css",
        "toml" => "toml",
        "sh" | "bash" | "zsh" | "fish" | "command" | "env" | "ksh" => "bash",
        "rb" | "gemspec" | "podspec" | "rake" | "ru" | "erb" => "ruby",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "m" | "mm" => "cpp",
        "cs" => "csharp",
        "go" => "go",
        "sql" => "sql",
        "scala" => "scala",
        "zig" => "zig",
        "mk" | "make" => "make",
        "cmake" => "cmake",
        "proto" => "proto",
        "graphql" | "gql" => "graphql",
        "diff" | "patch" => "diff",
        "ex" | "exs" => "elixir",
        "gitignore" | "properties" | "conf" | "ini" | "cfg" => "bash",
        _ => "text",
    }
}

/// Ký hiệu chú thích dòng của ngôn ngữ (None = không hỗ trợ).
pub fn line_comment(language: &str) -> Option<&'static str> {
    match language {
        "rust" | "javascript" | "typescript" | "tsx" | "swift" | "java" | "json" | "css" | "c"
        | "cpp" | "csharp" | "go" | "scala" | "zig" | "proto" | "graphql" => Some("//"),
        "python" | "yaml" | "toml" | "bash" | "ruby" | "make" | "cmake" | "elixir" => Some("#"),
        "sql" => Some("--"),
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
        assert_eq!(language_for(Path::new("app/Gemfile")), "ruby");
        assert_eq!(language_for(Path::new("ios/Podfile")), "ruby");
        assert_eq!(language_for(Path::new("a.podspec")), "ruby");
        assert_eq!(language_for(Path::new("android/app/build.gradle")), "java");
        assert_eq!(language_for(Path::new("run.sh")), "bash");
        assert_eq!(language_for(Path::new(".env.local")), "bash");
        assert_eq!(language_for(Path::new("Cargo.lock")), "toml");
    }
}
