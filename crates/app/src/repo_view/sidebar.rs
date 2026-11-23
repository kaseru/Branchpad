//! Cột trái: Files (cây file) · Thay đổi (+ ô commit) · Git (nhánh, remote, tag, stash, worktree).

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use branchpad_core::{
    git::{EntryKind, GitRef, RefKind, StatusEntry, WorkingSide},
    TreeRow,
};
use gpui::{
    div, hsla, prelude::*, px, AnyElement, Context, FocusHandle, Hsla, MouseButton,
    MouseDownEvent, SharedString, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    menu::{ContextMenuExt, PopupMenu},
    v_flex, ActiveTheme, Disableable, Icon, IconName, Sizable,
};

use crate::icons::{file_icon, BpIcon};
use super::{overlay::PromptKind, Detail, RepoView, SidebarTab};
use crate::actions::{FileOp, OnFile, OnRef, RefOp};
use crate::ui::{list_row, menu_item_with_shortcut, split_path, status_color, ROW_H};

fn file_menu(menu: PopupMenu, path: PathBuf, is_dir: bool, focus: &FocusHandle) -> PopupMenu {
    let a = |op: FileOp| -> Box<dyn gpui::Action> {
        Box::new(OnFile {
            op,
            path: path.clone(),
        })
    };
    let mut menu = menu.action_context(focus.clone());
    menu = menu_item_with_shortcut(menu, t!("sidebar.file_menu.new_file"), None, a(FileOp::NewFile));
    menu = menu_item_with_shortcut(menu, t!("sidebar.file_menu.new_folder"), None, a(FileOp::NewFolder));
    menu = menu.separator();
    menu = menu_item_with_shortcut(menu, t!("sidebar.menu.rename"), None, a(FileOp::Rename));
    menu = menu_item_with_shortcut(menu, t!("sidebar.file_menu.delete"), Some("⌘⌫"), a(FileOp::Delete));
    menu = menu.separator();
    menu = menu_item_with_shortcut(menu, t!("sidebar.file_menu.reveal"), Some("⌥⌘R"), a(FileOp::RevealInFinder));
    menu = menu_item_with_shortcut(menu, t!("sidebar.file_menu.copy_path"), Some("⌥⌘C"), a(FileOp::CopyPath));
    if is_dir {
        menu
    } else {
        menu = menu.separator();
        menu = menu_item_with_shortcut(menu, t!("sidebar.file_menu.open_editor"), Some("⌘O"), a(FileOp::OpenInEditor));
        menu = menu_item_with_shortcut(menu, t!("sidebar.file_menu.history"), None, a(FileOp::History));
        menu = menu_item_with_shortcut(menu, t!("sidebar.file_menu.blame"), None, a(FileOp::Blame));
        menu
    }
}

/// Right-click menu of a file in the Changes tab: stage / unstage / discard, then the usual file menu.
fn change_menu(menu: PopupMenu, path: PathBuf, staged: bool, unmerged: bool, focus: &FocusHandle) -> PopupMenu {
    let a = |op: FileOp| -> Box<dyn gpui::Action> { Box::new(OnFile { op, path: path.clone() }) };
    let mut menu = menu.action_context(focus.clone());
    if !unmerged {
        menu = if staged {
            menu_item_with_shortcut(menu, t!("sidebar.tip.unstage"), Some("Space"), a(FileOp::Unstage))
        } else {
            let m = menu_item_with_shortcut(menu, t!("sidebar.tip.stage"), Some("Space"), a(FileOp::Stage));
            menu_item_with_shortcut(m, t!("sidebar.tip.discard"), Some("⌘⌫"), a(FileOp::Discard))
        }
        .separator();
    }
    file_menu(menu, path.clone(), false, focus)
}

fn ref_menu(menu: PopupMenu, name: &str, items: &[(&'static str, RefOp)], focus: &FocusHandle) -> PopupMenu {
    let mut menu = menu.action_context(focus.clone());
    for (label, op) in items {
        if label.is_empty() {
            menu = menu.separator();
        } else {
            let shortcut = match op {
                RefOp::Checkout => Some("Space"),
                RefOp::Delete | RefOp::DeleteRemoteBranch | RefOp::TagDelete | RefOp::StashDrop | RefOp::WorktreeRemove => Some("⌘⌫"),
                _ => None,
            };
            menu = menu_item_with_shortcut(
                menu,
                crate::i18n::tr(label),
                shortcut,
                Box::new(OnRef {
                    op: *op,
                    name: name.to_string(),
                }),
            );
        }
    }
    menu
}

type ClickFn = Box<dyn Fn(&mut RepoView, &MouseDownEvent, &mut Window, &mut Context<RepoView>)>;

/// Gói closure bấm chuột (giúp Rust suy ra kiểu tham số).
fn click(f: impl Fn(&mut RepoView, &MouseDownEvent, &mut Window, &mut Context<RepoView>) + 'static) -> ClickFn {
    Box::new(f)
}

const LOCAL_BRANCH_MENU: &[(&str, RefOp)] = &[
    ("sidebar.menu.checkout", RefOp::Checkout),
    ("sidebar.menu.merge", RefOp::Merge),
    ("sidebar.menu.rebase", RefOp::Rebase),
    ("", RefOp::Checkout),
    ("sidebar.menu.push", RefOp::Push),
    ("sidebar.menu.force_push", RefOp::ForcePush),
    ("", RefOp::Checkout),
    ("sidebar.menu.rename", RefOp::Rename),
    ("sidebar.menu.delete", RefOp::Delete),
];

const REMOTE_BRANCH_MENU: &[(&str, RefOp)] = &[
    ("sidebar.menu.track", RefOp::Track),
    ("sidebar.menu.merge", RefOp::Merge),
    ("sidebar.menu.rebase", RefOp::Rebase),
    ("", RefOp::Checkout),
    ("sidebar.menu.delete_remote_branch", RefOp::DeleteRemoteBranch),
];

const REMOTE_MENU: &[(&str, RefOp)] = &[
    ("sidebar.menu.remote_fetch", RefOp::RemoteFetch),
    ("sidebar.menu.remote_remove", RefOp::RemoteRemove),
];

const TAG_MENU: &[(&str, RefOp)] = &[
    ("sidebar.menu.checkout", RefOp::Checkout),
    ("sidebar.menu.tag_push", RefOp::TagPush),
    ("sidebar.menu.delete", RefOp::TagDelete),
];

const STASH_MENU: &[(&str, RefOp)] = &[
    ("sidebar.menu.stash_apply", RefOp::StashApply),
    ("sidebar.menu.stash_pop", RefOp::StashPop),
    ("sidebar.menu.delete", RefOp::StashDrop),
];

const WORKTREE_MENU: &[(&str, RefOp)] = &[
    ("sidebar.menu.worktree_open", RefOp::WorktreeOpen),
    ("sidebar.menu.worktree_remove", RefOp::WorktreeRemove),
];

impl RepoView {
    pub(super) fn render_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let changes = self.status.entries.iter().filter(|e| e.kind != EntryKind::Ignored).count();
        let repo_name = self.repo.name().to_string();

        let nav_item = |id: &'static str, icon: Icon, label: SharedString, count: usize, is_active: bool| {
            let text_color = if is_active { gpui::white() } else { t.foreground };
            let icon_color = if is_active { gpui::white() } else { t.muted_foreground };
            h_flex()
                .id(id)
                .h(ROW_H + px(2.))
                .px_2()
                .gap_2()
                .w_full()
                .rounded_sm()
                .items_center()
                .cursor_pointer()
                .when(is_active, |d| {
                    d.bg(t.list_active)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .when(!is_active, |d| d.hover(|s| s.bg(t.list_hover)))
                .child(Icon::new(icon).size_4().text_color(icon_color))
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .truncate()
                        .text_color(text_color)
                        .child(label),
                )
                .when(count > 0, |d| {
                    let badge_bg = if is_active { gpui::white().opacity(0.25) } else { t.primary.opacity(0.18) };
                    let badge_text = if is_active { gpui::white() } else { t.primary };
                    d.child(
                        div()
                            .px(px(6.))
                            .py(px(1.5))
                            .rounded_full()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .bg(badge_bg)
                            .text_color(badge_text)
                            .child(count.to_string()),
                    )
                })
        };

        // Header repo macOS: [Icon] Repo Name
        let repo_header = h_flex()
            .h(px(38.))
            .px_3()
            .gap_2()
            .flex_none()
            .items_center()
            .border_b_1()
            .border_color(t.border)
            .child(
                div()
                    .size(px(20.))
                    .rounded_sm()
                    .bg(t.primary.opacity(0.15))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(t.primary)
                    .child(Icon::new(BpIcon::Branch).size_3()),
            )
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(t.foreground)
                    .truncate()
                    .child(repo_name),
            );

        // Khối 3 mục điều hướng chính chuẩn Fork / macOS Source List
        let nav_section = v_flex()
            .px_2()
            .py(px(4.))
            .gap(px(1.))
            .flex_none()
            .border_b_1()
            .border_color(t.border)
            .child(
                nav_item(
                    "sb-changes",
                    BpIcon::Patch.into(),
                    t!("sidebar.tab.changes").into(),
                    changes,
                    self.sidebar == SidebarTab::Changes,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sidebar = SidebarTab::Changes;
                    cx.notify();
                })),
            )
            .child(
                nav_item(
                    "sb-git",
                    BpIcon::Branch.into(),
                    "All Commits".into(),
                    0,
                    self.sidebar == SidebarTab::Git,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sidebar = SidebarTab::Git;
                    cx.notify();
                })),
            )
            .child(
                nav_item(
                    "sb-files",
                    IconName::FolderClosed.into(),
                    t!("sidebar.tab.files").into(),
                    0,
                    self.sidebar == SidebarTab::Files,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sidebar = SidebarTab::Files;
                    cx.notify();
                })),
            );

        let body = match self.sidebar {
            SidebarTab::Files => self.render_files(cx),
            SidebarTab::Changes => self.render_changes(window, cx),
            SidebarTab::Git => self.render_git(cx),
        };

        v_flex()
            .size_full()
            .child(repo_header)
            .child(nav_section)
            .child(div().flex_1().overflow_hidden().child(body))
    }

    // ---- Files ----

    fn render_files(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(tree) = self.tree.as_ref() else {
            return div()
                .p_2()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(t!("sidebar.files.unreadable"))
                .into_any_element();
        };
        let rows: Rc<Vec<TreeRow>> = Rc::new(tree.rows());
        let count = rows.len();
        let root = self.repo.workdir().to_path_buf();
        let (border, muted, hover) = {
            let t = cx.theme();
            (t.border, t.muted_foreground, t.list_hover)
        };
        v_flex()
            .size_full()
            .child(
                div().p_1().flex_none().child(
                    h_flex()
                        .id("files-search")
                        .h(px(26.))
                        .px_2()
                        .gap_2()
                        .items_center()
                        .rounded_md()
                        .border_1()
                        .border_color(border)
                        .cursor_pointer()
                        .hover(|s| s.bg(hover))
                        .text_sm()
                        .text_color(muted)
                        .child(Icon::new(IconName::Search).size_4())
                        .child(div().flex_1().child(t!("sidebar.files.search")))
                        .child(div().text_xs().child("⌘P"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.on_quick_open(&crate::actions::QuickOpen, window, cx)
                        })),
                ),
            )
            .child(
                h_flex()
                    .px_1()
                    .gap_1()
                    .flex_none()
                    .child(
                        Button::new("files-new-file").icon(IconName::Plus).label("File").xsmall().ghost().on_click({
                            let root = root.clone();
                            cx.listener(move |this, _, window, cx| {
                                this.open_prompt(PromptKind::NewFile { dir: root.clone() }, t!("sidebar.prompt.new_file"), "", window, cx)
                            })
                        }),
                    )
                    .child(
                        Button::new("files-new-dir").icon(IconName::Plus).label(t!("sidebar.files.folder")).xsmall().ghost().on_click({
                            let root = root.clone();
                            cx.listener(move |this, _, window, cx| {
                                this.open_prompt(PromptKind::NewFolder { dir: root.clone() }, t!("sidebar.prompt.new_folder"), "", window, cx)
                            })
                        }),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("files-toggle-center")
                            .icon(if self.force_center { IconName::PanelLeftClose } else { IconName::PanelLeftOpen })
                            .xsmall()
                            .ghost()
                            .tooltip(if self.force_center { t!("sidebar.files.hide_history") } else { t!("sidebar.files.show_history") })
                            .on_click(cx.listener(|this, _, window, cx| this.on_toggle_center(&crate::actions::ToggleCenter, window, cx))),
                    ),
            )
            .child(
                gpui::uniform_list(
                    "file-tree",
                    count,
                    cx.processor(move |this, range: Range<usize>, _window, cx| {
                        range
                            .filter_map(|i| rows.get(i).map(|r| this.render_tree_row(i, r, cx)))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.tree_scroll.clone())
                .flex_1(),
            )
            .into_any_element()
    }

    fn render_tree_row(&self, i: usize, row: &TreeRow, cx: &Context<Self>) -> AnyElement {
        let t = cx.theme();
        let e = &row.entry;
        let selected = self.selected_path.as_deref() == Some(e.path.as_path());
        let mark = if e.is_dir {
            self.status
                .dir_has_changes(&format!("{}/", e.rel))
                .then_some('M')
        } else {
            self.status.mark_of(&e.rel)
        };
        let color = mark.map(|m| status_color(m, cx)).unwrap_or(t.foreground);
        let path = e.path.clone();
        let menu_path = e.path.clone();
        let rel = e.rel.clone();
        let is_dir = e.is_dir;
        let focus = self.focus.clone();
        let (tree_icon, icon_color) = file_icon(&e.name, is_dir, row.expanded);
        list_row(("tree", i), selected, cx)
            .pl(px(10. + 16. * row.depth as f32))
            .gap_1()
            .child(
                div()
                    .w(px(14.))
                    .flex_none()
                    .text_xs()
                    .text_color(t.muted_foreground)
                    .child(if !is_dir { "" } else if row.expanded { "▾" } else { "▸" }),
            )
            .child(Icon::new(tree_icon).size_4().text_color(icon_color))
            .child(div().flex_1().truncate().text_color(color).child(SharedString::from(e.name.clone())))
            .when_some(mark.filter(|_| !is_dir), |d, m| {
                d.child(
                    div()
                        .size(px(16.))
                        .rounded_sm()
                        .bg(color.opacity(0.15))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(color)
                        .child(m.to_string()),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus);
                    if is_dir {
                        if let Some(tree) = this.tree.as_mut() {
                            if let Err(e) = tree.toggle(&this.repo, &path) {
                                this.error = Some(t!("sidebar.error.open_folder", e = e));
                            }
                        }
                        cx.notify();
                        return;
                    }
                    this.selected_path = Some(path.clone());
                    // With the middle column hidden (Files tab) there is no diff view: open at once.
                    if ev.click_count >= 2 {
                        this.open_in_editor(path.clone(), None, window, cx);
                    } else if !this.center_visible() {
                        this.open_in_editor_preview(path.clone(), window, cx);
                    } else {
                        this.show_file_changes(&rel, cx);
                    }
                    cx.notify();
                }),
            )
            .context_menu(move |menu, _, _| file_menu(menu, menu_path.clone(), is_dir, &focus))
            .into_any_element()
    }

    /// Bấm 1 lần vào file: hiện diff của nó (xung đột thì mở bộ xử lý xung đột).
    pub(crate) fn show_file_changes(&mut self, rel: &str, cx: &mut Context<Self>) {
        let entry = self.status.entries.iter().find(|e| e.path == rel).cloned();
        match entry {
            Some(e) if e.kind == EntryKind::Unmerged => self.show_conflict(e.path, cx),
            Some(e) if e.kind == EntryKind::Untracked => {
                self.show_working_diff(e.path, WorkingSide::Untracked, cx)
            }
            Some(e) if e.is_unstaged() => self.show_working_diff(e.path, WorkingSide::Unstaged, cx),
            Some(e) if e.is_staged() => self.show_working_diff(e.path, WorkingSide::Staged, cx),
            _ => {
                self.detail = Detail::Working {
                    path: rel.to_string(),
                    side: WorkingSide::Unstaged,
                    diff: Vec::new(),
                };
                cx.notify();
            }
        }
    }

    // ---- Thay đổi ----

    pub(crate) fn stage_paths(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
        self.run_op(t!("sidebar.op.staging"), None, cx, move |r| {
            let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
            r.stage(&refs)
        });
    }

    pub(crate) fn unstage_paths(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
        let head = self.status.head_oid.is_some();
        self.run_op(t!("sidebar.op.unstaging"), None, cx, move |r| {
            let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
            r.unstage(&refs, head)
        });
    }

    pub(crate) fn discard_paths(&mut self, paths: Vec<String>, untracked: bool, window: &mut Window, cx: &mut Context<Self>) {
        let what = if paths.len() == 1 { paths[0].clone() } else { t!("sidebar.discard.n_files", n = paths.len()) };
        let detail = if untracked {
            t!("sidebar.discard.detail_untracked")
        } else {
            t!("sidebar.discard.detail_unstaged")
        };
        confirm_then(window, cx, &t!("sidebar.discard.title", what = what), detail, t!("sidebar.discard.confirm"), move |this, cx| {
            this.run_op(t!("sidebar.op.discarding"), None, cx, move |r| {
                let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
                r.discard(&refs, untracked)
            });
        });
    }

    fn change_row(&self, section: &'static str, i: usize, e: &StatusEntry, side: Option<WorkingSide>, cx: &Context<Self>) -> AnyElement {
        let t = cx.theme();
        let mark = match (e.kind, side) {
            (EntryKind::Unmerged, _) => 'U',
            (EntryKind::Untracked, _) => '?',
            (_, Some(WorkingSide::Staged)) => e.index,
            _ => e.worktree,
        };
        let (name, dir) = split_path(&e.path);
        let (change_icon, change_color) = file_icon(&name, false, false);
        let staged_pane = side == Some(WorkingSide::Staged);
        let multi = self.change_sel.contains(&e.path) && self.change_sel_staged == staged_pane;
        let selected = multi
            || (self.change_sel.is_empty()
                && (matches!(&self.detail, Detail::Working { path, side: s, .. } if *path == e.path && Some(*s) == side)
                    || matches!(&self.detail, Detail::Conflict(v) if v.path == e.path)));
        let path = e.path.clone();
        let abs = self.repo.workdir().join(&e.path);
        let kind = e.kind;
        let focus = self.focus.clone();
        let menu_path = abs.clone();
        let p1 = path.clone();
        let p2 = path.clone();
        list_row((section, i), selected, cx)
            .child(div().w(px(12.)).flex_none().text_xs().text_color(status_color(mark, cx)).child(mark.to_string()))
            .child(Icon::new(change_icon).size_4().text_color(change_color))
            .child(div().flex_none().child(name))
            .child(div().flex_1().truncate().text_xs().text_color(t.muted_foreground).child(dir))
            .when(kind != EntryKind::Unmerged, |d| match side {
                Some(WorkingSide::Staged) => d.child(
                    Button::new((section, i)).icon(IconName::Minus).xsmall().ghost().tooltip(t!("sidebar.tip.unstage"))
                        .on_click(cx.listener(move |this, _, _, cx| this.unstage_paths(vec![p1.clone()], cx))),
                ),
                _ => d
                    .child(
                        Button::new(("discard", i)).icon(BpIcon::Undo).xsmall().ghost().tooltip(t!("sidebar.tip.discard"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.discard_paths(vec![p2.clone()], kind == EntryKind::Untracked, window, cx)
                            })),
                    )
                    .child(
                        Button::new((section, i)).icon(IconName::Plus).xsmall().ghost().tooltip(t!("sidebar.tip.stage"))
                            .on_click(cx.listener(move |this, _, _, cx| this.stage_paths(vec![p1.clone()], cx))),
                    ),
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus);
                    if ev.modifiers.secondary() {
                        this.select_change(&path, staged_pane, super::keys::SelectMode::Toggle, cx);
                        return;
                    }
                    if ev.modifiers.shift {
                        this.select_change(&path, staged_pane, super::keys::SelectMode::Range, cx);
                        return;
                    }
                    this.select_change(&path, staged_pane, super::keys::SelectMode::Replace, cx);
                    if ev.click_count >= 2 {
                        this.open_in_editor(abs.clone(), None, window, cx);
                    } else if kind == EntryKind::Unmerged {
                        this.show_conflict(path.clone(), cx);
                    } else if let Some(side) = side {
                        this.show_working_diff(path.clone(), side, cx);
                    }
                }),
            )
            .context_menu(move |menu, _, _| change_menu(menu, menu_path.clone(), staged_pane, kind == EntryKind::Unmerged, &focus))
            .into_any_element()
    }

    fn render_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let _ = window;
        let t = cx.theme();
        let conflicted: Vec<StatusEntry> = self.status.conflicted().cloned().collect();
        let staged: Vec<StatusEntry> = self.status.staged().cloned().collect();
        let unstaged: Vec<StatusEntry> = self.status.unstaged().cloned().collect();
        let untracked: Vec<StatusEntry> = self.status.untracked().cloned().collect();

        let (secondary, border, muted) = (t.secondary, t.border, t.muted_foreground);
        let section_head = |title: String, button: Option<Button>| {
            h_flex()
                .flex_none()
                .h(ROW_H + px(6.))
                .px_2()
                .gap_1()
                .bg(secondary.opacity(0.5))
                .border_y_1()
                .border_color(border)
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(muted)
                .child(div().flex_1().child(title))
                .children(button)
        };
        let hint = |text: &'static str| div().p_3().text_sm().text_color(muted).child(text);
        let pane = |id: &'static str| v_flex().id(id).flex_1().min_h_0().overflow_hidden();
        let staged_paths: Vec<String> = staged.iter().map(|e| e.path.clone()).collect();
        let unstaged_paths: Vec<String> = unstaged
            .iter()
            .chain(untracked.iter())
            .map(|e| e.path.clone())
            .collect();
        let unstaged_count = unstaged.len() + untracked.len();

        // Two fixed panes (like Fork): files to stage on top, staged files below.
        let unstaged_pane = pane("changes-unstaged-pane")
            .child(section_head(
                t!("sidebar.changes.unstaged", n = unstaged_count),
                Some({
                    let paths = unstaged_paths.clone();
                    Button::new("stage-all").icon(IconName::Plus).label(t!("sidebar.changes.stage_all")).xsmall().ghost()
                        .disabled(unstaged_count == 0)
                        .on_click(cx.listener(move |this, _, _, cx| this.stage_paths(paths.clone(), cx)))
                }),
            ))
            .child(
                v_flex()
                    .id("changes-unstaged-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .when(!conflicted.is_empty(), |d| {
                        d.child(section_head(t!("sidebar.changes.conflicts", n = conflicted.len()), None))
                            .children(conflicted.iter().enumerate().map(|(i, e)| self.change_row("ch-conflict", i, e, None, cx)))
                    })
                    .when(unstaged_count == 0 && conflicted.is_empty(), |d| d.child(hint(t!("sidebar.changes.unstaged_empty"))))
                    .children(unstaged.iter().enumerate().map(|(i, e)| self.change_row("ch-unstaged", i, e, Some(WorkingSide::Unstaged), cx)))
                    .children(untracked.iter().enumerate().map(|(i, e)| self.change_row("ch-untracked", i, e, Some(WorkingSide::Untracked), cx))),
            );
        let staged_pane = pane("changes-staged-pane")
            .child(section_head(
                t!("sidebar.changes.staged", n = staged.len()),
                Some({
                    let paths = staged_paths.clone();
                    Button::new("unstage-all").icon(IconName::Minus).label(t!("sidebar.changes.unstage_all")).xsmall().ghost()
                        .disabled(staged.is_empty())
                        .on_click(cx.listener(move |this, _, _, cx| this.unstage_paths(paths.clone(), cx)))
                }),
            ))
            .child(
                v_flex()
                    .id("changes-staged-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .when(staged.is_empty(), |d| d.child(hint(t!("sidebar.changes.staged_empty"))))
                    .children(staged.iter().enumerate().map(|(i, e)| self.change_row("ch-staged", i, e, Some(WorkingSide::Staged), cx))),
            );
        let list = v_flex().flex_1().min_h_0().child(unstaged_pane).child(staged_pane);

        let busy = self.busy.is_some();
        let commit_box = v_flex()
            .p_2()
            .gap_2()
            .flex_none()
            .border_t_1()
            .border_color(t.border)
            .child(div().h(px(96.)).child(Input::new(&self.commit_input).h_full()))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("amend")
                            .label(if self.amend { t!("sidebar.commit.amend_on") } else { t!("sidebar.commit.amend") })
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_amend(window, cx))),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("commit")
                            .label(if self.amend { t!("sidebar.commit.amend_button") } else { t!("sidebar.commit.commit") })
                            .small()
                            .primary()
                            .disabled(busy || (staged.is_empty() && !self.amend))
                            .tooltip("Cmd+Enter")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.commit_changes(&crate::actions::CommitChanges, window, cx)
                            })),
                    ),
            );

        v_flex().size_full().child(list).child(commit_box).into_any_element()
    }

    // ---- Git: nhánh, remote, tag, stash, worktree ----

    fn add_button(&self, id: &'static str, tooltip: &'static str, cx: &Context<Self>, f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static) -> Button {
        Button::new(id)
            .icon(IconName::Plus)
            .xsmall()
            .ghost()
            .tooltip(tooltip)
            .on_click(cx.listener(move |this, _, window, cx| f(this, window, cx)))
    }

    /// Mở các thư mục cha chứa một nhánh (ví dụ: "feat/foo/bar" -> mở "local:feat", "local:feat/foo").
    pub(super) fn ensure_branch_folder_open(&mut self, branch: &str, prefix: &str) {
        let mut path = String::new();
        let parts: Vec<&str> = branch.split('/').collect();
        if parts.len() > 1 {
            for part in &parts[..parts.len() - 1] {
                if !path.is_empty() {
                    path.push('/');
                }
                path.push_str(part);
                let key = format!("{prefix}{path}");
                self.ref_closed.remove(&key);
                self.ref_open.insert(key);
            }
        }
    }

    /// Mở/đóng một nhóm hoặc thư mục trong cột Git.
    fn toggle_ref_node(&mut self, key: String, default_open: bool, cx: &mut Context<Self>) {
        let is_currently_open = if self.ref_closed.contains(&key) {
            false
        } else if self.ref_open.contains(&key) {
            true
        } else {
            default_open
        };

        if is_currently_open {
            self.ref_open.remove(&key);
            self.ref_closed.insert(key);
        } else {
            self.ref_closed.remove(&key);
            self.ref_open.insert(key);
        }
        cx.notify();
    }

    fn render_git(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme();
        let (muted, fg, accent) = (t.muted_foreground, t.foreground, t.primary);
        let focus = self.focus.clone();
        let head_branch = self.status.branch.clone();
        if let Some(h) = head_branch.as_deref() {
            self.ensure_branch_folder_open(h, "local:");
        }
        let ref_open = self.ref_open.clone();
        let ref_closed = self.ref_closed.clone();
        let is_open = move |key: &str, default_open: bool| {
            if ref_closed.contains(key) {
                false
            } else if ref_open.contains(key) {
                true
            } else {
                default_open
            }
        };

        // Tiêu đề nhóm (Nhánh, Remote, …): bấm để thu gọn.
        let section = |key: &'static str, title: &'static str, default_open: bool, button: Option<Button>| {
            let open = is_open(key, default_open);
            h_flex()
                .id(SharedString::from(format!("sec-{key}")))
                .flex_none()
                .h(ROW_H + px(4.))
                .px_2()
                .gap_1()
                .mt_1()
                .child(
                    Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight })
                        .size_3()
                        .text_color(muted),
                )
                .child(div().flex_1().font_weight(gpui::FontWeight::SEMIBOLD).child(title))
                .children(button)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| this.toggle_ref_node(key.to_string(), default_open, cx)),
                )
                .into_any_element()
        };

        // Một dòng trong cây: [mũi tên] [icon] tên … phụ.
        let node = |id: SharedString,
                    depth: usize,
                    chevron: Option<bool>,
                    icon: Icon,
                    icon_color: Hsla,
                    label: String,
                    bold: bool,
                    selected: bool,
                    extra: Option<String>,
                    on_click: ClickFn,
                    menu: Option<(String, &'static [(&'static str, RefOp)])>| {
            let focus = focus.clone();
            let row = list_row(id, selected, cx)
                .pl(px(8. + 14. * depth as f32))
                .gap_1()
                .child(div().w(px(12.)).flex_none().when_some(chevron, |d, open| {
                    d.child(
                        Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight })
                            .size_3()
                            .text_color(muted),
                    )
                }))
                .child(Icon::new(icon).size_4().text_color(icon_color))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .when(bold, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                        .child(label),
                )
                .when_some(extra, |d, x| d.child(div().text_xs().text_color(muted).child(x)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        window.focus(&this.focus);
                        on_click(this, ev, window, cx)
                    }),
                )
            ;
            match menu {
                Some((name, items)) => row
                    .context_menu(move |m, _, _| ref_menu(m, &name, items, &focus))
                    .into_any_element(),
                None => row.into_any_element(),
            }
        };
        let folder_icon = |open: bool| -> Icon {
            if open { IconName::FolderOpen.into() } else { IconName::FolderClosed.into() }
        };
        let folder_color = hsla(0.58, 0.5, 0.62, 1.0);
        let toggle_click = |key: String, default_open: bool| {
            click(move |this, _, _, cx| this.toggle_ref_node(key.clone(), default_open, cx))
        };

        let mut items: Vec<AnyElement> = Vec::new();

        // ---- Nhánh local ----
        items.push(section(
            "branches",
            t!("sidebar.section.branches"),
            true,
            Some(self.add_button("add-branch", t!("sidebar.tip.new_branch"), cx, |this, window, cx| {
                this.open_prompt(PromptKind::CreateBranch { start: "HEAD".into() }, t!("sidebar.prompt.new_branch"), "", window, cx)
            })),
        ));
        if is_open("branches", true) {
            let locals: Vec<&GitRef> = self.refs.iter().filter(|r| r.kind == RefKind::Local).collect();
            let names: Vec<String> = locals.iter().map(|r| r.name.clone()).collect();
            for row in branch_rows(&names, "local:", head_branch.as_deref(), &is_open) {
                match row {
                    BranchRow::Folder { key, name, depth, open } => items.push(node(
                        SharedString::from(format!("lf-{key}")),
                        depth,
                        Some(open),
                        folder_icon(open),
                        folder_color,
                        name,
                        false,
                        false,
                        None,
                        toggle_click(key, open),
                        None,
                    )),
                    BranchRow::Leaf { idx, label, depth } => {
                        let r = locals[idx];
                        let is_head = head_branch.as_deref() == Some(r.name.as_str());
                        let mut extra = Vec::new();
                        if r.ahead > 0 {
                            extra.push(format!("{}↑", r.ahead));
                        }
                        if r.behind > 0 {
                            extra.push(format!("{}↓", r.behind));
                        }
                        if r.gone {
                            extra.push(t!("sidebar.branch.gone").into());
                        }
                        let (oid, name) = (r.oid.clone(), r.name.clone());
                        items.push(node(
                            SharedString::from(format!("local-{idx}")),
                            depth,
                            None,
                            if is_head { IconName::Check.into() } else { BpIcon::Branch.into() },
                            if is_head { fg } else { muted },
                            label,
                            is_head,
                            is_head,
                            Some(extra.join(" ")).filter(|x| !x.is_empty()),
                            click(move |this, ev, _, cx| {
                                if ev.click_count >= 2 {
                                    let n = name.clone();
                                    this.ensure_branch_folder_open(&n, "local:");
                                    this.run_op(t!("sidebar.op.checkout"), None, cx, move |r| r.checkout(&n));
                                } else {
                                    this.reveal_commit(&oid, cx);
                                }
                            }),
                            Some((r.name.clone(), LOCAL_BRANCH_MENU)),
                        ));
                    }
                }
            }
        }

        // ---- Remote ----
        items.push(section(
            "remotes",
            t!("sidebar.section.remotes"),
            false,
            Some(self.add_button("add-remote", t!("sidebar.tip.add_remote"), cx, |this, window, cx| {
                this.open_prompt(PromptKind::AddRemote, t!("sidebar.prompt.add_remote"), "", window, cx)
            })),
        ));
        if is_open("remotes", false) {
            for (ri, remote) in self.remotes.iter().enumerate() {
                let rkey = format!("remote:{}", remote.name);
                let open = is_open(&rkey, true);
                items.push(node(
                    SharedString::from(format!("remote-{ri}")),
                    0,
                    Some(open),
                    IconName::GitHub.into(),
                    muted,
                    remote.name.clone(),
                    false,
                    false,
                    None,
                    toggle_click(rkey.clone(), true),
                    Some((remote.name.clone(), REMOTE_MENU)),
                ));
                if !open {
                    continue;
                }
                let prefix = format!("{}/", remote.name);
                let rbs: Vec<&GitRef> = self
                    .refs
                    .iter()
                    .filter(|r| r.kind == RefKind::Remote && r.name.starts_with(&prefix))
                    .collect();
                let names: Vec<String> = rbs.iter().map(|r| r.name[prefix.len()..].to_string()).collect();
                for row in branch_rows(&names, &format!("{rkey}/"), None, &is_open) {
                    match row {
                        BranchRow::Folder { key, name, depth, open } => items.push(node(
                            SharedString::from(format!("rf-{key}")),
                            depth + 1,
                            Some(open),
                            folder_icon(open),
                            folder_color,
                            name,
                            false,
                            false,
                            None,
                            toggle_click(key, open),
                            None,
                        )),
                        BranchRow::Leaf { idx, label, depth } => {
                            let r = rbs[idx];
                            let (oid, name) = (r.oid.clone(), r.name.clone());
                            items.push(node(
                                SharedString::from(format!("remote-branch-{ri}-{idx}")),
                                depth + 1,
                                None,
                                BpIcon::Branch.into(),
                                muted,
                                label,
                                false,
                                false,
                                None,
                                click(move |this, ev, window, cx| {
                                    if ev.click_count >= 2 {
                                        this.on_ref_action(&OnRef { op: RefOp::Track, name: name.clone() }, window, cx);
                                    } else {
                                        this.reveal_commit(&oid, cx);
                                    }
                                }),
                                Some((r.name.clone(), REMOTE_BRANCH_MENU)),
                            ));
                        }
                    }
                }
            }
        }

        // ---- Tag ----
        items.push(section(
            "tags",
            t!("sidebar.section.tags"),
            false,
            Some(self.add_button("add-tag", t!("sidebar.tip.new_tag"), cx, |this, window, cx| {
                this.open_prompt(PromptKind::CreateTag { rev: "HEAD".into() }, t!("sidebar.prompt.new_tag"), "", window, cx)
            })),
        ));
        if is_open("tags", false) {
            for (i, r) in self.refs.iter().filter(|r| r.kind == RefKind::Tag).enumerate() {
                let oid = r.oid.clone();
                items.push(node(
                    SharedString::from(format!("tag-{i}")),
                    0,
                    None,
                    IconName::Star.into(),
                    muted,
                    r.name.clone(),
                    false,
                    false,
                    None,
                    click(move |this, _, _, cx| this.reveal_commit(&oid, cx)),
                    Some((r.name.clone(), TAG_MENU)),
                ));
            }
        }

        // ---- Stash ----
        items.push(section(
            "stashes",
            t!("sidebar.section.stashes"),
            true,
            Some(self.add_button("add-stash", t!("sidebar.tip.stash"), cx, |this, window, cx| this.prompt_stash(window, cx))),
        ));
        if is_open("stashes", true) {
            for (i, s) in self.stashes.iter().enumerate() {
                let name = s.name.clone();
                items.push(node(
                    SharedString::from(format!("stash-{i}")),
                    0,
                    None,
                    BpIcon::Stash.into(),
                    muted,
                    s.message.clone(),
                    false,
                    false,
                    Some(s.name.clone()),
                    click(move |this, _, _, cx| this.show_stash(name.clone(), cx)),
                    Some((s.name.clone(), STASH_MENU)),
                ));
            }
        }

        // ---- Worktree ----
        items.push(section(
            "worktrees",
            t!("sidebar.section.worktrees"),
            true,
            Some(self.add_button("add-worktree", t!("sidebar.tip.add_worktree"), cx, |this, window, cx| {
                this.open_prompt(PromptKind::AddWorktree, t!("sidebar.prompt.add_worktree"), "", window, cx)
            })),
        ));
        if is_open("worktrees", true) {
            for (i, w) in self.worktrees.iter().enumerate() {
                let path = w.path.to_string_lossy().into_owned();
                let open_path = w.path.clone();
                let label = w
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                let extra = match (&w.branch, w.is_main) {
                    (Some(b), true) => t!("sidebar.worktree.main", b = b),
                    (Some(b), false) => b.clone(),
                    (None, _) => t!("sidebar.worktree.detached").into(),
                };
                let current = w.path == self.repo.workdir();
                items.push(node(
                    SharedString::from(format!("worktree-{i}")),
                    0,
                    None,
                    IconName::FolderClosed.into(),
                    accent,
                    label,
                    current,
                    false,
                    Some(extra),
                    click(move |_, ev, _, cx| {
                        if ev.click_count >= 2 {
                            cx.emit(super::RepoViewEvent::OpenRepository(open_path.clone()));
                        }
                    }),
                    Some((path, WORKTREE_MENU)),
                ));
            }
        }

        v_flex()
            .id("git-refs")
            .size_full()
            .overflow_y_scroll()
            .children(items)
            .into_any_element()
    }
}

/// Một dòng của cây nhánh (nhóm theo dấu `/` trong tên).
enum BranchRow {
    Folder { key: String, name: String, depth: usize, open: bool },
    Leaf { idx: usize, label: String, depth: usize },
}

#[derive(Default)]
struct BranchNode {
    dirs: BTreeMap<String, BranchNode>,
    leaves: Vec<(String, usize)>,
}

/// Dựng cây từ tên nhánh: nhánh lẻ trước, thư mục sau; thư mục chứa nhánh `head` mở sẵn.
/// `is_open(key, mặc_định)` cho biết thư mục đang mở hay đóng.
fn branch_rows(
    names: &[String],
    prefix: &str,
    head: Option<&str>,
    is_open: &dyn Fn(&str, bool) -> bool,
) -> Vec<BranchRow> {
    let mut root = BranchNode::default();
    for (idx, name) in names.iter().enumerate() {
        let mut segs: Vec<&str> = name.split('/').collect();
        let leaf = segs.pop().unwrap_or(name);
        let mut node = &mut root;
        for s in segs {
            node = node.dirs.entry(s.to_string()).or_default();
        }
        node.leaves.push((leaf.to_string(), idx));
    }

    fn walk(
        node: &BranchNode,
        path: &str,
        depth: usize,
        prefix: &str,
        head: Option<&str>,
        is_open: &dyn Fn(&str, bool) -> bool,
        out: &mut Vec<BranchRow>,
    ) {
        let mut leaves = node.leaves.clone();
        leaves.sort();
        out.extend(leaves.into_iter().map(|(label, idx)| BranchRow::Leaf { idx, label, depth }));
        for (name, child) in &node.dirs {
            let child_path = if path.is_empty() { name.clone() } else { format!("{path}/{name}") };
            let key = format!("{prefix}{child_path}");
            let default_open = head.is_some_and(|h| h.starts_with(&format!("{child_path}/")));
            let open = is_open(&key, default_open);
            out.push(BranchRow::Folder { key, name: name.clone(), depth, open });
            if open {
                walk(child, &child_path, depth + 1, prefix, head, is_open, out);
            }
        }
    }

    let mut out = Vec::new();
    walk(&root, "", 0, prefix, head, is_open, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_branches_by_slash() {
        let names: Vec<String> = ["main", "feat/a", "feat/b", "dev/x/y", "zeta"].map(String::from).to_vec();
        let closed = |_: &str, d: bool| d;
        let rows = branch_rows(&names, "local:", Some("dev/x/y"), &closed);
        let shape: Vec<String> = rows
            .iter()
            .map(|r| match r {
                BranchRow::Folder { name, depth, open, .. } => format!("D{depth}:{name}:{open}"),
                BranchRow::Leaf { label, depth, .. } => format!("L{depth}:{label}"),
            })
            .collect();
        assert_eq!(
            shape,
            ["L0:main", "L0:zeta", "D0:dev:true", "D1:x:true", "L2:y", "D0:feat:false"]
        );
    }
}

/// Hỏi xác nhận (nút 1 = đồng ý) rồi chạy `f`.
pub(crate) fn confirm_then(
    window: &mut Window,
    cx: &mut Context<RepoView>,
    message: &str,
    detail: &str,
    ok: &str,
    f: impl FnOnce(&mut RepoView, &mut Context<RepoView>) + 'static,
) {
    crate::ui::confirm(window, cx, message, detail, &[t!("sidebar.cancel"), ok], move |this, _, _, cx| f(this, cx));
}
