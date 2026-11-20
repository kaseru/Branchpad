//! Cột giữa, phần trên: đồ thị nhánh + danh sách commit (tải dần, chỉ vẽ dòng đang hiện).

use std::ops::Range;

use branchpad_core::{
    git::{Anchor, GraphRow, LogFilter},
    now_unix, relative_time,
};
use gpui::{
    canvas, div, fill, point, prelude::*, px, size, AnyElement, Bounds, Context, FocusHandle,
    MouseButton, MouseDownEvent, PathBuilder, ScrollStrategy, SharedString, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    menu::{ContextMenuExt, PopupMenu},
    v_flex, ActiveTheme, Sizable,
};

use super::{CommitView, Detail, RepoView};
use crate::actions::{CommitOp, OnCommit};
use crate::ui::{lane_color, list_row, LANE_W, MONO};

/// Số làn tối đa vẽ ra (làn xa hơn bị cắt để danh sách không quá hẹp).
const MAX_LANES: usize = 16;

fn graph_cell(row: &GraphRow, lanes: usize) -> impl IntoElement {
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
                let mut b = PathBuilder::stroke(px(1.5));
                b.move_to(p0);
                if l.from_lane == l.to_lane {
                    b.line_to(p1);
                } else if l.from == Anchor::Center {
                    // Rẽ ra làn khác: cong sang ngang rồi xuống.
                    b.curve_to(p1, point(p1.x, p0.y));
                } else {
                    // Nhập vào chấm: xuống rồi cong vào.
                    b.curve_to(p1, point(p0.x, p1.y));
                }
                if let Ok(path) = b.build() {
                    window.paint_path(path, lane_color(l.color));
                }
            }
            if row.lane < MAX_LANES {
                let c = point(x(row.lane), y(Anchor::Center));
                let d = if row.is_merge { px(7.) } else { px(9.) };
                window.paint_quad(
                    fill(Bounds::centered_at(c, size(d, d)), lane_color(row.color))
                        .corner_radii(d / 2.),
                );
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
    menu.action_context(focus.clone())
        .menu("Checkout commit này", a(CommitOp::Checkout))
        .menu("Tạo nhánh tại đây…", a(CommitOp::CreateBranch))
        .menu("Tạo tag tại đây…", a(CommitOp::CreateTag))
        .separator()
        .menu("Cherry-pick vào nhánh hiện tại", a(CommitOp::CherryPick))
        .menu("Revert commit này", a(CommitOp::Revert))
        .separator()
        .menu("Reset nhánh hiện tại về đây (soft)", a(CommitOp::ResetSoft))
        .menu("Reset nhánh hiện tại về đây (mixed)", a(CommitOp::ResetMixed))
        .menu("Reset nhánh hiện tại về đây (hard)…", a(CommitOp::ResetHard))
        .separator()
        .menu("Rebase tương tác các commit sau commit này…", a(CommitOp::RebaseInteractive))
        .menu("Tạo file patch…", a(CommitOp::FormatPatch))
        .menu("Copy hash", a(CommitOp::CopyHash))
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
            .gap_1()
            .flex_none()
            .child(div().flex_1().child(Input::new(&self.search_input).small()))
            .when(self.filter != LogFilter::None, |d| {
                d.child(
                    Button::new("clear-filter")
                        .label("Bỏ lọc")
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

        let body = if count == 0 {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(muted)
                .child(if self.history_loading {
                    "Đang tải lịch sử…"
                } else if self.filter != LogFilter::None {
                    "Không có commit nào khớp"
                } else {
                    "Repo chưa có commit"
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

        v_flex().size_full().child(header).child(body)
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
        let (accent, muted, border) = (t.primary, t.muted_foreground, t.border);

        list_row(("commit", i), selected, cx)
            .gap_2()
            .pl_0()
            .when(show_graph, |d| {
                d.when_some(self.rows.get(i), |d, row| d.child(graph_cell(row, lanes)))
            })
            .children(badges.into_iter().take(4).map(|(label, kind)| {
                div()
                    .flex_none()
                    .px_1()
                    .rounded_sm()
                    .text_xs()
                    .border_1()
                    .border_color(border)
                    .when(kind == 0, |d| d.bg(accent).text_color(gpui::white()).border_color(accent))
                    .when(kind == 2, |d| d.text_color(muted))
                    .when(kind == 3, |d| d.italic())
                    .child(label)
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(SharedString::from(c.summary.clone())),
            )
            .child(
                div()
                    .w(px(140.))
                    .flex_none()
                    .truncate()
                    .text_color(muted)
                    .child(SharedString::from(c.author.clone())),
            )
            .child(
                div()
                    .w(px(100.))
                    .flex_none()
                    .text_color(muted)
                    .child(relative_time(c.time, now)),
            )
            .child(
                div()
                    .w(px(64.))
                    .flex_none()
                    .font_family(MONO)
                    .text_xs()
                    .text_color(muted)
                    .child(c.short_id().to_string()),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus);
                    this.select_commit(id.clone(), cx);
                }),
            )
            .context_menu(move |menu, _, _| commit_menu(menu, &menu_id, &focus))
            .into_any_element()
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
