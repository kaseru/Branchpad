//! Claude Code: `claude -p --output-format stream-json --verbose`.
//!
//! Dòng `system/init` có `session_id`; `assistant` chứa các khối `text` / `thinking` / `tool_use`;
//! `user` chứa `tool_result`; dòng `result` kết thúc lượt (kèm chi phí, token).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use crate::{first_line, truncate, AgentEvent, LineParser, RunOptions, ToolKind};

/// Lệnh chạy Claude Code (prompt đưa qua stdin).
pub(crate) fn command(opts: &RunOptions) -> Command {
    let mut cmd = Command::new(&opts.binary);
    cmd.args(["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "acceptEdits"]);
    if opts.allow_shell {
        cmd.args(["--allowedTools", "Bash"]);
    }
    if let Some(m) = opts.model.as_deref().filter(|m| !m.trim().is_empty()) {
        cmd.args(["--model", m.trim()]);
    }
    if let Some(e) = opts.effort.as_deref().filter(|e| !e.is_empty()) {
        cmd.args(["--effort", e]);
    }
    if let Some(id) = &opts.resume {
        cmd.args(["--resume", id]);
    }
    if let Some(token) = &opts.oauth_token {
        cmd.env(crate::CLAUDE_TOKEN_ENV, token.expose());
    }
    cmd
}

fn tool_kind(name: &str) -> ToolKind {
    match name {
        "Read" | "NotebookRead" | "LS" => ToolKind::Read,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => ToolKind::Edit,
        "Bash" | "BashOutput" | "KillShell" => ToolKind::Command,
        "Grep" | "Glob" => ToolKind::Search,
        "WebFetch" | "WebSearch" => ToolKind::Web,
        _ => ToolKind::Other,
    }
}

/// Phần đáng hiện của tham số công cụ: file, lệnh, mẫu tìm, URL...
fn tool_detail(input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str);
    let detail = s("file_path")
        .or_else(|| s("notebook_path"))
        .or_else(|| s("command"))
        .or_else(|| s("pattern"))
        .or_else(|| s("url"))
        .or_else(|| s("query"))
        .or_else(|| s("description"))
        .or_else(|| s("path"))
        .unwrap_or("");
    truncate(&first_line(detail, 200), 200)
}

/// Nội dung `tool_result`: chuỗi, hoặc mảng các khối `{type: text, text}`.
fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[derive(Default)]
pub struct ClaudeParser {
    /// Công cụ sửa file đang chạy: id → file. Báo `FileChanged` khi công cụ xong mà không lỗi.
    pending_edits: HashMap<String, PathBuf>,
}

impl LineParser for ClaudeParser {
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match v.get("type").and_then(Value::as_str) {
            Some("system") if v.get("subtype").and_then(Value::as_str) == Some("init") => {
                if let Some(id) = v.get("session_id").and_then(Value::as_str) {
                    out.push(AgentEvent::Started { session_id: id.to_string() });
                }
            }
            Some("assistant") => {
                let blocks = v.pointer("/message/content").and_then(Value::as_array);
                for b in blocks.into_iter().flatten() {
                    match b.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(t) = b.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                                out.push(AgentEvent::Text(t.to_string()));
                            }
                        }
                        Some("thinking") => {
                            if let Some(t) = b.get("thinking").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                                out.push(AgentEvent::Thinking(t.to_string()));
                            }
                        }
                        Some("tool_use") => {
                            let id = b.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                            let name = b.get("name").and_then(Value::as_str).unwrap_or("tool").to_string();
                            let input = b.get("input").cloned().unwrap_or(Value::Null);
                            let kind = tool_kind(&name);
                            if kind == ToolKind::Edit {
                                let file = input
                                    .get("file_path")
                                    .or_else(|| input.get("notebook_path"))
                                    .and_then(Value::as_str);
                                if let Some(f) = file {
                                    self.pending_edits.insert(id.clone(), PathBuf::from(f));
                                }
                            }
                            out.push(AgentEvent::ToolStart { id, kind, name, detail: tool_detail(&input) });
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                let blocks = v.pointer("/message/content").and_then(Value::as_array);
                for b in blocks.into_iter().flatten() {
                    if b.get("type").and_then(Value::as_str) != Some("tool_result") {
                        continue;
                    }
                    let id = b.get("tool_use_id").and_then(Value::as_str).unwrap_or_default().to_string();
                    let ok = !b.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                    let summary = first_line(&result_text(b.get("content")), 200);
                    let edited = self.pending_edits.remove(&id);
                    out.push(AgentEvent::ToolDone { id, ok, summary });
                    if let (true, Some(path)) = (ok, edited) {
                        out.push(AgentEvent::FileChanged(path));
                    }
                }
            }
            Some("result") => {
                let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("");
                let ok = !is_error && subtype == "success";
                let summary = v
                    .get("result")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        let errs = v.get("errors").and_then(Value::as_array)?;
                        let text: Vec<&str> = errs.iter().filter_map(Value::as_str).collect();
                        (!text.is_empty()).then(|| text.join("\n"))
                    })
                    .unwrap_or_else(|| subtype.to_string());
                let usage = v.get("usage");
                let tok = |k: &str| usage.and_then(|u| u.get(k)).and_then(Value::as_u64);
                let tokens = match (tok("input_tokens"), tok("output_tokens")) {
                    (None, None) => None,
                    (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
                };
                out.push(AgentEvent::Finished {
                    ok,
                    summary,
                    cost_usd: v.get("total_cost_usd").and_then(Value::as_f64),
                    tokens,
                });
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(text: &str) -> Vec<AgentEvent> {
        let mut p = ClaudeParser::default();
        text.lines().flat_map(|l| p.parse_line(l)).collect()
    }

    #[test]
    fn edit_session() {
        let ev = parse_all(include_str!("../tests/fixtures/claude_edit.jsonl"));
        assert_eq!(ev[0], AgentEvent::Started { session_id: "8f1c2d3e-0000-4000-8000-000000000001".into() });
        assert!(matches!(&ev[1], AgentEvent::Thinking(t) if t.starts_with("Need")));
        assert!(matches!(&ev[2], AgentEvent::Text(t) if t.contains("typo")));
        assert!(matches!(&ev[3], AgentEvent::ToolStart { kind: ToolKind::Read, detail, .. } if detail == "/tmp/demo/README.md"));
        assert!(matches!(&ev[4], AgentEvent::ToolDone { ok: true, .. }));
        // Read không phải sửa file: không có FileChanged.
        assert!(matches!(&ev[5], AgentEvent::ToolStart { kind: ToolKind::Edit, .. }));
        assert!(matches!(&ev[6], AgentEvent::ToolDone { ok: true, summary, .. } if summary.contains("updated")));
        assert_eq!(ev[7], AgentEvent::FileChanged(PathBuf::from("/tmp/demo/README.md")));
        assert!(matches!(&ev[8], AgentEvent::ToolStart { kind: ToolKind::Command, detail, .. } if detail == "cargo test"));
        assert!(matches!(&ev[9], AgentEvent::ToolDone { ok: false, .. }));
        assert!(matches!(&ev[10], AgentEvent::Text(_)));
        match &ev[11] {
            AgentEvent::Finished { ok, summary, cost_usd, tokens } => {
                assert!(*ok);
                assert!(summary.contains("Hello"));
                assert_eq!(*cost_usd, Some(0.0421));
                assert_eq!(*tokens, Some(1510));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(ev.len(), 12);
    }

    #[test]
    fn error_result() {
        let ev = parse_all(include_str!("../tests/fixtures/claude_error.jsonl"));
        assert!(matches!(&ev[1], AgentEvent::Finished { ok: false, summary, .. } if summary == "error_max_turns"));
    }

    #[test]
    fn token_goes_to_env_only() {
        let opts = RunOptions {
            kind: crate::CliKind::Claude,
            binary: "/bin/claude".into(),
            workdir: "/tmp".into(),
            prompt: "hi".into(),
            resume: None,
            model: None,
            effort: None,
            allow_shell: false,
            oauth_token: Some(crate::Secret::new("tok")),
        };
        let cmd = command(&opts);
        let env: Vec<_> = cmd.get_envs().collect();
        assert!(env.iter().any(|(k, v)| *k == crate::CLAUDE_TOKEN_ENV && v.is_some_and(|v| v == "tok")));
        assert!(!cmd.get_args().any(|a| a == "tok"));
    }

    #[test]
    fn garbage_is_ignored() {
        assert!(parse_all("not json\n{}\n{\"type\":42}\n").is_empty());
    }
}
