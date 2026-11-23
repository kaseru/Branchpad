//! Panel AI ở cột phải, cạnh trình soạn: giao việc cho Claude Code / Codex CLI chạy không tương tác
//! trong repo. Mỗi phiên là một tab; tiến trình hiện theo sự kiện JSON của CLI, file AI sửa hiện ở
//! tab Thay đổi nhờ watcher (không polling).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use branchpad_agent::{find_cli, AgentEvent, AgentRun, CliKind, RunOptions, Secret, ToolKind};
use futures::StreamExt;
use gpui::{div, hsla, prelude::*, px, AnyElement, App, Context, Entity, Hsla, ScrollHandle, SharedString, Window};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    v_flex, ActiveTheme, Disableable, Icon, IconName, Sizable,
};

use super::{RepoView, RepoViewEvent};
use crate::actions::ToggleAgent;
use crate::icons::{file_icon, BpIcon};
use crate::ui::split_path;

/// Cài đặt AI (từ màn Cài đặt).
#[derive(Clone, Debug, PartialEq)]
pub struct AgentConfig {
    /// CLI chọn sẵn; `None` = cái nào tìm thấy trước.
    pub cli: Option<CliKind>,
    pub claude_path: Option<PathBuf>,
    pub codex_path: Option<PathBuf>,
    pub claude_model: Option<String>,
    pub codex_model: Option<String>,
    /// Phiên mới chạy trong worktree mới (mặc định tắt: AI sửa thẳng thư mục đang làm).
    pub use_worktree: bool,
    /// Cho Claude chạy lệnh shell không hỏi.
    pub allow_shell: bool,
    /// Token `CLAUDE_CODE_OAUTH_TOKEN` lưu trong Keychain (nạp ở luồng nền khi mở app).
    pub claude_token: Option<Secret>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            cli: None,
            claude_path: None,
            codex_path: None,
            claude_model: None,
            codex_model: None,
            use_worktree: false,
            allow_shell: false,
            claude_token: None,
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

/// Icon đại diện cho từng agent.
pub(crate) fn agent_icon(kind: CliKind) -> Icon {
    match kind {
        CliKind::Claude => BpIcon::Claude.into(),
        CliKind::Codex => BpIcon::Codex.into(),
    }
}

/// Màu của icon agent: Claude màu cam đất, Codex theo màu chữ của giao diện.
pub(crate) fn agent_color(kind: CliKind, cx: &App) -> Hsla {
    match kind {
        CliKind::Claude => hsla(15. / 360., 0.63, 0.6, 1.),
        CliKind::Codex => cx.theme().foreground,
    }
}

/// Việc chờ tạo worktree xong mới chạy.
struct AgentRequest {
    kind: CliKind,
    binary: PathBuf,
    prompt: String,
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

/// Một phiên làm việc với AI (một tab trong panel).
struct AgentSession {
    /// Mã ổn định để gửi sự kiện đúng phiên (chỉ số trong danh sách đổi khi đóng tab).
    id: u64,
    cli: CliKind,
    title: String,
    items: Vec<AgentItem>,
    run: Option<AgentRun>,
    /// Đếm lần chạy trong phiên: bỏ qua sự kiện của lần chạy cũ.
    run_gen: u64,
    /// Mã phiên của CLI (để hỏi tiếp).
    cli_session: Option<String>,
    /// Thư mục CLI đang chạy (để đổi đường dẫn tương đối của Codex).
    run_dir: PathBuf,
    /// File đã sửa trong phiên (đường dẫn tuyệt đối).
    changed: BTreeSet<PathBuf>,
    finished: Option<FinishInfo>,
    scroll: ScrollHandle,
    /// Đang tạo worktree, CLI chưa chạy.
    preparing: bool,
}

impl AgentSession {
    fn new(id: u64, cli: CliKind) -> Self {
        Self {
            id,
            cli,
            title: String::new(),
            items: Vec::new(),
            run: None,
            run_gen: 0,
            cli_session: None,
            run_dir: PathBuf::new(),
            changed: BTreeSet::new(),
            finished: None,
            scroll: ScrollHandle::new(),
            preparing: false,
        }
    }

    fn is_running(&self) -> bool {
        self.run.is_some() || self.preparing
    }

    /// Phiên chưa gửi gì: còn đổi được CLI.
    fn is_blank(&self) -> bool {
        self.items.is_empty() && !self.is_running()
    }

    fn push(&mut self, item: AgentItem) {
        self.items.push(item);
        self.scroll.scroll_to_item(self.items.len() - 1);
    }
}

pub(crate) struct AgentPanel {
    pub(crate) input: Entity<InputState>,
    config: AgentConfig,
    /// CLI tìm thấy trên máy (dò ở luồng nền).
    claude_bin: Option<PathBuf>,
    codex_bin: Option<PathBuf>,
    detected: bool,
    /// Luôn có ít nhất một phiên.
    sessions: Vec<AgentSession>,
    active: usize,
    next_id: u64,
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
            claude_bin: None,
            codex_bin: None,
            detected: false,
            sessions: vec![AgentSession::new(0, CliKind::Claude)],
            active: 0,
            next_id: 1,
        }
    }

    /// Có phiên nào đang chạy.
    pub(crate) fn is_running(&self) -> bool {
        self.sessions.iter().any(AgentSession::is_running)
    }

    fn binary(&self, kind: CliKind) -> Option<&PathBuf> {
        match kind {
            CliKind::Claude => self.claude_bin.as_ref(),
            CliKind::Codex => self.codex_bin.as_ref(),
        }
    }

    /// CLI cho phiên mới: theo cài đặt, không thì cái nào có trên máy.
    fn default_cli(&self) -> CliKind {
        match self.config.cli {
            Some(k) => k,
            None if self.claude_bin.is_none() && self.codex_bin.is_some() => CliKind::Codex,
            None => CliKind::Claude,
        }
    }

    fn current(&self) -> &AgentSession {
        &self.sessions[self.active]
    }

    fn current_mut(&mut self) -> &mut AgentSession {
        &mut self.sessions[self.active]
    }

    fn session_mut(&mut self, id: u64) -> Option<&mut AgentSession> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    /// Thêm phiên mới và chọn nó.
    fn add_session(&mut self, cli: CliKind) {
        let id = self.next_id;
        self.next_id += 1;
        self.sessions.push(AgentSession::new(id, cli));
        self.active = self.sessions.len() - 1;
    }
}

/// Bảng màu lấy một lần từ theme để vẽ (không giữ mượn `cx`).
#[derive(Clone, Copy)]
struct Pal {
    fg: Hsla,
    muted: Hsla,
    border: Hsla,
    primary: Hsla,
    success: Hsla,
    danger: Hsla,
    warning: Hsla,
    secondary: Hsla,
    bg: Hsla,
    tab_bar: Hsla,
    tab_active: Hsla,
    hover: Hsla,
}

impl Pal {
    fn of(cx: &App) -> Self {
        let t = cx.theme();
        Pal {
            fg: t.foreground,
            muted: t.muted_foreground,
            border: t.border,
            primary: t.primary,
            success: t.success,
            danger: t.danger,
            warning: t.warning,
            secondary: t.secondary,
            bg: t.background,
            tab_bar: t.tab_bar,
            tab_active: t.tab_active,
            hover: t.list_hover,
        }
    }
}

impl RepoView {
    /// Áp cài đặt AI và dò lại CLI (Workspace gọi ngay sau khi tạo tab và khi đổi cài đặt).
    pub fn set_agent_config(&mut self, config: AgentConfig, cx: &mut Context<Self>) {
        self.agent.config = config;
        self.detect_agent_clis(cx);
    }

    /// Dò file chạy của các CLI ở luồng nền (kiểm tra vài đường dẫn trên đĩa).
    fn detect_agent_clis(&mut self, cx: &mut Context<Self>) {
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
                // Phiên chưa dùng thì theo CLI mặc định mới.
                let cli = a.default_cli();
                for s in a.sessions.iter_mut().filter(|s| s.is_blank()) {
                    s.cli = cli;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn select_agent_cli(&mut self, kind: CliKind, cx: &mut Context<Self>) {
        let s = self.agent.current_mut();
        if s.is_blank() {
            s.cli = kind;
            cx.notify();
        }
    }

    fn new_agent_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cli = self.agent.default_cli();
        // Phiên đang chọn còn trống thì dùng luôn, không đẻ thêm tab rỗng.
        if !self.agent.current().is_blank() {
            self.agent.add_session(cli);
        }
        self.agent.input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn select_agent_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.agent.sessions.len() {
            self.agent.active = i;
            cx.notify();
        }
    }

    /// Đóng phiên: đang chạy thì CLI bị dừng (drop `AgentRun`). Luôn giữ lại một phiên.
    fn close_agent_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        let a = &mut self.agent;
        if i >= a.sessions.len() {
            return;
        }
        a.sessions.remove(i);
        if a.sessions.is_empty() {
            let cli = a.default_cli();
            a.add_session(cli);
        }
        if a.active > i || a.active >= a.sessions.len() {
            a.active = a.active.saturating_sub(1);
        }
        cx.notify();
    }

    /// Nút Chạy / Hỏi tiếp, hoặc Cmd+Enter trong ô nhập.
    pub(crate) fn agent_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agent.current().is_running() {
            return;
        }
        let prompt = self.agent.input.read(cx).value().trim().to_string();
        if prompt.is_empty() {
            return;
        }
        let kind = self.agent.current().cli;
        let Some(binary) = self.agent.binary(kind).cloned() else {
            self.agent.current_mut().push(AgentItem::Error(t!("agent.error.not_found", cli = kind.label())));
            cx.notify();
            return;
        };
        let resume = self.agent.current().cli_session.clone();
        // Bật "Chạy trong worktree mới" trong Cài đặt: phiên mới làm ở worktree riêng.
        if resume.is_none() && self.agent.config.use_worktree {
            self.agent.input.update(cx, |s, cx| s.set_value("", window, cx));
            self.start_agent_in_worktree(AgentRequest { kind, binary, prompt }, cx);
            return;
        }
        // Hỏi tiếp chạy ở nơi phiên đã chạy (có thể là worktree).
        let s = self.agent.current();
        let workdir = if s.run_dir.as_os_str().is_empty() { self.repo.workdir().to_path_buf() } else { s.run_dir.clone() };
        let sid = s.id;
        self.agent.input.update(cx, |s, cx| s.set_value("", window, cx));
        self.start_agent(sid, kind, binary, workdir, prompt, resume, cx);
    }

    /// Tạo nhánh `ai/...` + worktree cạnh repo (`<repo>-worktrees/`) ở luồng nền, rồi chạy AI trong
    /// worktree đó ngay trong phiên này; thư mục đang làm không bị đụng tới, tab repo không đổi.
    fn start_agent_in_worktree(&mut self, req: AgentRequest, cx: &mut Context<Self>) {
        let branch = format!("ai/{}-{}", branch_slug(&req.prompt), utc_stamp(unix_now()));
        let s = self.agent.current_mut();
        let sid = s.id;
        let Some(target) = worktree_target(self.repo.workdir(), &branch) else {
            s.push(AgentItem::Error(t!("agent.error.worktree_path").to_string()));
            cx.notify();
            return;
        };
        // Khoá phiên trong lúc tạo worktree (tránh bấm Chạy hai lần).
        s.preparing = true;
        cx.notify();
        let repo = self.repo.clone();
        let target_str = target.to_string_lossy().into_owned();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { repo.add_worktree(&target_str, "HEAD", Some(&branch)) })
                .await;
            this.update(cx, |this, cx| {
                let Some(s) = this.agent.session_mut(sid) else { return };
                s.preparing = false;
                match result {
                    Ok(()) => {
                        this.start_agent(sid, req.kind, req.binary, target, req.prompt, None, cx);
                        // Nhánh ai/... mới hiện ở cột Git.
                        this.refresh(true, None, cx);
                    }
                    Err(e) => {
                        s.push(AgentItem::Error(t!("agent.error.worktree", e = e)));
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Chạy CLI trong `workdir` cho phiên đang chọn; sự kiện đi qua kênh async về luồng giao diện.
    pub(crate) fn start_agent(
        &mut self,
        sid: u64,
        kind: CliKind,
        binary: PathBuf,
        workdir: PathBuf,
        prompt: String,
        resume: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let (model, allow_shell) = (self.agent.config.model(kind), self.agent.config.allow_shell);
        let oauth_token = match kind {
            CliKind::Claude => self.agent.config.claude_token.clone(),
            CliKind::Codex => None,
        };
        let Some(s) = self.agent.session_mut(sid) else { return };
        if resume.is_none() {
            s.items.clear();
            s.changed.clear();
            s.cli_session = None;
        }
        if s.title.is_empty() {
            s.title = branchpad_short(prompt.lines().next().unwrap_or_default(), 40);
        }
        s.cli = kind;
        s.finished = None;
        s.run_gen += 1;
        s.run_dir = workdir.clone();
        s.push(AgentItem::User(prompt.clone()));
        let opts = RunOptions { kind, binary, workdir, prompt, resume, model, allow_shell, oauth_token };
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<AgentEvent>();
        match AgentRun::start(opts, move |ev| {
            // Phiên đã đóng thì bỏ sự kiện.
            let _ = tx.unbounded_send(ev);
        }) {
            Ok(run) => s.run = Some(run),
            Err(e) => {
                s.push(AgentItem::Error(t!("agent.error.start", cli = kind.label(), e = e)));
                cx.notify();
                return;
            }
        }
        let (sid, gen) = (s.id, s.run_gen);
        cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.next().await {
                if this.update(cx, |this, cx| this.on_agent_event(sid, gen, ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn stop_agent(&mut self, cx: &mut Context<Self>) {
        let s = self.agent.current_mut();
        if let Some(run) = &s.run {
            run.cancel();
            s.push(AgentItem::Warning(t!("agent.stopping").to_string()));
            cx.notify();
        }
    }

    fn on_agent_event(&mut self, sid: u64, gen: u64, ev: AgentEvent, cx: &mut Context<Self>) {
        let workdir = self.repo.workdir().to_path_buf();
        let Some(run_dir) = self
            .agent
            .sessions
            .iter()
            .find(|s| s.id == sid && s.run_gen == gen)
            .map(|s| s.run_dir.clone())
        else {
            return;
        };
        // Đọc trình soạn trước khi mượn phiên để sửa.
        let dirty_file = match &ev {
            AgentEvent::FileChanged(p) => {
                let abs = if p.is_absolute() { p.clone() } else { run_dir.join(p) };
                Some((self.editor.read(cx).is_dirty(&abs), abs))
            }
            _ => None,
        };
        let Some(s) = self.agent.session_mut(sid) else { return };
        let mut finished = None;
        match ev {
            AgentEvent::Started { session_id } => s.cli_session = Some(session_id),
            AgentEvent::Text(t) => s.push(AgentItem::Text(t)),
            AgentEvent::Thinking(t) => s.push(AgentItem::Thinking(t)),
            AgentEvent::ToolStart { id, kind, name, detail } => {
                let detail = rel_display(&detail, &run_dir, &workdir);
                s.push(AgentItem::Tool { id, kind, name, detail, state: ToolState::Running, summary: String::new() });
            }
            AgentEvent::ToolDone { id, ok, summary } => {
                let found = s.items.iter_mut().rev().find_map(|item| match item {
                    AgentItem::Tool { id: tid, state, summary: sm, .. } if *tid == id => Some((state, sm)),
                    _ => None,
                });
                if let Some((state, sm)) = found {
                    *state = if ok { ToolState::Ok } else { ToolState::Failed };
                    *sm = summary;
                }
            }
            AgentEvent::FileChanged(_) => {
                if let Some((dirty, abs)) = dirty_file {
                    if s.changed.insert(abs.clone()) {
                        let rel = rel_display(&abs.to_string_lossy(), &run_dir, &workdir);
                        s.push(AgentItem::File(rel.clone()));
                        if dirty {
                            s.push(AgentItem::Warning(t!("agent.warning.unsaved", file = rel)));
                        }
                    }
                }
            }
            AgentEvent::Error(e) => s.push(AgentItem::Error(e)),
            AgentEvent::Finished { ok, summary, cost_usd, tokens } => {
                s.run = None;
                s.finished = Some(FinishInfo { ok, cost_usd, tokens });
                if !ok && !summary.trim().is_empty() {
                    s.push(AgentItem::Error(summary.clone()));
                }
                finished = Some((ok, summary, s.changed.len()));
            }
        }
        if let Some((ok, summary, n)) = finished {
            if ok {
                let msg = summary.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string();
                self.info = Some(if n > 0 { t!("agent.done.files", n = n, msg = msg) } else { t!("agent.done.no_files", msg = msg) });
            }
            self.refresh(true, None, cx);
        }
        cx.notify();
    }

    // ---- Vẽ ----

    pub(super) fn render_agent(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = Pal::of(cx);
        let tabs = self.render_agent_tabs(p, cx);
        let s = self.agent.current();
        let kind = s.cli;
        let running = s.is_running();
        let missing = self.agent.detected && self.agent.binary(kind).is_none();

        let body: AnyElement = if missing {
            self.render_agent_missing(kind, p, cx)
        } else if s.items.is_empty() {
            self.render_agent_welcome(kind, p, cx)
        } else {
            let items: Vec<AnyElement> =
                s.items.iter().enumerate().map(|(i, item)| self.render_agent_item(i, s, item, p, cx)).collect();
            v_flex()
                .id(("agent-timeline", s.id as usize))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&s.scroll)
                .px_3()
                .py_3()
                .gap_3()
                .children(items)
                .into_any_element()
        };

        v_flex()
            .size_full()
            .bg(p.bg)
            .child(tabs)
            .child(self.render_agent_session_bar(p, cx))
            .child(body)
            .children(self.render_agent_status(p))
            .child(self.render_agent_composer(running, missing, p, cx))
            .into_any_element()
    }

    /// Thanh tab các phiên: icon agent + tên phiên + trạng thái + đóng; nút "+" và nút ẩn panel.
    fn render_agent_tabs(&self, p: Pal, cx: &Context<Self>) -> impl IntoElement {
        let tabs = self.agent.sessions.iter().enumerate().map(|(i, s)| {
            let active = i == self.agent.active;
            let title: SharedString = if s.title.is_empty() { t!("agent.session.new").into() } else { s.title.clone().into() };
            let status: Option<Icon> = if s.is_running() {
                Some(Icon::new(IconName::Loader).size_3().text_color(p.muted))
            } else {
                match &s.finished {
                    Some(f) if !f.ok => Some(Icon::new(IconName::CircleX).size_3().text_color(p.danger)),
                    _ => None,
                }
            };
            h_flex()
                .id(("agent-tab", s.id as usize))
                .h_full()
                .max_w(px(180.))
                .pl_2()
                .pr_1()
                .gap_1p5()
                .items_center()
                .flex_none()
                .text_sm()
                .cursor_pointer()
                .border_r_1()
                .border_color(p.border)
                .when(active, |d| d.bg(p.tab_active).text_color(p.fg))
                .when(!active, |d| d.text_color(p.muted).hover(|st| st.bg(p.hover)))
                .child(Icon::new(agent_icon(s.cli)).size_3p5().text_color(agent_color(s.cli, cx)).flex_none())
                .child(div().min_w_0().truncate().child(title))
                .children(status)
                .child(
                    div()
                        .id(("agent-tab-close", s.id as usize))
                        .size(px(16.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_sm()
                        .text_xs()
                        .text_color(p.muted)
                        .hover(|st| st.bg(p.hover).text_color(p.fg))
                        .child("×")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.close_agent_tab(i, cx);
                        })),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.select_agent_tab(i, cx)))
        });
        h_flex()
            .h(px(32.))
            .w_full()
            .flex_none()
            .bg(p.tab_bar)
            .border_b_1()
            .border_color(p.border)
            .child(h_flex().id("agent-tabs").flex_1().min_w_0().h_full().overflow_x_scroll().children(tabs))
            .child(
                h_flex()
                    .px_1()
                    .gap_0p5()
                    .flex_none()
                    .child(
                        Button::new("agent-tab-new")
                            .icon(IconName::Plus)
                            .xsmall()
                            .ghost()
                            .tooltip(t!("agent.session.new_tooltip"))
                            .on_click(cx.listener(|this, _, window, cx| this.new_agent_tab(window, cx))),
                    )
                    .child(
                        Button::new("agent-close")
                            .icon(IconName::PanelRightClose)
                            .xsmall()
                            .ghost()
                            .tooltip(t!("agent.panel.close"))
                            .on_click(cx.listener(|this, _, window, cx| this.on_toggle_agent(&ToggleAgent, window, cx))),
                    ),
            )
    }

    /// Dưới thanh tab: phiên chưa chạy thì chọn agent; đã chạy thì hiện agent + nơi chạy + trạng thái.
    fn render_agent_session_bar(&self, p: Pal, cx: &Context<Self>) -> impl IntoElement {
        let s = self.agent.current();
        let bar = h_flex()
            .h(px(36.))
            .px_2()
            .gap_1()
            .flex_none()
            .items_center()
            .border_b_1()
            .border_color(p.border);
        if s.is_blank() {
            let pickers = CliKind::ALL.into_iter().map(|k| {
                let selected = k == s.cli;
                let installed = !self.agent.detected || self.agent.binary(k).is_some();
                Button::new(("agent-cli", k as usize))
                    .icon(agent_icon(k).text_color(agent_color(k, cx)))
                    .label(k.label())
                    .xsmall()
                    .when(!selected, |b| b.ghost())
                    .when(!installed, |b| b.tooltip(t!("agent.cli.not_installed")))
                    .on_click(cx.listener(move |this, _, _, cx| this.select_agent_cli(k, cx)))
            });
            bar.children(pickers)
        } else {
            let place = s
                .run_dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.repo.name());
            let (label, color) = if s.is_running() {
                (t!("agent.status.running"), p.primary)
            } else {
                match &s.finished {
                    Some(f) if f.ok => (t!("agent.status.done"), p.success),
                    Some(_) => (t!("agent.status.failed"), p.danger),
                    None => (t!("agent.status.idle"), p.muted),
                }
            };
            bar.gap_2()
                .child(Icon::new(agent_icon(s.cli)).size_4().text_color(agent_color(s.cli, cx)))
                .child(div().flex_none().text_sm().font_weight(gpui::FontWeight::MEDIUM).text_color(p.fg).child(s.cli.label()))
                .child(div().flex_1().min_w_0().truncate().text_xs().text_color(p.muted).child(place))
                .when_some(self.agent_worktree_dir(s), |d, dir| {
                    d.child(
                        Button::new("agent-open-worktree")
                            .icon(BpIcon::Branch)
                            .label(t!("agent.worktree.open"))
                            .xsmall()
                            .ghost()
                            .tooltip(t!("agent.worktree.open_tooltip"))
                            .on_click(cx.listener(move |_, _, _, cx| cx.emit(RepoViewEvent::OpenRepository(dir.clone())))),
                    )
                })
                .child(
                    div()
                        .flex_none()
                        .px_2()
                        .py(px(1.))
                        .rounded_full()
                        .bg(color.opacity(0.14))
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(color)
                        .child(label),
                )
        }
    }

    /// Thư mục worktree riêng của phiên (khác thư mục repo đang mở), nếu có.
    fn agent_worktree_dir(&self, s: &AgentSession) -> Option<PathBuf> {
        let dir = &s.run_dir;
        (!dir.as_os_str().is_empty() && dir.as_path() != self.repo.workdir()).then(|| dir.clone())
    }

    /// Chưa có CLI trên máy: cách cài + mở Cài đặt.
    fn render_agent_missing(&self, kind: CliKind, p: Pal, cx: &Context<Self>) -> AnyElement {
        v_flex()
            .flex_1()
            .min_h_0()
            .p_4()
            .gap_3()
            .items_center()
            .justify_center()
            .child(agent_avatar(kind, 48., cx))
            .child(
                div()
                    .text_base()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(p.fg)
                    .child(t!("agent.missing.title", cli = kind.label())),
            )
            .child(div().text_sm().text_color(p.muted).text_center().child(t!("agent.missing.hint")))
            .child(
                div()
                    .w_full()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(p.border)
                    .bg(p.secondary)
                    .text_xs()
                    .font_family(crate::ui::MONO)
                    .text_color(p.fg)
                    .whitespace_normal()
                    .child(kind.install_hint()),
            )
            .child(
                Button::new("agent-open-settings")
                    .icon(BpIcon::Settings)
                    .label(t!("agent.missing.open_settings"))
                    .small()
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::actions::OpenSettings), cx)),
            )
            .into_any_element()
    }

    /// Phiên trống: lời chào + gợi ý bắt đầu (bấm để điền vào ô nhập).
    fn render_agent_welcome(&self, kind: CliKind, p: Pal, cx: &Context<Self>) -> AnyElement {
        const STARTERS: [&str; 3] = ["agent.starter.explain", "agent.starter.review", "agent.starter.tests"];
        let starters = STARTERS.into_iter().enumerate().map(|(i, key)| {
            let text: &'static str = crate::i18n::tr(key);
            h_flex()
                .id(("agent-starter", i))
                .w_full()
                .px_3()
                .py_2()
                .gap_2()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(p.border)
                .text_sm()
                .text_color(p.fg)
                .cursor_pointer()
                .hover(|st| st.bg(p.hover).border_color(p.primary.opacity(0.5)))
                .child(Icon::new(BpIcon::Sparkles).size_3p5().text_color(p.primary).flex_none())
                .child(div().flex_1().min_w_0().whitespace_normal().child(text))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.agent.input.update(cx, |s, cx| {
                        s.set_value(text, window, cx);
                        s.focus(window, cx);
                    });
                }))
        });
        v_flex()
            .id("agent-welcome")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_4()
            .py_6()
            .gap_4()
            .items_center()
            .child(agent_avatar(kind, 48., cx))
            .child(
                v_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .text_base()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(p.fg)
                            .child(t!("agent.empty.title", cli = kind.label())),
                    )
                    .child(div().text_sm().text_color(p.muted).text_center().child(t!("agent.empty.hint"))),
            )
            .child(v_flex().w_full().gap_2().children(starters))
            .into_any_element()
    }

    /// Dòng kết quả của lần chạy cuối: trạng thái · chi phí · token · số file.
    fn render_agent_status(&self, p: Pal) -> Option<impl IntoElement> {
        let s = self.agent.current();
        let f = s.finished.as_ref()?;
        let mut parts = Vec::new();
        if let Some(c) = f.cost_usd {
            parts.push(format!("${c:.2}"));
        }
        if let Some(n) = f.tokens {
            parts.push(t!("agent.status.tokens", n = format_tokens(n)));
        }
        if !s.changed.is_empty() {
            parts.push(t!("agent.status.files", n = s.changed.len()));
        }
        if parts.is_empty() {
            return None;
        }
        Some(
            h_flex()
                .px_3()
                .py_1()
                .gap_2()
                .flex_none()
                .items_center()
                .border_t_1()
                .border_color(p.border)
                .bg(p.secondary.opacity(0.5))
                .text_xs()
                .text_color(p.muted)
                .child(div().flex_1().truncate().child(parts.join("  ·  "))),
        )
    }

    /// Ô nhập ở đáy: nhập yêu cầu, gợi ý phím, nút Dừng hoặc Chạy / Hỏi tiếp.
    fn render_agent_composer(&self, running: bool, missing: bool, p: Pal, cx: &Context<Self>) -> impl IntoElement {
        let s = self.agent.current();
        let can_resume = s.cli_session.is_some();
        v_flex()
            .p_2()
            .gap_2()
            .flex_none()
            .border_t_1()
            .border_color(p.border)
            .child(div().h(px(88.)).child(Input::new(&self.agent.input).h_full()))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(p.muted)
                            .child(if can_resume { t!("agent.composer.hint_continue") } else { t!("agent.composer.hint") }),
                    )
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
                                .icon(agent_icon(s.cli))
                                .label(if can_resume { t!("agent.button.continue") } else { t!("agent.button.run") })
                                .small()
                                .primary()
                                .disabled(missing || !self.agent.detected)
                                .tooltip("Enter")
                                .on_click(cx.listener(|this, _, window, cx| this.agent_submit(window, cx))),
                        )
                    }),
            )
    }

    fn render_agent_item(&self, i: usize, s: &AgentSession, item: &AgentItem, p: Pal, cx: &Context<Self>) -> AnyElement {
        let kind = s.cli;
        let worktree = self.agent_worktree_dir(s);
        let text = |s: &str| div().text_sm().text_color(p.fg).whitespace_normal().child(s.to_string());
        match item {
            // Yêu cầu của người dùng: bong bóng bên phải.
            AgentItem::User(s) => h_flex()
                .w_full()
                .justify_end()
                .child(
                    div()
                        .max_w(gpui::relative(0.9))
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(p.primary.opacity(0.12))
                        .border_1()
                        .border_color(p.primary.opacity(0.2))
                        .child(text(s)),
                )
                .into_any_element(),
            // Câu trả lời: avatar agent bên trái, chữ trải hết bề ngang.
            AgentItem::Text(s) => h_flex()
                .w_full()
                .gap_2()
                .items_start()
                .child(agent_avatar(kind, 22., cx))
                .child(div().flex_1().min_w_0().pt(px(2.)).child(text(s)))
                .into_any_element(),
            AgentItem::Thinking(s) => div()
                .ml(px(30.))
                .pl_2()
                .border_l_2()
                .border_color(p.border)
                .text_xs()
                .italic()
                .text_color(p.muted)
                .whitespace_normal()
                .child(branchpad_short(s, 400))
                .into_any_element(),
            AgentItem::Tool { kind: tool, name, detail, state, summary, .. } => {
                let (state_icon, state_color) = match state {
                    ToolState::Running => (IconName::Loader, p.primary),
                    ToolState::Ok => (IconName::CircleCheck, p.success),
                    ToolState::Failed => (IconName::CircleX, p.danger),
                };
                h_flex()
                    .ml(px(30.))
                    .px_2()
                    .py_1p5()
                    .gap_2()
                    .items_start()
                    .rounded_md()
                    .border_1()
                    .border_color(p.border)
                    .bg(p.secondary.opacity(0.45))
                    .text_xs()
                    .child(Icon::new(tool_icon(*tool)).size_3p5().text_color(p.muted).flex_none().mt(px(1.)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .child(div().flex_none().font_weight(gpui::FontWeight::MEDIUM).text_color(p.fg).child(name.clone()))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .font_family(crate::ui::MONO)
                                            .text_color(p.muted)
                                            .child(detail.clone()),
                                    ),
                            )
                            .when(!summary.is_empty(), |d| {
                                let color = if *state == ToolState::Failed { p.danger } else { p.muted };
                                d.child(div().truncate().text_color(color).child(summary.clone()))
                            }),
                    )
                    .child(Icon::new(state_icon).size_3p5().text_color(state_color).flex_none().mt(px(1.)))
                    .into_any_element()
            }
            AgentItem::File(rel) => {
                let (name, dir) = split_path(rel);
                let (icon, color) = file_icon(&name, false, false);
                let rel = rel.clone();
                h_flex()
                    .id(("agent-file", i))
                    .ml(px(30.))
                    .px_2()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .rounded_md()
                    .border_1()
                    .border_color(p.success.opacity(0.35))
                    .bg(p.success.opacity(0.08))
                    .text_sm()
                    .cursor_pointer()
                    .hover(|st| st.bg(p.success.opacity(0.16)))
                    .child(Icon::new(icon).size_4().text_color(color).flex_none())
                    .child(div().flex_none().text_color(p.fg).child(name))
                    .child(div().flex_1().min_w_0().truncate().text_xs().text_color(p.muted).child(dir))
                    .child(div().flex_none().text_xs().text_color(p.success).child(t!("agent.file.view_diff")))
                    .on_click(cx.listener(move |this, _, _, cx| match &worktree {
                        // File nằm trong worktree: mở worktree thành tab để xem diff ở đó.
                        Some(dir) => cx.emit(RepoViewEvent::OpenRepository(dir.clone())),
                        None => this.show_file_changes(&rel, cx),
                    }))
                    .into_any_element()
            }
            AgentItem::Warning(s) => notice(IconName::CircleX, p.warning, s, p),
            AgentItem::Error(s) => notice(IconName::CircleX, p.danger, s, p),
        }
    }
}

/// Avatar tròn của agent: icon trên nền nhạt cùng màu.
fn agent_avatar(kind: CliKind, size: f32, cx: &App) -> impl IntoElement {
    let color = agent_color(kind, cx);
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(color.opacity(0.14))
        .border_1()
        .border_color(color.opacity(0.3))
        .child(Icon::new(agent_icon(kind)).size(px(size * 0.55)).text_color(color))
}

/// Khối thông báo (cảnh báo / lỗi) có icon, nền nhạt cùng màu.
fn notice(icon: IconName, color: Hsla, text: &str, p: Pal) -> AnyElement {
    h_flex()
        .ml(px(30.))
        .px_2()
        .py_1p5()
        .gap_2()
        .items_start()
        .rounded_md()
        .border_1()
        .border_color(color.opacity(0.35))
        .bg(color.opacity(0.1))
        .text_xs()
        .child(Icon::new(icon).size_3p5().text_color(color).flex_none().mt(px(1.)))
        .child(div().flex_1().min_w_0().text_color(p.fg).whitespace_normal().child(text.to_string()))
        .into_any_element()
}

/// 1234 → "1.2k".
fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.)
    } else {
        n.to_string()
    }
}

/// Đường dẫn trong thư mục chạy / repo thì hiện dạng tương đối.
fn rel_display(text: &str, run_dir: &Path, workdir: &Path) -> String {
    let p = Path::new(text);
    for base in [run_dir, workdir] {
        if base.as_os_str().is_empty() {
            continue;
        }
        if let Ok(rel) = p.strip_prefix(base) {
            if !rel.as_os_str().is_empty() {
                return rel.to_string_lossy().into_owned();
            }
        }
    }
    text.to_string()
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Bỏ dấu tiếng Việt (để tên nhánh đọc được).
fn fold_vietnamese(c: char) -> char {
    const GROUPS: &[(&str, char)] = &[
        ("àáạảãâầấậẩẫăằắặẳẵ", 'a'),
        ("èéẹẻẽêềếệểễ", 'e'),
        ("ìíịỉĩ", 'i'),
        ("òóọỏõôồốộổỗơờớợởỡ", 'o'),
        ("ùúụủũưừứựửữ", 'u'),
        ("ỳýỵỷỹ", 'y'),
        ("đ", 'd'),
    ];
    GROUPS.iter().find(|(g, _)| g.contains(c)).map_or(c, |(_, base)| *base)
}

/// Tên nhánh từ yêu cầu: tối đa 4 từ đầu, chữ thường không dấu, nối bằng `-`.
fn branch_slug(prompt: &str) -> String {
    let folded: String = prompt.chars().flat_map(char::to_lowercase).map(fold_vietnamese).collect();
    let words: Vec<&str> = folded
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(4)
        .collect();
    let mut slug = words.join("-");
    if slug.len() > 30 {
        slug.truncate(30);
        slug = slug.trim_end_matches('-').to_string();
    }
    if slug.is_empty() {
        "task".into()
    } else {
        slug
    }
}

/// `yyyymmdd-hhmm` theo giờ UTC.
fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Ngày dương lịch từ số ngày kể từ 1970-01-01 (thuật toán civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}", rem / 3600, rem % 3600 / 60)
}

/// Thư mục worktree: `<cha của repo>/<tên repo>-worktrees/<nhánh, `/` đổi thành `-`>`.
fn worktree_target(workdir: &Path, branch: &str) -> Option<PathBuf> {
    let parent = workdir.parent()?;
    let name = workdir.file_name()?.to_string_lossy();
    Some(parent.join(format!("{name}-worktrees")).join(branch.replace('/', "-")))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_from_prompt() {
        assert_eq!(branch_slug("Sửa lỗi danh sách rỗng khi mở app"), "sua-loi-danh-sach");
        assert_eq!(branch_slug("Fix the crash!"), "fix-the-crash");
        assert_eq!(branch_slug("!!!"), "task");
        assert!(branch_slug("aaaaaaaaaa bbbbbbbbbb cccccccccc dddd").len() <= 30);
    }

    #[test]
    fn stamp() {
        assert_eq!(utc_stamp(0), "19700101-0000");
        assert_eq!(utc_stamp(1_700_000_000), "20231114-2213");
        assert_eq!(utc_stamp(951_782_400), "20000229-0000");
    }

    #[test]
    fn worktree_dir_is_next_to_repo() {
        assert_eq!(
            worktree_target(Path::new("/src/app"), "ai/fix-20260101-1200"),
            Some(PathBuf::from("/src/app-worktrees/ai-fix-20260101-1200"))
        );
    }
}
