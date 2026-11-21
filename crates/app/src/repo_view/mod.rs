//! Một tab repo: cột trái (file, thay đổi, nhánh...), giữa (đồ thị + chi tiết),
//! phải (trình soạn).

mod commands;
mod detail;
mod history;
mod overlay;
mod sidebar;

use std::collections::BTreeSet;
use std::path::PathBuf;

use branchpad_core::{
    git::{
        self, BlameLine, ChangedFile, Commit, CommitDetail, ConflictPart, FileDiff, GitRef,
        GraphBuilder, GraphRow, LogFilter, LogQuery, Remote, Repo, RepoState, Resolution,
        SequenceStep, StashEntry, Status, TodoItem, WorkingSide, Worktree,
    },
    FileTree, WatchBatch, Watcher,
};
use futures::StreamExt;
use gpui::{
    div, prelude::*, px, App, Context, Entity, EventEmitter, FocusHandle, Focusable,
    SharedString, Subscription, UniformListScrollHandle, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{InputEvent, InputState},
    resizable::{h_resizable, resizable_panel, v_resizable},
    v_flex, ActiveTheme, Disableable, Icon, IconName, Sizable,
};

use crate::icons::BpIcon;
use crate::actions::{Cancel, CommitChanges, Fetch, Pull, Push, Refresh, ToggleEditor};
use crate::editor::{EditorEvent, EditorPane};
pub use overlay::Overlay;

/// Số commit mỗi lần tải thêm.
const HISTORY_PAGE: usize = 1000;

pub enum RepoViewEvent {
    /// Mở một thư mục (worktree) thành tab mới.
    OpenRepository(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarTab {
    Files,
    Changes,
    Git,
}

pub struct CommitView {
    pub id: String,
    pub detail: Option<CommitDetail>,
    pub files: Vec<ChangedFile>,
    pub selected: Option<usize>,
    pub diff: Vec<FileDiff>,
}

pub struct ConflictView {
    pub path: String,
    pub parts: Vec<ConflictPart>,
    pub choices: Vec<Option<Resolution>>,
}

pub struct RebaseView {
    pub base: Option<String>,
    pub items: Vec<TodoItem>,
}

/// Nội dung phần dưới của cột giữa.
pub enum Detail {
    Empty,
    Commit(CommitView),
    Working {
        path: String,
        side: WorkingSide,
        diff: Vec<FileDiff>,
    },
    Stash {
        name: String,
        files: Vec<ChangedFile>,
        selected: Option<usize>,
        diff: Vec<FileDiff>,
    },
    Conflict(ConflictView),
    Blame {
        path: String,
        lines: Vec<BlameLine>,
    },
    FileHistory {
        path: String,
        commits: Vec<Commit>,
    },
    Rebase(RebaseView),
}

struct RefsSnapshot {
    refs: Vec<GitRef>,
    remotes: Vec<Remote>,
    stashes: Vec<StashEntry>,
    worktrees: Vec<Worktree>,
}

struct Snapshot {
    status: Status,
    state: RepoState,
    refs: Option<RefsSnapshot>,
}

pub struct RepoView {
    pub(crate) repo: Repo,
    focus: FocusHandle,
    sidebar: SidebarTab,

    status: Status,
    state: RepoState,
    refs: Vec<GitRef>,
    refs_sig: Vec<(String, String)>,
    remotes: Vec<Remote>,
    stashes: Vec<StashEntry>,
    worktrees: Vec<Worktree>,
    tree: Option<FileTree>,
    selected_path: Option<PathBuf>,
    refreshing: bool,
    pending_refresh: Option<bool>,

    commits: Vec<Commit>,
    graph: GraphBuilder,
    rows: Vec<GraphRow>,
    history_end: bool,
    history_loading: bool,
    history_gen: u64,
    filter: LogFilter,
    search_input: Entity<InputState>,
    selected_commit: Option<String>,
    commit_scroll: UniformListScrollHandle,
    tree_scroll: UniformListScrollHandle,
    /// Nhóm/thư mục trong cột Git đã bị bấm đổi trạng thái so với mặc định.
    ref_toggled: std::collections::HashSet<String>,
    diff_scroll: UniformListScrollHandle,

    detail: Detail,

    editor: Entity<EditorPane>,
    show_editor: bool,
    commit_input: Entity<InputState>,
    amend: bool,

    overlay: Overlay,
    busy: Option<SharedString>,
    error: Option<String>,
    info: Option<String>,

    _watcher: Option<Watcher>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<RepoViewEvent> for RepoView {}

impl Focusable for RepoView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl RepoView {
    pub fn new(repo: Repo, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| EditorPane::new(repo.clone(), cx));
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("repo.search.placeholder"))
        });
        let commit_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line()
                .placeholder(t!("repo.commit.placeholder"))
        });

        let mut subs = Vec::new();
        subs.push(cx.subscribe_in(&search_input, window, |this, input, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                let q = input.read(cx).value().to_string();
                this.filter = LogFilter::parse(&q);
                this.reload_history(cx);
            }
        }));
        subs.push(cx.subscribe_in(&commit_input, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { secondary: true } = ev {
                this.commit_changes(&CommitChanges, window, cx);
            }
        }));
        subs.push(cx.subscribe(&editor, |this, _, ev: &EditorEvent, cx| match ev {
            EditorEvent::Saved => this.refresh(false, None, cx),
        }));

        let tree = match FileTree::new(&repo) {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("branchpad: cannot read folder {}: {e}", repo.workdir().display());
                None
            }
        };

        // Theo dõi file bằng sự kiện hệ điều hành; đợt thay đổi đi qua kênh async.
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<WatchBatch>();
        let watcher = match Watcher::start(repo.workdir(), repo.git_dir(), move |batch| {
            let _ = tx.unbounded_send(batch);
        }) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("branchpad: cannot watch file changes: {e}");
                None
            }
        };
        cx.spawn_in(window, async move |this, cx| {
            while let Some(batch) = rx.next().await {
                if this
                    .update_in(cx, |this, window, cx| this.on_fs_batch(batch, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let mut view = RepoView {
            repo,
            focus: cx.focus_handle(),
            sidebar: SidebarTab::Changes,
            status: Status::default(),
            state: RepoState::Clean,
            refs: Vec::new(),
            refs_sig: Vec::new(),
            remotes: Vec::new(),
            stashes: Vec::new(),
            worktrees: Vec::new(),
            tree,
            selected_path: None,
            refreshing: false,
            pending_refresh: None,
            commits: Vec::new(),
            graph: GraphBuilder::new(),
            rows: Vec::new(),
            history_end: false,
            history_loading: false,
            history_gen: 0,
            filter: LogFilter::None,
            search_input,
            selected_commit: None,
            commit_scroll: UniformListScrollHandle::new(),
            tree_scroll: UniformListScrollHandle::new(),
            ref_toggled: Default::default(),
            diff_scroll: UniformListScrollHandle::new(),
            detail: Detail::Empty,
            editor,
            show_editor: true,
            commit_input,
            amend: false,
            overlay: Overlay::None,
            busy: None,
            error: None,
            info: None,
            _watcher: watcher,
            _subs: subs,
        };
        view.refresh(true, None, cx);
        view.reload_history(cx);
        view
    }

    pub fn repo(&self) -> &Repo {
        &self.repo
    }

    pub fn has_unsaved(&self, cx: &App) -> bool {
        self.editor.read(cx).has_unsaved()
    }

    // ---- Chạy git ở luồng nền ----

    /// Chạy `job` ở luồng nền rồi gọi `done` trên luồng giao diện.
    /// `busy` hiện ở thanh trạng thái trong lúc chạy.
    pub(crate) fn spawn_git<T: Send + 'static>(
        &mut self,
        busy: Option<&str>,
        cx: &mut Context<Self>,
        job: impl FnOnce(&Repo) -> git::Result<T> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let repo = self.repo.clone();
        let label: Option<SharedString> = busy.map(|b| SharedString::from(b.to_string()));
        if label.is_some() {
            self.busy = label.clone();
            self.error = None;
            self.info = None;
            cx.notify();
        }
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { job(&repo) }).await;
            this.update(cx, |this, cx| {
                if label.is_some() && this.busy == label {
                    this.busy = None;
                }
                match result {
                    Ok(v) => done(this, v, cx),
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Như `spawn_git` nhưng `done` có `Window` (mở file, đặt focus...).
    pub(crate) fn spawn_git_in<T: Send + 'static>(
        &mut self,
        busy: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
        job: impl FnOnce(&Repo) -> git::Result<T> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let repo = self.repo.clone();
        let label: Option<SharedString> = busy.map(|b| SharedString::from(b.to_string()));
        if label.is_some() {
            self.busy = label.clone();
            self.error = None;
            self.info = None;
            cx.notify();
        }
        cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_spawn(async move { job(&repo) }).await;
            this.update_in(cx, |this, window, cx| {
                if label.is_some() && this.busy == label {
                    this.busy = None;
                }
                match result {
                    Ok(v) => done(this, v, window, cx),
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Thao tác thay đổi repo: chạy, báo kết quả, luôn tải lại trạng thái
    /// (kể cả khi lỗi giữa chừng như xung đột, vì repo vẫn có thể đã đổi).
    pub(crate) fn run_op(
        &mut self,
        busy: &str,
        success: Option<&str>,
        cx: &mut Context<Self>,
        job: impl FnOnce(&Repo) -> git::Result<()> + Send + 'static,
    ) {
        let repo = self.repo.clone();
        let label = SharedString::from(busy.to_string());
        let success = success.map(str::to_string);
        self.busy = Some(label.clone());
        self.error = None;
        self.info = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { job(&repo) }).await;
            this.update(cx, |this, cx| {
                if this.busy.as_ref() == Some(&label) {
                    this.busy = None;
                }
                match result {
                    Ok(()) => this.info = success,
                    Err(e) => this.error = Some(e.to_string()),
                }
                this.refresh(true, None, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ---- Trạng thái repo ----

    fn on_fs_batch(&mut self, batch: WatchBatch, window: &mut Window, cx: &mut Context<Self>) {
        if !batch.paths.is_empty() {
            self.editor
                .update(cx, |e, cx| e.reload_changed(&batch.paths, window, cx));
            if let Some(tree) = self.tree.as_mut() {
                tree.refresh_paths(&self.repo, batch.paths.iter());
            }
        }
        let paths = (!batch.git_changed).then_some(batch.paths);
        self.refresh(batch.git_changed, paths, cx);
    }

    /// Tải lại trạng thái. `full` = cả nhánh/tag/stash/worktree.
    /// `paths` = chỉ các file này đổi; nếu tất cả đều bị .gitignore thì bỏ qua.
    pub(crate) fn refresh(&mut self, full: bool, paths: Option<BTreeSet<PathBuf>>, cx: &mut Context<Self>) {
        if self.refreshing {
            self.pending_refresh = Some(self.pending_refresh.unwrap_or(false) || full);
            return;
        }
        self.refreshing = true;
        let repo = self.repo.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { load_snapshot(&repo, full, paths) })
                .await;
            this.update(cx, |this, cx| {
                this.refreshing = false;
                match result {
                    Ok(Some(s)) => this.apply_snapshot(s, cx),
                    Ok(None) => {}
                    Err(e) => this.error = Some(e.to_string()),
                }
                if let Some(full) = this.pending_refresh.take() {
                    this.refresh(full, None, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn apply_snapshot(&mut self, s: Snapshot, cx: &mut Context<Self>) {
        let head_changed = s.status.head_oid != self.status.head_oid;
        self.status = s.status;
        self.state = s.state;
        let mut reload = head_changed;
        if let Some(r) = s.refs {
            let sig: Vec<(String, String)> = r
                .refs
                .iter()
                .map(|x| (x.full.clone(), x.oid.clone()))
                .collect();
            reload |= sig != self.refs_sig;
            self.refs_sig = sig;
            self.refs = r.refs;
            self.remotes = r.remotes;
            self.stashes = r.stashes;
            self.worktrees = r.worktrees;
        }
        if reload {
            self.reload_history(cx);
        }
        if head_changed {
            if let Some(tree) = self.tree.as_mut() {
                tree.refresh_all(&self.repo);
            }
            self.editor.update(cx, |e, cx| e.refresh_bases(cx));
        }
        self.reload_working_detail(cx);
    }

    /// Diff đang xem của file đang làm: tải lại theo trạng thái mới.
    fn reload_working_detail(&mut self, cx: &mut Context<Self>) {
        let Detail::Working { path, side, .. } = &self.detail else {
            return;
        };
        let (path, side) = (path.clone(), *side);
        // File đã chuyển giữa staged/unstaged: theo nó sang bên còn diff.
        let entry = self.status.entries.iter().find(|e| e.path == path).cloned();
        let side = match (side, entry) {
            (_, None) => {
                self.detail = Detail::Empty;
                return;
            }
            (WorkingSide::Staged, Some(e)) if !e.is_staged() && e.is_unstaged() => WorkingSide::Unstaged,
            (WorkingSide::Unstaged, Some(e)) if !e.is_unstaged() && e.is_staged() => WorkingSide::Staged,
            (s, _) => s,
        };
        self.show_working_diff(path, side, cx);
    }

    // ---- Lịch sử ----

    pub(crate) fn reload_history(&mut self, cx: &mut Context<Self>) {
        self.history_gen += 1;
        self.commits.clear();
        self.rows.clear();
        self.graph = GraphBuilder::new();
        self.history_end = false;
        self.history_loading = false;
        self.load_more_history(cx);
    }

    pub(crate) fn load_more_history(&mut self, cx: &mut Context<Self>) {
        if self.history_loading || self.history_end {
            return;
        }
        self.history_loading = true;
        let gen = self.history_gen;
        let query = LogQuery {
            skip: self.commits.len(),
            limit: HISTORY_PAGE,
            filter: self.filter.clone(),
            path: None,
        };
        let repo = self.repo.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { repo.log(&query) }).await;
            this.update(cx, |this, cx| {
                if gen != this.history_gen {
                    return;
                }
                this.history_loading = false;
                match result {
                    Ok(list) => {
                        if list.len() < HISTORY_PAGE {
                            this.history_end = true;
                        }
                        for c in list {
                            this.rows.push(this.graph.push(&c.id, &c.parents));
                            this.commits.push(c);
                        }
                    }
                    Err(e) => {
                        this.history_end = true;
                        this.error = Some(e.to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ---- Hiển thị ----

    pub(crate) fn show_working_diff(&mut self, path: String, side: WorkingSide, cx: &mut Context<Self>) {
        let p = path.clone();
        self.spawn_git(
            None,
            cx,
            move |repo| repo.working_diff(&p, side),
            move |this, diff, _| {
                this.detail = Detail::Working { path, side, diff };
            },
        );
    }

    pub(crate) fn current_branch(&self) -> Option<&str> {
        self.status.branch.as_deref()
    }

    /// Remote mặc định: remote của upstream, rồi `origin`, rồi remote đầu tiên.
    pub(crate) fn default_remote(&self) -> Option<String> {
        if let Some(up) = &self.status.upstream {
            if let Some((r, _)) = up.split_once('/') {
                return Some(r.to_string());
            }
        }
        if self.remotes.iter().any(|r| r.name == "origin") {
            return Some("origin".into());
        }
        self.remotes.first().map(|r| r.name.clone())
    }

    // ---- Action ----

    fn on_refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(tree) = self.tree.as_mut() {
            tree.refresh_all(&self.repo);
        }
        self.refresh(true, None, cx);
        self.reload_history(cx);
    }

    fn on_fetch(&mut self, _: &Fetch, _: &mut Window, cx: &mut Context<Self>) {
        self.run_op(t!("repo.fetch.busy"), Some(t!("repo.fetch.done")), cx, |r| r.fetch_all());
    }

    fn on_pull(&mut self, _: &Pull, _: &mut Window, cx: &mut Context<Self>) {
        if self.status.upstream.is_none() {
            self.error = Some(t!("repo.pull.no_upstream").into());
            cx.notify();
            return;
        }
        self.run_op(t!("repo.pull.busy"), Some(t!("repo.pull.done")), cx, |r| {
            r.pull(git::PullMode::Merge)
        });
    }

    fn on_push(&mut self, _: &Push, _: &mut Window, cx: &mut Context<Self>) {
        let Some(branch) = self.current_branch().map(str::to_string) else {
            self.error = Some(t!("repo.push.detached").into());
            cx.notify();
            return;
        };
        self.push_branch(branch, false, cx);
    }

    pub(crate) fn push_branch(&mut self, branch: String, force: bool, cx: &mut Context<Self>) {
        let upstream = self
            .refs
            .iter()
            .find(|r| r.kind == git::RefKind::Local && r.name == branch)
            .and_then(|r| r.upstream.clone());
        let (remote, remote_branch, set_upstream) = match upstream.as_deref().and_then(|u| u.split_once('/')) {
            Some((r, b)) => (r.to_string(), b.to_string(), false),
            None => match self.default_remote() {
                Some(r) => (r, branch.clone(), true),
                None => {
                    self.error = Some(t!("repo.push.no_remote").into());
                    cx.notify();
                    return;
                }
            },
        };
        let msg = t!("repo.push.done", branch = branch, remote = remote, remote_branch = remote_branch);
        self.run_op(t!("repo.push.busy"), Some(&msg), cx, move |r| {
            r.push(&remote, &branch, &remote_branch, set_upstream, force)
        });
    }

    fn commit_changes(&mut self, _: &CommitChanges, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.commit_input.read(cx).value().to_string();
        let amend = self.amend;
        if self.status.staged().next().is_none() && !amend {
            self.error = Some(t!("repo.commit.nothing_staged").into());
            cx.notify();
            return;
        }
        let input = self.commit_input.clone();
        self.spawn_git_in(
            Some(t!("repo.commit.busy")),
            window,
            cx,
            move |r| r.commit(&message, amend),
            move |this, (), window, cx| {
                input.update(cx, |s, cx| s.set_value("", window, cx));
                this.amend = false;
                this.info = Some(t!("repo.commit.done").into());
                this.refresh(true, None, cx);
            },
        );
    }

    pub(crate) fn toggle_amend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.amend = !self.amend;
        if self.amend && self.commit_input.read(cx).value().trim().is_empty() {
            let input = self.commit_input.clone();
            self.spawn_git_in(
                None,
                window,
                cx,
                |r| r.last_commit_message(),
                move |_, msg, window, cx| input.update(cx, |s, cx| s.set_value(msg, window, cx)),
            );
        }
        cx.notify();
    }

    pub(crate) fn sequence(&mut self, step: SequenceStep, cx: &mut Context<Self>) {
        let label = match step {
            SequenceStep::Continue => t!("repo.sequence.continuing"),
            SequenceStep::Skip => t!("repo.sequence.skipping"),
            SequenceStep::Abort => t!("repo.sequence.aborting"),
        };
        self.run_op(label, None, cx, move |r| r.sequence(step));
    }

    fn on_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.overlay, Overlay::None) {
            self.overlay = Overlay::None;
            window.focus(&self.focus);
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    fn on_toggle_editor(&mut self, _: &ToggleEditor, _: &mut Window, cx: &mut Context<Self>) {
        self.show_editor = !self.show_editor;
        cx.notify();
    }

    pub(crate) fn open_in_editor(&mut self, path: PathBuf, line: Option<u32>, window: &mut Window, cx: &mut Context<Self>) {
        self.show_editor = true;
        self.editor
            .update(cx, |e, cx| e.open_file(path, line, window, cx));
        cx.notify();
    }

    pub(crate) fn open_revision_in_editor(&mut self, rev: String, rel: String, window: &mut Window, cx: &mut Context<Self>) {
        self.show_editor = true;
        self.editor
            .update(cx, |e, cx| e.open_revision(&rev, &rel, window, cx));
        cx.notify();
    }

    // ---- Vẽ ----

    fn render_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let branch = match &self.status.branch {
            Some(b) => b.clone(),
            None => match &self.status.head_oid {
                Some(h) => format!("(detached {})", &h[..h.len().min(7)]),
                None => t!("repo.toolbar.no_commits").into(),
            },
        };
        let mut track = String::new();
        if self.status.ahead > 0 {
            track.push_str(&format!("↑{} ", self.status.ahead));
        }
        if self.status.behind > 0 {
            track.push_str(&format!("↓{}", self.status.behind));
        }
        let busy = self.busy.is_some();
        h_flex()
            .h(px(44.))
            .px_3()
            .gap_2()
            .flex_none()
            .items_center()
            .border_b_1()
            .border_color(t.border)
            .child(
                h_flex()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(t.secondary.opacity(0.6))
                    .border_1()
                    .border_color(t.border)
                    .text_sm()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(Icon::new(BpIcon::Branch).size_4().text_color(t.primary))
                            .child(branch.clone()),
                    )
                    .when(!track.is_empty(), |d| {
                        d.child(
                            div()
                                .px_1()
                                .py(px(2.))
                                .rounded_sm()
                                .bg(t.primary.opacity(0.15))
                                .text_color(t.primary)
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_xs()
                                .child(track.clone()),
                        )
                    }),
            )
            .child(div().flex_1())
            .child(
                Button::new("tb-fetch").icon(BpIcon::Fetch).label("Fetch").small().ghost().disabled(busy).loading(self.busy.as_ref().is_some_and(|b| b.as_ref() == t!("repo.fetch.busy")))
                    .tooltip(t!("repo.toolbar.fetch_tooltip"))
                    .on_click(cx.listener(|this, _, w, cx| this.on_fetch(&Fetch, w, cx))),
            )
            .child(
                Button::new("tb-pull").icon(BpIcon::Pull).label(if self.status.behind > 0 { format!("Pull ↓{}", self.status.behind) } else { "Pull".into() })
                    .small().ghost().disabled(busy).loading(self.busy.as_ref().is_some_and(|b| b.as_ref() == t!("repo.pull.busy")))
                    .on_click(cx.listener(|this, _, w, cx| this.on_pull(&Pull, w, cx))),
            )
            .child(
                Button::new("tb-push").icon(BpIcon::Push).label(if self.status.ahead > 0 { format!("Push ↑{}", self.status.ahead) } else { "Push".into() })
                    .small().ghost().disabled(busy).loading(self.busy.as_ref().is_some_and(|b| b.as_ref() == t!("repo.push.busy")))
                    .on_click(cx.listener(|this, _, w, cx| this.on_push(&Push, w, cx))),
            )
            .child(
                Button::new("tb-stash").icon(BpIcon::Stash).label("Stash").small().ghost().disabled(busy)
                    .tooltip(t!("repo.toolbar.stash_tooltip"))
                    .on_click(cx.listener(|this, _, w, cx| this.prompt_stash(w, cx))),
            )
            .child(
                Button::new("tb-patch").icon(BpIcon::Patch).label("Apply patch…").small().ghost().disabled(busy)
                    .on_click(cx.listener(|this, _, w, cx| this.apply_patch(w, cx))),
            )
            .child(
                Button::new("tb-refresh").icon(BpIcon::Refresh).label(t!("repo.toolbar.refresh")).small().ghost()
                    .tooltip("Cmd+R")
                    .on_click(cx.listener(|this, _, w, cx| this.on_refresh(&Refresh, w, cx))),
            )
            .child(
                Button::new("tb-editor").icon(if self.show_editor { IconName::PanelRightClose } else { IconName::PanelRightOpen }).label(if self.show_editor { t!("repo.toolbar.hide_editor") } else { t!("repo.toolbar.show_editor") })
                    .small().ghost()
                    .on_click(cx.listener(|this, _, w, cx| this.on_toggle_editor(&ToggleEditor, w, cx))),
            )
    }

    fn render_state_banner(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        if self.state == RepoState::Clean {
            return None;
        }
        let t = cx.theme();
        let conflicts = self.status.conflicted().count();
        let can_skip = self.state.can_skip();
        Some(
            h_flex()
                .px_3()
                .py_1()
                .gap_2()
                .flex_none()
                .text_sm()
                .bg(t.warning.opacity(0.15))
                .child(div().flex_1().child(if conflicts > 0 {
                    t!("repo.banner.conflicts", state = crate::i18n::repo_state_label(self.state), n = conflicts)
                } else {
                    t!("repo.banner.resolved", state = crate::i18n::repo_state_label(self.state))
                }))
                .child(
                    Button::new("seq-continue").label(t!("repo.banner.continue")).small().primary()
                        .disabled(conflicts > 0 || self.busy.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.sequence(SequenceStep::Continue, cx))),
                )
                .when(can_skip, |d| {
                    d.child(
                        Button::new("seq-skip").label(t!("repo.banner.skip")).small().ghost()
                            .disabled(self.busy.is_some())
                            .on_click(cx.listener(|this, _, _, cx| this.sequence(SequenceStep::Skip, cx))),
                    )
                })
                .child(
                    Button::new("seq-abort").label(t!("common.cancel")).small().danger()
                        .disabled(self.busy.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.sequence(SequenceStep::Abort, cx))),
                ),
        )
    }

    fn render_message_bar(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        let t = cx.theme();
        // (text, accent color, icon)
        let (text, color, icon) = if let Some(b) = &self.busy {
            (b.to_string(), t.muted_foreground, IconName::Loader)
        } else if let Some(e) = &self.error {
            (e.clone(), t.danger, IconName::CircleX)
        } else if let Some(i) = &self.info {
            (i.clone(), t.success, IconName::CircleCheck)
        } else {
            return None;
        };
        let dismissable = self.busy.is_none();
        Some(
            h_flex()
                .px_3()
                .py_2()
                .gap_2()
                .flex_none()
                .items_start()
                .border_t_1()
                .border_color(color.opacity(0.5))
                .bg(color.opacity(0.12))
                .text_sm()
                .text_color(t.foreground)
                .child(Icon::new(icon).size_4().text_color(color).flex_none())
                .child(div().flex_1().max_h(px(120.)).overflow_hidden().whitespace_normal().child(text))
                .when(dismissable, |d| {
                    d.child(
                        Button::new("msg-close").label(t!("common.close")).xsmall().ghost().on_click(
                            cx.listener(|this, _, _, cx| {
                                this.error = None;
                                this.info = None;
                                cx.notify();
                            }),
                        ),
                    )
                }),
        )
    }
}

fn load_snapshot(repo: &Repo, full: bool, paths: Option<BTreeSet<PathBuf>>) -> git::Result<Option<Snapshot>> {
    if let Some(paths) = paths {
        // Chỉ file bị .gitignore đổi (build, node_modules...) thì không cần tải lại.
        if !paths.is_empty() && paths.len() <= 500 {
            let rels: Vec<String> = paths
                .iter()
                .filter_map(|p| p.strip_prefix(repo.workdir()).ok())
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            let ignored = repo.ignored(&rels)?;
            if rels.iter().all(|r| ignored.contains(r)) {
                return Ok(None);
            }
        }
    }
    let status = repo.status()?;
    let state = repo.state();
    let refs = if full {
        Some(RefsSnapshot {
            refs: repo.refs()?,
            remotes: repo.remotes()?,
            stashes: repo.stashes()?,
            worktrees: repo.worktrees()?,
        })
    } else {
        None
    };
    Ok(Some(Snapshot { status, state, refs }))
}

impl Render for RepoView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let border = t.border;
        let bg = t.background;
        let sidebar = self.render_sidebar(window, cx);
        let history = self.render_history(window, cx);
        let detail = self.render_detail(window, cx);
        let toolbar = self.render_toolbar(cx);
        let banner = self.render_state_banner(cx);
        let message = self.render_message_bar(cx);
        let overlay = self.render_overlay(window, cx);

        let center = v_flex()
            .size_full()
            .relative()
            .child(toolbar)
            .children(banner)
            .child(
                v_resizable("center-split")
                    .child(resizable_panel().size(px(420.)).child(history))
                    .child(resizable_panel().child(detail)),
            )
            .children(message)
            .children(overlay);

        let editor = self.editor.clone();
        v_flex()
            .key_context("RepoView")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_refresh))
            .on_action(cx.listener(Self::on_fetch))
            .on_action(cx.listener(Self::on_pull))
            .on_action(cx.listener(Self::on_push))
            .on_action(cx.listener(Self::commit_changes))
            .on_action(cx.listener(Self::on_cancel))
            .on_action(cx.listener(Self::on_toggle_editor))
            .on_action(cx.listener(Self::on_quick_open))
            .on_action(cx.listener(Self::on_find_in_files))
            .on_action(cx.listener(Self::on_goto_line))
            .on_action(cx.listener(Self::on_commit_action))
            .on_action(cx.listener(Self::on_ref_action))
            .on_action(cx.listener(Self::on_file_action))
            .size_full()
            .bg(bg)
            .child(
                h_resizable("repo-split")
                    .child(
                        resizable_panel()
                            .size(px(280.))
                            .size_range(px(180.)..px(600.))
                            .child(div().size_full().border_r_1().border_color(border).child(sidebar)),
                    )
                    .child(resizable_panel().child(center))
                    .when(self.show_editor, |r| {
                        r.child(
                            resizable_panel()
                                .size(px(560.))
                                .size_range(px(280.)..px(1600.))
                                .child(div().size_full().border_l_1().border_color(border).child(editor)),
                        )
                    }),
            )
    }
}
