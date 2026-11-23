//! Cột giữa, phần trên: đồ thị nhánh + danh sách commit (tải dần, chỉ vẽ dòng đang hiện).

use std::ops::Range;

use branchpad_core::{
    git::{Anchor, GraphRow, LogFilter},
    now_unix,
};
use gpui::{
    canvas, div, fill, point, prelude::*, px, size, AnyElement, Bounds, Context, FocusHandle,
    Hsla, MouseButton, MouseDownEvent, PathBuilder, ScrollStrategy, SharedString, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    menu::{ContextMenuExt, PopupMenu},
    v_flex, ActiveTheme, Icon, IconName, Sizable,
};

use super::{CommitView, Detail, RepoView};
use crate::actions::{CommitOp, OnCommit, OnRef, RefOp};
use crate::icons::BpIcon;
use crate::ui::{lane_color, list_row, menu_item_with_shortcut, LANE_W, MONO};

/// Số làn tối đa vẽ ra (làn xa hơn bị cắt để danh sách không quá hẹp).
const MAX_LANES: usize = 16;

fn graph_cell(row: &GraphRow, lanes: usize, bg: Hsla) -> impl IntoElement {
    let row = row.clone();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let x = |lane: usize| bounds.origin.x + px(LANE_W * lane as f32 + LANE_W / 2. + 4.);
            let y = |a: Anchor| match a {
                Anchor::Top => bounds.origin.y,
                Anchor::Center => bounds.origin.y + bounds.size.height / 2.,
                Anchor::Bottom => bounds.origin.y + bounds.size.height,
            };
            for l in &row.lines {
                if l.from_lane >= MAX_LANES || l.to_lane >= MAX_LANES {
                    continue;
                }
                let p0 = point(x(l.from_lane), y(l.from));
                let p1 = point(x(l.to_lane), y(l.to));
                let mut b = PathBuilder::stroke(px(1.25));
                b.move_to(p0);
                if l.from_lane == l.to_lane {
                    b.line_to(p1);
                } else {
                    // Kiểu Fork: đi ngang ở dòng của chấm, bo góc tròn rồi chạy thẳng đứng theo làn.
                    const K: f32 = 0.5523; // xấp xỉ cung tròn bằng Bézier bậc 3
                    let sgn: f32 = if l.to_lane >= l.from_lane { 1. } else { -1. };
                    let lanes_apart = l.to_lane.abs_diff(l.from_lane) as f32;
                    let r = px(LANE_W * lanes_apart).min(bounds.size.height / 2.);
                    match (l.from, l.to) {
                        // Rời chấm sang làn khác: ngang → bo góc → thẳng xuống.
                        (Anchor::Center, _) => {
                            b.line_to(point(p1.x - sgn * r, p0.y));
                            b.cubic_bezier_to(
                                point(p1.x, p0.y + r),
                                point(p1.x - sgn * r * (1. - K), p0.y),
                                point(p1.x, p0.y + r * (1. - K)),
                            );
                            b.line_to(p1);
                        }
                        // Từ làn khác vào chấm (kiểu Fork): cung elip nằm gọn trong dòng (không tràn ra ngoài vì bị cắt),
                        // thẳng đứng ở đầu và tiếp tuyến ngang khi chạm chấm.
                        (_, Anchor::Center) => {
                            let ry = bounds.size.height / 2.;
                            let start = point(p0.x, p1.y - ry);
                            let dx = p1.x - p0.x;
                            b = PathBuilder::stroke(px(1.25));
                            b.move_to(start);
                            b.cubic_bezier_to(
                                p1,
                                point(p0.x, p1.y - ry * 0.55),
                                point(p1.x - dx * 0.7, p1.y),
                            );
                        }
                        // Đổi làn xuyên hàng (hiếm): chữ S mượt.
                        _ => {
                            let dy = p1.y - p0.y;
                            b.cubic_bezier_to(p1, point(p0.x, p0.y + dy * 0.5), point(p1.x, p1.y - dy * 0.5));
                        }
                    }
                }
                if let Ok(path) = b.build() {
                    window.paint_path(path, lane_color(l.color));
                }
            }
            if row.lane < MAX_LANES {
                let c = point(x(row.lane), y(Anchor::Center));
                let color = lane_color(row.color);
                let d = px(10.);
                window.paint_quad(
                    fill(Bounds::centered_at(c, size(d, d)), color).corner_radii(d / 2.),
                );
                if !row.is_merge {
                    // Chấm thường là vòng tròn; merge là chấm đặc.
                    let inner = px(5.);
                    window.paint_quad(
                        fill(Bounds::centered_at(c, size(inner, inner)), bg).corner_radii(inner / 2.),
                    );
                }
            }
        },
    )
    .w(px(LANE_W * lanes as f32 + 8.))
    .h_full()
    .flex_none()
}

pub(super) fn commit_menu(menu: PopupMenu, id: &str, focus: &FocusHandle) -> PopupMenu {
    let a = |op: CommitOp| -> Box<dyn gpui::Action> {
        Box::new(OnCommit {
            op,
            id: id.to_string(),
        })
    };
    let m = menu.action_context(focus.clone());
    let m = menu_item_with_shortcut(m, t!("history.menu.checkout"), Some("Space"), a(CommitOp::Checkout));
    let m = menu_item_with_shortcut(m, t!("history.menu.create_branch"), None, a(CommitOp::CreateBranch));
    let m = menu_item_with_shortcut(m, t!("history.menu.create_tag"), None, a(CommitOp::CreateTag));
    let m = m.separator();
    let m = menu_item_with_shortcut(m, t!("history.menu.cherry_pick"), None, a(CommitOp::CherryPick));
    let m = menu_item_with_shortcut(m, t!("history.menu.revert"), None, a(CommitOp::Revert));
    let m = m.separator();
    let m = menu_item_with_shortcut(m, t!("history.menu.reset_soft"), None, a(CommitOp::ResetSoft));
    let m = menu_item_with_shortcut(m, t!("history.menu.reset_mixed"), None, a(CommitOp::ResetMixed));
    let m = menu_item_with_shortcut(m, t!("history.menu.reset_hard"), None, a(CommitOp::ResetHard));
    let m = m.separator();
    let m = menu_item_with_shortcut(m, t!("history.menu.rebase"), None, a(CommitOp::RebaseInteractive));
    let m = menu_item_with_shortcut(m, t!("history.menu.patch"), None, a(CommitOp::FormatPatch));
    menu_item_with_shortcut(m, "Copy hash", Some("⌥⌘C"), a(CommitOp::CopyHash))
}

/// Chỉ số màu ổn định theo tên tác giả (cho avatar chữ cái).
fn author_hue(author: &str) -> usize {
    author.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize))
}

/// Nhãn ref gọn: (chữ, loại) — loại 0 = nhánh hiện tại, 1 = nhánh, 2 = remote, 3 = tag.
fn ref_badges(refs: &[String]) -> Vec<(String, u8)> {
    refs.iter()
        .map(|r| {
            if let Some(b) = r.strip_prefix("HEAD -> ") {
                (b.to_string(), 0)
            } else if let Some(t) = r.strip_prefix("tag: ") {
                (t.to_string(), 3)
            } else if r == "HEAD" {
                ("HEAD".to_string(), 0)
            } else if r.contains('/') {
                (r.clone(), 2)
            } else {
                (r.clone(), 1)
            }
        })
        .collect()
}

impl RepoView {
    pub(super) fn render_history(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let muted = t.muted_foreground;
        let count = self.commits.len();
        let show_graph = self.filter == LogFilter::None;
        let lanes = self
            .rows
            .iter()
            .map(|r| r.width)
            .max()
            .unwrap_or(1)
            .min(MAX_LANES);
        let now = now_unix();

        let header = h_flex()
            .p_1()
            .gap_2()
            .flex_none()
            .border_b_1()
            .border_color(t.border)
            .child(div().flex_1().child(Input::new(&self.search_input).small()))
            .when(self.filter != LogFilter::None, |d| {
                d.child(
                    Button::new("clear-filter")
                        .label(t!("history.clear_filter"))
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.search_input
                                .update(cx, |s, cx| s.set_value("", window, cx));
                            this.filter = LogFilter::None;
                            this.reload_history(cx);
                        })),
                )
            });

        let graph_offset = if show_graph { px(LANE_W * lanes as f32 + 8.) } else { px(0.) };
        let table_columns = h_flex()
            .h(px(24.))
            .px_2()
            .gap_2()
            .flex_none()
            .bg(t.secondary.opacity(0.75))
            .border_b_1()
            .border_color(t.border)
            .text_xs()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(muted)
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pl(graph_offset)
                    .child("Description"),
            )
            .child(
                div()
                    .w(px(140.))
                    .flex_none()
                    .child("Author"),
            )
            .child(
                div()
                    .w(px(68.))
                    .flex_none()
                    .text_center()
                    .child("Commit"),
            )
            .child(
                div()
                    .w(px(110.))
                    .flex_none()
                    .text_right()
                    .pr_2()
                    .child("Date"),
            );

        let body = if count == 0 {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(muted)
                .child(if self.history_loading {
                    t!("history.loading")
                } else if self.filter != LogFilter::None {
                    t!("history.empty.no_match")
                } else {
                    t!("history.empty.no_commits")
                })
                .into_any_element()
        } else {
            gpui::uniform_list(
                "commits",
                count,
                cx.processor(move |this, range: Range<usize>, _window, cx| {
                    if range.end + 200 > this.commits.len() {
                        this.load_more_history(cx);
                    }
                    range
                        .map(|i| this.render_commit_row(i, show_graph, lanes, now, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.commit_scroll.clone())
            .flex_1()
            .into_any_element()
        };

        v_flex().size_full().child(header).child(table_columns).child(body)
    }

    fn render_commit_row(
        &self,
        i: usize,
        show_graph: bool,
        lanes: usize,
        now: i64,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some(c) = self.commits.get(i) else {
            return div().into_any_element();
        };
        let t = cx.theme();
        let selected = self.selected_commit.as_deref() == Some(c.id.as_str());
        let id = c.id.clone();
        let menu_id = c.id.clone();
        let focus = self.focus.clone();
        let badges = ref_badges(&c.refs);
        let (muted, fg, bg) = (t.muted_foreground, t.foreground, t.background);
        let lane = self.rows.get(i).map_or(0, |r| r.color);

        let fg_color = if selected { gpui::white() } else { fg };
        let muted_color = if selected { gpui::white().opacity(0.85) } else { muted };
        let chip_bg = if selected { gpui::white().opacity(0.2) } else { t.secondary.opacity(0.6) };
        let cell_bg = if selected { t.list_active } else { bg };

        list_row(("commit", i), selected, cx)
            .gap_2()
            .pl_0()
            .when(show_graph, |d| {
                d.when_some(self.rows.get(i), |d, row| d.child(graph_cell(row, lanes, cell_bg)))
            })
            .children(badges.into_iter().take(4).map(|(label, kind)| {
                let color = lane_color(lane);
                let icon: Icon = match kind {
                    0 => IconName::Check.into(),
                    2 => IconName::GitHub.into(),
                    3 => IconName::Star.into(),
                    _ => BpIcon::Branch.into(),
                };
                h_flex()
                    .flex_none()
                    .gap_1()
                    .px_1()
                    .h(px(18.))
                    .rounded_sm()
                    .text_xs()
                    .border_1()
                    .border_color(if selected { gpui::white().opacity(0.4) } else { color.opacity(0.5) })
                    .bg(if selected { gpui::white().opacity(0.18) } else { color.opacity(0.14) })
                    .text_color(fg_color)
                    .when(kind == 0, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                    .child(Icon::new(icon).size_3())
                    .child(label)
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(fg_color)
                    .child(SharedString::from(c.summary.clone())),
            )
            .child(
                h_flex()
                    .w(px(140.))
                    .flex_none()
                    .gap_1()
                    .items_center()
                    .text_color(muted_color)
                    .child(
                        div()
                            .size(px(16.))
                            .flex_none()
                            .rounded_full()
                            .bg(lane_color(author_hue(&c.author)))
                            .text_color(gpui::white())
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(c.author.chars().next().map(|ch| ch.to_uppercase().to_string()).unwrap_or_default()),
                    )
                    .child(div().min_w_0().truncate().child(SharedString::from(c.author.clone()))),
            )
            .child(
                div()
                    .w(px(68.))
                    .flex_none()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .px_1()
                    .py(px(1.5))
                    .rounded_sm()
                    .bg(chip_bg)
                    .font_family(MONO)
                    .text_xs()
                    .text_center()
                    .text_color(muted_color)
                    .child(c.short_id().to_string()),
            )
            .child(
                div()
                    .w(px(110.))
                    .flex_none()
                    .text_right()
                    .pr_2()
                    .text_color(muted_color)
                    .child(crate::i18n::relative_time(c.time, now)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus);
                    this.select_commit(id.clone(), cx);
                    if ev.click_count >= 2 {
                        this.checkout_commit(id.clone(), window, cx);
                    }
                }),
            )
            .context_menu(move |menu, _, _| commit_menu(menu, &menu_id, &focus))
            .into_any_element()
    }

    /// Double-click a commit (like Fork): jump to the branch that points at it, or check the
    /// commit out (detached HEAD, after a confirmation) when no branch does.
    fn checkout_commit(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(commit) = self.commits.iter().find(|c| c.id == id) else { return };
        let badges = ref_badges(&commit.refs);
        let short = commit.short_id().to_string();
        let first_of = |kind: u8| badges.iter().find(|(_, k)| *k == kind).map(|(n, _)| n.clone());
        if let Some(branch) = first_of(0).filter(|_| self.current_branch().is_some()) {
            self.info = Some(t!("history.checkout.already_on", name = branch).into());
            cx.notify();
        } else if let Some(branch) = first_of(1) {
            let msg = t!("commands.done.checkout", name = branch);
            self.run_op(t!("commands.busy.checkout"), Some(&msg), cx, move |r| r.checkout(&branch));
        } else if let Some(remote_branch) = first_of(2) {
            self.on_ref_action(&OnRef { op: RefOp::Track, name: remote_branch }, window, cx);
        } else {
            let msg = t!("commands.done.checkout_detached", short = short);
            crate::ui::confirm_with_tone(
                window,
                cx,
                crate::ui::ModalTone::Primary,
                &t!("history.checkout.confirm_title", short = short),
                t!("history.checkout.confirm_detail"),
                &[t!("common.cancel"), t!("history.checkout.confirm_ok")],
                move |this, _, _, cx| {
                    this.run_op(t!("commands.busy.checkout"), Some(&msg), cx, move |r| r.checkout_detached(&id));
                },
            );
        }
    }

    /// Chọn commit: tải thông tin + danh sách file.
    pub(crate) fn select_commit(&mut self, id: String, cx: &mut Context<Self>) {
        if self.selected_commit.as_deref() == Some(id.as_str())
            && matches!(&self.detail, Detail::Commit(v) if v.id == id)
        {
            return;
        }
        self.selected_commit = Some(id.clone());
        self.detail = Detail::Commit(CommitView {
            id: id.clone(),
            detail: None,
            files: Vec::new(),
            selected: None,
            diff: Vec::new(),
        });
        cx.notify();
        let job_id = id.clone();
        self.spawn_git(
            None,
            cx,
            move |repo| {
                let detail = repo.commit_detail(&job_id)?;
                let files = repo.commit_files(&job_id, detail.parents.first().map(String::as_str))?;
                Ok((detail, files))
            },
            move |this, (detail, files), cx| {
                if let Detail::Commit(v) = &mut this.detail {
                    if v.id == id {
                        v.detail = Some(detail);
                        v.files = files;
                        if v.files.len() == 1 {
                            this.select_commit_file(0, cx);
                        }
                    }
                }
            },
        );
    }

    /// Chọn file trong commit đang xem: hiện diff.
    pub(crate) fn select_commit_file(&mut self, index: usize, cx: &mut Context<Self>) {
        let Detail::Commit(v) = &mut self.detail else { return };
        let Some(file) = v.files.get(index) else { return };
        v.selected = Some(index);
        v.diff.clear();
        let id = v.id.clone();
        let parent = v.detail.as_ref().and_then(|d| d.parents.first().cloned());
        let path = file.path.clone();
        let check_id = id.clone();
        cx.notify();
        self.spawn_git(
            None,
            cx,
            move |repo| repo.commit_diff(&id, parent.as_deref(), &path),
            move |this, diff, _| {
                if let Detail::Commit(v) = &mut this.detail {
                    if v.id == check_id && v.selected == Some(index) {
                        v.diff = diff;
                    }
                }
            },
        );
    }

    /// Cuộn tới commit (nếu đã tải) và chọn nó.
    pub(crate) fn reveal_commit(&mut self, oid: &str, cx: &mut Context<Self>) {
        if let Some(i) = self.commits.iter().position(|c| c.id == oid) {
            self.commit_scroll.scroll_to_item(i, ScrollStrategy::Center);
        }
        self.select_commit(oid.to_string(), cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badges() {
        let b = ref_badges(&["HEAD -> main".into(), "origin/main".into(), "tag: v1".into(), "dev".into()]);
        assert_eq!(b, vec![("main".into(), 0), ("origin/main".into(), 2), ("v1".into(), 3), ("dev".into(), 1)]);
    }
}
