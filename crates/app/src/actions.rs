//! Action, phím tắt và menu của app.

use std::path::PathBuf;

use gpui::{actions, Action, App, KeyBinding, Menu, MenuItem};

actions!(
    branchpad,
    [
        Quit,
        OpenSettings,
        OpenRepository,
        CloseRepository,
        NextRepository,
        PreviousRepository,
        Save,
        CloseEditorTab,
        QuickOpen,
        FindInFiles,
        GotoLine,
        ToggleComment,
        CommitChanges,
        Refresh,
        Fetch,
        Pull,
        Push,
        Cancel,
        ToggleEditor,
    ]
);

/// Thao tác trên một commit (menu chuột phải trong lịch sử).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitOp {
    Checkout,
    CreateBranch,
    CreateTag,
    CherryPick,
    Revert,
    ResetSoft,
    ResetMixed,
    ResetHard,
    FormatPatch,
    CopyHash,
    RebaseInteractive,
}

#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = branchpad, no_json)]
pub struct OnCommit {
    pub op: CommitOp,
    pub id: String,
}

/// Thao tác trên nhánh / tag / stash / worktree / remote (cột trái).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefOp {
    Checkout,
    Merge,
    Rebase,
    Rename,
    Delete,
    Push,
    ForcePush,
    Track,
    DeleteRemoteBranch,
    TagPush,
    TagDelete,
    StashApply,
    StashPop,
    StashDrop,
    WorktreeOpen,
    WorktreeRemove,
    RemoteFetch,
    RemoteRemove,
}

#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = branchpad, no_json)]
pub struct OnRef {
    pub op: RefOp,
    pub name: String,
}

/// Thao tác trên file/thư mục (cây file, danh sách thay đổi).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileOp {
    NewFile,
    NewFolder,
    Rename,
    Delete,
    RevealInFinder,
    CopyPath,
    History,
    Blame,
    OpenInEditor,
}

#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = branchpad, no_json)]
pub struct OnFile {
    pub op: FileOp,
    pub path: PathBuf,
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-o", OpenRepository, Some("Workspace")),
        KeyBinding::new("cmd-shift-w", CloseRepository, Some("Workspace")),
        KeyBinding::new("cmd-alt-right", NextRepository, Some("Workspace")),
        KeyBinding::new("cmd-alt-left", PreviousRepository, Some("Workspace")),
        KeyBinding::new("cmd-s", Save, Some("EditorPane")),
        KeyBinding::new("cmd-w", CloseEditorTab, Some("EditorPane")),
        KeyBinding::new("cmd-/", ToggleComment, Some("EditorPane")),
        KeyBinding::new("ctrl-g", GotoLine, Some("RepoView")),
        KeyBinding::new("cmd-p", QuickOpen, Some("RepoView")),
        KeyBinding::new("cmd-shift-f", FindInFiles, Some("RepoView")),
        KeyBinding::new("cmd-enter", CommitChanges, Some("RepoView")),
        KeyBinding::new("cmd-r", Refresh, Some("RepoView")),
        KeyBinding::new("cmd-shift-p", Pull, Some("RepoView")),
        KeyBinding::new("cmd-shift-u", Push, Some("RepoView")),
        KeyBinding::new("cmd-alt-b", ToggleEditor, Some("RepoView")),
        KeyBinding::new("escape", Cancel, Some("RepoView")),
    ]);
}

pub fn menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "Branchpad".into(),
            items: vec![
                MenuItem::action(t!("actions.app.settings"), OpenSettings),
                MenuItem::separator(),
                MenuItem::action(t!("actions.app.quit"), Quit),
            ],
        },
        Menu {
            name: t!("actions.menu.file").into(),
            items: vec![
                MenuItem::action(t!("actions.file.open_folder"), OpenRepository),
                MenuItem::action(t!("actions.file.quick_open"), QuickOpen),
                MenuItem::separator(),
                MenuItem::action(t!("actions.file.save"), Save),
                MenuItem::action(t!("actions.file.close_tab"), CloseEditorTab),
                MenuItem::action(t!("actions.file.close_repo"), CloseRepository),
            ],
        },
        Menu {
            name: t!("actions.menu.edit").into(),
            items: vec![
                MenuItem::action(t!("actions.edit.find_in_files"), FindInFiles),
                MenuItem::action(t!("actions.edit.goto_line"), GotoLine),
                MenuItem::action(t!("actions.edit.toggle_comment"), ToggleComment),
            ],
        },
        Menu {
            name: "Git".into(),
            items: vec![
                MenuItem::action("Commit", CommitChanges),
                MenuItem::separator(),
                MenuItem::action("Fetch", Fetch),
                MenuItem::action("Pull", Pull),
                MenuItem::action("Push", Push),
                MenuItem::separator(),
                MenuItem::action(t!("actions.git.refresh"), Refresh),
            ],
        },
        Menu {
            name: t!("actions.menu.view").into(),
            items: vec![
                MenuItem::action(t!("actions.view.toggle_editor"), ToggleEditor),
                MenuItem::action(t!("actions.view.next_repo"), NextRepository),
                MenuItem::action(t!("actions.view.previous_repo"), PreviousRepository),
            ],
        },
    ]
}
