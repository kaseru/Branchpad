//! Cửa sổ chính: mỗi repo một tab, nhớ các repo đã mở, quản lý cài đặt và giao diện.

use std::path::PathBuf;

use branchpad_core::{git::Repo, repo_scan::find_git_repos, Settings};
use gpui::{
    div, prelude::*, px, Context, Entity, FocusHandle, MouseButton, PathPromptOptions,
    SharedString, Subscription, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Icon, Sizable,
};

use crate::actions::{
    Cancel, CloseRepository, NextRepository, OpenRepository, OpenSettings, PreviousRepository,
};
use crate::icons::BpIcon;
use crate::repo_view::{RepoView, RepoViewEvent};
use crate::ui::confirm;

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
    _appearance: Subscription,
}

impl Workspace {
    pub fn new(cli_paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = Settings::load();
        crate::ui::apply_settings(&settings, window, cx);

        let appearance = cx.observe_window_appearance(window, |this, window, cx| {
            crate::ui::apply_settings(&this.settings, window, cx);
        });

        let mut ws = Workspace {
            tabs: Vec::new(),
            active: 0,
            focus: cx.focus_handle(),
            error: None,
            settings,
            show_settings: false,
            _appearance: appearance,
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
        let sub = cx.subscribe_in(
            &view,
            window,
            |this, _, event: &RepoViewEvent, window, cx| match event {
                RepoViewEvent::OpenRepository(path) => this.open_path(path.clone(), window, cx),
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

    fn persist(&mut self) {
        self.settings.repos = self.tabs.iter().map(|t| t.path.clone()).collect();
        self.settings.active = self.active;
        if let Err(e) = self.settings.save() {
            eprintln!("branchpad: cannot save settings: {e}");
        }
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
        if tab.view.read(cx).has_unsaved(cx) {
            let path = tab.path.clone();
            confirm(
                window,
                cx,
                &t!("workspace.close.unsaved_title", name = tab.name),
                t!("workspace.close.unsaved_detail"),
                &[t!("common.cancel"), t!("common.close")],
                move |this: &mut Self, _, _, cx| {
                    if let Some(i) = this.tabs.iter().position(|t| t.path == path) {
                        this.remove_tab(i, cx);
                    }
                },
            );
            return;
        }
        self.remove_tab(i, cx);
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
        cx.notify();
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
        cx.notify();
    }

    pub(crate) fn set_row_height(&mut self, height: f32, cx: &mut Context<Self>) {
        self.settings.row_height = height;
        let _ = self.settings.save();
        cx.notify();
    }

    pub(crate) fn set_mono_font(&mut self, font: String, cx: &mut Context<Self>) {
        self.settings.mono_font = font;
        let _ = self.settings.save();
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
        cx.notify();
    }

    // ---- Vẽ thanh Tab chính ----

    fn render_tab_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        h_flex()
            .id("repo-tabs")
            .h(px(34.))
            .w_full()
            .flex_none()
            .bg(t.tab_bar)
            .border_b_1()
            .border_color(t.border)
            .items_center()
            .child(
                h_flex()
                    .id("repo-tab-scroll")
                    .flex_1()
                    .h_full()
                    .items_center()
                    .overflow_x_scroll()
                    .children(self.tabs.iter().enumerate().map(|(i, tab)| {
                        let active = i == self.active;
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
            .on_action(cx.listener(Self::open_repository))
            .on_action(cx.listener(Self::close_repository))
            .on_action(cx.listener(Self::next_repository))
            .on_action(cx.listener(Self::previous_repository))
            .on_action(cx.listener(Self::open_settings))
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
