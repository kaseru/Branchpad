//! Cột giữa, phần dưới: thông tin commit + file + diff, diff file đang làm,
//! stash, xử lý xung đột, blame, lịch sử file, rebase tương tác.

use std::ops::Range;
use std::rc::Rc;

use branchpad_core::{
    git::{
        conflict_count, resolve, ChangedFile, ConflictPart, FileDiff, LineKind, Resolution,
        TodoAction, WorkingSide,
    },
    now_unix,
};
use gpui::{
    div, prelude::*, px, AnyElement, Context, Hsla, MouseButton, MouseDownEvent, SharedString,
    Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, PopupMenuItem},
    text::TextView,
    v_flex, ActiveTheme, Disableable, Icon, Sizable,
};

use super::{Detail, RepoView};
use crate::actions::FileOp;
use crate::icons::file_icon;
use crate::ui::{confirm, list_row, status_color, MONO, ROW_H};

/// Kiểu diff đang hiện — quyết định nút nào có trên mỗi đoạn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffMode {
    ReadOnly,
    Unstaged,
    Staged,
    Untracked,
}

impl From<WorkingSide> for DiffMode {
    fn from(s: WorkingSide) -> Self {
        match s {
            WorkingSide::Unstaged => DiffMode::Unstaged,
            WorkingSide::Staged => DiffMode::Staged,
            WorkingSide::Untracked => DiffMode::Untracked,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkOp {
    Stage,
    Unstage,
    Discard,
}

#[derive(Clone, Copy)]
pub(super) enum DiffRow {
    File(usize),
    Hunk(usize, usize),
    Line(usize, usize, usize),
}

fn diff_rows(files: &[FileDiff]) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    for (fi, f) in files.iter().enumerate() {
        rows.push(DiffRow::File(fi));
        for (hi, h) in f.hunks.iter().enumerate() {
            rows.push(DiffRow::Hunk(fi, hi));
            rows.extend((0..h.lines.len()).map(|li| DiffRow::Line(fi, hi, li)));
        }
    }
    rows
}

impl RepoView {
    /// Diff đang hiện trong phần chi tiết (nếu có).
    fn current_diff(&self) -> Option<(&[FileDiff], DiffMode)> {
        match &self.detail {
            Detail::Commit(v) => Some((&v.diff, DiffMode::ReadOnly)),
            Detail::Working { diff, side, .. } => Some((diff, (*side).into())),
            Detail::Stash { diff, .. } => Some((diff, DiffMode::ReadOnly)),
            _ => None,
        }
    }

    pub(super) fn render_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let muted = t.muted_foreground;
        let body: AnyElement = match &self.detail {
            Detail::Empty => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(muted)
                .child(t!("detail.empty.select"))
                .into_any_element(),
            Detail::Commit(_) => self.render_commit_detail(window, cx),
            Detail::Working { .. } => self.render_working_detail(cx),
            Detail::Stash { .. } => self.render_stash_detail(cx),
            Detail::Conflict(_) => self.render_conflict(window, cx),
            Detail::Blame { .. } => self.render_blame(cx),
            Detail::FileHistory { .. } => self.render_file_history(cx),
            Detail::Rebase(_) => self.render_rebase(window, cx),
        };
        div().size_full().overflow_hidden().child(body)
    }

    // ---- Diff ----

    fn render_diff(&self, cx: &Context<Self>) -> AnyElement {
        let Some((files, _)) = self.current_diff() else {
            return div().into_any_element();
        };
        if files.is_empty() {
            return div()
                .p_3()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t!("detail.empty.no_changes"))
                .into_any_element();
        }
        let rows = {
            let mut cache = self.diff_rows_cache.borrow_mut();
            match &*cache {
                Some((g, r)) if *g == self.diff_gen => r.clone(),
                _ => {
                    let r = Rc::new(diff_rows(files));
                    *cache = Some((self.diff_gen, r.clone()));
                    r
                }
            }
        };
        let count = rows.len();
        gpui::uniform_list(
            "diff",
            count,
            cx.processor(move |this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| this.render_diff_row(rows[i], i, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.diff_scroll.clone())
        .size_full()
        .font_family(MONO)
        .into_any_element()
    }

    fn render_diff_row(&self, row: DiffRow, i: usize, cx: &Context<Self>) -> AnyElement {
        let Some((files, mode)) = self.current_diff() else {
            return div().into_any_element();
        };
        let t = cx.theme();
        let base = h_flex().id(("diff-row", i)).h(ROW_H).w_full().text_sm();
        match row {
            DiffRow::File(fi) => {
                let f = &files[fi];
                // Cùng chiều cao `ROW_H` với mọi hàng khác (uniform_list); viền dưới nằm trong hàng, không cộng thêm.
                base.px_3()
                    .bg(t.secondary.opacity(0.6))
                    .border_b_1()
                    .border_color(t.border)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(match (&f.old_path, &f.new_path) {
                        (Some(o), Some(n)) if o != n => format!("{o} → {n}"),
                        _ => f.path().to_string(),
                    })
                    .when(f.binary, |d| d.child(div().pl_2().text_color(t.muted_foreground).child(t!("detail.binary"))))
                    .into_any_element()
            }
            DiffRow::Hunk(fi, hi) => {
                let h = &files[fi].hunks[hi];
                let btn = |id: &'static str, label: &'static str, op: HunkOp| {
                    Button::new((id, i))
                        .label(label)
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(move |this, _, window, cx| this.hunk_action(fi, hi, op, window, cx)))
                };
                base.px_3()
                    .gap_2()
                    .bg(t.info.opacity(0.08))
                    .border_b_1()
                    .border_color(t.border.opacity(0.3))
                    .text_color(t.muted_foreground)
                    .child(div().flex_1().truncate().child(h.header.clone()))
                    .when(matches!(mode, DiffMode::Unstaged | DiffMode::Untracked), |d| {
                        d.child(btn("stage-hunk", t!("detail.hunk.stage"), HunkOp::Stage))
                    })
                    .when(mode == DiffMode::Unstaged, |d| {
                        d.child(btn("discard-hunk", t!("detail.hunk.discard"), HunkOp::Discard))
                    })
                    .when(mode == DiffMode::Staged, |d| {
                        d.child(btn("unstage-hunk", t!("detail.hunk.unstage"), HunkOp::Unstage))
                    })
                    .into_any_element()
            }
            DiffRow::Line(fi, hi, li) => {
                let l = &files[fi].hunks[hi].lines[li];
                let (sign, bg, gutter): (&str, Option<Hsla>, Option<Hsla>) = match l.kind {
                    LineKind::Added => ("+", Some(t.success.opacity(0.14)), Some(t.success.opacity(0.24))),
                    LineKind::Removed => ("-", Some(t.danger.opacity(0.14)), Some(t.danger.opacity(0.24))),
                    LineKind::Context => (" ", None, None),
                    LineKind::NoNewline => ("", None, None),
                };
                let sign_color = match l.kind {
                    LineKind::Added => t.success,
                    LineKind::Removed => t.danger,
                    _ => t.muted_foreground,
                };
                let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
                let line_text = l.text.clone();
                base.context_menu(move |menu, _, _| {
                    let text = line_text.clone();
                    menu.item(PopupMenuItem::new(t!("actions.edit.copy")).on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                    }))
                })
                .when_some(bg, |d, c| d.bg(c))
                    .child(
                        div()
                            .w(px(88.))
                            .flex_none()
                            .h_full()
                            .flex()
                            .when_some(gutter, |d, c| d.bg(c))
                            .text_color(t.muted_foreground)
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
                            .when(l.kind == LineKind::NoNewline, |d| d.italic().text_color(t.muted_foreground))
                            .child(l.text.replace('\t', "    ")),
                    )
                    .into_any_element()
            }
        }
    }

    fn hunk_action(&mut self, fi: usize, hi: usize, op: HunkOp, window: &mut Window, cx: &mut Context<Self>) {
        let Some((files, _)) = self.current_diff() else { return };
        let Some(file) = files.get(fi).cloned() else { return };
        let run = move |this: &mut Self, cx: &mut Context<Self>| {
            let (busy, f) = (
                match op {
                    HunkOp::Stage => t!("detail.busy.stage_hunk"),
                    HunkOp::Unstage => t!("detail.busy.unstage_hunk"),
                    HunkOp::Discard => t!("detail.busy.discard_hunk"),
                },
                file,
            );
            this.run_op(busy, None, cx, move |r| match op {
                HunkOp::Stage => r.stage_hunk(&f, hi),
                HunkOp::Unstage => r.unstage_hunk(&f, hi),
                HunkOp::Discard => r.discard_hunk(&f, hi),
            });
        };
        if op == HunkOp::Discard {
            confirm(
                window,
                cx,
                t!("detail.confirm.discard_hunk.title"),
                t!("detail.confirm.discard_hunk.detail"),
                &[t!("detail.cancel"), t!("detail.hunk.discard")],
                move |this: &mut Self, _, _, cx| run(this, cx),
            );
        } else {
            run(self, cx);
        }
    }

    // ---- File đang làm ----

    fn render_working_detail(&self, cx: &Context<Self>) -> AnyElement {
        let Detail::Working { path, side, .. } = &self.detail else {
            return div().into_any_element();
        };
        let t = cx.theme();
        let side = *side;
        let abs = self.repo.workdir().join(path);
        let p = path.clone();
        let header = h_flex()
            .h(px(34.))
            .px_2()
            .gap_1()
            .flex_none()
            .border_b_1()
            .border_color(t.border)
            .text_sm()
            .child(div().flex_1().truncate().child(SharedString::from(path.clone())))
            .child(div().text_xs().text_color(t.muted_foreground).px_2().child(match side {
                WorkingSide::Unstaged => t!("detail.side.unstaged"),
                WorkingSide::Staged => t!("detail.side.staged"),
                WorkingSide::Untracked => t!("detail.side.untracked"),
            }))
            .child({
                let p = p.clone();
                match side {
                    WorkingSide::Staged => Button::new("wd-unstage").label(t!("detail.file.unstage")).xsmall().ghost()
                        .on_click(cx.listener(move |this, _, _, cx| this.unstage_paths(vec![p.clone()], cx))),
                    _ => Button::new("wd-stage").label("Stage file").xsmall().ghost()
                        .on_click(cx.listener(move |this, _, _, cx| this.stage_paths(vec![p.clone()], cx))),
                }
            })
            .when(side != WorkingSide::Staged, |d| {
                let p = p.clone();
                d.child(
                    Button::new("wd-discard").label(t!("detail.file.discard")).xsmall().ghost()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.discard_paths(vec![p.clone()], side == WorkingSide::Untracked, window, cx)
                        })),
                )
            })
            .child(self.file_button("wd-open", t!("detail.file.open"), FileOp::OpenInEditor, abs.clone(), cx))
            .when(side != WorkingSide::Untracked, |d| {
                d.child(self.file_button("wd-history", t!("detail.file.history"), FileOp::History, abs.clone(), cx))
                    .child(self.file_button("wd-blame", "Blame", FileOp::Blame, abs.clone(), cx))
            });
        v_flex()
            .size_full()
            .child(header)
            .child(div().flex_1().overflow_hidden().child(self.render_diff(cx)))
            .into_any_element()
    }

    fn file_button(
        &self,
        id: &'static str,
        label: &'static str,
        op: FileOp,
        path: std::path::PathBuf,
        cx: &Context<Self>,
    ) -> Button {
        Button::new(id).label(label).xsmall().ghost().on_click(cx.listener(
            move |this, _, window, cx| {
                this.on_file_action(
                    &crate::actions::OnFile {
                        op,
                        path: path.clone(),
                    },
                    window,
                    cx,
                )
            },
        ))
    }

    // ---- Commit ----

    /// Danh sách file của commit / stash (uniform_list: chỉ dựng các dòng đang thấy).
    fn render_file_list(&self, id: &'static str, count: usize, selected: Option<usize>, cx: &Context<Self>) -> impl IntoElement {
        div()
            .w(px(280.))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                gpui::uniform_list(
                    id,
                    count,
                    cx.processor(move |this, range: Range<usize>, _window, cx| {
                        let files: &[ChangedFile] = match &this.detail {
                            Detail::Commit(v) => &v.files,
                            Detail::Stash { files, .. } => files,
                            _ => return Vec::new(),
                        };
                        range
                            .filter_map(|i| files.get(i).map(|f| (i, f)))
                            .map(|(i, f)| {
                                let path = f.path.clone();
                                let (file_ico, file_color) = file_icon(&f.path, false, false);
                                list_row((id, i), selected == Some(i), cx)
                                    .child(
                                        div()
                                            .w(px(12.))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(status_color(f.status, cx))
                                            .child(f.status.to_string()),
                                    )
                                    .child(Icon::new(file_ico).size_4().text_color(file_color))
                                    .child(div().flex_1().truncate().child(SharedString::from(f.path.clone())))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                                            if ev.click_count >= 2 {
                                                this.open_detail_file_revision(&path, window, cx);
                                            } else {
                                                this.select_detail_file(i, cx);
                                            }
                                        }),
                                    )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .size_full(),
            )
    }

    fn select_detail_file(&mut self, i: usize, cx: &mut Context<Self>) {
        match &self.detail {
            Detail::Commit(_) => self.select_commit_file(i, cx),
            Detail::Stash { .. } => self.select_stash_file(i, cx),
            _ => {}
        }
    }

    /// Bấm đúp file của commit/stash: mở bản ở commit đó (chỉ đọc).
    fn open_detail_file_revision(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let rev = match &self.detail {
            Detail::Commit(v) => v.id.clone(),
            Detail::Stash { name, .. } => name.clone(),
            _ => return,
        };
        self.open_revision_in_editor(rev, path.to_string(), window, cx);
    }

    fn render_commit_detail(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Detail::Commit(v) = &self.detail else {
            return div().into_any_element();
        };
        let t = cx.theme();
        let now = now_unix();
        let info = match &v.detail {
            None => div().id("commit-info").p_2().text_sm().text_color(t.muted_foreground).child(t!("detail.loading")),
            Some(d) => div()
                .id("commit-info")
                .p_2()
                .max_h(px(160.))
                .overflow_y_scroll()
                .flex_none()
                .text_sm()
                .border_b_1()
                .border_color(t.border)
                .child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .text_color(t.muted_foreground)
                        .child(div().font_family(MONO).child(d.id.clone()))
                        .child(format!("{} <{}>", d.author, d.email))
                        .child(crate::i18n::relative_time(d.author_time, now))
                        .when(d.parents.len() > 1, |x| x.child("(merge)")),
                )
                .child(div().pt_1().whitespace_normal().cursor_text().child(selectable_text(
                    format!("commit-msg:{}", d.id),
                    &d.message,
                    window,
                    cx,
                ))),
        };
        v_flex()
            .size_full()
            .child(info)
            .child(
                h_flex()
                    .flex_1()
                    .overflow_hidden()
                    .child(self.render_file_list("commit-files", v.files.len(), v.selected, cx))
                    .child(div().flex_1().h_full().overflow_hidden().child(self.render_diff(cx))),
            )
            .into_any_element()
    }

    // ---- Stash ----

    pub(crate) fn show_stash(&mut self, name: String, cx: &mut Context<Self>) {
        let n = name.clone();
        self.spawn_git(
            None,
            cx,
            move |r| r.stash_files(&n),
            move |this, files, cx| {
                this.diff_gen += 1;
                this.detail = Detail::Stash {
                    name,
                    files,
                    selected: None,
                    diff: Vec::new(),
                };
                this.select_stash_file(0, cx);
            },
        );
    }

    fn select_stash_file(&mut self, i: usize, cx: &mut Context<Self>) {
        let Detail::Stash { name, files, selected, .. } = &mut self.detail else { return };
        let Some(f) = files.get(i) else { return };
        *selected = Some(i);
        let (n, path, check) = (name.clone(), f.path.clone(), name.clone());
        cx.notify();
        self.spawn_git(
            None,
            cx,
            move |r| r.stash_diff(&n, &path),
            move |this, d, _| {
                if let Detail::Stash { name, selected, diff, .. } = &mut this.detail {
                    if *name == check && *selected == Some(i) {
                        *diff = d;
                        this.diff_gen += 1;
                    }
                }
            },
        );
    }

    fn render_stash_detail(&self, cx: &Context<Self>) -> AnyElement {
        let Detail::Stash { name, files, selected, .. } = &self.detail else {
            return div().into_any_element();
        };
        let t = cx.theme();
        let message = self
            .stashes
            .iter()
            .find(|s| s.name == *name)
            .map(|s| s.message.clone())
            .unwrap_or_default();
        let header = h_flex()
            .h(px(34.))
            .px_2()
            .gap_1()
            .flex_none()
            .border_b_1()
            .border_color(t.border)
            .text_sm()
            .child(div().flex_1().truncate().child(format!("{name}: {message}")))
            .child(self.ref_button("st-apply", t!("detail.stash.apply"), crate::actions::RefOp::StashApply, name, cx))
            .child(self.ref_button("st-pop", "Pop", crate::actions::RefOp::StashPop, name, cx))
            .child(self.ref_button("st-drop", t!("detail.stash.drop"), crate::actions::RefOp::StashDrop, name, cx));
        v_flex()
            .size_full()
            .child(header)
            .child(
                h_flex()
                    .flex_1()
                    .overflow_hidden()
                    .child(self.render_file_list("stash-files", files.len(), *selected, cx))
                    .child(div().flex_1().h_full().overflow_hidden().child(self.render_diff(cx))),
            )
            .into_any_element()
    }

    pub(super) fn ref_button(
        &self,
        id: &'static str,
        label: &'static str,
        op: crate::actions::RefOp,
        name: &str,
        cx: &Context<Self>,
    ) -> Button {
        let name = name.to_string();
        Button::new(id).label(label).xsmall().ghost().on_click(cx.listener(
            move |this, _, window, cx| {
                this.on_ref_action(
                    &crate::actions::OnRef {
                        op,
                        name: name.clone(),
                    },
                    window,
                    cx,
                )
            },
        ))
    }

    // ---- Xung đột ----

    pub(crate) fn show_conflict(&mut self, path: String, cx: &mut Context<Self>) {
        let abs = self.repo.workdir().join(&path);
        match std::fs::read_to_string(&abs) {
            Ok(text) => {
                let parts = branchpad_core::git::parse_conflicts(&text);
                let n = conflict_count(&parts);
                self.detail = Detail::Conflict(super::ConflictView {
                    path,
                    parts,
                    choices: vec![None; n],
                });
            }
            Err(e) => self.error = Some(t!("detail.err.read", path = path, e = e)),
        }
        cx.notify();
    }

    fn render_conflict(&self, _window: &mut Window, cx: &Context<Self>) -> AnyElement {
        let Detail::Conflict(v) = &self.detail else {
            return div().into_any_element();
        };
        let t = cx.theme();
        let total = v.choices.len();
        let chosen = v.choices.iter().filter(|c| c.is_some()).count();
        let path = v.path.clone();
        let abs = self.repo.workdir().join(&v.path);

        let header = h_flex()
            .h(px(34.))
            .px_2()
            .gap_1()
            .flex_none()
            .border_b_1()
            .border_color(t.border)
            .text_sm()
            .child(div().flex_1().truncate().child(t!("detail.conflict.header", path = v.path, chosen = chosen, total = total)))
            .child({
                let p = path.clone();
                Button::new("cf-ours-all").label(t!("detail.conflict.ours_all")).xsmall().ghost()
                    .on_click(cx.listener(move |this, _, _, cx| this.take_side(p.clone(), false, cx)))
            })
            .child({
                let p = path.clone();
                Button::new("cf-theirs-all").label(t!("detail.conflict.theirs_all")).xsmall().ghost()
                    .on_click(cx.listener(move |this, _, _, cx| this.take_side(p.clone(), true, cx)))
            })
            .child(self.file_button("cf-open", t!("detail.conflict.open_editor"), FileOp::OpenInEditor, abs, cx))
            .child(
                Button::new("cf-save").label(if total == 0 { t!("detail.conflict.mark_resolved") } else { t!("detail.conflict.save_mark_resolved") })
                    .xsmall().primary()
                    .disabled(chosen < total)
                    .on_click(cx.listener(|this, _, _, cx| this.save_conflict(cx))),
            );

        let mut blocks: Vec<AnyElement> = Vec::new();
        let mut ci = 0usize;
        for (pi, part) in v.parts.iter().enumerate() {
            match part {
                ConflictPart::Common(text) => {
                    let lines: Vec<&str> = text.lines().collect();
                    let shown = if lines.len() > 8 {
                        let mut s: Vec<String> = lines[..3].iter().map(|l| l.to_string()).collect();
                        s.push(t!("detail.conflict.unchanged_lines", n = lines.len() - 6));
                        s.extend(lines[lines.len() - 3..].iter().map(|l| l.to_string()));
                        s
                    } else {
                        lines.iter().map(|l| l.to_string()).collect()
                    };
                    blocks.push(
                        div()
                            .px_2()
                            .text_xs()
                            .font_family(MONO)
                            .text_color(t.muted_foreground)
                            .children(shown.into_iter().map(|l| div().child(l)))
                            .into_any_element(),
                    );
                }
                ConflictPart::Conflict(b) => {
                    let idx = ci;
                    ci += 1;
                    let choice = v.choices.get(idx).copied().flatten();
                    let pick = |id: &'static str, label: &'static str, r: Resolution| {
                        Button::new((id, pi))
                            .label(label)
                            .xsmall()
                            .when(choice == Some(r), |b| b.primary())
                            .when(choice != Some(r), |b| b.ghost())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Detail::Conflict(v) = &mut this.detail {
                                    if let Some(c) = v.choices.get_mut(idx) {
                                        *c = if *c == Some(r) { None } else { Some(r) };
                                    }
                                }
                                cx.notify();
                            }))
                    };
                    let side = |label: String, text: &str, color: Hsla, dim: bool| {
                        v_flex()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .font_family(MONO)
                            .bg(color.opacity(if dim { 0.05 } else { 0.15 }))
                            .when(dim, |d| d.opacity(0.5))
                            .child(div().text_color(t.muted_foreground).child(label))
                            .children(text.lines().map(|l| div().child(l.replace('\t', "    "))))
                    };
                    let ours_dim = matches!(choice, Some(Resolution::Theirs));
                    let theirs_dim = matches!(choice, Some(Resolution::Ours));
                    blocks.push(
                        v_flex()
                            .m_2()
                            .border_1()
                            .border_color(if choice.is_some() { t.success } else { t.warning })
                            .rounded_md()
                            .overflow_hidden()
                            .child(
                                h_flex()
                                    .px_2()
                                    .py_1()
                                    .gap_1()
                                    .bg(t.secondary)
                                    .text_xs()
                                    .child(div().flex_1().child(t!("detail.conflict.block", n = idx + 1)))
                                    .child(pick("cf-ours", t!("detail.conflict.ours"), Resolution::Ours))
                                    .child(pick("cf-theirs", t!("detail.conflict.theirs"), Resolution::Theirs))
                                    .child(pick("cf-both", t!("detail.conflict.both"), Resolution::OursThenTheirs))
                                    .child(pick("cf-both-rev", t!("detail.conflict.both_rev"), Resolution::TheirsThenOurs)),
                            )
                            .child(side(t!("detail.conflict.ours_label", label = b.ours_label), &b.ours, t.success, ours_dim))
                            .when_some(b.base.as_ref(), |d, base| {
                                d.child(side(t!("detail.conflict.base_label", label = b.base_label), base, t.muted_foreground, true))
                            })
                            .child(side(t!("detail.conflict.theirs_label", label = b.theirs_label), &b.theirs, t.info, theirs_dim))
                            .into_any_element(),
                    );
                }
            }
        }

        v_flex()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("conflict-body")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(blocks),
            )
            .into_any_element()
    }

    fn take_side(&mut self, path: String, theirs: bool, cx: &mut Context<Self>) {
        let p = path.clone();
        self.run_op(t!("detail.busy.apply_choice"), Some(t!("detail.done.conflict_resolved")), cx, move |r| {
            r.checkout_side(&p, theirs)?;
            r.mark_resolved(&p)
        });
        self.detail = Detail::Empty;
    }

    fn save_conflict(&mut self, cx: &mut Context<Self>) {
        let Detail::Conflict(v) = &self.detail else { return };
        let text = resolve(&v.parts, &v.choices);
        let path = v.path.clone();
        let abs = self.repo.workdir().join(&path);
        if let Err(e) = std::fs::write(&abs, text) {
            self.error = Some(t!("detail.err.save", path = path, e = e));
            cx.notify();
            return;
        }
        let msg = t!("detail.done.conflict_path", path = path);
        self.run_op(t!("detail.busy.mark_resolved"), Some(&msg), cx, move |r| r.mark_resolved(&path));
        self.detail = Detail::Empty;
    }

    // ---- Blame / lịch sử file ----

    fn render_blame(&self, cx: &Context<Self>) -> AnyElement {
        let Detail::Blame { path, lines } = &self.detail else {
            return div().into_any_element();
        };
        let t = cx.theme();
        let count = lines.len();
        let now = now_unix();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(px(34.))
                    .px_2()
                    .flex_none()
                    .border_b_1()
                    .border_color(t.border)
                    .text_sm()
                    .child(format!("Blame: {path}")),
            )
            .child(
                gpui::uniform_list(
                    "blame",
                    count,
                    cx.processor(move |this, range: Range<usize>, _window, cx| {
                        let Detail::Blame { lines, .. } = &this.detail else {
                            return Vec::new();
                        };
                        let t = cx.theme();
                        range
                            .filter_map(|i| lines.get(i).map(|l| (i, l)))
                            .map(|(i, l)| {
                                let first = i == 0 || lines[i - 1].commit != l.commit;
                                let id = l.commit.clone();
                                list_row(("blame-row", i), false, cx)
                                    .text_xs()
                                    .font_family(MONO)
                                    .child(
                                        div()
                                            .w(px(300.))
                                            .flex_none()
                                            .truncate()
                                            .text_color(t.muted_foreground)
                                            .when(first, |d| {
                                                d.child(format!(
                                                    "{} {} · {} · {}",
                                                    &l.commit[..l.commit.len().min(7)],
                                                    l.author,
                                                    crate::i18n::relative_time(l.time, now),
                                                    l.summary
                                                ))
                                            }),
                                    )
                                    .child(div().w(px(44.)).flex_none().text_right().pr_2().text_color(t.muted_foreground).child(l.line_no.to_string()))
                                    .child(div().flex_1().whitespace_nowrap().overflow_hidden().child(l.text.replace('\t', "    ")))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            if !id.starts_with("0000000") {
                                                this.reveal_commit(&id, cx);
                                            }
                                        }),
                                    )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1(),
            )
            .into_any_element()
    }

    fn render_file_history(&self, cx: &Context<Self>) -> AnyElement {
        let Detail::FileHistory { path, commits } = &self.detail else {
            return div().into_any_element();
        };
        let t = cx.theme();
        let now = now_unix();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(px(34.))
                    .px_2()
                    .flex_none()
                    .border_b_1()
                    .border_color(t.border)
                    .text_sm()
                    .child(t!("detail.history.title", path = path, n = commits.len())),
            )
            .child(
                gpui::uniform_list(
                    "file-history",
                    commits.len(),
                    cx.processor(move |this, range: Range<usize>, _window, cx| {
                        let Detail::FileHistory { commits, .. } = &this.detail else { return Vec::new() };
                        let muted = cx.theme().muted_foreground;
                        range
                            .filter_map(|i| commits.get(i).map(|c| (i, c)))
                            .map(|(i, c)| {
                                let id = c.id.clone();
                                list_row(("fh", i), false, cx)
                                    .child(div().w(px(64.)).flex_none().font_family(MONO).text_xs().child(c.short_id().to_string()))
                                    .child(div().flex_1().truncate().child(SharedString::from(c.summary.clone())))
                                    .child(div().w(px(140.)).flex_none().truncate().text_color(muted).child(SharedString::from(c.author.clone())))
                                    .child(div().w(px(100.)).flex_none().text_color(muted).child(crate::i18n::relative_time(c.time, now)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| this.reveal_commit(&id, cx)),
                                    )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1(),
            )
            .into_any_element()
    }

    // ---- Rebase tương tác ----

    fn render_rebase(&self, _window: &mut Window, cx: &Context<Self>) -> AnyElement {
        let Detail::Rebase(v) = &self.detail else {
            return div().into_any_element();
        };
        let t = cx.theme();
        let n = v.items.len();
        let base = v
            .base
            .as_deref()
            .map(|b| b[..b.len().min(8)].to_string())
            .unwrap_or_else(|| t!("detail.rebase.root").into());
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(px(34.))
                    .px_2()
                    .gap_1()
                    .flex_none()
                    .border_b_1()
                    .border_color(t.border)
                    .text_sm()
                    .child(div().flex_1().child(t!("detail.rebase.title", n = n, base = base)))
                    .child(
                        Button::new("rb-cancel").label(t!("detail.cancel")).xsmall().ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.detail = Detail::Empty;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("rb-start").label(t!("detail.rebase.start")).xsmall().primary()
                            .disabled(n == 0 || self.busy.is_some())
                            .on_click(cx.listener(|this, _, _, cx| this.start_rebase(cx))),
                    ),
            )
            .child(
                gpui::uniform_list(
                    "rebase-items",
                    n,
                    cx.processor(move |this, range: Range<usize>, _window, cx| {
                        let Detail::Rebase(v) = &this.detail else { return Vec::new() };
                        let muted = cx.theme().muted_foreground;
                        let n = v.items.len();
                        range
                            .filter_map(|i| v.items.get(i).map(|item| (i, item)))
                            .map(|(i, item)| {
                                let dropped = item.action == TodoAction::Drop;
                                list_row(("rb", i), false, cx)
                                    .child(
                                        Button::new(("rb-action", i))
                                            .label(crate::i18n::todo_action_label(item.action))
                                            .xsmall()
                                            .ghost()
                                            .tooltip(t!("detail.rebase.action_tooltip"))
                                            .on_click(cx.listener(move |this, _, window, cx| this.cycle_rebase_action(i, window, cx))),
                                    )
                                    .child(div().w(px(64.)).flex_none().font_family(MONO).text_xs().child(item.id[..item.id.len().min(7)].to_string()))
                                    .child(
                                        div()
                                            .flex_1()
                                            .truncate()
                                            .when(dropped, |d| d.line_through().text_color(muted))
                                            .child(SharedString::from(
                                                item.message
                                                    .as_deref()
                                                    .and_then(|m| m.lines().next())
                                                    .unwrap_or(&item.summary)
                                                    .to_string(),
                                            )),
                                    )
                                    .child(
                                        Button::new(("rb-up", i)).label("↑").xsmall().ghost().disabled(i == 0)
                                            .on_click(cx.listener(move |this, _, _, cx| this.move_rebase_item(i, true, cx))),
                                    )
                                    .child(
                                        Button::new(("rb-down", i)).label("↓").xsmall().ghost().disabled(i + 1 >= n)
                                            .on_click(cx.listener(move |this, _, _, cx| this.move_rebase_item(i, false, cx))),
                                    )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1(),
            )
            .into_any_element()
    }

    fn move_rebase_item(&mut self, i: usize, up: bool, cx: &mut Context<Self>) {
        if let Detail::Rebase(v) = &mut self.detail {
            let j = if up { i.checked_sub(1) } else { Some(i + 1) };
            if let Some(j) = j.filter(|j| *j < v.items.len()) {
                v.items.swap(i, j);
            }
        }
        cx.notify();
    }

    fn cycle_rebase_action(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Detail::Rebase(v) = &mut self.detail else { return };
        let Some(item) = v.items.get_mut(i) else { return };
        item.action = item.action.next();
        if item.action == TodoAction::Reword {
            let initial = item.message.clone().unwrap_or_else(|| item.summary.clone());
            self.open_prompt(super::overlay::PromptKind::Reword(i), t!("detail.rebase.new_message"), &initial, window, cx);
        }
        cx.notify();
    }

    fn start_rebase(&mut self, cx: &mut Context<Self>) {
        let Detail::Rebase(v) = &self.detail else { return };
        let base = v.base.clone();
        let items = v.items.clone();
        self.run_op(t!("detail.busy.rebase"), Some(t!("detail.done.rebase")), cx, move |r| {
            r.rebase_interactive(base.as_deref(), &items)
        });
        self.detail = Detail::Empty;
    }
}

/// Chữ chọn được (kéo chuột, bấm đúp chọn từ): dựng bằng khối code của TextView để giữ nguyên xuống dòng
/// và ký tự đặc biệt (không bị hiểu là markdown).
pub(super) fn selectable_text(id: impl Into<SharedString>, text: &str, window: &mut Window, cx: &mut gpui::App) -> TextView {
    let id: SharedString = id.into();
    TextView::markdown(id, format!("````\n{}\n````", text.trim_end()), window, cx)
        .selectable(true)
        .selection_slop(px(24.), px(4.))
}
