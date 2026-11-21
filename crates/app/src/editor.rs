//! Cột phải: trình soạn nhiều tab (bộ soạn code của gpui-component, tô màu tree-sitter).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use branchpad_core::{git::Repo, language_for, line_comment, line_marks, toggle_comment, LineMark, LineMarkKind};
use gpui::{
    canvas, div, fill, point, prelude::*, px, size, App, Bounds, Context, Entity, EventEmitter,
    FocusHandle, Focusable, SharedString, Subscription, Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputEvent, InputState, Position},
    v_flex, ActiveTheme,
};

use crate::actions::{CloseEditorTab, Save, ToggleComment};
use crate::ui::{confirm, MONO};

/// File lớn hơn mức này không mở (tránh treo app).
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// File nhỏ hơn mức này thì tính lại dấu thay đổi ở lề sau mỗi lần gõ.
const LIVE_MARKS_BYTES: usize = 512 * 1024;

pub struct EditorTab {
    pub path: PathBuf,
    pub rel: String,
    /// Có giá trị = bản của commit cũ, chỉ đọc.
    pub rev: Option<String>,
    pub state: Entity<InputState>,
    saved: String,
    pub dirty: bool,
    language: &'static str,
    /// Nội dung ở HEAD, để đánh dấu dòng thêm/sửa/xoá.
    base: Option<String>,
    marks: Vec<LineMark>,
    line_count: usize,
    _sub: Subscription,
}

impl EditorTab {
    fn title(&self) -> String {
        let name = Path::new(&self.rel)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.rel.clone());
        match &self.rev {
            Some(r) => format!("{name} @ {}", &r[..r.len().min(7)]),
            None => name,
        }
    }

    fn update_marks(&mut self, text: &str) {
        self.line_count = text.lines().count().max(1);
        self.marks = match &self.base {
            Some(base) if self.rev.is_none() => line_marks(base, text),
            // File chưa có trong git: mọi dòng là dòng mới.
            None if self.rev.is_none() => (0..self.line_count)
                .map(|line| LineMark { line, kind: LineMarkKind::Added })
                .collect(),
            _ => Vec::new(),
        };
    }
}

pub enum EditorEvent {
    /// Đã lưu file (cột giữa tải lại danh sách thay đổi).
    Saved,
}

impl EventEmitter<EditorEvent> for EditorPane {}

pub struct EditorPane {
    repo: Repo,
    tabs: Vec<EditorTab>,
    active: usize,
    focus: FocusHandle,
    error: Option<String>,
}

impl Focusable for EditorPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EditorPane {
    pub fn new(repo: Repo, cx: &mut Context<Self>) -> Self {
        Self {
            repo,
            tabs: Vec::new(),
            active: 0,
            focus: cx.focus_handle(),
            error: None,
        }
    }

    pub fn has_unsaved(&self) -> bool {
        self.tabs.iter().any(|t| t.dirty)
    }

    fn rel_of(&self, path: &Path) -> String {
        path.strip_prefix(self.repo.workdir())
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }

    fn make_tab(
        &mut self,
        path: PathBuf,
        rev: Option<String>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> EditorTab {
        let rel = self.rel_of(&path);
        let language = language_for(&path);
        let read_only = rev.is_some();
        let initial = text.clone();
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor(language)
                .line_number(true)
                .searchable(true)
                // gpui-component 0.4.0 panic "line layout should exist" khi vẽ indent guide.
                .indent_guides(false)
                .default_value(initial)
        });
        let sub = cx.subscribe_in(&state, window, Self::on_input_event);
        let base = if read_only {
            None
        } else {
            self.repo
                .show_file("HEAD", &rel)
                .ok()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        };
        let mut tab = EditorTab {
            path,
            rel,
            rev,
            state,
            saved: text.clone(),
            dirty: false,
            language,
            base,
            marks: Vec::new(),
            line_count: 1,
            _sub: sub,
        };
        tab.update_marks(&text);
        tab
    }

    fn on_input_event(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change { .. }) {
            return;
        }
        let Some(tab) = self.tabs.iter_mut().find(|t| t.state == *state) else {
            return;
        };
        if tab.rev.is_some() {
            return;
        }
        let text = tab.state.read(cx).value().to_string();
        tab.dirty = text != tab.saved;
        if text.len() <= LIVE_MARKS_BYTES {
            tab.update_marks(&text);
        }
        cx.notify();
    }

    /// Mở file trong thư mục làm việc (sửa được). `line` đếm từ 1.
    pub fn open_file(
        &mut self,
        path: PathBuf,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(i) = self.tabs.iter().position(|t| t.path == path && t.rev.is_none()) {
            self.active = i;
        } else {
            match read_text(&path) {
                Ok(text) => {
                    let tab = self.make_tab(path, None, text, window, cx);
                    self.tabs.push(tab);
                    self.active = self.tabs.len() - 1;
                    self.error = None;
                }
                Err(e) => {
                    self.error = Some(e);
                    cx.notify();
                    return;
                }
            }
        }
        if let Some(line) = line {
            self.goto_line(line, window, cx);
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Mở bản file ở một commit (chỉ đọc).
    pub fn open_revision(&mut self, rev: &str, rel: &str, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.repo.workdir().join(rel);
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| t.path == path && t.rev.as_deref() == Some(rev))
        {
            self.active = i;
            cx.notify();
            return;
        }
        match self.repo.show_file(rev, rel) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => {
                    let tab = self.make_tab(path, Some(rev.to_string()), text, window, cx);
                    self.tabs.push(tab);
                    self.active = self.tabs.len() - 1;
                    self.error = None;
                }
                Err(_) => self.error = Some(t!("editor.error.binary", name = rel)),
            },
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.state.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    /// Ghi tab `i` ra đĩa.
    fn save_tab(&mut self, i: usize, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.tabs.get_mut(i) else { return false };
        if tab.rev.is_some() || !tab.dirty {
            return true;
        }
        let text = tab.state.read(cx).value().to_string();
        match std::fs::write(&tab.path, &text) {
            Ok(()) => {
                tab.saved = text.clone();
                tab.dirty = false;
                tab.update_marks(&text);
                self.error = None;
                cx.emit(EditorEvent::Saved);
                cx.notify();
                true
            }
            Err(e) => {
                self.error = Some(t!("editor.error.save_failed", name = tab.rel, error = e));
                cx.notify();
                false
            }
        }
    }

    fn save(&mut self, _: &Save, _window: &mut Window, cx: &mut Context<Self>) {
        self.save_tab(self.active, cx);
    }

    fn close_active(&mut self, _: &CloseEditorTab, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab(self.active, window, cx);
    }

    pub fn close_tab(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(i) else { return };
        if tab.dirty {
            let msg = t!("editor.close.save_title", name = tab.rel);
            let path = tab.path.clone();
            confirm(
                window,
                cx,
                &msg,
                t!("editor.close.save_detail"),
                &[t!("common.cancel"), t!("editor.close.save"), t!("editor.close.dont_save")],
                move |this: &mut Self, answer, _window, cx| {
                    let Some(i) = this.tabs.iter().position(|t| t.path == path && t.rev.is_none())
                    else {
                        return;
                    };
                    if answer == 1 && !this.save_tab(i, cx) {
                        return;
                    }
                    this.remove_tab(i, cx);
                },
            );
            return;
        }
        self.remove_tab(i, cx);
    }

    fn remove_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.tabs.len() {
            self.tabs.remove(i);
        }
        if self.active >= i && self.active > 0 {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        cx.notify();
    }

    /// File bị sửa từ app khác: tải lại các tab chưa sửa trong app.
    pub fn reload_changed(&mut self, paths: &BTreeSet<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let mut changed = false;
        for tab in self.tabs.iter_mut() {
            if tab.rev.is_some() || tab.dirty || !paths.contains(&tab.path) {
                continue;
            }
            let Ok(text) = read_text(&tab.path) else { continue };
            if text == tab.saved {
                continue;
            }
            tab.saved = text.clone();
            tab.update_marks(&text);
            tab.state.update(cx, |s, cx| s.set_value(text, window, cx));
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    /// HEAD đổi (commit, checkout...): tính lại dấu thay đổi ở lề.
    pub fn refresh_bases(&mut self, cx: &mut Context<Self>) {
        for tab in self.tabs.iter_mut().filter(|t| t.rev.is_none()) {
            tab.base = self
                .repo
                .show_file("HEAD", &tab.rel)
                .ok()
                .map(|b| String::from_utf8_lossy(&b).into_owned());
            let text = tab.state.read(cx).value().to_string();
            tab.update_marks(&text);
        }
        cx.notify();
    }

    /// Nhảy tới dòng `line` (từ 1).
    pub fn goto_line(&mut self, line: u32, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            let pos = Position {
                line: line.saturating_sub(1),
                character: 0,
            };
            tab.state.update(cx, |s, cx| s.set_cursor_position(pos, window, cx));
        }
    }

    fn toggle_comment(&mut self, _: &ToggleComment, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        if tab.rev.is_some() {
            return;
        }
        let Some(prefix) = line_comment(tab.language) else { return };
        let state = tab.state.clone();
        let (text, cursor) = {
            let s = state.read(cx);
            (s.value().to_string(), s.cursor_position())
        };
        let line = cursor.line as usize;
        let new_text = toggle_comment(&text, line, line, prefix);
        if new_text != text {
            state.update(cx, |s, cx| {
                s.set_value(new_text, window, cx);
                s.set_cursor_position(cursor, window, cx);
            });
        }
    }

    fn render_tabs(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        h_flex()
            .id("editor-tabs")
            .h(px(32.))
            .w_full()
            .overflow_x_scroll()
            .border_b_1()
            .border_color(t.border)
            .bg(t.tab_bar)
            .children(self.tabs.iter().enumerate().map(|(i, tab)| {
                let active = i == self.active;
                h_flex()
                    .id(("editor-tab", i))
                    .h_full()
                    .px_3()
                    .gap_2()
                    .text_sm()
                    .border_r_1()
                    .border_color(t.border)
                    .when(active, |d| d.bg(t.tab_active).text_color(t.foreground))
                    .when(!active, |d| d.text_color(t.muted_foreground))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.active = i;
                        this.focus_active(window, cx);
                        cx.notify();
                    }))
                    .child(SharedString::from(tab.title()))
                    .when(tab.rev.is_some(), |d| d.child(div().text_xs().child(t!("editor.read_only"))))
                    .child(
                        div()
                            .id(("editor-tab-close", i))
                            .w(px(14.))
                            .text_center()
                            .rounded_sm()
                            .hover(|s| s.bg(gpui::black().opacity(0.1)))
                            .child(if tab.dirty { "●" } else { "×" })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_tab(i, window, cx);
                            })),
                    )
            }))
    }

    /// Thanh dọc bên phải: vị trí các dòng thêm/sửa/xoá so với git trên cả file.
    fn render_change_bar(&self, tab: &EditorTab, cx: &App) -> impl IntoElement {
        let marks = tab.marks.clone();
        let total = tab.line_count.max(1) as f32;
        let t = cx.theme();
        let (added, modified, deleted) = (t.success, t.warning, t.danger);
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let h = bounds.size.height;
                for m in &marks {
                    let y = bounds.origin.y + h * (m.line as f32 / total);
                    let color = match m.kind {
                        LineMarkKind::Added => added,
                        LineMarkKind::Modified => modified,
                        LineMarkKind::Deleted => deleted,
                    };
                    let tall = (h / total).max(px(2.));
                    window.paint_quad(fill(
                        Bounds::new(point(bounds.origin.x, y), size(bounds.size.width, tall)),
                        color,
                    ));
                }
            },
        )
        .w(px(4.))
        .h_full()
    }
}

fn read_text(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(t!("editor.error.too_large", path = path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| t!("editor.error.binary", name = path.display()))
}

impl Render for EditorPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let (bg, muted, danger) = (t.background, t.muted_foreground, t.danger);
        let body = match self.tabs.get(self.active) {
            Some(tab) => h_flex()
                .flex_1()
                .size_full()
                .overflow_hidden()
                .child(
                    div().flex_1().h_full().font_family(MONO).child(
                        Input::new(&tab.state)
                            .h_full()
                            .disabled(tab.rev.is_some()),
                    ),
                )
                .child(self.render_change_bar(tab, cx))
                .into_any_element(),
            None => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(muted)
                .child(t!("editor.empty.open_hint"))
                .child(t!("editor.empty.quick_open_hint"))
                .into_any_element(),
        };
        v_flex()
            .key_context("EditorPane")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::close_active))
            .on_action(cx.listener(Self::toggle_comment))
            .size_full()
            .bg(bg)
            .when(!self.tabs.is_empty(), |d| d.child(self.render_tabs(cx)))
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_xs()
                        .bg(danger.opacity(0.12))
                        .text_color(danger)
                        .child(e),
                )
            })
            .child(body)
    }
}
