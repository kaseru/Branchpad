//! Hộp nổi trên cột giữa: ô nhập một dòng (tên nhánh, tên file...),
//! mở file nhanh (Cmd+P), tìm chữ trong cả thư mục (Cmd+Shift+F).

use std::rc::Rc;

use branchpad_core::{fuzzy_filter, git::GrepMatch};
use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, MouseButton, MouseDownEvent, SharedString,
    Subscription, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, ActiveTheme, Sizable,
};

use super::RepoView;
use crate::actions::{FindInFiles, GotoLine, QuickOpen};
use crate::ui::{list_row, split_path, MONO};

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
    label: SharedString,
    input: Entity<InputState>,
    _sub: Subscription,
}

pub struct QuickOpenState {
    input: Entity<InputState>,
    files: Rc<Vec<String>>,
    results: Vec<usize>,
    _sub: Subscription,
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
            }
        });
        input.update(cx, |s, cx| s.focus(window, cx));
        self.overlay = Overlay::Prompt(Prompt {
            kind,
            label: SharedString::from(label.to_string()),
            input,
            _sub: sub,
        });
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
        self.open_prompt(PromptKind::GotoLine, "Đến dòng số", "", window, cx);
    }

    pub(super) fn on_quick_open(&mut self, _: &QuickOpen, window: &mut Window, cx: &mut Context<Self>) {
        self.spawn_git_in(
            None,
            window,
            cx,
            |r| r.list_files(),
            |this, files, window, cx| {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("Gõ tên file…"));
                let sub = cx.subscribe_in(&input, window, |this, input, ev: &InputEvent, window, cx| {
                    let Overlay::QuickOpen(q) = &mut this.overlay else { return };
                    match ev {
                        InputEvent::Change { .. } => {
                            let query = input.read(cx).value().to_string();
                            q.results = fuzzy_filter(&query, &q.files, QUICK_OPEN_LIMIT);
                            cx.notify();
                        }
                        InputEvent::PressEnter { .. } => {
                            if let Some(rel) = q.results.first().and_then(|i| q.files.get(*i)).cloned() {
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
                    _sub: sub,
                });
            },
        );
    }

    fn open_quick_result(&mut self, rel: String, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        let path = self.repo.workdir().join(&rel);
        if let Some(tree) = self.tree.as_mut() {
            if let Err(e) = tree.reveal(&self.repo, &path) {
                eprintln!("branchpad: không mở được thư mục chứa {rel}: {e}");
            }
        }
        self.selected_path = Some(path.clone());
        self.open_in_editor(path, None, window, cx);
    }

    pub(super) fn on_find_in_files(&mut self, _: &FindInFiles, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Tìm chữ trong cả thư mục (Enter để tìm)")
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
        let (bg, border, muted) = (t.background, t.border, t.muted_foreground);
        let content: AnyElement = match &self.overlay {
            Overlay::None => return None,
            Overlay::Prompt(p) => h_flex()
                .p_2()
                .gap_2()
                .child(div().text_sm().flex_none().child(p.label.clone()))
                .child(div().flex_1().child(Input::new(&p.input).small()))
                .child(
                    Button::new("prompt-ok").label("OK").small().primary().on_click(cx.listener(
                        |this, _, window, cx| {
                            if let Overlay::Prompt(p) = &this.overlay {
                                let v = p.input.read(cx).value().to_string();
                                this.submit_prompt(v, window, cx);
                            }
                        },
                    )),
                )
                .child(
                    Button::new("prompt-cancel").label("Huỷ").small().ghost().on_click(cx.listener(
                        |this, _, window, cx| {
                            this.overlay = super::Overlay::None;
                            window.focus(&this.focus);
                            cx.notify();
                        },
                    )),
                )
                .into_any_element(),
            Overlay::QuickOpen(q) => v_flex()
                .p_2()
                .gap_1()
                .child(Input::new(&q.input).small())
                .child(
                    v_flex()
                        .id("quick-open-results")
                        .max_h(px(360.))
                        .overflow_y_scroll()
                        .children(q.results.iter().enumerate().filter_map(|(n, &i)| {
                            let rel = q.files.get(i)?.clone();
                            let (name, dir) = split_path(&rel);
                            Some(
                                list_row(("qo", n), n == 0, cx)
                                    .child(div().flex_none().child(name))
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
                .into_any_element(),
            Overlay::Find(f) => v_flex()
                .p_2()
                .gap_1()
                .child(Input::new(&f.input).small())
                .child(div().text_xs().text_color(muted).child(if f.searching {
                    "Đang tìm…".to_string()
                } else if f.results.len() >= FIND_LIMIT {
                    format!("Hơn {FIND_LIMIT} kết quả, chỉ hiện {FIND_LIMIT} kết quả đầu")
                } else {
                    format!("{} kết quả", f.results.len())
                }))
                .child(
                    v_flex()
                        .id("find-results")
                        .max_h(px(420.))
                        .overflow_y_scroll()
                        .children(f.results.iter().enumerate().map(|(n, m)| {
                            let path = self.repo.workdir().join(&m.path);
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
                                        this.open_in_editor(path.clone(), Some(line), window, cx)
                                    }),
                                )
                        })),
                )
                .into_any_element(),
        };
        Some(
            div()
                .absolute()
                .top(px(44.))
                .left(px(24.))
                .right(px(24.))
                .bg(bg)
                .border_1()
                .border_color(border)
                .rounded_lg()
                .shadow_lg()
                .child(content)
                .into_any_element(),
        )
    }
}
