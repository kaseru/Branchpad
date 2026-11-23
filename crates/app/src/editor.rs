//! Cột phải: trình soạn nhiều tab (bộ soạn code của gpui-component, tô màu tree-sitter).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use branchpad_core::{
    delete_line, duplicate_line, git::Repo, language_for, line_comment, line_marks, minimal_edit, move_line,
    toggle_comment, compare_lines, CompareKind, CompareLine, LineMark, LineMarkKind,
};
use std::rc::Rc;

use gpui::{
    canvas, div, fill, img, point, prelude::*, px, size, App, Bounds, Context, Entity, EventEmitter,
    ClickEvent, ClipboardItem, EntityInputHandler, FocusHandle, Focusable, MouseButton, ObjectFit, SharedString, StyledImage, Subscription, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState, Position},
    menu::{ContextMenuExt, PopupMenu},
    text::TextView,
    v_flex, ActiveTheme, IconName, Sizable,
};

use crate::actions::{
    CloseEditorTab, CopyLine, CutLine, DeleteLine, DuplicateLine, MoveLineDown, MoveLineUp, NextEditorTab,
    PreviousEditorTab, Save, SaveAll, ToggleComment, TogglePreview, ToggleWordWrap,
};
use crate::ui::{confirm_with_tone, ModalTone, MONO};

/// File lớn hơn mức này không mở (tránh treo app).
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// File nhỏ hơn mức này thì tính lại dấu thay đổi ở lề sau mỗi lần gõ.
const LIVE_MARKS_BYTES: usize = 512 * 1024;
/// File lớn hơn mức này hoặc có dòng quá dài thì không tô màu cú pháp (tránh đơ khi chuyển file).
const HIGHLIGHT_MAX_BYTES: usize = 1024 * 1024;
const HIGHLIGHT_MAX_LINE: usize = 5000;

/// File ảnh xem được (chỉ xem, không sửa).
fn is_image(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        matches!(
            e.to_string_lossy().to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico"
        )
    })
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Sửa `state` từ `old` thành `new` bằng đúng đoạn khác nhau, đi qua lịch sử của
/// trình soạn nên vẫn hoàn tác (Cmd+Z) / làm lại được. `set_value` thì xoá sạch lịch sử.
fn apply_edit(state: &Entity<InputState>, old: &str, new: &str, window: &mut Window, cx: &mut App) {
    let (start, end, replacement) = minimal_edit(old, new);
    if start == end && replacement.is_empty() {
        return;
    }
    let from = old[..start].encode_utf16().count();
    let to = old[..end].encode_utf16().count();
    let replacement = replacement.to_string();
    state.update(cx, |s, cx| s.replace_text_in_range(Some(from..to), &replacement, window, cx));
}

/// Menu chuột phải trong trình soạn.
fn editor_menu(menu: PopupMenu, focus: &Option<FocusHandle>) -> PopupMenu {
    use gpui_component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
    let mut menu = match focus {
        Some(f) => menu.action_context(f.clone()),
        None => menu,
    };
    menu = menu.menu(t!("actions.edit.undo"), Box::new(Undo));
    menu = menu.menu(t!("actions.edit.redo"), Box::new(Redo));
    menu = menu.separator();
    menu = menu.menu(t!("actions.edit.cut"), Box::new(Cut));
    menu = menu.menu(t!("actions.edit.copy"), Box::new(Copy));
    menu = menu.menu(t!("actions.edit.paste"), Box::new(Paste));
    menu = menu.separator();
    menu.menu(t!("actions.edit.select_all"), Box::new(SelectAll))
}

/// Dữ liệu kéo thả một tab để đổi vị trí.
#[derive(Clone)]
struct DraggedTab {
    ix: usize,
    title: SharedString,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        div()
            .px_3()
            .py_1()
            .text_sm()
            .rounded_sm()
            .border_1()
            .border_color(t.border)
            .bg(t.tab_active)
            .text_color(t.foreground)
            .opacity(0.9)
            .child(self.title.clone())
    }
}

pub struct EditorTab {
    pub path: PathBuf,
    pub rel: String,
    /// Có giá trị = bản của commit cũ, chỉ đọc.
    pub rev: Option<String>,
    pub state: Entity<InputState>,
    saved: String,
    pub dirty: bool,
    language: &'static str,
    /// Chỉ file markdown: hiện khung xem trước bên phải.
    preview: bool,
    /// Tab xem nhanh (chữ nghiêng): bấm file khác sẽ thay thế; sửa hoặc bấm đúp để gim.
    transient: bool,
    /// Tab so sánh hai file (chỉ xem).
    compare: Option<Rc<Vec<CompareLine>>>,
    /// Tự xuống dòng (mặc định bật).
    wrap: bool,
    /// Đếm lần sửa, để tự lưu chỉ chạy cho lần sửa cuối.
    change_seq: u64,
    /// File trên đĩa đã bị app khác sửa khi tab này còn thay đổi chưa lưu.
    conflict: bool,
    /// File ảnh: dung lượng, tab chỉ xem ảnh.
    image_size: Option<u64>,
    /// Nội dung ở HEAD, để đánh dấu dòng thêm/sửa/xoá.
    base: Option<String>,
    marks: Vec<LineMark>,
    line_count: usize,
    _sub: Subscription,
    /// Vẽ lại thanh trạng thái khi con trỏ di chuyển.
    _obs: Subscription,
}

impl EditorTab {
    fn title(&self) -> String {
        if self.compare.is_some() {
            return self.rel.clone();
        }
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
    font_size: f32,
    font_family: SharedString,
    auto_save: bool,
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
            font_size: 14.0,
            font_family: SharedString::from(MONO),
            auto_save: false,
        }
    }

    pub fn set_auto_save(&mut self, on: bool) {
        self.auto_save = on;
    }

    /// Cỡ chữ và font của trình soạn (từ Cài đặt / phím phóng to thu nhỏ).
    pub fn set_style(&mut self, size: f32, family: SharedString, cx: &mut Context<Self>) {
        self.font_size = size;
        self.font_family = family;
        cx.notify();
    }

    pub fn has_unsaved(&self) -> bool {
        self.tabs.iter().any(|t| t.dirty)
    }

    /// File `path` đang mở và có thay đổi chưa lưu.
    pub fn is_dirty(&self, path: &Path) -> bool {
        self.tabs.iter().any(|t| t.rev.is_none() && t.dirty && t.path == path)
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
        let language = if text.len() > HIGHLIGHT_MAX_BYTES
            || text.lines().any(|l| l.len() > HIGHLIGHT_MAX_LINE)
        {
            "text"
        } else {
            language_for(&path)
        };
        let image_size = if rev.is_none() && is_image(&path) {
            Some(std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0))
        } else {
            None
        };
        let read_only = rev.is_some() || image_size.is_some();
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
        let obs = cx.observe(&state, |_, _, cx| cx.notify());
        let base = if read_only || image_size.is_some() {
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
            preview: language == "markdown",
            transient: false,
            compare: None,
            wrap: true,
            change_seq: 0,
            conflict: false,
            image_size,
            base,
            marks: Vec::new(),
            line_count: 1,
            _sub: sub,
            _obs: obs,
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
        if tab.dirty {
            tab.transient = false;
        }
        if text.len() <= LIVE_MARKS_BYTES {
            tab.update_marks(&text);
        }
        if self.auto_save && tab.dirty && !tab.conflict {
            tab.change_seq += 1;
            let (path, seq) = (tab.path.clone(), tab.change_seq);
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(1000)).await;
                this.update(cx, |this, cx| {
                    let found = this.tabs.iter().position(|t| {
                        t.path == path && t.rev.is_none() && t.change_seq == seq && t.dirty && !t.conflict
                    });
                    if let Some(i) = found {
                        this.save_tab(i, cx);
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    /// Mở file trong thư mục làm việc (sửa được) thành tab thật. `line` đếm từ 1.
    pub fn open_file(&mut self, path: PathBuf, line: Option<u32>, window: &mut Window, cx: &mut Context<Self>) {
        self.open_file_with(path, line, true, window, cx);
    }

    /// `pin = false`: mở ở tab xem nhanh, thay thế tab xem nhanh hiện có.
    pub fn open_file_with(
        &mut self,
        path: PathBuf,
        line: Option<u32>,
        pin: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(i) = self.tabs.iter().position(|t| t.path == path && t.rev.is_none()) {
            self.active = i;
            if pin {
                self.tabs[i].transient = false;
            }
        } else {
            let loaded = if is_image(&path) { Ok(String::new()) } else { read_text(&path) };
            match loaded {
                Ok(text) => {
                    let mut tab = self.make_tab(path, None, text, window, cx);
                    tab.transient = !pin;
                    let slot = if pin { None } else { self.tabs.iter().position(|t| t.transient && !t.dirty) };
                    match slot {
                        Some(i) => {
                            self.tabs[i] = tab;
                            self.active = i;
                        }
                        None => {
                            self.tabs.push(tab);
                            self.active = self.tabs.len() - 1;
                        }
                    }
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

    /// Danh sách tab đang mở: (tên, đường dẫn tương đối, đã sửa chưa lưu, đang xem).
    pub fn tab_summaries(&self) -> Vec<(String, String, bool, bool)> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| (t.title(), t.rel.clone(), t.dirty, i == self.active))
            .collect()
    }

    pub fn activate_tab(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if i < self.tabs.len() {
            self.active = i;
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    /// So sánh nội dung hai file (bản đang mở trong app nếu có, không thì đọc từ đĩa).
    pub fn open_compare(&mut self, left: PathBuf, right: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let title = format!("{} ↔ {}", self.rel_of(&left), self.rel_of(&right));
        let key = PathBuf::from(format!("{title}.compare"));
        if let Some(i) = self.tabs.iter().position(|t| t.path == key) {
            self.active = i;
            cx.notify();
            return;
        }
        let load = |this: &Self, p: &PathBuf, cx: &App| -> Result<String, String> {
            match this.tabs.iter().find(|t| t.path == *p && t.rev.is_none() && t.image_size.is_none()) {
                Some(t) => Ok(t.state.read(cx).value().to_string()),
                None => read_text(p),
            }
        };
        let (a, b) = match (load(self, &left, cx), load(self, &right, cx)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => {
                self.error = Some(e);
                cx.notify();
                return;
            }
        };
        let lines = Rc::new(compare_lines(&a, &b));
        let mut tab = self.make_tab(key, Some("compare".into()), String::new(), window, cx);
        tab.rel = title;
        tab.compare = Some(lines);
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.error = None;
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

    pub fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
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

    /// Bỏ thay đổi của mình, lấy bản trên đĩa.
    fn reload_from_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        match read_text(&tab.path) {
            Ok(text) => {
                let old = tab.state.read(cx).value().to_string();
                apply_edit(&tab.state, &old, &text, window, cx);
                tab.saved = text.clone();
                tab.dirty = false;
                tab.conflict = false;
                tab.update_marks(&text);
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    /// Giữ bản đang sửa; lần lưu sau sẽ ghi đè bản trên đĩa.
    fn keep_mine(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        if let Ok(text) = read_text(&tab.path) {
            tab.saved = text;
        }
        tab.conflict = false;
        let current = tab.state.read(cx).value().to_string();
        tab.dirty = current != tab.saved;
        cx.notify();
    }

    fn save_all(&mut self, _: &SaveAll, _window: &mut Window, cx: &mut Context<Self>) {
        for i in 0..self.tabs.len() {
            if !self.save_tab(i, cx) {
                break;
            }
        }
    }

    fn switch_tab(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.tabs.len() as isize;
        if n < 2 {
            return;
        }
        self.active = (self.active as isize + step).rem_euclid(n) as usize;
        self.focus_active(window, cx);
        cx.notify();
    }

    fn next_tab(&mut self, _: &NextEditorTab, window: &mut Window, cx: &mut Context<Self>) {
        self.switch_tab(1, window, cx);
    }

    fn previous_tab(&mut self, _: &PreviousEditorTab, window: &mut Window, cx: &mut Context<Self>) {
        self.switch_tab(-1, window, cx);
    }

    fn toggle_word_wrap(&mut self, _: &ToggleWordWrap, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        if tab.image_size.is_some() {
            return;
        }
        tab.wrap = !tab.wrap;
        let wrap = tab.wrap;
        tab.state.update(cx, |s, cx| s.set_soft_wrap(wrap, window, cx));
        cx.notify();
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
            confirm_with_tone(
                window,
                cx,
                ModalTone::Primary,
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
            if tab.rev.is_some() || !paths.contains(&tab.path) {
                continue;
            }
            let Ok(text) = read_text(&tab.path) else { continue };
            if text == tab.saved {
                continue;
            }
            if tab.dirty {
                // File đổi từ bên ngoài khi tab còn thay đổi chưa lưu: hỏi người dùng.
                if !tab.conflict {
                    tab.conflict = true;
                    changed = true;
                }
                continue;
            }
            tab.saved = text.clone();
            tab.update_marks(&text);
            let old = tab.state.read(cx).value().to_string();
            apply_edit(&tab.state, &old, &text, window, cx);
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

    fn toggle_preview(&mut self, _: &TogglePreview, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active).filter(|t| t.language == "markdown") {
            tab.preview = !tab.preview;
            cx.notify();
        }
    }

    /// Kéo tab `from` thả vào vị trí của tab `to`; tab đang mở vẫn là tab cũ.
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
        cx.notify();
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
        apply_edit(&state, &text, &new_text, window, cx);
        state.update(cx, |s, cx| s.set_cursor_position(cursor, window, cx));
    }

    /// Chạy một thao tác dòng (nhân đôi, xoá, đổi chỗ) trên dòng của con trỏ.
    fn line_op(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        op: impl Fn(&str, usize) -> Option<(String, usize)>,
    ) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        if tab.rev.is_some() || tab.image_size.is_some() {
            return;
        }
        let state = tab.state.clone();
        let (text, cursor) = {
            let s = state.read(cx);
            (s.value().to_string(), s.cursor_position())
        };
        if let Some((new_text, new_line)) = op(&text, cursor.line as usize) {
            apply_edit(&state, &text, &new_text, window, cx);
            let pos = Position { line: new_line as u32, character: cursor.character };
            state.update(cx, |s, cx| s.set_cursor_position(pos, window, cx));
        }
    }

    /// Cắt/chép: có vùng chọn thì làm như thường, không có thì lấy cả dòng của con trỏ.
    fn copy_or_cut_line(&mut self, cut: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        if tab.image_size.is_some() {
            return;
        }
        let state = tab.state.clone();
        let read_only = tab.rev.is_some();
        let has_selection = state
            .update(cx, |s, cx| s.selected_text_range(true, window, cx))
            .is_some_and(|sel| !sel.range.is_empty());
        if has_selection {
            if cut && !read_only {
                window.dispatch_action(Box::new(gpui_component::input::Cut), cx);
            } else {
                window.dispatch_action(Box::new(gpui_component::input::Copy), cx);
            }
            return;
        }
        let (text, cursor) = {
            let s = state.read(cx);
            (s.value().to_string(), s.cursor_position())
        };
        let Some(line) = text.split('\n').nth(cursor.line as usize) else { return };
        cx.write_to_clipboard(ClipboardItem::new_string(format!("{line}\n")));
        if cut && !read_only {
            self.line_op(window, cx, delete_line);
        }
    }

    fn cut_line(&mut self, _: &CutLine, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_or_cut_line(true, window, cx);
    }

    fn copy_line(&mut self, _: &CopyLine, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_or_cut_line(false, window, cx);
    }

    fn duplicate_line(&mut self, _: &DuplicateLine, window: &mut Window, cx: &mut Context<Self>) {
        self.line_op(window, cx, duplicate_line);
    }

    fn delete_line(&mut self, _: &DeleteLine, window: &mut Window, cx: &mut Context<Self>) {
        self.line_op(window, cx, delete_line);
    }

    fn move_line_up(&mut self, _: &MoveLineUp, window: &mut Window, cx: &mut Context<Self>) {
        self.line_op(window, cx, |t, l| move_line(t, l, true));
    }

    fn move_line_down(&mut self, _: &MoveLineDown, window: &mut Window, cx: &mut Context<Self>) {
        self.line_op(window, cx, |t, l| move_line(t, l, false));
    }

    fn render_tabs(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let md = self.tabs.get(self.active).filter(|t| t.language == "markdown").map(|t| t.preview);
        h_flex()
            .h(px(32.))
            .w_full()
            .border_b_1()
            .border_color(t.border)
            .bg(t.tab_bar)
            .child(
                h_flex()
                    .id("editor-tabs")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_x_scroll()
                    .children(self.tabs.iter().enumerate().map(|(i, tab)| {
                        let active = i == self.active;
                        let drag = DraggedTab { ix: i, title: SharedString::from(tab.title()) };
                        let border = t.border;
                        let hover_bg = t.tab_active.opacity(0.6);
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
                            .on_drag(drag, |d, _, _, cx| cx.new(|_| d.clone()))
                            .drag_over::<DraggedTab>(move |s, _, _, _| s.bg(hover_bg).border_l_2().border_color(border))
                            .on_drop(cx.listener(move |this, d: &DraggedTab, _, cx| {
                                this.move_tab(d.ix, i, cx);
                            }))
                            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                                this.active = i;
                                if ev.click_count() >= 2 {
                                    if let Some(tab) = this.tabs.get_mut(i) {
                                        tab.transient = false;
                                    }
                                }
                                this.focus_active(window, cx);
                                cx.notify();
                            }))
                            .on_mouse_down(
                                MouseButton::Middle,
                                cx.listener(move |this, _, window, cx| {
                                    this.close_tab(i, window, cx);
                                }),
                            )
                            .child(div().when(tab.transient, |d| d.italic()).child(SharedString::from(tab.title())))
                            .when(tab.rev.is_some() && tab.compare.is_none(), |d| d.child(div().text_xs().child(t!("editor.read_only"))))
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
                    })),
            )
            .when_some(md, |d, on| {
                d.child(
                    div().px_1().child(
                        Button::new("editor-md-preview")
                            .icon(if on { IconName::Eye } else { IconName::EyeOff })
                            .label(t!("editor.preview"))
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_preview(&TogglePreview, window, cx);
                            })),
                    ),
                )
            })
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview_src = self
            .tabs
            .get(self.active)
            .filter(|t| t.language == "markdown" && t.preview)
            .map(|t| (t.rel.clone(), t.state.read(cx).value().to_string()));
        let preview = preview_src.map(|(rel, src)| {
            TextView::markdown(SharedString::from(format!("md-preview:{rel}")), src, window, cx)
                .selectable(true)
                .scrollable(true)
        });
        let (font_family, font_size) = (self.font_family.clone(), self.font_size);
        let status = self
            .tabs
            .get(self.active)
            .filter(|t| t.image_size.is_none() && t.compare.is_none())
            .map(|t| {
                let pos = t.state.read(cx).cursor_position();
                let lang = if t.language == "text" { "Plain Text" } else { t.language };
                format!("Ln {}, Col {}  ·  {} lines  ·  {}  ·  UTF-8", pos.line + 1, pos.character + 1, t.line_count, lang)
            });
        let t = cx.theme();
        let (bg, muted, danger, border, warning) = (t.background, t.muted_foreground, t.danger, t.border, t.warning);
        let menu_focus = self.tabs.get(self.active).map(|t| t.state.focus_handle(cx));
        let body = match self.tabs.get(self.active) {
            Some(tab) if tab.compare.is_some() => {
                let lines = tab.compare.clone().unwrap_or_default();
                let (success, danger, muted) = (cx.theme().success, cx.theme().danger, cx.theme().muted_foreground);
                let count = lines.len();
                if count == 0 {
                    div().p_4().text_sm().text_color(muted).child(t!("editor.compare.identical")).into_any_element()
                } else {
                    gpui::uniform_list(
                        "editor-compare",
                        count,
                        cx.processor(move |_, range: std::ops::Range<usize>, _, _| {
                            range
                                .map(|i| {
                                    let l = &lines[i];
                                    let (sign, bg, gutter) = match l.kind {
                                        CompareKind::Added => ("+", Some(success.opacity(0.14)), Some(success.opacity(0.24))),
                                        CompareKind::Removed => ("-", Some(danger.opacity(0.14)), Some(danger.opacity(0.24))),
                                        CompareKind::Same => (" ", None, None),
                                    };
                                    let sign_color = match l.kind {
                                        CompareKind::Added => success,
                                        CompareKind::Removed => danger,
                                        CompareKind::Same => muted,
                                    };
                                    let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
                                    h_flex()
                                        .id(("cmp-row", i))
                                        .h(px(20.))
                                        .w_full()
                                        .text_sm()
                                        .when_some(bg, |d, c| d.bg(c))
                                        .child(
                                            div()
                                                .w(px(88.))
                                                .flex_none()
                                                .h_full()
                                                .flex()
                                                .when_some(gutter, |d, c| d.bg(c))
                                                .text_color(muted)
                                                .child(div().w(px(44.)).text_right().pr_1().child(num(l.old_no)))
                                                .child(div().w(px(44.)).text_right().pr_1().child(num(l.new_no))),
                                        )
                                        .child(
                                            div()
                                                .w(px(16.))
                                                .flex_none()
                                                .text_center()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(sign_color)
                                                .child(sign),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .whitespace_nowrap()
                                                .overflow_hidden()
                                                .child(l.text.replace('\t', "    ")),
                                        )
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .size_full()
                    .font_family(font_family)
                    .text_size(px(font_size))
                    .into_any_element()
                }
            }
            Some(tab) if tab.image_size.is_some() => v_flex()
                .flex_1()
                .size_full()
                .overflow_hidden()
                .items_center()
                .gap_2()
                .p_4()
                .child(
                    div().flex_1().min_h_0().w_full().flex().items_center().justify_center().child(
                        img(tab.path.clone())
                            .max_w_full()
                            .max_h_full()
                            .object_fit(ObjectFit::Contain)
                            .with_fallback(move || {
                                div().text_sm().text_color(muted).child(t!("editor.image.failed")).into_any_element()
                            }),
                    ),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{} · {}", tab.rel, format_size(tab.image_size.unwrap_or(0)))),
                )
                .into_any_element(),
            Some(tab) => h_flex()
                .flex_1()
                .size_full()
                .overflow_hidden()
                .child(
                    div().id("editor-body").flex_1().min_w_0().h_full().font_family(font_family).text_size(px(font_size))
                    .context_menu(move |menu, _, _| editor_menu(menu, &menu_focus))
                    .child(
                        Input::new(&tab.state)
                            .h_full()
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false)
                            .disabled(tab.rev.is_some()),
                    ),
                )
                .child(self.render_change_bar(tab, cx))
                .when_some(preview, |d, view| {
                    d.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_hidden()
                            .border_l_1()
                            .border_color(border)
                            .p_4()
                            .child(view),
                    )
                })
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
            .on_action(cx.listener(Self::toggle_preview))
            .on_action(cx.listener(Self::duplicate_line))
            .on_action(cx.listener(Self::cut_line))
            .on_action(cx.listener(Self::save_all))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::previous_tab))
            .on_action(cx.listener(Self::toggle_word_wrap))
            .on_action(cx.listener(Self::copy_line))
            .on_action(cx.listener(Self::delete_line))
            .on_action(cx.listener(Self::move_line_up))
            .on_action(cx.listener(Self::move_line_down))
            .size_full()
            .bg(bg)
            .when(!self.tabs.is_empty(), |d| d.child(self.render_tabs(cx)))
            .when(self.tabs.get(self.active).is_some_and(|t| t.conflict), |d| {
                let name = self.tabs.get(self.active).map(|t| t.rel.clone()).unwrap_or_default();
                d.child(
                    h_flex()
                        .flex_none()
                        .px_3()
                        .py_1()
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .bg(warning.opacity(0.15))
                        .child(div().flex_1().child(t!("editor.conflict.message", name = name)))
                        .child(
                            Button::new("editor-conflict-reload")
                                .label(t!("editor.conflict.reload"))
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(|this, _, window, cx| this.reload_from_disk(window, cx))),
                        )
                        .child(
                            Button::new("editor-conflict-keep")
                                .label(t!("editor.conflict.keep"))
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| this.keep_mine(cx))),
                        ),
                )
            })
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
            .when_some(status, |d, text| {
                d.child(
                    h_flex()
                        .flex_none()
                        .h(px(22.))
                        .px_3()
                        .justify_end()
                        .items_center()
                        .border_t_1()
                        .border_color(border)
                        .text_xs()
                        .text_color(muted)
                        .child(text),
                )
            })
    }
}
