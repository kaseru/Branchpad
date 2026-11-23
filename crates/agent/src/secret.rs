//! Token đăng nhập Claude Code (`CLAUDE_CODE_OAUTH_TOKEN`, tạo bằng `claude setup-token`) lưu trong
//! Keychain của macOS qua lệnh `security`; không bao giờ ghi ra file cài đặt.

use std::fmt;
use std::io;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};

/// Biến môi trường Claude Code đọc token.
pub const CLAUDE_TOKEN_ENV: &str = "CLAUDE_CODE_OAUTH_TOKEN";

#[cfg(target_os = "macos")]
const SERVICE: &str = "Branchpad";
#[cfg(target_os = "macos")]
const SECURITY: &str = "/usr/bin/security";
/// Mã thoát của `security` khi không có mục trong Keychain.
#[cfg(target_os = "macos")]
const NOT_FOUND: i32 = 44;

/// Chuỗi bí mật: `Debug` không in nội dung (tránh lộ token vào log).
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[cfg(target_os = "macos")]
fn security(args: &[&str]) -> io::Result<std::process::Output> {
    Command::new(SECURITY).args(args).stdin(Stdio::null()).output()
}

#[cfg(target_os = "macos")]
fn failed(what: &str, out: &std::process::Output) -> io::Error {
    let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
    io::Error::other(format!("{what}: {}", if msg.is_empty() { out.status.to_string() } else { msg }))
}

/// Đọc token đã lưu; `None` nếu chưa có. Chạy đồng bộ: gọi ở luồng nền.
#[cfg(target_os = "macos")]
pub fn load_claude_token() -> io::Result<Option<Secret>> {
    let out = security(&["find-generic-password", "-s", SERVICE, "-a", CLAUDE_TOKEN_ENV, "-w"])?;
    match out.status.code() {
        Some(0) => {
            let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
            Ok((!token.is_empty()).then(|| Secret(token)))
        }
        Some(NOT_FOUND) => Ok(None),
        _ => Err(failed("cannot read token from Keychain", &out)),
    }
}

/// Lưu (hoặc thay) token. Chạy đồng bộ: gọi ở luồng nền.
#[cfg(target_os = "macos")]
pub fn save_claude_token(token: &str) -> io::Result<()> {
    let out = security(&["add-generic-password", "-U", "-s", SERVICE, "-a", CLAUDE_TOKEN_ENV, "-w", token])?;
    if out.status.success() {
        Ok(())
    } else {
        Err(failed("cannot save token to Keychain", &out))
    }
}

/// Xoá token (chưa có cũng không lỗi). Chạy đồng bộ: gọi ở luồng nền.
#[cfg(target_os = "macos")]
pub fn delete_claude_token() -> io::Result<()> {
    let out = security(&["delete-generic-password", "-s", SERVICE, "-a", CLAUDE_TOKEN_ENV])?;
    match out.status.code() {
        Some(0) | Some(NOT_FOUND) => Ok(()),
        _ => Err(failed("cannot delete token from Keychain", &out)),
    }
}

#[cfg(not(target_os = "macos"))]
fn unsupported() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "storing the token is only supported on macOS (Keychain)")
}

#[cfg(not(target_os = "macos"))]
pub fn load_claude_token() -> io::Result<Option<Secret>> {
    Ok(None)
}

#[cfg(not(target_os = "macos"))]
pub fn save_claude_token(_token: &str) -> io::Result<()> {
    Err(unsupported())
}

#[cfg(not(target_os = "macos"))]
pub fn delete_claude_token() -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_value() {
        let s = Secret::new("sk-ant-oat-secret");
        assert_eq!(format!("{s:?}"), "Secret(***)");
        assert_eq!(s.expose(), "sk-ant-oat-secret");
    }
}
