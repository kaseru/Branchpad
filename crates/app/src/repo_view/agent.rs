//! Panel AI ở cột phải, cạnh trình soạn: giao việc cho Claude Code / Codex CLI chạy không tương tác
//! trong repo. Mỗi phiên là một tab; tiến trình hiện theo sự kiện JSON của CLI, file AI sửa hiện ở
//! tab Thay đổi nhờ watcher (không polling).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use branchpad_agent::{find_cli, list_models, AgentEvent, AgentRun, CliKind, RunOptions, Secret, ToolKind};
use branchpad_core::agent_history::{self, HistoryItem, SessionRecord, SessionSummary};
use futures::StreamExt;
use gpui::{
    div, hsla, prelude::*, px, AnyElement, AnyView, App, Context, Corner, Entity, EntityInputHandler, Focusable, Hsla,
    MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, ScrollHandle, SharedString, StyleRefinement, Subscription,
    WeakEntity, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    menu::{ContextMenuExt, DropdownMenu, PopupMenuItem},
    spinner::Spinner,
    text::TextView,
    v_flex, ActiveTheme, Disableable, Icon, IconName, Sizable,
};

use super::{RepoView, RepoViewEvent};
use crate::actions::{NewLine, ToggleAgent};
use crate::icons::{file_icon, BpIcon};
use crate::ui::split_path;

/// Cài đặt AI (từ màn Cài đặt).
#[derive(Clone, Debug, PartialEq)]
pub struct AgentConfig {
    /// CLI chọn sẵn; `None` = cái nào tìm thấy trước.
    pub cli: Option<CliKind>,
    pub claude_path: Option<PathBuf>,
    pub codex_path: Option<PathBuf>,
    pub agy_path: Option<PathBuf>,
    pub claude_model: Option<String>,
    pub codex_model: Option<String>,
    pub agy_model: Option<String>,
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
            agy_path: None,
            claude_model: None,
            codex_model: None,
            agy_model: None,
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
            CliKind::Agy => self.agy_path.as_deref(),
        }
    }

    pub fn model(&self, kind: CliKind) -> Option<String> {
        match kind {
            CliKind::Claude => self.claude_model.clone(),
            CliKind::Codex => self.codex_model.clone(),
            CliKind::Agy => self.agy_model.clone(),
        }
    }
}

/// Icon đại diện cho từng agent.
pub(crate) fn agent_icon(kind: CliKind) -> Icon {
    match kind {
        CliKind::Claude => BpIcon::Claude.into(),
        CliKind::Codex => BpIcon::Codex.into(),
        CliKind::Agy => BpIcon::Agy.into(),
    }
}

/// Màu của icon agent: Claude màu cam đất, Codex theo màu chữ của giao diện, Antigravity xanh dương.
pub(crate) fn agent_color(kind: CliKind, cx: &App) -> Hsla {
    match kind {
        CliKind::Claude => hsla(15. / 360., 0.63, 0.6, 1.),
        CliKind::Codex => cx.theme().foreground,
        CliKind::Agy => hsla(217. / 360., 0.89, 0.61, 1.),
    }
}

/// Thân panel AI là view riêng có cache: gõ phím ở ô nhập không dựng lại markdown của cả cuộc chat
/// (rất giật khi chat dài). RepoView báo đổi (`cx.notify()`) thì view này vẽ lại theo.
pub(crate) struct AgentBody {
    repo: WeakEntity<RepoView>,
    _obs: Subscription,
}

impl Render for AgentBody {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.repo.update(cx, |rv, cx| rv.render_agent_body(window, cx)).unwrap_or_else(|_| div().into_any_element())
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
    /// Mốc kết thúc một lượt: xong / lỗi / dừng, kèm token, chi phí, số file.
    TurnEnd { ok: bool, text: String },
    /// AI xin quyền dùng một công cụ (Claude): người dùng trả lời ngay trong chat.
    Permission { id: String, tool: String, detail: String, input: serde_json::Value, state: PermState },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PermState {
    Pending,
    Allowed,
    /// Cho phép công cụ này cho cả phiên.
    AllowedAlways,
    Denied,
    /// CLI không còn chờ (hết lượt / bị ngắt / mở lại từ lịch sử).
    Expired,
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
    /// Người dùng đã bấm Dừng (lượt kết thúc là "đã dừng", không phải lỗi).
    stopping: bool,
    /// Công cụ người dùng đã chọn "luôn cho phép" trong phiên này.
    always_allow: BTreeSet<String>,
    /// Nhóm "nhật ký làm việc" đang mở (theo chỉ số mục đầu nhóm); mặc định thu gọn.
    expanded: BTreeSet<usize>,
    /// Model / mức suy luận chọn ở ô nhập; `None` = mặc định (model trong Cài đặt, effort của CLI).
    model: Option<String>,
    effort: Option<String>,
    /// Mã file lịch sử (lưu sau mỗi lượt để mở lại / hỏi tiếp khi mở lại app).
    record_id: String,
    created: i64,
    /// Tin nhắn gõ trong lúc AI đang chạy: gửi lần lượt khi lượt hiện tại xong.
    queued: Vec<String>,
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
            stopping: false,
            expanded: BTreeSet::new(),
            always_allow: BTreeSet::new(),
            model: None,
            effort: None,
            record_id: new_record_id(id),
            created: branchpad_core::now_unix(),
            queued: Vec::new(),
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
        self.scroll.scroll_to_bottom();
    }
}

pub(crate) struct AgentPanel {
    pub(crate) input: Entity<InputState>,
    config: AgentConfig,
    /// CLI tìm thấy trên máy (dò ở luồng nền).
    claude_bin: Option<PathBuf>,
    codex_bin: Option<PathBuf>,
    agy_bin: Option<PathBuf>,
    detected: bool,
    /// Luôn có ít nhất một phiên.
    sessions: Vec<AgentSession>,
    active: usize,
    next_id: u64,
    /// Model của agy (lấy bằng `agy models` ở luồng nền).
    agy_models: Vec<(String, String)>,
    /// Thư mục lịch sử phiên của repo này (`None` = không có thư mục cài đặt).
    history_dir: Option<PathBuf>,
    /// Đang xem danh sách lịch sử thay cho dòng hội thoại.
    show_history: bool,
    /// `None` = đang tải; `Err` = lỗi đọc.
    history: Option<Result<Vec<SessionSummary>, String>>,
    /// Chỗ nhấn chuột trong một tin nhắn (để biết người dùng kéo chọn chữ hay chỉ bấm).
    press_at: Option<Point<Pixels>>,
    /// Đoạn vừa bôi đen trong tin nhắn (phiên, chỉ số tin, chữ): hiện nút "Trích dẫn để trả lời".
    quote: Option<(u64, usize, String)>,
    /// Đoạn đã chọn "trích dẫn" chờ gửi kèm tin nhắn kế tiếp (hiện thành thẻ trên ô nhập).
    quotes: Vec<String>,
    /// View thân panel (tạo ở lần vẽ đầu, cần entity của RepoView).
    body: Option<Entity<AgentBody>>,
}

impl AgentPanel {
    pub(crate) fn new(repo_dir: &Path, window: &mut Window, cx: &mut Context<RepoView>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(2, 10)
                .placeholder(t!("agent.input.placeholder"))
        });
        Self {
            input,
            config: AgentConfig::default(),
            claude_bin: None,
            codex_bin: None,
            agy_bin: None,
            detected: false,
            sessions: vec![AgentSession::new(0, CliKind::Claude)],
            active: 0,
            next_id: 1,
            agy_models: Vec::new(),
            history_dir: agent_history::default_base().map(|base| agent_history::history_dir(&base, repo_dir)),
            show_history: false,
            history: None,
            press_at: None,
            quote: None,
            quotes: Vec::new(),
            body: None,
        }
    }

    /// Phiên đã có nội dung (đã lưu lịch sử) theo thứ tự tab, và phiên đang chọn.
    pub(crate) fn open_sessions(&self) -> (Vec<String>, Option<String>) {
        let ids = self.sessions.iter().filter(|s| !s.items.is_empty()).map(|s| s.record_id.clone()).collect();
        let current = self.current();
        (ids, (!current.items.is_empty()).then(|| current.record_id.clone()))
    }

    /// Có phiên nào đang chạy.
    pub(crate) fn is_running(&self) -> bool {
        self.sessions.iter().any(AgentSession::is_running)
    }

    fn binary(&self, kind: CliKind) -> Option<&PathBuf> {
        match kind {
            CliKind::Claude => self.claude_bin.as_ref(),
            CliKind::Codex => self.codex_bin.as_ref(),
            CliKind::Agy => self.agy_bin.as_ref(),
        }
    }

    /// CLI cho phiên mới: theo cài đặt, không thì cái nào có trên máy.
    fn default_cli(&self) -> CliKind {
        self.config
            .cli
            .or_else(|| CliKind::ALL.into_iter().find(|k| self.binary(*k).is_some()))
            .unwrap_or(CliKind::Claude)
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

    /// Model chọn được cho CLI: danh sách có sẵn (agy: lấy từ `agy models`) + model trong Cài đặt.
    fn models(&self, kind: CliKind) -> Vec<(String, String)> {
        let mut list: Vec<(String, String)> = match kind {
            CliKind::Agy => self.agy_models.clone(),
            _ => kind.builtin_models().iter().map(|(v, l)| (v.to_string(), l.to_string())).collect(),
        };
        if let Some(m) = self.config.model(kind).filter(|m| !m.trim().is_empty()) {
            if !list.iter().any(|(v, _)| *v == m) {
                list.insert(0, (m.clone(), m));
            }
        }
        list
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
                        find_cli(CliKind::Agy, config.path_override(CliKind::Agy)),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                let a = &mut this.agent;
                (a.claude_bin, a.codex_bin, a.agy_bin) = found;
                a.detected = true;
                // Phiên chưa dùng thì theo CLI mặc định mới.
                let cli = a.default_cli();
                for s in a.sessions.iter_mut().filter(|s| s.is_blank()) {
                    s.cli = cli;
                }
                if a.agy_models.is_empty() {
                    if let Some(bin) = a.agy_bin.clone() {
                        this.load_agy_models(bin, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Lấy danh sách model của agy (`agy models`) ở luồng nền.
    fn load_agy_models(&mut self, bin: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let models = cx.background_spawn(async move { list_models(CliKind::Agy, &bin) }).await;
            this.update(cx, |this, cx| {
                this.agent.agy_models = models;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_agent_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        self.agent.current_mut().model = model;
        cx.notify();
    }

    fn set_agent_effort(&mut self, effort: Option<String>, cx: &mut Context<Self>) {
        self.agent.current_mut().effort = effort;
        cx.notify();
    }

    // ---- Lịch sử phiên ----

    /// Lưu phiên `sid` vào lịch sử ở luồng nền (sau mỗi lượt gửi / khi có mã phiên / khi xong).
    fn save_agent_session(&mut self, sid: u64, cx: &mut Context<Self>) {
        let Some(dir) = self.agent.history_dir.clone() else { return };
        let Some(s) = self.agent.sessions.iter().find(|s| s.id == sid) else { return };
        if s.items.is_empty() {
            return;
        }
        let record = SessionRecord {
            id: s.record_id.clone(),
            cli: s.cli.id().to_string(),
            title: s.title.clone(),
            cli_session: s.cli_session.clone(),
            model: s.model.clone(),
            effort: s.effort.clone(),
            run_dir: s.run_dir.clone(),
            created: s.created,
            updated: branchpad_core::now_unix(),
            items: s.items.iter().map(to_history).collect(),
        };
        cx.background_spawn(async move {
            if let Err(e) = agent_history::save_session(&dir, &record) {
                eprintln!("branchpad: cannot save AI session {}: {e}", record.id);
            }
        })
        .detach();
    }

    /// Nút lịch sử: bật/tắt danh sách các phiên đã lưu (tải lại mỗi lần mở).
    fn toggle_agent_history(&mut self, cx: &mut Context<Self>) {
        let a = &mut self.agent;
        a.show_history = !a.show_history;
        if !a.show_history {
            cx.notify();
            return;
        }
        a.history = None;
        cx.notify();
        let dir = a.history_dir.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    match dir {
                        Some(dir) => agent_history::list_sessions(&dir).map_err(|e| e.to_string()),
                        None => Ok(Vec::new()),
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.agent.history = Some(result);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Mở một phiên đã lưu thành tab (đã mở thì chuyển sang tab đó); hỏi tiếp được nếu có mã phiên CLI.
    fn open_agent_history(&mut self, record_id: String, cx: &mut Context<Self>) {
        if let Some(i) = self.agent.sessions.iter().position(|s| s.record_id == record_id) {
            self.agent.active = i;
            self.agent.show_history = false;
            cx.notify();
            return;
        }
        let Some(dir) = self.agent.history_dir.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { agent_history::load_session(&dir, &record_id) }).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(record) => this.restore_agent_session(record),
                    Err(e) => this.agent.history = Some(Err(e.to_string())),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn restore_agent_session(&mut self, record: SessionRecord) {
        let a = &mut self.agent;
        let cli = CliKind::from_id(&record.cli).unwrap_or_default();
        // Tab đang chọn còn trống thì thay luôn, không thì mở tab mới.
        if !a.current().is_blank() {
            a.add_session(cli);
        }
        let s = a.current_mut();
        s.cli = cli;
        s.title = record.title;
        s.cli_session = record.cli_session;
        s.model = record.model;
        s.effort = record.effort;
        s.record_id = record.id;
        s.created = record.created;
        s.items = record.items.into_iter().map(from_history).collect();
        s.changed = s
            .items
            .iter()
            .filter_map(|i| match i {
                AgentItem::File(rel) => Some(record.run_dir.join(rel)),
                _ => None,
            })
            .collect();
        s.run_dir = record.run_dir;
        s.scroll.scroll_to_bottom();
        a.show_history = false;
    }

    /// Mở lại các phiên đang mở lúc tắt app (đọc file lịch sử nhỏ, làm ngay khi mở repo).
    pub(super) fn restore_agent_sessions(&mut self, ids: Vec<String>, active: Option<String>) {
        let Some(dir) = self.agent.history_dir.clone() else { return };
        for id in ids {
            match agent_history::load_session(&dir, &id) {
                Ok(record) => self.restore_agent_session(record),
                Err(e) => eprintln!("branchpad: cannot reopen AI session {id}: {e}"),
            }
        }
        if let Some(i) = active.and_then(|id| self.agent.sessions.iter().position(|s| s.record_id == id)) {
            self.agent.active = i;
        }
    }

    fn delete_agent_history(&mut self, record_id: String, cx: &mut Context<Self>) {
        let Some(dir) = self.agent.history_dir.clone() else { return };
        if let Some(Ok(list)) = &mut self.agent.history {
            list.retain(|s| s.id != record_id);
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { agent_history::delete_session(&dir, &record_id) }).await;
            if let Err(e) = result {
                this.update(cx, |this, cx| {
                    this.agent.history = Some(Err(e.to_string()));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn select_agent_cli(&mut self, kind: CliKind, cx: &mut Context<Self>) {
        let s = self.agent.current_mut();
        if s.is_blank() {
            s.cli = kind;
            s.model = None;
            s.effort = None;
            cx.notify();
        }
    }

    fn new_agent_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cli = self.agent.default_cli();
        // Phiên đang chọn còn trống thì dùng luôn, không đẻ thêm tab rỗng.
        if !self.agent.current().is_blank() {
            self.agent.add_session(cli);
        }
        self.agent.show_history = false;
        self.agent.input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn select_agent_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.agent.sessions.len() {
            self.agent.active = i;
            self.agent.show_history = false;
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

    /// Nút gửi, Enter hoặc Cmd+Enter trong ô nhập. AI đang chạy thì tin nhắn vào hàng chờ.
    pub(crate) fn agent_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut prompt = self.agent.input.read(cx).value().trim().to_string();
        // Có thẻ trích dẫn: ghép vào đầu tin nhắn dạng khối `>` (xem `split_quotes`).
        if !self.agent.quotes.is_empty() {
            let header = t!("agent.quote.header", cli = self.agent.current().cli.label());
            prompt = join_quotes(&header, &std::mem::take(&mut self.agent.quotes), &prompt);
        }
        // Ô trống mà hàng chờ còn tin (đã bấm Dừng): gửi tin kế tiếp.
        if prompt.is_empty() {
            let s = self.agent.current_mut();
            if s.is_running() || s.queued.is_empty() {
                return;
            }
            prompt = s.queued.remove(0);
        }
        self.agent.show_history = false;
        self.agent.input.update(cx, |s, cx| s.set_value("", window, cx));
        let s = self.agent.current_mut();
        if s.is_running() {
            s.queued.push(prompt);
            cx.notify();
            return;
        }
        let sid = s.id;
        self.send_agent_prompt(sid, prompt, cx);
    }

    /// Gửi một tin nhắn cho phiên `sid` (phiên chưa chạy): lượt mới, hoặc hỏi tiếp nếu đã có mã phiên CLI.
    fn send_agent_prompt(&mut self, sid: u64, prompt: String, cx: &mut Context<Self>) {
        let Some(s) = self.agent.sessions.iter().find(|s| s.id == sid) else { return };
        let (kind, resume) = (s.cli, s.cli_session.clone());
        // Hỏi tiếp chạy ở nơi phiên đã chạy (có thể là worktree).
        let workdir = if s.run_dir.as_os_str().is_empty() { self.repo.workdir().to_path_buf() } else { s.run_dir.clone() };
        let Some(binary) = self.agent.binary(kind).cloned() else {
            if let Some(s) = self.agent.session_mut(sid) {
                s.push(AgentItem::Error(t!("agent.error.not_found", cli = kind.label())));
            }
            cx.notify();
            return;
        };
        // Bật "Chạy trong worktree mới" trong Cài đặt: phiên mới làm ở worktree riêng.
        if resume.is_none() && self.agent.config.use_worktree {
            self.start_agent_in_worktree(sid, AgentRequest { kind, binary, prompt }, cx);
            return;
        }
        self.start_agent(sid, kind, binary, workdir, prompt, resume, cx);
    }

    /// Bỏ một tin nhắn khỏi hàng chờ của phiên đang chọn.
    fn remove_queued(&mut self, i: usize, cx: &mut Context<Self>) {
        let q = &mut self.agent.current_mut().queued;
        if i < q.len() {
            q.remove(i);
        }
        cx.notify();
    }

    /// Tạo nhánh `ai/...` + worktree cạnh repo (`<repo>-worktrees/`) ở luồng nền, rồi chạy AI trong
    /// worktree đó ngay trong phiên này; thư mục đang làm không bị đụng tới, tab repo không đổi.
    fn start_agent_in_worktree(&mut self, sid: u64, req: AgentRequest, cx: &mut Context<Self>) {
        let branch = format!("ai/{}-{}", branch_slug(&req.prompt), utc_stamp(unix_now()));
        let workdir = self.repo.workdir().to_path_buf();
        let Some(s) = self.agent.session_mut(sid) else { return };
        let Some(target) = worktree_target(&workdir, &branch) else {
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
        let (config_model, allow_shell) = (self.agent.config.model(kind), self.agent.config.allow_shell);
        let oauth_token = match kind {
            CliKind::Claude => self.agent.config.claude_token.clone(),
            CliKind::Codex | CliKind::Agy => None,
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
        let model = s.model.clone().or(config_model);
        // Mức suy luận chỉ gửi khi CLI có tuỳ chọn này.
        let effort = s.effort.clone().filter(|e| kind.efforts().contains(&e.as_str()));
        // Claude hỏi quyền ngay trong chat khi cần (Codex / agy không có giao thức hỏi quyền).
        let ask_permissions = kind == CliKind::Claude;
        let opts = RunOptions { kind, binary, workdir, prompt, resume, model, effort, allow_shell, ask_permissions, oauth_token };
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
        self.save_agent_session(sid, cx);
        cx.notify();
    }

    fn stop_agent(&mut self, cx: &mut Context<Self>) {
        let s = self.agent.current_mut();
        if let Some(run) = &s.run {
            run.cancel();
            s.stopping = true;
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
        let mut next = None;
        let mut save = false;
        match ev {
            AgentEvent::Started { session_id } => {
                s.cli_session = Some(session_id);
                save = true;
            }
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
            AgentEvent::PermissionRequest { id, tool, detail, input } => {
                if s.always_allow.contains(&tool) {
                    // Đã chọn "luôn cho phép" công cụ này trong phiên: trả lời luôn.
                    let sent = s.run.as_ref().map(|run| run.respond_permission(&id, true, &input));
                    if let Some(Err(e)) = sent {
                        s.push(AgentItem::Error(t!("agent.permission.send_failed", e = e)));
                    }
                } else {
                    let detail = rel_display(&detail, &run_dir, &workdir);
                    s.push(AgentItem::Permission { id, tool, detail, input, state: PermState::Pending });
                }
            }
            AgentEvent::PermissionCancelled { id } => {
                for item in s.items.iter_mut() {
                    if let AgentItem::Permission { id: pid, state: state @ PermState::Pending, .. } = item {
                        if *pid == id {
                            *state = PermState::Expired;
                        }
                    }
                }
            }
            AgentEvent::Finished { ok, summary, cost_usd, tokens } => {
                s.run = None;
                let stopped = s.stopping;
                s.stopping = false;
                s.finished = Some(FinishInfo { ok, cost_usd, tokens });
                // Bước CLI chưa báo xong khi hết lượt: không để vòng xoay quay mãi.
                for item in s.items.iter_mut() {
                    if let AgentItem::Permission { state: state @ PermState::Pending, .. } = item {
                        *state = PermState::Expired;
                    }
                    if let AgentItem::Tool { state: state @ ToolState::Running, summary: sm, .. } = item {
                        *state = ToolState::Failed;
                        if sm.is_empty() {
                            *sm = t!("agent.tool.unfinished").to_string();
                        }
                    }
                }
                if !ok && !stopped && !summary.trim().is_empty() {
                    s.push(AgentItem::Error(summary.clone()));
                }
                // Lượt vừa xong: có câu trả lời không, có bước nào lỗi không.
                let turn = s.items.iter().rposition(|i| matches!(i, AgentItem::User(_))).map_or(0, |i| i + 1);
                let replied = s.items[turn..].iter().any(|i| matches!(i, AgentItem::Text(_)));
                let failed_steps = s.items[turn..]
                    .iter()
                    .filter(|i| matches!(i, AgentItem::Tool { state: ToolState::Failed, .. }))
                    .count();
                if ok && !replied && !stopped {
                    s.push(AgentItem::Warning(t!("agent.warning.no_reply").to_string()));
                }
                if failed_steps > 0 && !stopped {
                    s.push(AgentItem::Warning(t!("agent.warning.failed_steps", n = failed_steps)));
                }
                let mut parts = vec![if stopped {
                    t!("agent.status.stopped").to_string()
                } else if ok {
                    t!("agent.status.done").to_string()
                } else {
                    t!("agent.status.failed").to_string()
                }];
                if let Some(c) = cost_usd {
                    parts.push(format!("${c:.2}"));
                }
                if let Some(n) = tokens {
                    parts.push(t!("agent.status.tokens", n = format_tokens(n)));
                }
                if !s.changed.is_empty() {
                    parts.push(t!("agent.status.files", n = s.changed.len()));
                }
                s.push(AgentItem::TurnEnd { ok: ok && !stopped, text: parts.join("  ·  ") });
                finished = Some((ok, summary, s.changed.len()));
                // Bấm Dừng thì giữ hàng chờ lại để người dùng tự quyết; không thì gửi tin kế tiếp.
                if !stopped && !s.queued.is_empty() {
                    next = Some(s.queued.remove(0));
                }
                save = true;
            }
        }
        if save {
            self.save_agent_session(sid, cx);
        }
        if let Some((ok, summary, n)) = finished {
            if ok {
                let msg = summary.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string();
                self.info = Some(if n > 0 { t!("agent.done.files", n = n, msg = msg) } else { t!("agent.done.no_files", msg = msg) });
            }
            self.refresh(true, None, cx);
        }
        if let Some(prompt) = next {
            self.send_agent_prompt(sid, prompt, cx);
        }
        cx.notify();
    }

    // ---- Vẽ ----

    pub(super) fn render_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = Pal::of(cx);
        let tabs = self.render_agent_tabs(p, cx);
        let s = self.agent.current();
        let running = s.is_running();
        let missing = self.agent.detected && self.agent.binary(s.cli).is_none();
        let body = match &self.agent.body {
            Some(body) => body.clone(),
            None => {
                let repo = cx.entity();
                let body = cx.new(|cx| AgentBody { repo: repo.downgrade(), _obs: cx.observe(&repo, |_, _, cx| cx.notify()) });
                self.agent.body = Some(body.clone());
                body
            }
        };

        v_flex()
            .size_full()
            .bg(p.bg)
            .child(tabs)
            .child(self.render_agent_session_bar(p, cx))
            .child(AnyView::from(body).cached(StyleRefinement::default().flex_1().min_h_0().w_full()))
            .child(self.render_agent_composer(running, missing, p, window, cx))
            .into_any_element()
    }

    /// Thân panel: lịch sử / chưa cài CLI / màn chào / dòng hội thoại. Vẽ qua `AgentBody` (có cache).
    fn render_agent_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = Pal::of(cx);
        let s = self.agent.current();
        let kind = s.cli;
        let running = s.is_running();
        let missing = self.agent.detected && self.agent.binary(kind).is_none();

        let body: AnyElement = if self.agent.show_history {
            self.render_agent_history(p, cx)
        } else if missing {
            self.render_agent_missing(kind, p, cx)
        } else if s.items.is_empty() {
            self.render_agent_welcome(kind, p, cx)
        } else {
            // Cửa sổ chat chỉ hiện kết quả; các bước làm việc liền nhau gom thành một dòng "nhật ký" thu gọn.
            let mut items: Vec<AnyElement> = Vec::new();
            let mut i = 0;
            while i < s.items.len() {
                if is_work(&s.items[i]) {
                    let start = i;
                    while i < s.items.len() && is_work(&s.items[i]) {
                        i += 1;
                    }
                    let live = running && i == s.items.len();
                    items.push(self.render_agent_work_group(s, start, i, live, p, window, cx));
                } else {
                    items.push(self.render_agent_item(i, s, &s.items[i], p, window, cx));
                    i += 1;
                }
            }
            // Đang chạy mà chưa có bước nào ở cuối: dòng "đang làm" có vòng xoay, để biết AI chưa dừng.
            if running && !s.items.last().is_some_and(is_work) {
                let waiting = s.items.iter().any(|i| matches!(i, AgentItem::Permission { state: PermState::Pending, .. }));
                let label = if s.stopping {
                    t!("agent.stopping")
                } else if waiting {
                    t!("agent.permission.waiting")
                } else if s.preparing {
                    t!("agent.status.preparing")
                } else {
                    t!("agent.status.running")
                };
                items.push(
                    h_flex()
                        .ml(px(30.))
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .text_color(p.muted)
                        .child(Spinner::new().color(p.primary).xsmall())
                        .child(label)
                        .into_any_element(),
                );
            }
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
        v_flex().size_full().bg(p.bg).child(body).into_any_element()
    }

    /// Thanh tab các phiên: icon agent + tên phiên + trạng thái + đóng; nút "+" và nút ẩn panel.
    fn render_agent_tabs(&self, p: Pal, cx: &Context<Self>) -> impl IntoElement {
        let tabs = self.agent.sessions.iter().enumerate().map(|(i, s)| {
            let active = i == self.agent.active;
            let title: SharedString = if s.title.is_empty() { t!("agent.session.new").into() } else { s.title.clone().into() };
            let status: Option<AnyElement> = if s.is_running() {
                Some(Spinner::new().color(p.primary).xsmall().into_any_element())
            } else {
                match &s.finished {
                    Some(f) if !f.ok => Some(Icon::new(IconName::CircleX).size_3().text_color(p.danger).into_any_element()),
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
                        Button::new("agent-history")
                            .icon(BpIcon::History)
                            .xsmall()
                            .when(!self.agent.show_history, |b| b.ghost())
                            .tooltip(t!("agent.history.tooltip"))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_agent_history(cx))),
                    )
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
                        .flex()
                        .items_center()
                        .gap_1()
                        .when(s.is_running(), |d| d.child(Spinner::new().color(color).xsmall()))
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

    /// Khung nhập dạng thẻ: hàng chờ (nếu có) ở trên, ô nhập tự giãn, hàng model / mức suy luận / nút gửi ở dưới.
    fn render_agent_composer(&self, running: bool, missing: bool, p: Pal, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let s = self.agent.current();
        let can_resume = s.cli_session.is_some();
        let input = self.agent.input.read(cx);
        let focused = input.focus_handle(cx).is_focused(window);
        let empty = input.value().trim().is_empty();
        let send_label = if running {
            t!("agent.button.queue")
        } else if can_resume {
            t!("agent.button.continue")
        } else {
            t!("agent.button.run")
        };
        let hint = if running {
            t!("agent.composer.hint_running")
        } else if can_resume {
            t!("agent.composer.hint_continue")
        } else {
            t!("agent.composer.hint")
        };
        let queued = s.queued.iter().enumerate().map(|(i, text)| {
            h_flex()
                .gap_2()
                .pl_2p5()
                .pr_1()
                .py_1()
                .items_center()
                .rounded_lg()
                .border_1()
                .border_color(p.border)
                .bg(p.bg)
                .child(div().flex_1().min_w_0().truncate().text_sm().text_color(p.fg).child(text.lines().next().unwrap_or_default().to_string()))
                .child(
                    Button::new(("agent-queued-remove", i))
                        .icon(IconName::Close)
                        .xsmall()
                        .ghost()
                        .tooltip(t!("agent.queue.remove"))
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_queued(i, cx))),
                )
        });
        v_flex()
            .key_context("AgentComposer")
            .on_action(cx.listener(|this, _: &NewLine, window, cx| {
                this.agent.input.update(cx, |s, cx| s.replace_text_in_range(None, "\n", window, cx));
            }))
            .flex_none()
            .px_3()
            .pt_1()
            .pb_2()
            .gap_1p5()
            .when(!s.queued.is_empty(), |d| {
                d.child(
                    div()
                        .px_1()
                        .text_xs()
                        .text_color(p.muted)
                        .child(if running { t!("agent.queue.title", n = s.queued.len()) } else { t!("agent.queue.paused", n = s.queued.len()) }),
                )
                .children(queued)
            })
            .child(
                v_flex()
                    .rounded_xl()
                    .border_1()
                    .border_color(if focused { p.primary } else { p.border })
                    .bg(p.secondary)
                    .shadow_sm()
                    .when(!self.agent.quotes.is_empty(), |d| {
                        let header = t!("agent.quote.header", cli = s.cli.label());
                        d.child(v_flex().px_2().pt_2().gap_1p5().children(self.agent.quotes.iter().enumerate().map(|(qi, q)| {
                            let remove = Button::new(("agent-quote-remove", qi))
                                .icon(IconName::Close)
                                .xsmall()
                                .ghost()
                                .tooltip(t!("agent.quote.remove"))
                                .on_click(cx.listener(move |this, _, _, cx| this.remove_agent_quote(qi, cx)));
                            quote_card(Some(header.as_str()), &branchpad_short(q, 280), Some(remove), p, cx)
                        })))
                    })
                    // Chữ ô nhập cùng cỡ với chữ trong chat (text_sm).
                    .child(div().px_1().pt_1().child(Input::new(&self.agent.input).appearance(false).small()))
                    .child(
                        h_flex()
                            .px_1p5()
                            .pb_1p5()
                            .gap_1()
                            .items_center()
                            .child(self.render_agent_model_picker(s, cx))
                            .children(self.render_agent_effort_picker(s, cx))
                            .child(div().flex_1())
                            .when(running, |d| {
                                d.child(
                                    Button::new("agent-stop")
                                        .icon(IconName::CircleX)
                                        .label(t!("agent.button.stop"))
                                        .small()
                                        .danger()
                                        .on_click(cx.listener(|this, _, _, cx| this.stop_agent(cx))),
                                )
                            })
                            // Đang chạy: nút gửi vẫn bấm được, tin nhắn vào hàng chờ.
                            .child(
                                Button::new("agent-run")
                                    .icon(IconName::ArrowUp)
                                    .small()
                                    .primary()
                                    .disabled(
                                        missing
                                            || !self.agent.detected
                                            || (empty && self.agent.quotes.is_empty() && (running || s.queued.is_empty())),
                                    )
                                    .tooltip(format!("{send_label} (↵)"))
                                    .on_click(cx.listener(|this, _, window, cx| this.agent_submit(window, cx))),
                            ),
                    ),
            )
            .child(div().px_1().truncate().text_xs().text_color(p.muted).child(hint))
    }

    /// Một nhóm bước làm việc liền nhau (`start..end`): dòng tóm tắt, bấm để mở / đóng nhật ký chi tiết.
    fn render_agent_work_group(
        &self,
        s: &AgentSession,
        start: usize,
        end: usize,
        live: bool,
        p: Pal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let steps = &s.items[start..end];
        let tools = steps.iter().filter(|i| matches!(i, AgentItem::Tool { .. })).count();
        let failed = steps.iter().filter(|i| matches!(i, AgentItem::Tool { state: ToolState::Failed, .. })).count();
        let open = s.expanded.contains(&start);
        // Đang chạy: tóm tắt là bước mới nhất; xong: số bước (+ số bước lỗi).
        let headline: String = if live {
            let last = steps.iter().rev().find_map(|i| match i {
                AgentItem::Tool { name, detail, .. } => Some(format!("{name}  {detail}")),
                _ => None,
            });
            match (s.stopping, last) {
                (true, _) => t!("agent.stopping").to_string(),
                (false, Some(l)) => l,
                (false, None) => t!("agent.log.thinking").to_string(),
            }
        } else if tools == 0 {
            t!("agent.log.thoughts").to_string()
        } else {
            t!("agent.log.steps", n = tools)
        };
        let state_el = if live {
            Spinner::new().color(p.primary).xsmall().into_any_element()
        } else if failed > 0 {
            Icon::new(IconName::CircleX).size_3p5().text_color(p.danger).into_any_element()
        } else {
            Icon::new(IconName::CircleCheck).size_3p5().text_color(p.muted).into_any_element()
        };
        let header = h_flex()
            .id(("agent-log", start))
            .w_full()
            .px_2()
            .py_1()
            .gap_2()
            .items_center()
            .rounded_md()
            .text_xs()
            .text_color(p.muted)
            .cursor_pointer()
            .hover(|st| st.bg(p.hover).text_color(p.fg))
            .child(div().flex_none().child(state_el))
            .child(div().flex_1().min_w_0().truncate().when(live, |d| d.font_family(crate::ui::MONO)).child(headline))
            .when(failed > 0 && !live, |d| d.child(div().flex_none().text_color(p.danger).child(t!("agent.log.failed", n = failed))))
            .child(
                Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight })
                    .size_3p5()
                    .flex_none(),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_agent_log(start, cx)));
        let details: Vec<AnyElement> = if open {
            steps.iter().enumerate().map(|(k, item)| self.render_agent_item(start + k, s, item, p, window, cx)).collect()
        } else {
            Vec::new()
        };
        v_flex()
            .ml(px(30.))
            .gap_1()
            .child(header)
            .when(open, |d| {
                d.child(
                    v_flex()
                        .ml_2()
                        .pl_3()
                        .py_1()
                        .gap_1p5()
                        .border_l_1()
                        .border_color(p.border)
                        .children(details),
                )
            })
            .into_any_element()
    }

    /// Trả lời một yêu cầu xin quyền (mục thứ `index` của phiên đang chọn).
    fn answer_agent_permission(&mut self, index: usize, decision: PermState, cx: &mut Context<Self>) {
        let s = self.agent.current_mut();
        let Some(AgentItem::Permission { id, tool, input, state, .. }) = s.items.get(index) else { return };
        if *state != PermState::Pending {
            return;
        }
        let (id, tool, input) = (id.clone(), tool.clone(), input.clone());
        let allow = decision != PermState::Denied;
        let result = match &s.run {
            Some(run) => run.respond_permission(&id, allow, &input).map_err(|e| e.to_string()),
            None => Err(t!("agent.permission.expired").to_string()),
        };
        let new_state = match result {
            Ok(()) => decision,
            Err(e) => {
                s.push(AgentItem::Error(t!("agent.permission.send_failed", e = e)));
                PermState::Expired
            }
        };
        if new_state == PermState::AllowedAlways {
            s.always_allow.insert(tool);
        }
        if let Some(AgentItem::Permission { state, .. }) = s.items.get_mut(index) {
            *state = new_state;
        }
        cx.notify();
    }

    fn toggle_agent_log(&mut self, start: usize, cx: &mut Context<Self>) {
        let s = self.agent.current_mut();
        if !s.expanded.remove(&start) {
            s.expanded.insert(start);
        }
        cx.notify();
    }

    /// Nút chọn model ở ô nhập (menu mở lên trên): Mặc định + model của CLI.
    fn render_agent_model_picker(&self, s: &AgentSession, cx: &Context<Self>) -> impl IntoElement {
        let kind = s.cli;
        let models = self.agent.models(kind);
        let current = s.model.clone();
        let label: SharedString = match &current {
            Some(m) => models.iter().find(|(v, _)| v == m).map_or_else(|| m.clone(), |(_, l)| l.clone()).into(),
            None => t!("agent.model.default").into(),
        };
        let view = cx.entity();
        Button::new("agent-model")
            .icon(agent_icon(kind).text_color(agent_color(kind, cx)))
            .label(label)
            .xsmall()
            .ghost()
            .tooltip(t!("agent.model.tooltip"))
            .dropdown_menu_with_anchor(Corner::BottomLeft, move |menu, _, _| {
                let options = std::iter::once((None, t!("agent.model.default").to_string()))
                    .chain(models.iter().map(|(v, l)| (Some(v.clone()), l.clone())));
                options.fold(menu.max_h(px(320.)).scrollable(true), |menu, (value, label)| {
                    let view = view.clone();
                    let checked = value == current;
                    menu.item(PopupMenuItem::new(label).checked(checked).on_click(move |_, _, cx| {
                        let value = value.clone();
                        view.update(cx, |this, cx| this.set_agent_model(value, cx));
                    }))
                })
            })
    }

    /// Nút chọn mức suy luận; CLI không có tuỳ chọn này (agy) thì không hiện.
    fn render_agent_effort_picker(&self, s: &AgentSession, cx: &Context<Self>) -> Option<impl IntoElement> {
        let efforts = s.cli.efforts();
        if efforts.is_empty() {
            return None;
        }
        let current = s.effort.clone();
        let label: SharedString = match &current {
            Some(e) => t!("agent.effort.value", e = e).into(),
            None => t!("agent.effort.default").into(),
        };
        let view = cx.entity();
        Some(
            Button::new("agent-effort")
                .label(label)
                .xsmall()
                .ghost()
                .tooltip(t!("agent.effort.tooltip"))
                .dropdown_menu_with_anchor(Corner::BottomLeft, move |menu, _, _| {
                    let options = std::iter::once((None, t!("agent.effort.default").to_string()))
                        .chain(efforts.iter().map(|e| (Some(e.to_string()), e.to_string())));
                    options.fold(menu, |menu, (value, label)| {
                        let view = view.clone();
                        let checked = value == current;
                        menu.item(PopupMenuItem::new(label).checked(checked).on_click(move |_, _, cx| {
                            let value = value.clone();
                            view.update(cx, |this, cx| this.set_agent_effort(value, cx));
                        }))
                    })
                }),
        )
    }

    /// Danh sách phiên đã lưu: icon agent, tên, thời gian, số lượt; bấm để mở lại, nút xoá.
    fn render_agent_history(&self, p: Pal, cx: &Context<Self>) -> AnyElement {
        let center = |text: String, color: Hsla| {
            v_flex()
                .flex_1()
                .min_h_0()
                .p_4()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(color)
                .text_center()
                .child(text)
                .into_any_element()
        };
        let list = match &self.agent.history {
            None => return center(t!("agent.history.loading").to_string(), p.muted),
            Some(Err(e)) => return center(t!("agent.history.error", e = e), p.danger),
            Some(Ok(list)) if list.is_empty() => return center(t!("agent.history.empty").to_string(), p.muted),
            Some(Ok(list)) => list,
        };
        let now = branchpad_core::now_unix();
        let rows = list.iter().enumerate().map(|(i, h)| {
            let kind = CliKind::from_id(&h.cli).unwrap_or_default();
            let open_id = h.id.clone();
            let delete_id = h.id.clone();
            let title: SharedString = if h.title.is_empty() { t!("agent.session.new").into() } else { h.title.clone().into() };
            let mut meta = vec![kind.label().to_string(), crate::i18n::relative_time(h.updated, now)];
            meta.push(t!("agent.history.messages", n = h.messages));
            if !h.can_resume {
                meta.push(t!("agent.history.no_resume").to_string());
            }
            h_flex()
                .id(("agent-history-row", i))
                .w_full()
                .px_3()
                .py_2()
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(p.border)
                .cursor_pointer()
                .hover(|st| st.bg(p.hover))
                .child(agent_avatar(kind, 26., cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(div().text_sm().text_color(p.fg).truncate().child(title))
                        .child(div().text_xs().text_color(p.muted).truncate().child(meta.join("  ·  "))),
                )
                .child(
                    Button::new(("agent-history-delete", i))
                        .label("×")
                        .xsmall()
                        .ghost()
                        .tooltip(t!("agent.history.delete"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.delete_agent_history(delete_id.clone(), cx);
                        })),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.open_agent_history(open_id.clone(), cx)))
        });
        v_flex()
            .id("agent-history")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(p.muted)
                    .child(t!("agent.history.title")),
            )
            .children(rows)
            .into_any_element()
    }

    /// Chữ của một tin nhắn: markdown, bôi đen được để copy (Cmd+C); vừa bôi đen thì hiện nút trích dẫn.
    fn render_agent_message(
        &self,
        sid: u64,
        i: usize,
        body: &str,
        p: Pal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let quoting = self.agent.quote.as_ref().is_some_and(|(qs, qi, _)| *qs == sid && *qi == i);
        let view = TextView::markdown(SharedString::from(format!("agent-msg:{sid}:{i}")), body.to_string(), window, cx)
            .selectable(true)
            // Kéo chọn được từ lề trái/phải và khoảng đệm quanh chữ (không chỉ đúng trên chữ).
            .selection_slop(px(48.), px(6.))
            // Chọn xong (kéo / bấm đúp / bấm ba lần): hiện nút trích dẫn cho đoạn vừa chọn.
            .on_select({
                let view = cx.entity();
                move |text, _, cx| {
                    view.update(cx, |this, cx| {
                        this.agent.quote = Some((sid, i, text));
                        cx.notify();
                    })
                }
            });
        v_flex()
            .gap_1()
            .child(
                div()
                    .id(("agent-msg", i))
                    .text_sm()
                    .text_color(p.fg)
                    .cursor_text()
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, _| this.agent.press_at = Some(e.position)))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseUpEvent, window, cx| this.on_agent_text_selected(sid, i, e.position, window, cx)),
                    )
                    .child(view)
                    // Chuột phải: copy / trích dẫn đoạn đang chọn, copy cả tin nhắn.
                    .context_menu({
                        let view = cx.entity();
                        let whole = body.to_string();
                        move |menu, _, cx| {
                            let selected = view
                                .read(cx)
                                .agent
                                .quote
                                .as_ref()
                                .filter(|(qs, qi, _)| *qs == sid && *qi == i)
                                .map(|(_, _, t)| t.clone());
                            let mut menu = menu;
                            if let Some(text) = selected {
                                menu = menu
                                    .item(PopupMenuItem::new(t!("agent.menu.copy")).on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()))
                                    }))
                                    .item(PopupMenuItem::new(t!("agent.quote.button")).on_click({
                                        let view = view.clone();
                                        move |_, window, cx| view.update(cx, |this, cx| this.quote_into_composer(window, cx))
                                    }))
                                    .separator();
                            }
                            let whole = whole.clone();
                            menu.item(PopupMenuItem::new(t!("agent.menu.copy_message")).on_click(move |_, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(whole.clone()))
                            }))
                        }
                    }),
            )
            .when(quoting, |d| {
                d.child(
                    h_flex().child(
                        Button::new(("agent-quote", i))
                            .label(t!("agent.quote.button"))
                            .xsmall()
                            .outline()
                            .tooltip(t!("agent.quote.tooltip"))
                            .on_click(cx.listener(|this, _, window, cx| this.quote_into_composer(window, cx))),
                    ),
                )
            })
            .into_any_element()
    }

    /// Nhả chuột trong tin nhắn: có kéo thì lấy đoạn đã bôi đen (qua lệnh Copy của TextView, rồi trả lại
    /// clipboard cũ) để hiện nút trích dẫn; chỉ bấm thì ẩn nút.
    fn on_agent_text_selected(&mut self, sid: u64, i: usize, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        // Đoạn chọn do TextView báo qua `on_select` (kéo / bấm đúp / bấm ba lần); ở đây chỉ ẩn nút
        // khi người dùng bấm một lần (không kéo) để bỏ chọn.
        let _ = (sid, i, window);
        let Some(start) = self.agent.press_at.take() else { return };
        let dragged = (at.x - start.x).abs() > px(3.) || (at.y - start.y).abs() > px(3.);
        if !dragged && self.agent.quote.is_some() {
            self.agent.quote = None;
            cx.notify();
        }
    }

    /// Đưa đoạn đã bôi đen vào ô nhập dưới dạng trích dẫn markdown (`> ...`), giữ chữ đang gõ dở.
    /// Nút "Trích dẫn": đoạn đã bôi đen thành thẻ trích dẫn trên ô nhập (gửi kèm tin nhắn kế tiếp),
    /// chữ đang gõ dở giữ nguyên.
    fn quote_into_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, _, text)) = self.agent.quote.take() else { return };
        if !self.agent.quotes.contains(&text) {
            self.agent.quotes.push(text);
        }
        self.agent.input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn remove_agent_quote(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.agent.quotes.len() {
            self.agent.quotes.remove(i);
        }
        cx.notify();
    }

    fn render_agent_item(
        &self,
        i: usize,
        s: &AgentSession,
        item: &AgentItem,
        p: Pal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let kind = s.cli;
        let sid = s.id;
        let worktree = self.agent_worktree_dir(s);
        match item {
            // Yêu cầu của người dùng: bong bóng bên phải.
            AgentItem::User(s) => {
                // Khối trích dẫn ở đầu tin nhắn vẽ thành thẻ riêng, phần còn lại là markdown.
                let (quotes, rest) = split_quotes(s);
                let quote_cards: Vec<AnyElement> =
                    quotes.iter().map(|(h, b)| quote_card(h.as_deref(), b, None, p, cx).into_any_element()).collect();
                let message = (!rest.trim().is_empty()).then(|| self.render_agent_message(sid, i, &rest, p, window, cx));
                let body = v_flex().gap_2().children(quote_cards).children(message);
                h_flex()
                .w_full()
                .justify_end()
                .child(
                    // flex_none: bong bóng rộng theo nội dung (một dòng nếu đủ chỗ), chỉ xuống dòng khi chạm
                    // giới hạn 85% bề rộng panel; không thì flex co nó về bề rộng tối thiểu và chữ gãy sớm.
                    div()
                        .flex_none()
                        .max_w(gpui::relative(0.85))
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(p.primary.opacity(0.12))
                        .border_1()
                        .border_color(p.primary.opacity(0.2))
                        .child(body),
                )
                .into_any_element()
            }
            // Câu trả lời: avatar agent bên trái, chữ trải hết bề ngang.
            AgentItem::Text(s) => {
                let body = self.render_agent_message(sid, i, s, p, window, cx);
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_start()
                    .child(agent_avatar(kind, 22., cx))
                    .child(div().flex_1().min_w_0().pt(px(2.)).child(body))
                    .into_any_element()
            }
            // Suy nghĩ / bước công cụ chỉ hiện trong nhóm "nhật ký" khi người dùng mở ra: một dòng gọn mỗi mục.
            AgentItem::Thinking(s) => div()
                .pl_2()
                .border_l_2()
                .border_color(p.border)
                .text_xs()
                .italic()
                .text_color(p.muted)
                .whitespace_normal()
                .child(branchpad_short(s, 300))
                .into_any_element(),
            AgentItem::Tool { kind: tool, name, detail, state, summary, .. } => {
                let state_el = match state {
                    ToolState::Running => Spinner::new().color(p.primary).xsmall().into_any_element(),
                    ToolState::Ok => Icon::new(IconName::CircleCheck).size_3().text_color(p.success).into_any_element(),
                    ToolState::Failed => Icon::new(IconName::CircleX).size_3().text_color(p.danger).into_any_element(),
                };
                // Bước lỗi mà CLI không nói lý do: vẫn ghi rõ là lỗi / bị chặn.
                let failure = (*state == ToolState::Failed)
                    .then(|| if summary.is_empty() { t!("agent.tool.failed").to_string() } else { summary.clone() });
                v_flex()
                    .text_xs()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(div().flex_none().child(state_el))
                            .child(Icon::new(tool_icon(*tool)).size_3().text_color(p.muted).flex_none())
                            .child(div().flex_none().text_color(p.fg).child(name.clone()))
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
                    .when_some(failure, |d, f| d.child(div().pl(px(34.)).whitespace_normal().text_color(p.danger).child(f)))
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
            // Mốc hết lượt: đường kẻ + nhãn trạng thái ở giữa, để biết rõ AI đã dừng.
            // Xin quyền: thẻ nổi bật với nút trả lời; trả lời xong chỉ còn một dòng kết quả.
            AgentItem::Permission { tool, detail, state, .. } => {
                let state = *state;
                let head = h_flex()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(agent_icon(kind)).size_3p5().text_color(agent_color(kind, cx)).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .text_color(p.fg)
                            .child(t!("agent.permission.title", cli = kind.label(), tool = tool)),
                    );
                let detail_box = (!detail.is_empty()).then(|| {
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(p.secondary)
                        .text_xs()
                        .font_family(crate::ui::MONO)
                        .text_color(p.fg)
                        .whitespace_normal()
                        .child(detail.clone())
                });
                let (accent, outcome): (Hsla, Option<(IconName, &'static str)>) = match state {
                    PermState::Pending => (p.warning, None),
                    PermState::Allowed => (p.success, Some((IconName::CircleCheck, t!("agent.permission.allowed")))),
                    PermState::AllowedAlways => (p.success, Some((IconName::CircleCheck, t!("agent.permission.allowed_always")))),
                    PermState::Denied => (p.danger, Some((IconName::CircleX, t!("agent.permission.denied")))),
                    PermState::Expired => (p.muted, Some((IconName::CircleX, t!("agent.permission.expired")))),
                };
                v_flex()
                    .ml(px(30.))
                    .p_2()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(accent.opacity(0.45))
                    .bg(accent.opacity(0.08))
                    .child(head)
                    .children(detail_box)
                    .child(match outcome {
                        Some((icon, label)) => h_flex()
                            .gap_1()
                            .items_center()
                            .text_xs()
                            .text_color(accent)
                            .child(Icon::new(icon).size_3())
                            .child(label)
                            .into_any_element(),
                        None => h_flex()
                            .gap_2()
                            .child(
                                Button::new(("agent-perm-allow", i))
                                    .label(t!("agent.permission.allow"))
                                    .xsmall()
                                    .primary()
                                    .on_click(cx.listener(move |this, _, _, cx| this.answer_agent_permission(i, PermState::Allowed, cx))),
                            )
                            .child(
                                Button::new(("agent-perm-always", i))
                                    .label(t!("agent.permission.allow_always"))
                                    .xsmall()
                                    .tooltip(t!("agent.permission.allow_always_tooltip"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.answer_agent_permission(i, PermState::AllowedAlways, cx)
                                    })),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new(("agent-perm-deny", i))
                                    .label(t!("agent.permission.deny"))
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| this.answer_agent_permission(i, PermState::Denied, cx))),
                            )
                            .into_any_element(),
                    })
                    .into_any_element()
            }
            AgentItem::TurnEnd { ok, text } => {
                let color = if *ok { p.success } else { p.danger };
                let line = || div().flex_1().h(px(1.)).bg(p.border);
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(line())
                    .child(
                        h_flex()
                            .flex_none()
                            .max_w(gpui::relative(0.8))
                            .gap_1()
                            .px_2()
                            .py(px(2.))
                            .items_center()
                            .rounded_full()
                            .border_1()
                            .border_color(color.opacity(0.35))
                            .bg(color.opacity(0.1))
                            .text_xs()
                            .text_color(color)
                            .child(Icon::new(if *ok { IconName::CircleCheck } else { IconName::CircleX }).size_3().flex_none())
                            .child(div().min_w_0().truncate().child(text.clone())),
                    )
                    .child(line())
                    .into_any_element()
            }
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

/// Mã file lịch sử: thời điểm tạo (nano giây, hệ 16) + số thứ tự phiên trong tab.
fn new_record_id(id: u64) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}-{id}")
}

/// Thẻ trích dẫn kiểu DevDesk: vạch màu bên trái, dòng nguồn nhỏ có icon, đoạn trích chữ mờ;
/// `action` (nút bỏ) nằm góc phải khi đang soạn.
fn quote_card(header: Option<&str>, body: &str, action: Option<Button>, p: Pal, _cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .rounded_md()
        .bg(p.bg.opacity(0.6))
        .border_1()
        .border_color(p.border)
        .overflow_hidden()
        .child(div().w(px(3.)).flex_none().bg(p.primary))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .px_2()
                .py_1p5()
                .gap_0p5()
                .when_some(header, |d, h| {
                    d.child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(p.primary)
                            .child(div().flex_none().child("❝"))
                            .child(div().min_w_0().truncate().child(h.to_string())),
                    )
                })
                .child(div().text_sm().text_color(p.muted).whitespace_normal().child(body.to_string())),
        )
        .children(action.map(|b| div().flex_none().p_0p5().child(b)))
}

/// Ghép thẻ trích dẫn vào tin nhắn: mỗi đoạn là một khối `> [nguồn]` + các dòng `> ...`, cách nhau dòng trống.
fn join_quotes(header: &str, quotes: &[String], text: &str) -> String {
    let mut out = String::new();
    for q in quotes {
        out.push_str(&format!("> [{header}]\n"));
        for line in q.lines() {
            out.push_str(&format!("> {line}\n"));
        }
        out.push('\n');
    }
    out.push_str(text);
    out.trim_end().to_string()
}

/// Tách các khối trích dẫn ở đầu tin nhắn: (danh sách (nguồn, nội dung), phần còn lại).
/// Dòng đầu khối dạng `> [..]` là nguồn; khối không có dòng nguồn vẫn được nhận.
fn split_quotes(text: &str) -> (Vec<(Option<String>, String)>, String) {
    let mut quotes: Vec<(Option<String>, String)> = Vec::new();
    let mut lines = text.lines().peekable();
    let mut in_block = false;
    while let Some(line) = lines.peek() {
        if let Some(q) = line.strip_prefix('>') {
            let q = q.strip_prefix(' ').unwrap_or(q);
            let header = q.strip_prefix('[').and_then(|h| h.strip_suffix(']')).filter(|h| !h.contains("]("));
            match (header, in_block) {
                (Some(h), _) => quotes.push((Some(h.to_string()), String::new())),
                (None, false) => quotes.push((None, q.to_string())),
                (None, true) => {
                    if let Some(last) = quotes.last_mut() {
                        if !last.1.is_empty() {
                            last.1.push('\n');
                        }
                        last.1.push_str(q);
                    }
                }
            }
            in_block = true;
        } else if line.trim().is_empty() && in_block {
            in_block = false;
        } else if line.trim().is_empty() && !quotes.is_empty() {
            // Dòng trống giữa các khối / sau khối cuối.
        } else {
            break;
        }
        lines.next();
    }
    let rest: Vec<&str> = lines.collect();
    (quotes, rest.join("\n"))
}

/// Mục thuộc "nhật ký làm việc" (ẩn trong nhóm thu gọn), không phải kết quả.
fn is_work(item: &AgentItem) -> bool {
    matches!(item, AgentItem::Tool { .. } | AgentItem::Thinking(_))
}

fn tool_kind_id(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Read => "read",
        ToolKind::Edit => "edit",
        ToolKind::Command => "command",
        ToolKind::Search => "search",
        ToolKind::Web => "web",
        ToolKind::Other => "other",
    }
}

fn tool_kind_from_id(id: &str) -> ToolKind {
    match id {
        "read" => ToolKind::Read,
        "edit" => ToolKind::Edit,
        "command" => ToolKind::Command,
        "search" => ToolKind::Search,
        "web" => ToolKind::Web,
        _ => ToolKind::Other,
    }
}

fn to_history(item: &AgentItem) -> HistoryItem {
    match item {
        AgentItem::User(t) => HistoryItem::User { text: t.clone() },
        AgentItem::Text(t) => HistoryItem::Text { text: t.clone() },
        AgentItem::Thinking(t) => HistoryItem::Thinking { text: t.clone() },
        AgentItem::Tool { kind, name, detail, state, summary, .. } => HistoryItem::Tool {
            kind: tool_kind_id(*kind).to_string(),
            name: name.clone(),
            detail: detail.clone(),
            ok: match state {
                ToolState::Running => None,
                ToolState::Ok => Some(true),
                ToolState::Failed => Some(false),
            },
            summary: summary.clone(),
        },
        AgentItem::File(path) => HistoryItem::File { path: path.clone() },
        AgentItem::Warning(t) => HistoryItem::Warning { text: t.clone() },
        AgentItem::Error(t) => HistoryItem::Error { text: t.clone() },
        AgentItem::TurnEnd { ok, text } => HistoryItem::TurnEnd { ok: *ok, text: text.clone() },
        AgentItem::Permission { tool, detail, state, .. } => HistoryItem::Permission {
            tool: tool.clone(),
            detail: detail.clone(),
            decision: match state {
                PermState::Allowed | PermState::AllowedAlways => "allowed",
                PermState::Denied => "denied",
                PermState::Pending | PermState::Expired => "expired",
            }
            .to_string(),
        },
    }
}

fn from_history(item: HistoryItem) -> AgentItem {
    match item {
        HistoryItem::User { text } => AgentItem::User(text),
        HistoryItem::Text { text } => AgentItem::Text(text),
        HistoryItem::Thinking { text } => AgentItem::Thinking(text),
        HistoryItem::Tool { kind, name, detail, ok, summary } => AgentItem::Tool {
            id: String::new(),
            kind: tool_kind_from_id(&kind),
            name,
            detail,
            // Lượt bị ngắt khi tắt app: coi như lỗi thay vì quay mãi.
            state: if ok == Some(true) { ToolState::Ok } else { ToolState::Failed },
            summary,
        },
        HistoryItem::File { path } => AgentItem::File(path),
        HistoryItem::Warning { text } => AgentItem::Warning(text),
        HistoryItem::Error { text } => AgentItem::Error(text),
        HistoryItem::TurnEnd { ok, text } => AgentItem::TurnEnd { ok, text },
        HistoryItem::Permission { tool, detail, decision } => AgentItem::Permission {
            id: String::new(),
            tool,
            detail,
            input: serde_json::Value::Null,
            state: match decision.as_str() {
                "allowed" => PermState::Allowed,
                "denied" => PermState::Denied,
                _ => PermState::Expired,
            },
        },
    }
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
    fn quotes_round_trip() {
        let msg = join_quotes("From Claude", &["line one\nline two".to_string(), "other".to_string()], "my question");
        let (quotes, rest) = split_quotes(&msg);
        assert_eq!(quotes.len(), 2);
        assert_eq!(quotes[0], (Some("From Claude".into()), "line one\nline two".into()));
        assert_eq!(quotes[1].1, "other");
        assert_eq!(rest, "my question");
        let (none, rest) = split_quotes("plain text\n> not a leading quote");
        assert!(none.is_empty());
        assert!(rest.starts_with("plain"));
        let (q, rest) = split_quotes("> old style quote\n\nreply");
        assert_eq!(q, vec![(None, "old style quote".to_string())]);
        assert_eq!(rest, "reply");
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
