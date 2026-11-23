//! Tab "AI" ở cột trái: giao việc cho Claude Code / Codex CLI chạy không tương tác trong repo,
//! hiện tiến trình theo sự kiện JSON của CLI; file AI sửa hiện ở tab Thay đổi nhờ watcher.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use branchpad_agent::{find_cli, AgentEvent, AgentRun, CliKind, RunOptions, ToolKind};
use futures::StreamExt;
use gpui::{div, prelude::*, px, AnyElement, Context, Entity, ScrollHandle, Window};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    v_flex, ActiveTheme, Disableable, Icon, IconName, Sizable,
};

use super::{RepoView, SidebarTab};
use crate::icons::{file_icon, BpIcon};
use crate::ui::{list_row, split_path};

/// Cài đặt AI (từ màn Cài đặt).
#[derive(Clone, Debug, PartialEq)]
pub struct AgentConfig {
    /// CLI chọn sẵn; `None` = cái nào tìm thấy trước.
    pub cli: Option<CliKind>,
    pub claude_path: Option<PathBuf>,
    pub codex_path: Option<PathBuf>,
    pub claude_model: Option<String>,
    pub codex_model: Option<String>,
    /// Mặc định chạy trong worktree mới.
    pub use_worktree: bool,
    /// Cho Claude chạy lệnh shell không hỏi.
    pub allow_shell: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            cli: None,
            claude_path: None,
            codex_path: None,
            claude_model: None,
            codex_model: None,
            use_worktree: true,
            allow_shell: false,
        }
    }
}

impl AgentConfig {
    pub fn path_override(&self, kind: CliKind) -> Option<&Path> {
        match kind {
            CliKind::Claude => self.claude_path.as_deref(),
            CliKind::Codex => self.codex_path.as_deref(),
        }
    }

    pub fn model(&self, kind: CliKind) -> Option<String> {
        match kind {
            CliKind::Claude => self.claude_model.clone(),
            CliKind::Codex => self.codex_model.clone(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolState {
    Running,
    Ok,
    Failed,
}

enum AgentItem {
    /// Yêu cầu người dùng gửi.
    User(String),
    Text(String),
    Thinking(String),
    Tool { id: String, kind: ToolKind, name: String, detail: String, state: ToolState, summary: String },
    /// File AI đã sửa (đường dẫn tương đối với repo).
    File(String),
    Warning(String),
    Error(String),
}

struct FinishInfo {
    ok: bool,
    cost_usd: Option<f64>,
    tokens: Option<u64>,
}

pub(crate) struct AgentPanel {
    pub(crate) input: Entity<InputState>,
    config: AgentConfig,
    cli: CliKind,
    /// CLI tìm thấy trên máy (dò ở luồng nền).
    claude_bin: Option<PathBuf>,
    codex_bin: Option<PathBuf>,
    detected: bool,
    items: Vec<AgentItem>,
    run: Option<AgentRun>,
    /// Đếm lần chạy: bỏ qua sự kiện của lần chạy cũ.
    run_gen: u64,
    /// Phiên của lần chạy trước (để hỏi tiếp), chỉ dùng với cùng CLI.
    session: Option<(CliKind, String)>,
    /// Thư mục CLI đang chạy (để đổi đường dẫn tương đối của Codex).
    run_dir: PathBuf,
    /// File đã sửa trong phiên (đường dẫn tuyệt đối).
    changed: BTreeSet<PathBuf>,
    finished: Option<FinishInfo>,
    scroll: ScrollHandle,
}

impl AgentPanel {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<RepoView>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line()
                .placeholder(t!("agent.input.placeholder"))
        });
        Self {
            input,
            config: AgentConfig::default(),
            cli: CliKind::Claude,
            claude_bin: None,
            codex_bin: None,
            detected: false,
            items: Vec::new(),
            run: None,
            run_gen: 0,
            session: None,
            run_dir: PathBuf::new(),
            changed: BTreeSet::new(),
            finished: None,
            scroll: ScrollHandle::new(),
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        self.run.is_some()
    }

    fn binary(&self, kind: CliKind) -> Option<&PathBuf> {
        match kind {
            CliKind::Claude => self.claude_bin.as_ref(),
            CliKind::Codex => self.codex_bin.as_ref(),
        }
    }

    fn push(&mut self, item: AgentItem) {
        self.items.push(item);
        self.scroll.scroll_to_item(self.items.len() - 1);
    }
}

impl RepoView {
    /// Áp cài đặt AI và dò lại CLI.
    pub fn set_agent_config(&mut self, config: AgentConfig, cx: &mut Context<Self>) {
        self.agent.config = config;
        self.detect_agent_clis(cx);
    }

    /// Dò file chạy của các CLI ở luồng nền (kiểm tra vài đường dẫn trên đĩa).
    pub(crate) fn detect_agent_clis(&mut self, cx: &mut Context<Self>) {
        let config = self.agent.config.clone();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    (
                        find_cli(CliKind::Claude, config.path_override(CliKind::Claude)),
                        find_cli(CliKind::Codex, config.path_override(CliKind::Codex)),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                let a = &mut this.agent;
                (a.claude_bin, a.codex_bin) = found;
                a.detected = true;
                if !a.is_running() {
                    a.cli = match a.config.cli {
                        Some(k) => k,
                        None if a.claude_bin.is_none() && a.codex_bin.is_some() => CliKind::Codex,
                        None => CliKind::Claude,
                    };
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn select_agent_cli(&mut self, kind: CliKind, cx: &mut Context<Self>) {
        if !self.agent.is_running() {
            self.agent.cli = kind;
            cx.notify();
        }
    }

    /// Nút Chạy / Hỏi tiếp, hoặc Cmd+Enter trong ô nhập.
    pub(crate) fn agent_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agent.is_running() {
            return;
        }
        let prompt = self.agent.input.read(cx).value().trim().to_string();
        if prompt.is_empty() {
            return;
        }
        let kind = self.agent.cli;
        let Some(binary) = self.agent.binary(kind).cloned() else {
            self.agent.push(AgentItem::Error(t!("agent.error.not_found", cli = kind.label())));
            cx.notify();
            return;
        };
        let resume = match &self.agent.session {
            Some((k, id)) if *k == kind => Some(id.clone()),
            _ => None,
        };
        let workdir = self.repo.workdir().to_path_buf();
        self.agent.input.update(cx, |s, cx| s.set_value("", window, cx));
        self.start_agent(kind, binary, workdir, prompt, resume, cx);
    }

    /// Chạy CLI trong `workdir`; sự kiện đi qua kênh async về luồng giao diện (không polling).
    pub(crate) fn start_agent(
        &mut self,
        kind: CliKind,
        binary: PathBuf,
        workdir: PathBuf,
        prompt: String,
        resume: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let a = &mut self.agent;
        if resume.is_none() {
            a.items.clear();
            a.changed.clear();
            a.session = None;
        }
        a.finished = None;
        a.run_gen += 1;
        a.run_dir = workdir.clone();
        a.push(AgentItem::User(prompt.clone()));
        let opts = RunOptions {
            kind,
            binary,
            workdir,
            prompt,
            resume,
            model: a.config.model(kind),
            allow_shell: a.config.allow_shell,
        };
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<AgentEvent>();
        match AgentRun::start(opts, move |ev| {
            // Panel đã đóng thì bỏ sự kiện.
            let _ = tx.unbounded_send(ev);
        }) {
            Ok(run) => a.run = Some(run),
            Err(e) => {
                a.push(AgentItem::Error(t!("agent.error.start", cli = kind.label(), e = e)));
                cx.notify();
                return;
            }
        }
        let gen = a.run_gen;
        cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.next().await {
                if this.update(cx, |this, cx| this.on_agent_event(gen, kind, ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn stop_agent(&mut self, cx: &mut Context<Self>) {
        if let Some(run) = &self.agent.run {
            run.cancel();
            self.agent.push(AgentItem::Warning(t!("agent.stopping").to_string()));
            cx.notify();
        }
    }

    fn new_agent_session(&mut self, cx: &mut Context<Self>) {
        if self.agent.is_running() {
            return;
        }
        let a = &mut self.agent;
        a.items.clear();
        a.changed.clear();
        a.session = None;
        a.finished = None;
        cx.notify();
    }

    fn on_agent_event(&mut self, gen: u64, kind: CliKind, ev: AgentEvent, cx: &mut Context<Self>) {
        if gen != self.agent.run_gen {
            return;
        }
        match ev {
            AgentEvent::Started { session_id } => self.agent.session = Some((kind, session_id)),
            AgentEvent::Text(t) => self.agent.push(AgentItem::Text(t)),
            AgentEvent::Thinking(t) => self.agent.push(AgentItem::Thinking(t)),
            AgentEvent::ToolStart { id, kind, name, detail } => {
                let detail = self.rel_display(&detail);
                self.agent.push(AgentItem::Tool { id, kind, name, detail, state: ToolState::Running, summary: String::new() });
            }
            AgentEvent::ToolDone { id, ok, summary } => {
                let found = self.agent.items.iter_mut().rev().find_map(|item| match item {
                    AgentItem::Tool { id: tid, state, summary: s, .. } if *tid == id => Some((state, s)),
                    _ => None,
                });
                if let Some((state, s)) = found {
                    *state = if ok { ToolState::Ok } else { ToolState::Failed };
                    *s = summary;
                }
            }
            AgentEvent::FileChanged(path) => {
                let abs = if path.is_absolute() { path } else { self.agent.run_dir.join(path) };
                if self.agent.changed.insert(abs.clone()) {
                    let rel = self.rel_display(&abs.to_string_lossy());
                    self.agent.push(AgentItem::File(rel.clone()));
                    if self.editor.read(cx).is_dirty(&abs) {
                        self.agent.push(AgentItem::Warning(t!("agent.warning.unsaved", file = rel)));
                    }
                }
            }
            AgentEvent::Error(e) => self.agent.push(AgentItem::Error(e)),
            AgentEvent::Finished { ok, summary, cost_usd, tokens } => {
                let a = &mut self.agent;
                a.run = None;
                a.finished = Some(FinishInfo { ok, cost_usd, tokens });
                let n = a.changed.len();
                if ok {
                    let msg = summary.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string();
                    self.info = Some(if n > 0 { t!("agent.done.files", n = n, msg = msg) } else { t!("agent.done.no_files", msg = msg) });
                } else if !summary.trim().is_empty() {
                    self.agent.push(AgentItem::Error(summary));
                }
                self.refresh(true, None, cx);
                if n > 0 && ok {
                    self.sidebar = SidebarTab::Changes;
                }
            }
        }
        cx.notify();
    }

    /// Đường dẫn trong thư mục chạy thì hiện dạng tương đối.
    fn rel_display(&self, text: &str) -> String {
        let p = Path::new(text);
        for base in [self.agent.run_dir.as_path(), self.repo.workdir()] {
            if let Ok(rel) = p.strip_prefix(base) {
                if !rel.as_os_str().is_empty() {
                    return rel.to_string_lossy().into_owned();
                }
            }
        }
        text.to_string()
    }

    // ---- Vẽ ----

    pub(super) fn render_agent(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let running = self.agent.is_running();
        let kind = self.agent.cli;
        let t = cx.theme();
        let (border, muted) = (t.border, t.muted_foreground);

        let cli_row = h_flex()
            .p_1()
            .gap_1()
            .flex_none()
            .border_b_1()
            .border_color(border)
            .children(CliKind::ALL.into_iter().map(|k| {
                let selected = k == kind;
                Button::new(("agent-cli", k as usize))
                    .label(k.label())
                    .xsmall()
                    .when(selected, |b| b.primary())
                    .when(!selected, |b| b.ghost())
                    .disabled(running)
                    .on_click(cx.listener(move |this, _, _, cx| this.select_agent_cli(k, cx)))
            }));

        let missing = self.agent.detected && self.agent.binary(kind).is_none();
        let body: AnyElement = if missing {
            v_flex()
                .flex_1()
                .p_3()
                .gap_2()
                .text_sm()
                .child(div().text_color(t.foreground).child(t!("agent.missing.title", cli = kind.label())))
                .child(div().text_xs().text_color(muted).child(t!("agent.missing.hint")))
                .child(
                    div()
                        .p_2()
                        .rounded_md()
                        .bg(t.secondary)
                        .text_xs()
                        .font_family(crate::ui::MONO)
                        .whitespace_normal()
                        .child(kind.install_hint()),
                )
                .child(
                    Button::new("agent-open-settings")
                        .label(t!("agent.missing.open_settings"))
                        .xsmall()
                        .ghost()
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::actions::OpenSettings), cx)),
                )
                .into_any_element()
        } else if self.agent.items.is_empty() {
            v_flex()
                .flex_1()
                .p_3()
                .gap_2()
                .text_sm()
                .text_color(muted)
                .child(t!("agent.empty.title"))
                .child(div().text_xs().child(t!("agent.empty.hint")))
                .into_any_element()
        } else {
            let items: Vec<AnyElement> = self
                .agent
                .items
                .iter()
                .enumerate()
                .map(|(i, item)| self.render_agent_item(i, item, cx))
                .collect();
            v_flex()
                .id("agent-timeline")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.agent.scroll)
                .p_2()
                .gap_1()
                .children(items)
                .into_any_element()
        };

        let status = self.agent.finished.as_ref().map(|f| {
            let mut parts = vec![if f.ok { t!("agent.status.done").to_string() } else { t!("agent.status.failed").to_string() }];
            if let Some(c) = f.cost_usd {
                parts.push(format!("${c:.2}"));
            }
            if let Some(n) = f.tokens {
                parts.push(t!("agent.status.tokens", n = n));
            }
            let n = self.agent.changed.len();
            if n > 0 {
                parts.push(t!("agent.status.files", n = n));
            }
            h_flex()
                .px_2()
                .py_1()
                .gap_1()
                .flex_none()
                .items_center()
                .border_t_1()
                .border_color(border)
                .text_xs()
                .text_color(if f.ok { t.success } else { t.danger })
                .child(Icon::new(if f.ok { IconName::CircleCheck } else { IconName::CircleX }).size_3())
                .child(div().flex_1().truncate().child(parts.join(" · ")))
        });

        let can_resume = matches!(&self.agent.session, Some((k, _)) if *k == kind);
        let has_session = !self.agent.items.is_empty();
        let input_box = v_flex()
            .p_2()
            .gap_2()
            .flex_none()
            .border_t_1()
            .border_color(border)
            .child(div().h(px(84.)).child(Input::new(&self.agent.input).h_full()))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("agent-new")
                            .label(t!("agent.button.new"))
                            .xsmall()
                            .ghost()
                            .disabled(running || !has_session)
                            .on_click(cx.listener(|this, _, _, cx| this.new_agent_session(cx))),
                    )
                    .child(div().flex_1())
                    .when(running, |d| {
                        d.child(
                            Button::new("agent-stop")
                                .label(t!("agent.button.stop"))
                                .small()
                                .danger()
                                .on_click(cx.listener(|this, _, _, cx| this.stop_agent(cx))),
                        )
                    })
                    .when(!running, |d| {
                        d.child(
                            Button::new("agent-run")
                                .label(if can_resume { t!("agent.button.continue") } else { t!("agent.button.run") })
                                .small()
                                .primary()
                                .disabled(missing || !self.agent.detected)
                                .tooltip("Cmd+Enter")
                                .on_click(cx.listener(|this, _, window, cx| this.agent_submit(window, cx))),
                        )
                    }),
            );

        v_flex()
            .size_full()
            .child(cli_row)
            .child(body)
            .children(status)
            .child(input_box)
            .into_any_element()
    }

    fn render_agent_item(&self, i: usize, item: &AgentItem, cx: &Context<Self>) -> AnyElement {
        let t = cx.theme();
        let text = |s: &str| div().text_sm().text_color(t.foreground).whitespace_normal().child(s.to_string());
        match item {
            AgentItem::User(s) => div()
                .p_2()
                .rounded_md()
                .bg(t.primary.opacity(0.12))
                .child(text(s))
                .into_any_element(),
            AgentItem::Text(s) => text(s).into_any_element(),
            AgentItem::Thinking(s) => div()
                .text_xs()
                .italic()
                .text_color(t.muted_foreground)
                .whitespace_normal()
                .child(branchpad_short(s, 400))
                .into_any_element(),
            AgentItem::Tool { kind, name, detail, state, summary, .. } => {
                let (state_icon, state_color) = match state {
                    ToolState::Running => (IconName::Loader, t.muted_foreground),
                    ToolState::Ok => (IconName::CircleCheck, t.success),
                    ToolState::Failed => (IconName::CircleX, t.danger),
                };
                h_flex()
                    .gap_1()
                    .items_start()
                    .text_xs()
                    .child(Icon::new(state_icon).size_3().text_color(state_color).flex_none().mt(px(2.)))
                    .child(Icon::new(tool_icon(*kind)).size_3().text_color(t.muted_foreground).flex_none().mt(px(2.)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(div().flex_none().font_weight(gpui::FontWeight::MEDIUM).child(name.clone()))
                                    .child(div().flex_1().truncate().text_color(t.muted_foreground).child(detail.clone())),
                            )
                            .when(!summary.is_empty(), |d| {
                                d.child(div().truncate().text_color(state_color).child(summary.clone()))
                            }),
                    )
                    .into_any_element()
            }
            AgentItem::File(rel) => {
                let (name, dir) = split_path(rel);
                let (icon, color) = file_icon(&name, false, false);
                let rel = rel.clone();
                list_row(("agent-file", i), false, cx)
                    .child(Icon::new(icon).size_4().text_color(color))
                    .child(div().flex_none().child(name))
                    .child(div().flex_1().truncate().text_xs().text_color(t.muted_foreground).child(dir))
                    .on_click(cx.listener(move |this, _, _, cx| this.show_file_changes(&rel, cx)))
                    .into_any_element()
            }
            AgentItem::Warning(s) => div()
                .text_xs()
                .text_color(t.warning)
                .whitespace_normal()
                .child(s.clone())
                .into_any_element(),
            AgentItem::Error(s) => div()
                .p_2()
                .rounded_md()
                .bg(t.danger.opacity(0.12))
                .text_xs()
                .text_color(t.danger)
                .whitespace_normal()
                .child(s.clone())
                .into_any_element(),
        }
    }
}

fn tool_icon(kind: ToolKind) -> Icon {
    match kind {
        ToolKind::Read => IconName::Eye.into(),
        ToolKind::Edit => BpIcon::FileCode.into(),
        ToolKind::Command => BpIcon::FileTerminal.into(),
        ToolKind::Search | ToolKind::Web => IconName::Search.into(),
        ToolKind::Other => BpIcon::Settings.into(),
    }
}

/// Cắt chuỗi dài (suy nghĩ của AI) còn `max` ký tự.
fn branchpad_short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}
