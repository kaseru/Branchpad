//! Cửa sổ chính: mỗi repo một tab, nhớ các repo đã mở, quản lý cài đặt và giao diện.

use std::path::PathBuf;

use branchpad_agent::{cli_version, delete_claude_token, find_cli, load_claude_token, save_claude_token, CliKind, Secret};
use branchpad_core::{git::Repo, repo_scan::find_git_repos, Settings};
use gpui::{
    div, prelude::*, px, Context, Entity, ExternalPaths, FocusHandle, MouseButton, PathPromptOptions,
    SharedString, Subscription, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{InputEvent, InputState},
    v_flex, ActiveTheme, Icon, Sizable,
};

use crate::actions::{
    Cancel, CloseRepository, NextRepository, OpenRepository, OpenSettings, PreviousRepository, ZoomIn,
    ZoomOut, ZoomReset,
};
use crate::icons::BpIcon;
use crate::repo_view::{AgentConfig, RepoView, RepoViewEvent};
use crate::ui::confirm;

/// Dữ liệu kéo thả một tab repo để đổi vị trí.
#[derive(Clone)]
struct DraggedRepoTab {
    ix: usize,
    name: SharedString,
}

impl Render for DraggedRepoTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        div()
            .px_3()
            .py_1()
            .text_xs()
            .rounded_sm()
            .border_1()
            .border_color(t.border)
            .bg(t.tab_active)
            .text_color(t.foreground)
            .opacity(0.9)
            .child(self.name.clone())
    }
}

/// Ô nhập của mục AI trong Cài đặt.
pub(crate) struct AgentInputs {
    pub(crate) claude_path: Entity<InputState>,
    pub(crate) codex_path: Entity<InputState>,
    pub(crate) claude_model: Entity<InputState>,
    pub(crate) codex_model: Entity<InputState>,
    /// Ô dán token Claude (che ký tự); lưu xong thì xoá trắng, không hiện lại token.
    pub(crate) claude_token: Entity<InputState>,
}

/// CLI tìm thấy trên máy (hiện trong Cài đặt): đường dẫn và phiên bản.
pub(crate) struct AgentCliInfo {
    pub(crate) kind: CliKind,
    pub(crate) path: Option<PathBuf>,
    pub(crate) version: Option<String>,
}

struct RepoTab {
    /// Thư mục gốc repo (lưu vào settings).
    path: PathBuf,
    name: SharedString,
    view: Entity<RepoView>,
    _sub: Subscription,
}

pub struct Workspace {
    tabs: Vec<RepoTab>,
    active: usize,
    focus: FocusHandle,
    error: Option<String>,
    pub(crate) settings: Settings,
    pub(crate) show_settings: bool,
    pub(crate) agent_inputs: AgentInputs,
    pub(crate) agent_found: Vec<AgentCliInfo>,
    pub(crate) agent_checking: bool,
    /// Token Claude trong Keychain (`None` = chưa có / chưa nạp xong).
    pub(crate) claude_token: Option<Secret>,
    pub(crate) token_loaded: bool,
    pub(crate) token_busy: bool,
    pub(crate) token_error: Option<String>,
    _appearance: Subscription,
    _agent_subs: Vec<Subscription>,
}

impl Workspace {
    pub fn new(cli_paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = Settings::load();
        crate::ui::apply_settings(&settings, window, cx);

        let appearance = cx.observe_window_appearance(window, |this, window, cx| {
            crate::ui::apply_settings(&this.settings, window, cx);
        });

        let mut text_input = |value: Option<String>, placeholder: &'static str| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            if let Some(v) = value.filter(|v| !v.is_empty()) {
                input.update(cx, |s, cx| s.set_value(v, window, cx));
            }
            input
        };
        let path_text = |p: &Option<PathBuf>| p.as_ref().map(|p| p.to_string_lossy().into_owned());
        let agent_inputs = AgentInputs {
            claude_path: text_input(path_text(&settings.claude_path), t!("settings.agent.path_placeholder")),
            codex_path: text_input(path_text(&settings.codex_path), t!("settings.agent.path_placeholder")),
            claude_model: text_input(settings.claude_model.clone(), t!("settings.agent.model_placeholder")),
            codex_model: text_input(settings.codex_model.clone(), t!("settings.agent.model_placeholder")),
            claude_token: cx.new(|cx| InputState::new(window, cx).masked(true).placeholder(t!("settings.agent.token_placeholder"))),
        };
        // Đăng ký sau khi đặt giá trị ban đầu để không lưu lại ngay lúc mở app.
        let agent_subs = vec![
            Self::watch_agent_input(&agent_inputs.claude_path, |s, v| s.claude_path = (!v.is_empty()).then(|| PathBuf::from(v)), window, cx),
            Self::watch_agent_input(&agent_inputs.codex_path, |s, v| s.codex_path = (!v.is_empty()).then(|| PathBuf::from(v)), window, cx),
            Self::watch_agent_input(&agent_inputs.claude_model, |s, v| s.claude_model = (!v.is_empty()).then_some(v), window, cx),
            Self::watch_agent_input(&agent_inputs.codex_model, |s, v| s.codex_model = (!v.is_empty()).then_some(v), window, cx),
        ];

        let mut ws = Workspace {
            tabs: Vec::new(),
            active: 0,
            focus: cx.focus_handle(),
            error: None,
            settings,
            show_settings: false,
            agent_inputs,
            agent_found: Vec::new(),
            agent_checking: false,
            claude_token: None,
            token_loaded: false,
            token_busy: false,
            token_error: None,
            _appearance: appearance,
            _agent_subs: agent_subs,
        };

        let mut errors = Vec::new();
        for path in &ws.settings.repos.clone() {
            if let Err(e) = ws.add_tab(path.clone(), window, cx) {
                errors.push(format!("{}: {e}", path.display()));
            }
        }
        ws.active = ws.settings.active.min(ws.tabs.len().saturating_sub(1));
        for path in cli_paths {
            match ws.add_folder(path.clone(), window, cx) {
                Ok(i) => ws.active = i,
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
        if !errors.is_empty() {
            ws.error = Some(t!("workspace.open_failed", errors = errors.join("\n")));
        }
        ws.persist();
        ws.load_token_from_keychain(cx);
        window.focus(&ws.focus);
        ws
    }

    /// Mở repo thành tab (nếu đã mở thì trả về tab đó).
    fn add_tab(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        let root = repo.workdir().to_path_buf();
        if let Some(i) = self.tabs.iter().position(|t| t.path == root) {
            return Ok(i);
        }
        let name = SharedString::from(repo.name());
        let view = cx.new(|cx| RepoView::new(repo, window, cx));
        let (size, family) = (self.settings.editor_font_size, SharedString::from(self.settings.mono_font.clone()));
        view.update(cx, |v, cx| v.set_editor_style(size, family, cx));
        let auto_save = self.settings.auto_save;
        view.update(cx, |v, cx| v.set_auto_save(auto_save, cx));
        let agent = self.agent_config();
        view.update(cx, |v, cx| v.set_agent_config(agent, cx));
        let agent_width = self.settings.agent_panel_width;
        view.update(cx, |v, cx| v.set_agent_width(agent_width, cx));
        let sub = cx.subscribe_in(
            &view,
            window,
            |this, _, event: &RepoViewEvent, window, cx| match event {
                RepoViewEvent::OpenRepository(path) => this.open_path(path.clone(), window, cx),
                RepoViewEvent::AgentWidthChanged(width) => this.set_agent_width(*width, cx),
            },
        );
        self.tabs.push(RepoTab {
            path: root,
            name,
            view,
            _sub: sub,
        });
        Ok(self.tabs.len() - 1)
    }

    /// Mở một thư mục: là repo (hoặc nằm trong repo) thì mở thành một tab, nếu không
    /// thì quét các git repo bên trong và mở từng cái. Trả về tab đầu tiên.
    fn add_folder(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let err = match self.add_tab(path.clone(), window, cx) {
            Ok(i) => return Ok(i),
            Err(e) => e,
        };
        if !path.is_dir() {
            return Err(err);
        }
        let repos = find_git_repos(&path);
        let mut first = None;
        let mut errors = Vec::new();
        for repo in &repos {
            match self.add_tab(repo.clone(), window, cx) {
                Ok(i) => {
                    first.get_or_insert(i);
                }
                Err(e) => errors.push(format!("{}: {e}", repo.display())),
            }
        }
        match first {
            Some(i) => {
                if !errors.is_empty() {
                    self.error = Some(t!("workspace.open_failed", errors = errors.join("\n")));
                }
                Ok(i)
            }
            None if errors.is_empty() => {
                Err(t!("workspace.no_repo_found", path = path.display()))
            }
            None => Err(errors.join("\n")),
        }
    }

    pub fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        match self.add_folder(path, window, cx) {
            Ok(i) => {
                self.active = i;
            }
            Err(e) => self.error = Some(e),
        }
        self.persist();
        cx.notify();
    }

    /// Kéo thả từ Finder/Explorer vào cửa sổ: thư mục thì mở thành repo, file thì mở trong
    /// trình soạn của repo chứa nó (chưa mở thì mở repo đó trước).
    pub(crate) fn open_dropped(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        for path in paths {
            if path.is_dir() {
                self.open_path(path.clone(), window, cx);
                continue;
            }
            let found = self.tabs.iter().position(|t| path.starts_with(&t.path));
            let index = match found {
                Some(i) => Some(i),
                None => match path.parent().map(|p| self.add_tab(p.to_path_buf(), window, cx)) {
                    Some(Ok(i)) => Some(i),
                    // Không phải git repo: không phải lỗi, mở file trong repo đang chọn.
                    // Chỉ báo lỗi khi chưa có repo nào để mở file vào.
                    Some(Err(e)) => {
                        if self.tabs.is_empty() {
                            self.error = Some(e);
                        }
                        None
                    }
                    None => None,
                },
            };
            // Không thuộc repo nào: mở trong repo đang chọn.
            let Some(i) = index.or_else(|| (!self.tabs.is_empty()).then_some(self.active)) else {
                continue;
            };
            self.active = i;
            let view = self.tabs[i].view.clone();
            let path = path.clone();
            view.update(cx, |v, cx| v.open_in_editor(path, None, window, cx));
        }
        self.persist();
        cx.notify();
    }

    fn persist(&mut self) {
        self.settings.repos = self.tabs.iter().map(|t| t.path.clone()).collect();
        self.settings.active = self.active;
        if let Err(e) = self.settings.save() {
            eprintln!("branchpad: cannot save settings: {e}");
        }
    }

    /// Kéo tab repo `from` thả vào vị trí của tab `to`; repo đang mở vẫn là repo cũ.
    fn move_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to || from >= self.tabs.len() || to >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = if self.active == from {
            to
        } else if from < self.active && to >= self.active {
            self.active - 1
        } else if from > self.active && to <= self.active {
            self.active + 1
        } else {
            self.active
        };
        self.persist();
        cx.notify();
    }

    fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.tabs.len() && i != self.active {
            self.active = i;
            self.persist();
            cx.notify();
        }
    }

    fn open_repository(&mut self, _: &OpenRepository, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("workspace.open_folder.prompt").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(path) = paths.into_iter().next() {
                    this.update_in(cx, |this, window, cx| this.open_path(path, window, cx))
                        .ok();
                }
            }
        })
        .detach();
    }

    fn close_tab(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(i) else { return };
        // Luôn hỏi trước khi đóng; có file chưa lưu thì cảnh báo mạnh (nút đỏ).
        let unsaved = tab.view.read(cx).has_unsaved(cx);
        let path = tab.path.clone();
        let (title, detail) = if unsaved {
            (t!("workspace.close.unsaved_title", name = tab.name), t!("workspace.close.unsaved_detail"))
        } else {
            (t!("workspace.close.title", name = tab.name), t!("workspace.close.detail"))
        };
        let on_answer = move |this: &mut Self, _: usize, _: &mut Window, cx: &mut Context<Self>| {
            if let Some(i) = this.tabs.iter().position(|t| t.path == path) {
                this.remove_tab(i, cx);
            }
        };
        let buttons = [t!("common.cancel"), t!("common.close")];
        if unsaved {
            confirm(window, cx, &title, &detail, &buttons, on_answer);
        } else {
            crate::ui::confirm_with_tone(
                window,
                cx,
                crate::ui::ModalTone::Primary,
                &title,
                &detail,
                &buttons,
                on_answer,
            );
        }
    }

    fn remove_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.tabs.len() {
            return;
        }
        self.tabs.remove(i);
        if self.active > i || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.persist();
        cx.notify();
    }

    fn close_repository(&mut self, _: &CloseRepository, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab(self.active, window, cx);
    }

    fn next_repository(&mut self, _: &NextRepository, _: &mut Window, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            self.select((self.active + 1) % self.tabs.len(), cx);
        }
    }

    fn previous_repository(&mut self, _: &PreviousRepository, _: &mut Window, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            self.select((self.active + self.tabs.len() - 1) % self.tabs.len(), cx);
        }
    }

    fn open_settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.show_settings = !self.show_settings;
        if self.show_settings {
            self.check_agent_clis(cx);
        }
        cx.notify();
    }

    // ---- Cài đặt AI ----

    /// Cài đặt AI cho các tab repo.
    fn agent_config(&self) -> AgentConfig {
        let s = &self.settings;
        AgentConfig {
            cli: CliKind::from_id(&s.agent_cli),
            claude_path: s.claude_path.clone(),
            codex_path: s.codex_path.clone(),
            claude_model: s.claude_model.clone(),
            codex_model: s.codex_model.clone(),
            use_worktree: s.agent_use_worktree,
            allow_shell: s.agent_allow_shell,
            claude_token: self.claude_token.clone(),
        }
    }

    /// Lưu cài đặt và đẩy cài đặt AI xuống mọi tab repo.
    fn save_agent_settings(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.settings.save() {
            eprintln!("branchpad: cannot save settings: {e}");
        }
        self.apply_agent_config(cx);
    }

    /// Đẩy cài đặt AI (kèm token) xuống mọi tab repo.
    fn apply_agent_config(&mut self, cx: &mut Context<Self>) {
        let config = self.agent_config();
        for tab in &self.tabs {
            tab.view.update(cx, |v, cx| v.set_agent_config(config.clone(), cx));
        }
        cx.notify();
    }

    fn watch_agent_input(
        input: &Entity<InputState>,
        apply: fn(&mut Settings, String),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(input, window, move |this, input, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change { .. }) {
                let value = input.read(cx).value().trim().to_string();
                apply(&mut this.settings, value);
                this.save_agent_settings(cx);
            }
        })
    }

    /// Lưu bề rộng panel AI và áp cho mọi tab repo.
    fn set_agent_width(&mut self, width: f32, cx: &mut Context<Self>) {
        if (self.settings.agent_panel_width - width).abs() < 0.5 {
            return;
        }
        self.settings.agent_panel_width = width;
        if let Err(e) = self.settings.save() {
            eprintln!("branchpad: cannot save settings: {e}");
        }
        for tab in &self.tabs {
            tab.view.update(cx, |v, cx| v.set_agent_width(width, cx));
        }
    }

    /// Nạp token Claude từ Keychain ở luồng nền (lệnh `security`).
    fn load_token_from_keychain(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { load_claude_token() }).await;
            this.update(cx, |this, cx| {
                this.token_loaded = true;
                match result {
                    Ok(token) => this.claude_token = token,
                    Err(e) => {
                        eprintln!("branchpad: {e}");
                        this.token_error = Some(e.to_string());
                    }
                }
                this.apply_agent_config(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Nút Lưu: ghi token đang dán vào Keychain, xoá trắng ô nhập.
    pub(crate) fn save_token_to_keychain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.agent_inputs.claude_token.read(cx).value().trim().to_string();
        if token.is_empty() || self.token_busy {
            return;
        }
        self.token_busy = true;
        self.token_error = None;
        self.agent_inputs.claude_token.update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let secret = Secret::new(token);
            let to_save = secret.clone();
            let result = cx.background_spawn(async move { save_claude_token(to_save.expose()) }).await;
            this.update(cx, |this, cx| {
                this.token_busy = false;
                match result {
                    Ok(()) => this.claude_token = Some(secret),
                    Err(e) => this.token_error = Some(e.to_string()),
                }
                this.apply_agent_config(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Nút Xoá: bỏ token khỏi Keychain; Claude quay về dùng đăng nhập sẵn của CLI.
    pub(crate) fn delete_token_from_keychain(&mut self, cx: &mut Context<Self>) {
        if self.token_busy {
            return;
        }
        self.token_busy = true;
        self.token_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { delete_claude_token() }).await;
            this.update(cx, |this, cx| {
                this.token_busy = false;
                match result {
                    Ok(()) => this.claude_token = None,
                    Err(e) => this.token_error = Some(e.to_string()),
                }
                this.apply_agent_config(cx);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn set_agent_cli(&mut self, kind: Option<CliKind>, cx: &mut Context<Self>) {
        self.settings.agent_cli = kind.map(|k| k.id().to_string()).unwrap_or_default();
        self.save_agent_settings(cx);
    }

    pub(crate) fn set_agent_worktree(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.agent_use_worktree = on;
        self.save_agent_settings(cx);
    }

    pub(crate) fn set_agent_allow_shell(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.agent_allow_shell = on;
        self.save_agent_settings(cx);
    }

    /// Dò lại CLI và phiên bản (`--version`) ở luồng nền, để hiện trong Cài đặt.
    pub(crate) fn check_agent_clis(&mut self, cx: &mut Context<Self>) {
        if self.agent_checking {
            return;
        }
        self.agent_checking = true;
        let overrides = [self.settings.claude_path.clone(), self.settings.codex_path.clone()];
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    CliKind::ALL
                        .into_iter()
                        .zip(overrides)
                        .map(|(kind, path)| {
                            let path = find_cli(kind, path.as_deref());
                            let version = path.as_deref().and_then(cli_version);
                            AgentCliInfo { kind, path, version }
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                this.agent_checking = false;
                this.agent_found = found;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn on_cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        if self.show_settings {
            self.show_settings = false;
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    // ---- Cài đặt giao diện & font chữ ----

    pub(crate) fn set_theme(&mut self, theme: String, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.theme = theme;
        let _ = self.settings.save();
        crate::ui::apply_settings(&self.settings, window, cx);
        cx.notify();
    }

    pub(crate) fn set_ui_font_size(&mut self, size: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.ui_font_size = size;
        let _ = self.settings.save();
        crate::ui::apply_settings(&self.settings, window, cx);
        cx.notify();
    }

    pub(crate) fn set_editor_font_size(&mut self, size: f32, _window: &mut Window, cx: &mut Context<Self>) {
        self.settings.editor_font_size = size;
        let _ = self.settings.save();
        self.apply_editor_style(cx);
        cx.notify();
    }

    /// Đẩy cỡ chữ / font của trình soạn xuống mọi repo đang mở.
    fn apply_editor_style(&self, cx: &mut Context<Self>) {
        let (size, family) = (self.settings.editor_font_size, SharedString::from(self.settings.mono_font.clone()));
        for tab in &self.tabs {
            tab.view.update(cx, |v, cx| v.set_editor_style(size, family.clone(), cx));
        }
    }

    pub(crate) fn set_auto_save(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.auto_save = on;
        let _ = self.settings.save();
        for tab in &self.tabs {
            tab.view.update(cx, |v, cx| v.set_auto_save(on, cx));
        }
        cx.notify();
    }

    fn zoom_editor(&mut self, delta: f32, window: &mut Window, cx: &mut Context<Self>) {
        let size = if delta == 0.0 { 14.0 } else { (self.settings.editor_font_size + delta).clamp(9.0, 40.0) };
        self.set_editor_font_size(size, window, cx);
    }

    fn zoom_in(&mut self, _: &ZoomIn, window: &mut Window, cx: &mut Context<Self>) {
        self.zoom_editor(1.0, window, cx);
    }

    fn zoom_out(&mut self, _: &ZoomOut, window: &mut Window, cx: &mut Context<Self>) {
        self.zoom_editor(-1.0, window, cx);
    }

    fn zoom_reset(&mut self, _: &ZoomReset, window: &mut Window, cx: &mut Context<Self>) {
        self.zoom_editor(0.0, window, cx);
    }

    pub(crate) fn set_row_height(&mut self, height: f32, cx: &mut Context<Self>) {
        self.settings.row_height = height;
        let _ = self.settings.save();
        cx.notify();
    }

    pub(crate) fn set_mono_font(&mut self, font: String, cx: &mut Context<Self>) {
        self.settings.mono_font = font;
        let _ = self.settings.save();
        self.apply_editor_style(cx);
        cx.notify();
    }

    pub(crate) fn reset_settings_to_default(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repos = self.settings.repos.clone();
        let active = self.settings.active;
        self.settings = Settings::default();
        self.settings.repos = repos;
        self.settings.active = active;
        let _ = self.settings.save();
        crate::ui::apply_settings(&self.settings, window, cx);
        let inputs = &self.agent_inputs;
        for input in [&inputs.claude_path, &inputs.codex_path, &inputs.claude_model, &inputs.codex_model] {
            input.update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.save_agent_settings(cx);
        cx.notify();
    }

    // ---- Vẽ thanh Tab chính ----

    fn render_tab_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        h_flex()
            .id("repo-tabs")
            .h(px(38.))
            .w_full()
            // macOS: tab nằm ngay trong thanh tiêu đề, chừa chỗ cho 3 nút đóng/thu/phóng.
            .when(cfg!(target_os = "macos"), |d| d.pl(px(78.)))
            .flex_none()
            .bg(t.tab_bar)
            .border_b_1()
            .border_color(t.border)
            .items_center()
            .child(
                h_flex()
                    .id("repo-tab-scroll")
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .overflow_x_scroll()
                    .children(self.tabs.iter().enumerate().map(|(i, tab)| {
                        let active = i == self.active;
                        let hover_bg = t.tab_active.opacity(0.6);
                        let drag_border = t.primary;
                        h_flex()
                            .id(("repo-tab", i))
                            .h_full()
                            .px_3()
                            .gap_2()
                            .text_xs()
                            .items_center()
                            .border_r_1()
                            .border_color(t.border)
                            .cursor_pointer()
                            .when(active, |d| {
                                d.bg(t.tab_active)
                                    .text_color(t.foreground)
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                            })
                            .when(!active, |d| {
                                d.text_color(t.muted_foreground)
                                    .hover(|s| s.bg(t.list_hover))
                            })
                            .on_drag(
                                DraggedRepoTab { ix: i, name: tab.name.clone() },
                                |d, _, _, cx| cx.new(|_| d.clone()),
                            )
                            .drag_over::<DraggedRepoTab>(move |s, _, _, _| {
                                s.bg(hover_bg).border_l_2().border_color(drag_border)
                            })
                            .on_drop(cx.listener(move |this, d: &DraggedRepoTab, _, cx| {
                                this.move_tab(d.ix, i, cx);
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| this.select(i, cx)))
                            .on_mouse_down(
                                MouseButton::Middle,
                                cx.listener(move |this, _, window, cx| {
                                    this.close_tab(i, window, cx);
                                }),
                            )
                            .child(Icon::new(BpIcon::Branch).size_3().text_color(if active { t.primary } else { t.muted_foreground }))
                            .child(tab.name.clone())
                            .child(
                                div()
                                    .id(("repo-tab-close", i))
                                    .size(px(16.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .hover(|s| s.bg(gpui::white().opacity(0.12)))
                                    .child("×")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.close_tab(i, window, cx);
                                    })),
                            )
                    }))
                    .child(
                        Button::new("open-repo")
                            .label("+")
                            .ghost()
                            .xsmall()
                            .tooltip(t!("workspace.open_folder.tooltip"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_repository(&OpenRepository, window, cx)
                            })),
                    ),
            )
            // Phần trống của thanh tab: kéo để di chuyển cửa sổ (như Chrome).
            .child(
                div()
                    .id("repo-tabs-drag")
                    .flex_1()
                    .h_full()
                    .window_control_area(gpui::WindowControlArea::Drag)
                    // Bấm đúp vào chỗ trống: phóng to / thu nhỏ cửa sổ như thanh tiêu đề.
                    .on_click(|ev, window, _| {
                        if ev.click_count() == 2 {
                            window.titlebar_double_click();
                        }
                    }),
            )
            // Nút mở Cài đặt ở góc phải thanh Tab
            .child(
                h_flex()
                    .px_2()
                    .flex_none()
                    .child(
                        Button::new("tb-settings")
                            .icon(BpIcon::Settings)
                            .ghost()
                            .xsmall()
                            .tooltip(t!("actions.app.settings"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_settings = !this.show_settings;
                                cx.notify();
                            })),
                    ),
            )
    }

    // ---- Màn hình chào mừng (khi chưa mở repo nào) ----

    fn render_welcome_view(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let fg = t.foreground;
        let muted = t.muted_foreground;
        let card = t.secondary;
        let border = t.border;
        let primary = t.primary;

        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .p_8()
            .gap_6()
            .child(
                v_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(64.))
                            .rounded_lg()
                            .bg(primary.opacity(0.12))
                            .border_1()
                            .border_color(primary.opacity(0.3))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(primary)
                            .child(Icon::new(BpIcon::Branch).size_8()),
                    )
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(fg)
                            .child("Branchpad"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(t!("workspace.empty.hint")),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        Button::new("open-repo-welcome")
                            .label(t!("workspace.open_folder.label"))
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_repository(&OpenRepository, window, cx)
                            })),
                    )
                    .child(
                        Button::new("settings-welcome")
                            .icon(BpIcon::Settings)
                            .label(t!("settings.title"))
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_settings = true;
                                cx.notify();
                            })),
                    ),
            )
            // Gợi ý phím tắt nhanh
            .child(
                h_flex()
                    .p_3()
                    .gap_4()
                    .rounded_lg()
                    .bg(card)
                    .border_1()
                    .border_color(border)
                    .text_xs()
                    .text_color(muted)
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().px_1().py(px(2.)).rounded_sm().bg(border).text_color(fg).child("⌘O"))
                            .child(t!("actions.file.open_folder")),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().px_1().py(px(2.)).rounded_sm().bg(border).text_color(fg).child("⌘P"))
                            .child(t!("actions.file.quick_open")),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().px_1().py(px(2.)).rounded_sm().bg(border).text_color(fg).child("⌘,"))
                            .child(t!("actions.app.settings")),
                    ),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let (bg, fg, danger) = (t.background, t.foreground, t.danger);
        let content = match self.tabs.get(self.active) {
            Some(tab) => tab.view.clone().into_any_element(),
            None => self.render_welcome_view(cx).into_any_element(),
        };

        let modal = if self.show_settings {
            Some(self.render_settings_modal(window, cx))
        } else {
            None
        };

        v_flex()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.open_dropped(paths.paths(), window, cx);
            }))
            .on_action(cx.listener(Self::open_repository))
            .on_action(cx.listener(Self::close_repository))
            .on_action(cx.listener(Self::next_repository))
            .on_action(cx.listener(Self::previous_repository))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .on_action(cx.listener(Self::on_cancel))
            .size_full()
            .bg(bg)
            .text_color(fg)
            .relative()
            .child(self.render_tab_bar(cx))
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    h_flex()
                        .px_3()
                        .py_1()
                        .gap_2()
                        .text_sm()
                        .bg(danger.opacity(0.12))
                        .text_color(danger)
                        .child(div().flex_1().child(e))
                        .child(
                            Button::new("dismiss-ws-error")
                                .label(t!("common.close"))
                                .ghost()
                                .xsmall()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.error = None;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(div().flex_1().overflow_hidden().child(content))
            .children(modal)
            .children(gpui_component::Root::render_dialog_layer(window, cx))
    }
}
