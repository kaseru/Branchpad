//! Keyboard shortcuts and multi-selection of the Changes tab (Fork-style).

use std::collections::BTreeSet;

use branchpad_core::git::{EntryKind, StatusEntry, WorkingSide};
use gpui::{Context, Window};

use super::{overlay::PromptKind, Detail, RepoView, SidebarTab};
use crate::actions::{
    ChangeDown, ChangeUp, DiscardSelected, NewBranch, SelectAllChanges, ShowChangesTab, ShowFilesTab,
    ShowGitTab, StageAll, StashChanges, ToggleStageSelected, UnstageAll,
};

/// How a click changes the selection.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectMode {
    /// Plain click: select only this file.
    Replace,
    /// Cmd+click: add / remove this file.
    Toggle,
    /// Shift+click: select the range from the last clicked file.
    Range,
}

impl RepoView {
    /// Files of the Unstaged pane (conflicts, modified, untracked — in display order) or the Staged pane.
    pub(super) fn pane_entries(&self, staged: bool) -> Vec<StatusEntry> {
        if staged {
            self.status.staged().cloned().collect()
        } else {
            self.status
                .conflicted()
                .chain(self.status.unstaged())
                .chain(self.status.untracked())
                .cloned()
                .collect()
        }
    }

    /// Selected files and the pane they belong to; falls back to the file shown in the diff.
    fn effective_selection(&self) -> (Vec<String>, bool) {
        if !self.change_sel.is_empty() {
            return (self.change_sel.iter().cloned().collect(), self.change_sel_staged);
        }
        match &self.detail {
            Detail::Working { path, side, .. } => (vec![path.clone()], *side == WorkingSide::Staged),
            _ => (Vec::new(), false),
        }
    }

    pub(super) fn select_change(&mut self, path: &str, staged: bool, mode: SelectMode, cx: &mut Context<Self>) {
        if self.change_sel_staged != staged {
            self.change_sel.clear();
            self.change_sel_staged = staged;
        }
        match mode {
            SelectMode::Replace => {
                self.change_sel = BTreeSet::from([path.to_string()]);
            }
            SelectMode::Toggle => {
                if !self.change_sel.remove(path) {
                    self.change_sel.insert(path.to_string());
                }
            }
            SelectMode::Range => {
                let list = self.pane_entries(staged);
                let find = |p: &str| list.iter().position(|e| e.path == p);
                let to = find(path);
                let from = self.change_cursor.as_deref().and_then(find).or(to);
                if let (Some(a), Some(b)) = (from, to) {
                    let (lo, hi) = (a.min(b), a.max(b));
                    self.change_sel = list[lo..=hi].iter().map(|e| e.path.clone()).collect();
                }
            }
        }
        if mode != SelectMode::Range {
            self.change_cursor = Some(path.to_string());
        }
        cx.notify();
    }

    /// Show the diff (or conflict resolver) of a file of the Changes tab.
    pub(super) fn show_change(&mut self, entry: &StatusEntry, staged: bool, cx: &mut Context<Self>) {
        match (entry.kind, staged) {
            (EntryKind::Unmerged, _) => self.show_conflict(entry.path.clone(), cx),
            (_, true) => self.show_working_diff(entry.path.clone(), WorkingSide::Staged, cx),
            (EntryKind::Untracked, false) => self.show_working_diff(entry.path.clone(), WorkingSide::Untracked, cx),
            (_, false) => self.show_working_diff(entry.path.clone(), WorkingSide::Unstaged, cx),
        }
    }

    pub(super) fn on_select_all_changes(&mut self, _: &SelectAllChanges, _: &mut Window, cx: &mut Context<Self>) {
        let staged = self.change_sel_staged;
        self.change_sel = self.pane_entries(staged).into_iter().map(|e| e.path).collect();
        cx.notify();
    }

    fn move_change(&mut self, delta: isize, cx: &mut Context<Self>) {
        let staged = if self.change_sel.is_empty() {
            matches!(&self.detail, Detail::Working { side: WorkingSide::Staged, .. })
        } else {
            self.change_sel_staged
        };
        let list = self.pane_entries(staged);
        if list.is_empty() {
            return;
        }
        let current = self
            .change_cursor
            .as_deref()
            .and_then(|c| list.iter().position(|e| e.path == c));
        let next = match current {
            Some(i) => (i as isize + delta).clamp(0, list.len() as isize - 1) as usize,
            None => 0,
        };
        let entry = list[next].clone();
        self.change_sel_staged = staged;
        self.change_sel = BTreeSet::from([entry.path.clone()]);
        self.change_cursor = Some(entry.path.clone());
        self.show_change(&entry, staged, cx);
        cx.notify();
    }

    pub(super) fn on_change_up(&mut self, _: &ChangeUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_change(-1, cx);
    }

    pub(super) fn on_change_down(&mut self, _: &ChangeDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_change(1, cx);
    }

    /// Space: stage the selected files, or unstage them when they are in the Staged pane.
    pub(super) fn on_toggle_stage_selected(&mut self, _: &ToggleStageSelected, _: &mut Window, cx: &mut Context<Self>) {
        let (paths, staged) = self.effective_selection();
        // Conflicted files are resolved in the conflict view, not staged here.
        let paths: Vec<String> = paths
            .into_iter()
            .filter(|p| !self.status.entries.iter().any(|e| e.path == *p && e.kind == EntryKind::Unmerged))
            .collect();
        if paths.is_empty() {
            return;
        }
        self.change_sel.clear();
        if staged {
            self.unstage_paths(paths, cx);
        } else {
            self.stage_paths(paths, cx);
        }
    }

    pub(super) fn on_stage_all(&mut self, _: &StageAll, _: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<String> = self
            .status
            .unstaged()
            .chain(self.status.untracked())
            .map(|e| e.path.clone())
            .collect();
        if !paths.is_empty() {
            self.change_sel.clear();
            self.stage_paths(paths, cx);
        }
    }

    pub(super) fn on_unstage_all(&mut self, _: &UnstageAll, _: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<String> = self.status.staged().map(|e| e.path.clone()).collect();
        if !paths.is_empty() {
            self.change_sel.clear();
            self.unstage_paths(paths, cx);
        }
    }

    /// Backspace / Delete: discard the selected unstaged files (after a confirmation).
    pub(super) fn on_discard_selected(&mut self, _: &DiscardSelected, window: &mut Window, cx: &mut Context<Self>) {
        let (paths, staged) = self.effective_selection();
        if staged || paths.is_empty() {
            return;
        }
        let is_untracked =
            |p: &String| self.status.entries.iter().any(|e| e.path == *p && e.kind == EntryKind::Untracked);
        let (untracked, tracked): (Vec<String>, Vec<String>) = paths
            .into_iter()
            .filter(|p| !self.status.entries.iter().any(|e| e.path == *p && e.kind == EntryKind::Unmerged))
            .partition(|p| is_untracked(p));
        self.change_sel.clear();
        match (tracked.is_empty(), untracked.is_empty()) {
            (true, true) => {}
            (false, true) => self.discard_paths(tracked, false, window, cx),
            (true, false) => self.discard_paths(untracked, true, window, cx),
            (false, false) => {
                let n = tracked.len() + untracked.len();
                crate::ui::confirm(
                    window,
                    cx,
                    &t!("sidebar.discard.title", what = t!("sidebar.discard.n_files", n = n)),
                    t!("sidebar.discard.detail_mixed"),
                    &[t!("common.cancel"), t!("sidebar.discard.confirm")],
                    move |this, _, _, cx| {
                        this.run_op(t!("sidebar.op.discarding"), None, cx, move |r| {
                            let refs: Vec<&str> = tracked.iter().map(String::as_str).collect();
                            r.discard(&refs, false)?;
                            let refs: Vec<&str> = untracked.iter().map(String::as_str).collect();
                            r.discard(&refs, true)
                        });
                    },
                );
            }
        }
    }

    fn show_sidebar(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        self.sidebar = tab;
        cx.notify();
    }

    pub(super) fn on_show_files_tab(&mut self, _: &ShowFilesTab, _: &mut Window, cx: &mut Context<Self>) {
        self.show_sidebar(SidebarTab::Files, cx);
    }

    pub(super) fn on_show_changes_tab(&mut self, _: &ShowChangesTab, _: &mut Window, cx: &mut Context<Self>) {
        self.show_sidebar(SidebarTab::Changes, cx);
    }

    pub(super) fn on_show_git_tab(&mut self, _: &ShowGitTab, _: &mut Window, cx: &mut Context<Self>) {
        self.show_sidebar(SidebarTab::Git, cx);
    }

    pub(super) fn on_new_branch(&mut self, _: &NewBranch, window: &mut Window, cx: &mut Context<Self>) {
        self.open_prompt(
            PromptKind::CreateBranch { start: "HEAD".into() },
            t!("sidebar.prompt.new_branch"),
            "",
            window,
            cx,
        );
    }

    pub(super) fn on_stash_changes(&mut self, _: &StashChanges, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_stash(window, cx);
    }
}
