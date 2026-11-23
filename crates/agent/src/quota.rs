//! Hạn mức (quota) gói của CLI, cùng cách DevDesk lấy:
//! - Claude Code: `claude -p /usage` (dòng `Current session: 12% used · resets 5:09pm (Asia/…)`); dùng token
//!   `setup-token` thì `/usage` không trả hạn mức, đọc header `anthropic-ratelimit-unified-*` của một lần gọi
//!   Haiku 1 token (qua `curl`, token đưa vào stdin, không nằm trên dòng lệnh).
//! - Antigravity: `agy -p /quota` (mỗi dòng `<nhóm model>  <loại hạn mức>  <% còn lại>  <giờ reset ISO>`).
//! - Codex: chưa hỗ trợ.
//!
//! Chạy đồng bộ: gọi ở luồng nền. Không tự chạy định kỳ; app lấy khi mở panel / hết lượt / bấm làm mới.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::{path_env, CliKind, Secret};

/// Một hạn mức: tên (5h, Week…), % còn lại, giờ reset (chữ như CLI in ra).
#[derive(Clone, Debug, PartialEq)]
pub struct QuotaItem {
    pub name: String,
    pub remaining: Option<u8>,
    pub reset: Option<String>,
}

/// Model gọi thử để đọc header hạn mức (rẻ nhất).
const PROBE_MODEL: &str = "claude-haiku-5-5";

pub fn fetch_quota(kind: CliKind, binary: &Path, token: Option<&Secret>) -> Result<Vec<QuotaItem>, String> {
    match kind {
        CliKind::Claude => match token {
            Some(t) => claude_quota_from_headers(t),
            None => run(binary, &["-p", "/usage"]).map(|out| parse_claude_usage(&out)),
        },
        CliKind::Agy => run(binary, &["-p", "/quota"]).map(|out| parse_agy_quota(&out)),
        CliKind::Codex => Err("Codex quota is not supported yet".into()),
    }
    .and_then(|items| if items.is_empty() { Err("the CLI returned no quota information".into()) } else { Ok(items) })
}

fn run(binary: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(binary)
        .args(args)
        .env("PATH", path_env(binary))
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { out.status.to_string() } else { err });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `Current session: 12% used · resets 5:09pm (Asia/Ho_Chi_Minh)` → 5h, còn 88%.
pub(crate) fn parse_claude_usage(text: &str) -> Vec<QuotaItem> {
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("Current "))
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let pct: f32 = rest.split('%').next()?.trim().parse().ok()?;
            let reset = rest.split_once("resets ").map(|(_, r)| r.trim().to_string());
            let name = name.trim_start_matches("Current ").trim();
            let name = if name.starts_with("session") {
                "5h".to_string()
            } else if let Some(rest) = name.strip_prefix("week") {
                let rest = rest.trim().trim_start_matches('(').trim_end_matches(')');
                if rest.is_empty() || rest == "all models" { "Week".to_string() } else { format!("Week · {rest}") }
            } else {
                name.to_string()
            };
            Some(QuotaItem { name, remaining: Some((100.0 - pct).clamp(0.0, 100.0).round() as u8), reset })
        })
        .collect()
}

/// Dòng `Gemini Models  five_hour  80%  2026-11-23T10:00:00Z`.
pub(crate) fn parse_agy_quota(text: &str) -> Vec<QuotaItem> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.to_lowercase().starts_with("quota"))
        .filter_map(|line| {
            let parts: Vec<&str> = line
                .split('\t')
                .flat_map(|p| p.split("  "))
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect();
            if parts.len() < 3 {
                return None;
            }
            let pct: Option<f32> = parts[2].split('%').next().and_then(|p| p.trim().parse().ok());
            pct?;
            let limit = parts[1].to_lowercase();
            let window = if limit.contains("week") {
                "Week"
            } else if limit.contains("five") || limit.contains('5') || limit.contains("hour") {
                "5h"
            } else {
                parts[1]
            };
            Some(QuotaItem {
                name: format!("{} · {window}", parts[0]),
                remaining: pct.map(|p| p.clamp(0.0, 100.0).round() as u8),
                reset: parts.get(3).map(|r| r.to_string()),
            })
        })
        .collect()
}

/// Header `anthropic-ratelimit-unified-{5h,7d}-utilization` (0..1) và `-reset` (giây Unix).
pub(crate) fn parse_claude_headers(headers: &str) -> Vec<QuotaItem> {
    let get = |name: &str| {
        headers.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    };
    [("5h", "5h"), ("7d", "Week")]
        .iter()
        .filter_map(|(window, label)| {
            let util: f32 = get(&format!("anthropic-ratelimit-unified-{window}-utilization"))?.parse().ok()?;
            let reset = get(&format!("anthropic-ratelimit-unified-{window}-reset")).map(|r| format!("@{r}"));
            Some(QuotaItem {
                name: label.to_string(),
                remaining: Some((100.0 - util * 100.0).clamp(0.0, 100.0).round() as u8),
                reset,
            })
        })
        .collect()
}

fn claude_quota_from_headers(token: &Secret) -> Result<Vec<QuotaItem>, String> {
    let body = serde_json::json!({
        "model": PROBE_MODEL,
        "max_tokens": 1,
        // Token OAuth của Claude Code chỉ nhận request mang system prompt của Claude Code.
        "system": [{ "type": "text", "text": "You are Claude Code, Anthropic's official CLI for Claude." }],
        "messages": [{ "role": "user", "content": "hi" }],
    })
    .to_string();
    // Cấu hình curl qua stdin để token không nằm trên dòng lệnh (ps thấy được).
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let config = format!(
        "url = \"https://api.anthropic.com/v1/messages\"\nheader = \"Authorization: Bearer {}\"\nheader = \"anthropic-beta: oauth-2025-04-20\"\nheader = \"anthropic-version: 2023-06-01\"\nheader = \"content-type: application/json\"\ndata = \"{}\"\n",
        quote(token.expose()),
        quote(&body),
    );
    let mut child = Command::new("curl")
        .args(["-sS", "--max-time", "30", "-D", "-", "-o", "/dev/null", "--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(config.as_bytes()).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(parse_claude_headers(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_usage_lines() {
        let q = parse_claude_usage(
            "Usage\n  Current session: 12% used · resets 5:09pm (Asia/Ho_Chi_Minh)\n  Current week (all models): 40% used · resets Nov 28\n  Current week (Sonnet only): 3% used\n",
        );
        assert_eq!(q.len(), 3);
        assert_eq!(q[0], QuotaItem { name: "5h".into(), remaining: Some(88), reset: Some("5:09pm (Asia/Ho_Chi_Minh)".into()) });
        assert_eq!(q[1].name, "Week");
        assert_eq!(q[2].name, "Week · Sonnet only");
        assert_eq!(q[2].reset, None);
    }

    #[test]
    fn agy_quota_lines() {
        let q = parse_agy_quota("Quota\nGemini Models\tfive_hour\t80%\t2026-11-23T10:00:00Z\nClaude/GPT  weekly  35%  2026-11-28T00:00:00Z\nbad line\n");
        assert_eq!(q.len(), 2);
        assert_eq!(q[0], QuotaItem { name: "Gemini Models · 5h".into(), remaining: Some(80), reset: Some("2026-11-23T10:00:00Z".into()) });
        assert_eq!(q[1].name, "Claude/GPT · Week");
        assert_eq!(q[1].remaining, Some(35));
    }

    #[test]
    fn claude_rate_limit_headers() {
        let q = parse_claude_headers(
            "HTTP/2 200\r\nanthropic-ratelimit-unified-5h-utilization: 0.25\r\nAnthropic-Ratelimit-Unified-5h-Reset: 1790000000\r\nanthropic-ratelimit-unified-7d-utilization: 0.6\r\n",
        );
        assert_eq!(q, vec![
            QuotaItem { name: "5h".into(), remaining: Some(75), reset: Some("@1790000000".into()) },
            QuotaItem { name: "Week".into(), remaining: Some(40), reset: None },
        ]);
    }
}
