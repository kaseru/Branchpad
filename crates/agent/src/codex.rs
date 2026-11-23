//! Codex: `codex exec --json`.
//!
//! `thread.started` có `thread_id`; `item.started` / `item.completed` mang một mục
//! (`agent_message`, `reasoning`, `command_execution`, `file_change`, `mcp_tool_call`, `web_search`...);
//! `turn.completed` / `turn.failed` kết thúc lượt.

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use crate::{first_line, last_line, AgentEvent, LineParser, RunOptions, ToolKind};

/// Lệnh chạy Codex (prompt đưa qua stdin bằng `-`).
/// Sandbox `workspace-write`: chỉ ghi trong thư mục repo, chặn mạng. Codex ở chế độ `exec` không hỏi duyệt.
pub(crate) fn command(opts: &RunOptions) -> Command {
    let mut cmd = Command::new(&opts.binary);
    cmd.arg("exec");
    if opts.resume.is_some() {
        cmd.arg("resume");
    }
    cmd.args(["--json", "-c", "sandbox_mode=\"workspace-write\""]);
    if let Some(m) = opts.model.as_deref().filter(|m| !m.trim().is_empty()) {
        cmd.args(["-m", m.trim()]);
    }
    if let Some(e) = opts.effort.as_deref().filter(|e| !e.is_empty()) {
        cmd.arg("-c").arg(format!("model_reasoning_effort=\"{e}\""));
    }
    for image in &opts.images {
        cmd.arg("-i").arg(image);
    }
    match &opts.resume {
        Some(id) => {
            cmd.arg(id);
        }
        None => {
            cmd.arg("-C").arg(&opts.workdir);
        }
    }
    cmd.arg("-");
    cmd
}

#[derive(Default)]
pub struct CodexParser {
    /// Mục công cụ đã báo `ToolStart` (mục chỉ có `item.completed` thì báo bù).
    started: HashSet<String>,
    /// Câu trả lời cuối, dùng làm tóm tắt khi hết lượt.
    last_message: String,
}

impl CodexParser {
    /// Công cụ của một mục: (loại, tên, chi tiết). `None` nếu mục không phải công cụ.
    fn tool(item: &Value) -> Option<(ToolKind, String, String)> {
        let s = |k: &str| item.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        match item.get("type").and_then(Value::as_str)? {
            "command_execution" => Some((ToolKind::Command, "shell".into(), first_line(&s("command"), 200))),
            "file_change" => {
                let files: Vec<String> = Self::changes(item).into_iter().map(|(p, _)| p.display().to_string()).collect();
                Some((ToolKind::Edit, "edit".into(), first_line(&files.join(", "), 200)))
            }
            "mcp_tool_call" => Some((ToolKind::Other, format!("{}.{}", s("server"), s("tool")), String::new())),
            "web_search" => Some((ToolKind::Web, "web_search".into(), first_line(&s("query"), 200))),
            _ => None,
        }
    }

    fn changes(item: &Value) -> Vec<(PathBuf, String)> {
        item.get("changes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|c| {
                let path = c.get("path").and_then(Value::as_str)?;
                let kind = c.get("kind").and_then(Value::as_str).unwrap_or("update");
                Some((PathBuf::from(path), kind.to_string()))
            })
            .collect()
    }

    fn start_tool(&mut self, id: &str, item: &Value, out: &mut Vec<AgentEvent>) {
        if let Some((kind, name, detail)) = Self::tool(item) {
            if self.started.insert(id.to_string()) {
                out.push(AgentEvent::ToolStart { id: id.to_string(), kind, name, detail });
            }
        }
    }

    fn completed(&mut self, id: &str, item: &Value, out: &mut Vec<AgentEvent>) {
        let status = item.get("status").and_then(Value::as_str).unwrap_or("completed");
        match item.get("type").and_then(Value::as_str) {
            Some("agent_message") => {
                if let Some(t) = item.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                    self.last_message = t.to_string();
                    out.push(AgentEvent::Text(t.to_string()));
                }
            }
            Some("reasoning") => {
                if let Some(t) = item.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                    out.push(AgentEvent::Thinking(t.to_string()));
                }
            }
            Some("error") => {
                let msg = item.get("message").and_then(Value::as_str).unwrap_or("error");
                out.push(AgentEvent::Error(msg.to_string()));
            }
            Some("command_execution") => {
                self.start_tool(id, item, out);
                let code = item.get("exit_code").and_then(Value::as_i64);
                let ok = status != "failed" && code.map_or(true, |c| c == 0);
                let output = item.get("aggregated_output").and_then(Value::as_str).unwrap_or("");
                let summary = match code {
                    Some(c) if c != 0 => format!("exit {c}: {}", last_line(output, 180)),
                    _ => last_line(output, 200),
                };
                out.push(AgentEvent::ToolDone { id: id.to_string(), ok, summary });
            }
            Some("file_change") => {
                self.start_tool(id, item, out);
                let ok = status != "failed";
                let changes = Self::changes(item);
                let summary = changes
                    .iter()
                    .map(|(p, k)| format!("{k} {}", p.display()))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push(AgentEvent::ToolDone { id: id.to_string(), ok, summary: first_line(&summary, 200) });
                if ok {
                    out.extend(changes.into_iter().map(|(p, _)| AgentEvent::FileChanged(p)));
                }
            }
            Some("mcp_tool_call") | Some("web_search") => {
                self.start_tool(id, item, out);
                out.push(AgentEvent::ToolDone { id: id.to_string(), ok: status != "failed", summary: String::new() });
            }
            _ => {}
        }
    }
}

impl LineParser for CodexParser {
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match v.get("type").and_then(Value::as_str) {
            Some("thread.started") => {
                if let Some(id) = v.get("thread_id").and_then(Value::as_str) {
                    out.push(AgentEvent::Started { session_id: id.to_string() });
                }
            }
            Some("item.started") => {
                if let Some(item) = v.get("item") {
                    let id = item.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                    self.start_tool(&id, item, &mut out);
                }
            }
            Some("item.completed") => {
                if let Some(item) = v.get("item") {
                    let id = item.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                    self.completed(&id, item, &mut out);
                }
            }
            Some("turn.completed") => {
                let tok = |k: &str| v.pointer(&format!("/usage/{k}")).and_then(Value::as_u64);
                let tokens = match (tok("input_tokens"), tok("output_tokens")) {
                    (None, None) => None,
                    (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
                };
                out.push(AgentEvent::Finished {
                    ok: true,
                    summary: std::mem::take(&mut self.last_message),
                    cost_usd: None,
                    tokens,
                });
            }
            Some("turn.failed") => {
                let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("turn failed");
                out.push(AgentEvent::Finished { ok: false, summary: msg.to_string(), cost_usd: None, tokens: None });
            }
            Some("error") => {
                let msg = v.get("message").and_then(Value::as_str).unwrap_or("error");
                out.push(AgentEvent::Error(msg.to_string()));
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
        let mut p = CodexParser::default();
        text.lines().flat_map(|l| p.parse_line(l)).collect()
    }

    #[test]
    fn edit_session() {
        let ev = parse_all(include_str!("../tests/fixtures/codex_edit.jsonl"));
        assert_eq!(ev[0], AgentEvent::Started { session_id: "0199a213-81c0-7800-8aa1-bbab2a035a53".into() });
        assert!(matches!(&ev[1], AgentEvent::Thinking(_)));
        assert!(matches!(&ev[2], AgentEvent::ToolStart { kind: ToolKind::Command, detail, .. } if detail.contains("cat README.md")));
        assert!(matches!(&ev[3], AgentEvent::ToolDone { ok: true, summary, .. } if summary == "Helo world"));
        // file_change chỉ có item.completed: báo bù ToolStart.
        assert!(matches!(&ev[4], AgentEvent::ToolStart { kind: ToolKind::Edit, .. }));
        assert!(matches!(&ev[5], AgentEvent::ToolDone { ok: true, .. }));
        assert_eq!(ev[6], AgentEvent::FileChanged(PathBuf::from("/tmp/demo/README.md")));
        assert_eq!(ev[7], AgentEvent::FileChanged(PathBuf::from("docs/new.md")));
        assert!(matches!(&ev[8], AgentEvent::ToolStart { kind: ToolKind::Command, .. }));
        assert!(matches!(&ev[9], AgentEvent::ToolDone { ok: false, summary, .. } if summary.starts_with("exit 101")));
        assert!(matches!(&ev[10], AgentEvent::ToolStart { name, .. } if name == "docs.search"));
        assert!(matches!(&ev[11], AgentEvent::ToolDone { ok: true, .. }));
        assert!(matches!(&ev[12], AgentEvent::Text(_)));
        assert!(matches!(&ev[13], AgentEvent::Finished { ok: true, summary, tokens: Some(24885), .. } if summary.contains("README")));
        assert_eq!(ev.len(), 14);
    }

    #[test]
    fn failed_turn() {
        let ev = parse_all(include_str!("../tests/fixtures/codex_error.jsonl"));
        assert!(matches!(&ev[1], AgentEvent::Error(m) if m.contains("disconnected")));
        assert!(matches!(&ev[2], AgentEvent::Finished { ok: false, summary, .. } if summary.contains("usage limit")));
    }

    #[test]
    fn resume_command_has_id_before_stdin_marker() {
        let opts = RunOptions {
            kind: crate::CliKind::Codex,
            binary: "/bin/codex".into(),
            workdir: "/tmp".into(),
            prompt: "hi".into(),
            resume: Some("abc".into()),
            model: None,
            effort: None,
            allow_shell: false,
            ask_permissions: false,
            images: Vec::new(),
            oauth_token: None,
        };
        let cmd = command(&opts);
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(&args[..2], ["exec", "resume"]);
        assert_eq!(&args[args.len() - 2..], ["abc", "-"]);
    }
}
