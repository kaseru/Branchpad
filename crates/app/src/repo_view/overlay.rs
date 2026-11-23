//! Hộp nổi trên cột giữa: ô nhập một dòng (tên nhánh, tên file...),
//! mở file nhanh (Cmd+P), tìm chữ trong cả thư mục (Cmd+Shift+F).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use branchpad_core::{fuzzy_filter, git::GrepMatch};
use gpui::{
    div, prelude::*, px, AnyElement, App, Context, Entity, Hsla, MouseButton, MouseDownEvent, SharedString,
    Subscription, Window,
};
use gpui_component::{
    dialog::DialogButtonProps,
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, ActiveTheme, Icon, IconName, Sizable, WindowExt,
};

use super::RepoView;
use crate::actions::{FindInFiles, GotoLine, QuickOpen};
use crate::icons::BpIcon;
use crate::ui::{list_row, split_path, MONO};

/// Trả về (Icon, Màu, Nhãn loại, Hint mô tả) cho từng loại modal prompt.
fn prompt_meta(kind: &PromptKind, cx: &App) -> (Icon, Hsla, &'static str, Option<String>) {
    let t = cx.theme();
    match kind {
        PromptKind::CreateBranch { start } => (
            BpIcon::Branch.into(),
            t.primary,
            "CREATE BRANCH",
            Some(format!("New branch starting from '{start}'")),
        ),
        PromptKind::TrackBranch { remote_branch } => (
            BpIcon::Branch.into(),
            t.primary,
            "TRACK REMOTE BRANCH",
            Some(format!("Tracking remote '{remote_branch}'")),
        ),
        PromptKind::RenameBranch { old } => (
            BpIcon::Branch.into(),
            t.info,
            "RENAME BRANCH",
            Some(format!("Current branch name: '{old}'")),
        ),
        PromptKind::CreateTag { rev } => (
            IconName::Star.into(),
            t.warning,
            "CREATE TAG",
            Some(format!("Target commit: '{rev}'")),
        ),
        PromptKind::AddRemote => (
            IconName::GitHub.into(),
            t.primary,
            "ADD REMOTE",
            Some("Enter remote URL or name".into()),
        ),
        PromptKind::NewFile { dir } => (
            IconName::File.into(),
            t.success,
            "NEW FILE",
            Some(format!("Inside folder: {}", dir.display())),
        ),
        PromptKind::NewFolder { dir } => (
            IconName::FolderClosed.into(),
            t.warning,
            "NEW FOLDER",
            Some(format!("Inside folder: {}", dir.display())),
        ),
        PromptKind::RenameFile { path } => (
            IconName::File.into(),
            t.info,
            "RENAME FILE",
            Some(format!("Current path: {}", path.display())),
        ),
        PromptKind::Stash => (
            BpIcon::Stash.into(),
            t.info,
            "STASH CHANGES",
            Some("Save uncommitted changes to a new stash".into()),
        ),
        PromptKind::AddWorktree => (
            IconName::FolderClosed.into(),
            t.info,
            "ADD WORKTREE",
            Some("Create a new linked working tree".into()),
        ),
        PromptKind::GotoLine => (
            BpIcon::FileTerminal.into(),
            t.primary,
            "GO TO LINE",
            Some("Enter line number".into()),
        ),
        PromptKind::Reword(idx) => (
            BpIcon::Patch.into(),
            t.success,
            "REWORD COMMIT",
            Some(format!("Commit index: {idx}")),
        ),
    }
}

/// Ô nhập đang hỏi điều gì.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptKind {
    CreateBranch { start: String },
    CreateTag { rev: String },
    RenameBranch { old: String },
    TrackBranch { remote_branch: String },
    AddRemote,
    NewFile { dir: std::path::PathBuf },
    NewFolder { dir: std::path::PathBuf },
    RenameFile { path: std::path::PathBuf },
    Stash,
    AddWorktree,
    GotoLine,
    Reword(usize),
}

pub struct Prompt {
    pub kind: PromptKind,
    input: Entity<InputState>,
    _sub: Subscription,
}

pub struct QuickOpenState {
    input: Entity<InputState>,
    files: Rc<Vec<String>>,
    results: Vec<usize>,
    /// Gõ thẳng đường dẫn file có thật (tuyệt đối, `~/…` hoặc tương đối với repo, có thể kèm `:dòng`):
    /// hiện dòng "Mở …" đầu danh sách, Enter mở luôn (kể cả file ngoài repo).
    direct: Option<(PathBuf, Option<u32>)>,
    pub selected_index: usize,
    pub scroll_handle: gpui::ScrollHandle,
    _sub: Subscription,
}

/// Chuỗi gõ ở Cmd+P là đường dẫn tới một file có thật? `~` = thư mục nhà; tương đối thì tính từ `root`.
/// Hỗ trợ đuôi `:dòng` (hoặc `:dòng:cột`).
pub(crate) fn direct_path(query: &str, root: &Path) -> Option<(PathBuf, Option<u32>)> {
    let q = query.trim().trim_matches(|c| c == '"' || c == '\'');
    if q.is_empty() {
        return None;
    }
    let candidates = {
        let mut parts = q.rsplitn(3, ':').collect::<Vec<_>>();
        parts.reverse();
        // (đường dẫn, dòng): thử cả nguyên chuỗi lẫn bỏ `:dòng[:cột]`.
        let mut c = vec![(q.to_string(), None)];
        if parts.len() >= 2 {
            if let Ok(line) = parts[1].parse::<u32>() {
                c.push((parts[0].to_string(), Some(line)));
            }
        }
        if parts.len() == 3 {
            if let (Ok(line), Ok(_)) = (parts[1].parse::<u32>(), parts[2].parse::<u32>()) {
                c.push((parts[0].to_string(), Some(line)));
            }
        }
        c
    };
    candidates.into_iter().find_map(|(p, line)| {
        let path = if let Some(rest) = p.strip_prefix("~/") {
            PathBuf::from(std::env::var_os("HOME")?).join(rest)
        } else if Path::new(&p).is_absolute() {
            PathBuf::from(&p)
        } else {
            root.join(&p)
        };
        path.is_file().then_some((path, line))
    })
}

pub struct FindState {
    input: Entity<InputState>,
    results: Vec<GrepMatch>,
    searching: bool,
    _sub: Subscription,
}

pub enum Overlay {
    None,
    Prompt(Prompt),
    QuickOpen(QuickOpenState),
    Find(FindState),
}

/// Số kết quả tối đa.
const QUICK_OPEN_LIMIT: usize = 60;
const FIND_LIMIT: usize = 2000;

impl RepoView {
    pub(crate) fn open_prompt(
        &mut self,
        kind: PromptKind,
        label: &str,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initial = initial.to_string();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        let sub = cx.subscribe_in(&input, window, |this, input, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                let value = input.read(cx).value().to_string();
                this.submit_prompt(value, window, cx);
                window.close_dialog(cx);
            }
        });
        let title = SharedString::from(label.to_string());
        let modal_kind = kind.clone();
        self.overlay = Overlay::Prompt(Prompt {
            kind,
            input: input.clone(),
            _sub: sub,
        });

        // Standard modal: title, input, Cancel / OK.
        let entity = cx.entity();
        let field = input.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let (ok_entity, cancel_entity, ok_input) = (entity.clone(), entity.clone(), field.clone());
            let t = cx.theme();
            let (icon, color, category, hint) = prompt_meta(&modal_kind, cx);

            dialog
                .title(title.clone())
                .width(px(480.))
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t!("overlay.ok"))
                        .cancel_text(t!("overlay.cancel")),
                )
                .child(
                    v_flex()
                        .gap_3()
                        .pt_1()
                        .child(
                            h_flex()
                                .gap_3()
                                .items_center()
                                .p(px(10.))
                                .rounded_lg()
                                .bg(t.secondary)
                                .border_1()
                                .border_color(t.border)
                                .child(
                                    div()
                                        .size(px(40.))
                                        .flex_none()
                                        .rounded_lg()
                                        .bg(color.opacity(0.18))
                                        .border_1()
                                        .border_color(color.opacity(0.45))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_color(color)
                                        .child(Icon::new(icon).size_5()),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(color)
                                                .child(category),
                                        )
                                        .when_some(hint, |d, h| {
                                            d.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(t.muted_foreground)
                                                    .whitespace_normal()
                                                    .child(h),
                                            )
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .p(px(2.))
                                .rounded_md()
                                .bg(t.background)
                                .border_1()
                                .border_color(t.border)
                                .child(Input::new(&field)),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    let value = ok_input.read(cx).value().to_string();
                    ok_entity.update(cx, |this, cx| this.submit_prompt(value, window, cx));
                    true
                })
                .on_cancel(move |_, window, cx| {
                    cancel_entity.update(cx, |this, cx| {
                        this.overlay = Overlay::None;
                        window.focus(&this.focus);
                        cx.notify();
                    });
                    true
                })
        });
        window.defer(cx, move |window, cx| input.update(cx, |s, cx| s.focus(window, cx)));
        cx.notify();
    }

    fn submit_prompt(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::Prompt(p) = std::mem::replace(&mut self.overlay, Overlay::None) else {
            return;
        };
        window.focus(&self.focus);
        self.handle_prompt(p.kind, value, window, cx);
        cx.notify();
    }

    pub(super) fn on_goto_line(&mut self, _: &GotoLine, window: &mut Window, cx: &mut Context<Self>) {
        self.open_prompt(PromptKind::GotoLine, t!("overlay.goto_line"), "", window, cx);
    }

    pub(super) fn on_quick_open(&mut self, _: &QuickOpen, window: &mut Window, cx: &mut Context<Self>) {
        self.spawn_git_in(
            None,
            window,
            cx,
            |r| r.list_files(),
            |this, files, window, cx| {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder(t!("overlay.quick_open.placeholder")));
                let sub = cx.subscribe_in(&input, window, |this, input, ev: &InputEvent, window, cx| {
                    let Overlay::QuickOpen(q) = &mut this.overlay else { return };
                    match ev {
                        InputEvent::Change { .. } => {
                            let query = input.read(cx).value().to_string();
                            q.direct = direct_path(&query, this.repo.workdir());
                            q.results = fuzzy_filter(&query, &q.files, QUICK_OPEN_LIMIT);
                            q.selected_index = 0;
                            q.scroll_handle.scroll_to_item(0);
                            cx.notify();
                        }
                        InputEvent::PressEnter { .. } => {
                            // Gõ thẳng đường dẫn: ưu tiên khi là đường dẫn tuyệt đối / `~`, hoặc không có kết quả nào.
                            let query = input.read(cx).value().to_string();
                            let explicit = matches!(query.trim().chars().next(), Some('/' | '~'));
                            if let Some((path, line)) = q.direct.clone().filter(|_| explicit || q.results.is_empty()) {
                                this.open_direct_path(path, line, window, cx);
                                return;
                            }
                            let target = q
                                .results
                                .get(q.selected_index)
                                .or_else(|| q.results.first())
                                .and_then(|i| q.files.get(*i))
                                .cloned();
                            if let Some(rel) = target {
                                this.open_quick_result(rel, window, cx);
                            }
                        }
                        _ => {}
                    }
                });
                input.update(cx, |s, cx| s.focus(window, cx));
                let files = Rc::new(files);
                let results = (0..files.len().min(QUICK_OPEN_LIMIT)).collect();
                this.overlay = Overlay::QuickOpen(QuickOpenState {
                    input,
                    files,
                    results,
                    direct: None,
                    selected_index: 0,
                    scroll_handle: gpui::ScrollHandle::new(),
                    _sub: sub,
                });
            },
        );
    }

    pub(super) fn quick_open_prev(&mut self, cx: &mut Context<Self>) {
        if let Overlay::QuickOpen(q) = &mut self.overlay {
            if q.selected_index > 0 {
                q.selected_index -= 1;
                q.scroll_handle.scroll_to_item(q.selected_index);
                cx.notify();
            }
        }
    }

    pub(super) fn quick_open_next(&mut self, cx: &mut Context<Self>) {
        if let Overlay::QuickOpen(q) = &mut self.overlay {
            if !q.results.is_empty() && q.selected_index + 1 < q.results.len() {
                q.selected_index += 1;
                q.scroll_handle.scroll_to_item(q.selected_index);
                cx.notify();
            }
        }
    }

    /// Mở file theo đường dẫn gõ ở Cmd+P (có thể ngoài repo); trong repo thì mở cả thư mục trên cây file.
    fn open_direct_path(&mut self, path: PathBuf, line: Option<u32>, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        if path.starts_with(self.repo.workdir()) {
            if let Some(tree) = self.tree.as_mut() {
                if let Err(e) = tree.reveal(&self.repo, &path) {
                    eprintln!("branchpad: cannot open folder containing {}: {e}", path.display());
                }
            }
            self.selected_path = Some(path.clone());
        }
        self.open_in_editor(path, line, window, cx);
    }

    fn open_quick_result(&mut self, rel: String, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        let path = self.repo.workdir().join(&rel);
        if let Some(tree) = self.tree.as_mut() {
            if let Err(e) = tree.reveal(&self.repo, &path) {
                eprintln!("branchpad: cannot open folder containing {rel}: {e}");
            }
        }
        self.selected_path = Some(path.clone());
        self.open_in_editor(path, None, window, cx);
    }

    pub(super) fn on_find_in_files(&mut self, _: &FindInFiles, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("overlay.find.placeholder"))
        });
        let sub = cx.subscribe_in(&input, window, |this, input, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                let q = input.read(cx).value().to_string();
                this.run_find(q, cx);
            }
        });
        input.update(cx, |s, cx| s.focus(window, cx));
        self.overlay = Overlay::Find(FindState {
            input,
            results: Vec::new(),
            searching: false,
            _sub: sub,
        });
        cx.notify();
    }

    fn run_find(&mut self, query: String, cx: &mut Context<Self>) {
        let Overlay::Find(f) = &mut self.overlay else { return };
        f.searching = true;
        cx.notify();
        self.spawn_git(
            None,
            cx,
            move |r| r.grep(&query, FIND_LIMIT),
            |this, results, _| {
                if let Overlay::Find(f) = &mut this.overlay {
                    f.results = results;
                    f.searching = false;
                }
            },
        );
    }

    pub(super) fn render_overlay(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = cx.theme();
        let (border, muted) = (t.border, t.muted_foreground);
        let content: AnyElement = match &self.overlay {
            // The prompt is shown as a modal dialog (see `open_prompt`).
            Overlay::None | Overlay::Prompt(_) => return None,
            Overlay::QuickOpen(q) => {
                let selected_index = q.selected_index;
                let scroll_handle = q.scroll_handle.clone();
                v_flex()
                    .p_3()
                    .gap_2()
                    .on_action(cx.listener(|this, _: &gpui_component::input::MoveUp, _, cx| {
                        this.quick_open_prev(cx);
                    }))
                    .on_action(cx.listener(|this, _: &gpui_component::input::MoveDown, _, cx| {
                        this.quick_open_next(cx);
                    }))
                    .on_action(cx.listener(|this, _: &gpui_component::input::Escape, _, cx| {
                        this.overlay = Overlay::None;
                        cx.notify();
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(t.primary)
                            .child(Icon::new(IconName::File).size_4())
                            .child("QUICK OPEN FILE (⌘P)"),
                    )
                    .child(Input::new(&q.input).small())
                    .child(
                        v_flex()
                            .id("quick-open-results")
                            .track_scroll(&scroll_handle)
                            .max_h(px(380.))
                            .overflow_y_scroll()
                            .gap(px(2.))
                            .children(q.direct.clone().map(|(path, line)| {
                                let label = match line {
                                    Some(l) => format!("{}:{l}", path.display()),
                                    None => path.display().to_string(),
                                };
                                list_row("qo-direct", q.results.is_empty(), cx)
                                    .child(Icon::new(IconName::File).size_4().text_color(t.primary))
                                    .child(div().flex_none().font_weight(gpui::FontWeight::MEDIUM).child(t!("overlay.quick_open.open_path")))
                                    .child(div().flex_1().truncate().text_xs().text_color(muted).child(label))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                            this.open_direct_path(path.clone(), line, window, cx)
                                        }),
                                    )
                            }))
                            .children(q.results.iter().enumerate().filter_map(|(n, &i)| {
                                let rel = q.files.get(i)?.clone();
                                let (name, dir) = split_path(&rel);
                                Some(
                                    list_row(("qo", n), n == selected_index, cx)
                                        .child(div().flex_none().font_weight(gpui::FontWeight::MEDIUM).child(name))
                                        .child(div().flex_1().truncate().text_xs().text_color(muted).child(dir))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                                this.open_quick_result(rel.clone(), window, cx)
                                            }),
                                        ),
                                )
                            })),
                    )
                    .into_any_element()
            }
            Overlay::Find(f) => v_flex()
                .p_3()
                .gap_2()
                .on_action(cx.listener(|this, _: &gpui_component::input::Escape, _, cx| {
                    this.overlay = Overlay::None;
                    cx.notify();
                }))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(t.primary)
                        .child(Icon::new(BpIcon::Refresh).size_4())
                        .child("FIND IN FILES (⌘⇧F)"),
                )
                .child(Input::new(&f.input).small())
                .child(div().text_xs().text_color(muted).child(if f.searching {
                    t!("overlay.find.searching").to_string()
                } else if f.results.len() >= FIND_LIMIT {
                    t!("overlay.find.limit", n = FIND_LIMIT)
                } else {
                    t!("overlay.find.count", n = f.results.len())
                }))
                .child({
                    // uniform_list cần chiều cao xác định: tối đa 420px, ít kết quả thì thấp hơn.
                    let count = f.results.len();
                    let workdir = self.repo.workdir().to_path_buf();
                    gpui::uniform_list(
                        "find-results",
                        count,
                        cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                            let Overlay::Find(f) = &this.overlay else { return Vec::new() };
                            let muted = cx.theme().muted_foreground;
                            range
                                .filter_map(|n| f.results.get(n).map(|m| (n, m)))
                                .map(|(n, m)| {
                                    let path = workdir.join(&m.path);
                                    let line = m.line_no;
                                    list_row(("find", n), false, cx)
                                        .child(
                                            div()
                                                .w(px(260.))
                                                .flex_none()
                                                .truncate()
                                                .text_xs()
                                                .text_color(muted)
                                                .child(format!("{}:{}", m.path, m.line_no)),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .truncate()
                                                .font_family(MONO)
                                                .text_xs()
                                                .child(m.text.trim().to_string()),
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                                this.overlay = Overlay::None;
                                                this.open_in_editor(path.clone(), Some(line), window, cx)
                                            }),
                                        )
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .h((crate::ui::ROW_H * count as f32).min(px(420.)))
                })
                .into_any_element(),
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .bg(gpui::black().opacity(0.65))
                .flex()
                .items_start()
                .justify_center()
                .pt(px(48.))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                    this.overlay = Overlay::None;
                    cx.notify();
                }))
                .child(
                    div()
                        .w(px(580.))
                        .max_h(px(520.))
                        .bg(t.secondary)
                        .border_1()
                        .border_color(border)
                        .rounded_lg()
                        .shadow_lg()
                        .overflow_hidden()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .child(content),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod direct_path_tests {
    use super::direct_path;

    #[test]
    fn absolute_relative_and_line() {
        let dir = std::env::temp_dir().join(format!("bp-qo-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "x").unwrap();
        let abs = dir.join("src/a.rs");
        assert_eq!(direct_path(&abs.display().to_string(), &dir), Some((abs.clone(), None)));
        assert_eq!(direct_path("src/a.rs:12", &dir), Some((abs.clone(), Some(12))));
        assert_eq!(direct_path("src/a.rs:12:3", &dir), Some((abs.clone(), Some(12))));
        assert_eq!(direct_path("nope.rs", &dir), None);
        assert_eq!(direct_path("", &dir), None);
        std::fs::remove_dir_all(dir).ok();
    }
}
