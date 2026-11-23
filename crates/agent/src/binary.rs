//! Tìm file chạy của CLI. App mở từ Finder có PATH rất ngắn, nên dò thêm các chỗ hay cài.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::CliKind;

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// Các thư mục hay chứa CLI (không gồm PATH hiện tại).
fn common_dirs(kind: Option<CliKind>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home() {
        dirs.push(home.join(".local/bin"));
        if kind.is_none() || kind == Some(CliKind::Claude) {
            dirs.push(home.join(".claude/local"));
        }
        dirs.push(home.join(".npm-global/bin"));
        dirs.push(home.join(".bun/bin"));
        dirs.push(home.join(".volta/bin"));
        dirs.push(home.join(".cargo/bin"));
    }
    for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        dirs.push(PathBuf::from(d));
    }
    dirs
}

fn candidates_in(dir: &Path, name: &str) -> Vec<PathBuf> {
    if cfg!(windows) {
        ["exe", "cmd"].iter().map(|ext| dir.join(format!("{name}.{ext}"))).collect()
    } else {
        vec![dir.join(name)]
    }
}

/// Đường dẫn CLI: `override_path` (cài đặt) → biến môi trường `BRANCHPAD_CLAUDE`/`BRANCHPAD_CODEX`
/// → các thư mục hay cài → PATH. `None` nếu không thấy.
pub fn find_cli(kind: CliKind, override_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = override_path.filter(|p| !p.as_os_str().is_empty()) {
        return p.is_file().then(|| p.to_path_buf());
    }
    if let Some(p) = std::env::var_os(kind.env_var()).filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let mut dirs = common_dirs(Some(kind));
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.iter()
        .flat_map(|d| candidates_in(d, kind.binary_name()))
        .find(|p| p.is_file())
}

/// PATH cho tiến trình CLI: thư mục chứa CLI + các thư mục hay cài + PATH hiện tại.
/// Cần vì CLI cài qua npm là script `#!/usr/bin/env node`, mở từ Finder sẽ không thấy `node`.
pub fn path_env(binary: &Path) -> OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = binary.parent() {
        dirs.push(parent.to_path_buf());
    }
    dirs.extend(common_dirs(None));
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    std::env::join_paths(dirs).unwrap_or_else(|_| std::env::var_os("PATH").unwrap_or_default())
}

/// Phiên bản CLI (`--version`, dòng đầu). Chạy đồng bộ: gọi ở luồng nền.
pub fn cli_version(binary: &Path) -> Option<String> {
    let out = Command::new(binary)
        .arg("--version")
        .env("PATH", path_env(binary))
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(line.to_string())
}

/// Model của CLI (giá trị truyền cho `--model`, tên hiển thị). Claude/Codex: danh sách có sẵn;
/// agy: chạy `agy models` (dòng `<id>\t<tên>`; agy nhận tên hiển thị ở `--model`). Chạy đồng bộ: gọi ở luồng nền.
pub fn list_models(kind: CliKind, binary: &Path) -> Vec<(String, String)> {
    if kind != CliKind::Agy {
        return kind.builtin_models().iter().map(|(v, l)| (v.to_string(), l.to_string())).collect();
    }
    let out = match Command::new(binary).arg("models").env("PATH", path_env(binary)).stdin(Stdio::null()).output() {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!("branchpad-agent: `agy models` failed: {}", String::from_utf8_lossy(&o.stderr).trim());
            return Vec::new();
        }
        Err(e) => {
            eprintln!("branchpad-agent: cannot run `agy models`: {e}");
            return Vec::new();
        }
    };
    parse_agy_models(&String::from_utf8_lossy(&out.stdout))
}

fn parse_agy_models(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(_, label)| label.trim())
        .filter(|label| !label.is_empty())
        .map(|label| (label.to_string(), label.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agy_models_use_display_names() {
        let m = parse_agy_models("Fetching available models...\ngemini-3.8-flash-high\tGemini 3.8 Flash (High)\nclaude-sonnet-4-6\tClaude Sonnet 4.6 (Thinking)\n");
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].0, "Gemini 3.8 Flash (High)");
    }

    #[test]
    fn override_must_exist() {
        assert_eq!(find_cli(CliKind::Claude, Some(Path::new("/nonexistent/claude"))), None);
    }

    #[test]
    fn path_env_starts_with_binary_dir() {
        let p = path_env(Path::new("/some/dir/claude"));
        let first = std::env::split_paths(&p).next();
        assert_eq!(first, Some(PathBuf::from("/some/dir")));
    }
}
