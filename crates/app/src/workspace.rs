//! Cửa sổ chính: mỗi repo một tab, nhớ các repo đã mở.

use std::path::PathBuf;

use branchpad_core::{git::Repo, repo_scan::find_git_repos, Settings};
use gpui::{
    div, prelude::*, px, Context, Entity, FocusHandle, PathPromptOptions, SharedString,
    Subscription, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Sizable,
};

use crate::actions::{CloseRepository, NextRepository, OpenRepository, PreviousRepository};
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
    _appearance: Subscription,
}

impl Workspace {
    pub fn new(cli_paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Sáng/tối theo hệ thống.
        crate::ui::apply_appearance(window, cx);
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            crate::ui::apply_appearance(window, cx);
        });

        let settings = Settings::load();
        let mut ws = Workspace {
            tabs: Vec::new(),
            active: 0,
            focus: cx.focus_handle(),
            error: None,
            _appearance: appearance,
        };

        let mut errors = Vec::new();
        for path in &settings.repos {
            if let Err(e) = ws.add_tab(path.clone(), window, cx) {
                errors.push(format!("{}: {e}", path.display()));
            }
        }
        ws.active = settings.active.min(ws.tabs.len().saturating_sub(1));
        for path in cli_paths {
            match ws.add_folder(path.clone(), window, cx) {
                Ok(i) => ws.active = i,
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
        if !errors.is_empty() {
            ws.error = Some(format!("Không mở được repo:\n{}", errors.join("\n")));
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
                    self.error = Some(format!("Không mở được repo:\n{}", errors.join("\n")));
                }
                Ok(i)
            }
            None if errors.is_empty() => {
                Err(format!("Không tìm thấy git repo nào trong {}", path.display()))
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

    fn persist(&self) {
        let settings = Settings {
            repos: self.tabs.iter().map(|t| t.path.clone()).collect(),
            active: self.active,
        };
        if let Err(e) = settings.save() {
            eprintln!("branchpad: không lưu được settings: {e}");
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
            prompt: Some("Mở thư mục".into()),
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
                &format!("{} còn file chưa lưu", tab.name),
                "Đóng repo sẽ bỏ các thay đổi chưa lưu trong trình soạn.",
                &["Huỷ", "Đóng"],
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
            .overflow_x_scroll()
            .children(self.tabs.iter().enumerate().map(|(i, tab)| {
                let active = i == self.active;
                h_flex()
                    .id(("repo-tab", i))
                    .h_full()
                    .px_3()
                    .gap_2()
                    .text_sm()
                    .border_r_1()
                    .border_color(t.border)
                    .when(active, |d| d.bg(t.tab_active).text_color(t.foreground))
                    .when(!active, |d| d.text_color(t.muted_foreground))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(i, cx)))
                    .child(tab.name.clone())
                    .child(
                        div()
                            .id(("repo-tab-close", i))
                            .px_1()
                            .rounded_sm()
                            .hover(|s| s.bg(gpui::black().opacity(0.1)))
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
                    .small()
                    .tooltip("Mở thư mục (Cmd+O)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_repository(&OpenRepository, window, cx)
                    })),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let (bg, fg, muted, danger) = (t.background, t.foreground, t.muted_foreground, t.danger);
        let content = match self.tabs.get(self.active) {
            Some(tab) => tab.view.clone().into_any_element(),
            None => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_3()
                .child(div().text_xl().text_color(fg).child("Branchpad"))
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("Mở một repo git, hoặc thư mục chứa nhiều repo, để bắt đầu, hoặc chạy `branchpad .` trong Terminal."),
                )
                .child(
                    Button::new("open-repo-empty")
                        .label("Mở thư mục…")
                        .primary()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_repository(&OpenRepository, window, cx)
                        })),
                )
                .into_any_element(),
        };
        v_flex()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::open_repository))
            .on_action(cx.listener(Self::close_repository))
            .on_action(cx.listener(Self::next_repository))
            .on_action(cx.listener(Self::previous_repository))
            .size_full()
            .bg(bg)
            .text_color(fg)
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
                                .label("Đóng")
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
    }
}
