//! Action, phím tắt và menu của app.

use std::path::PathBuf;

use gpui::{actions, Action, App, KeyBinding, Menu, MenuItem};

actions!(
    branchpad,
    [
        Quit,
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
            items: vec![MenuItem::action("Thoát Branchpad", Quit)],
        },
        Menu {
            name: "Tệp".into(),
            items: vec![
                MenuItem::action("Mở thư mục…", OpenRepository),
                MenuItem::action("Mở file nhanh…", QuickOpen),
                MenuItem::separator(),
                MenuItem::action("Lưu", Save),
                MenuItem::action("Đóng tab file", CloseEditorTab),
                MenuItem::action("Đóng repo", CloseRepository),
            ],
        },
        Menu {
            name: "Sửa".into(),
            items: vec![
                MenuItem::action("Tìm trong thư mục…", FindInFiles),
                MenuItem::action("Đến dòng…", GotoLine),
                MenuItem::action("Bật/tắt chú thích", ToggleComment),
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
                MenuItem::action("Tải lại", Refresh),
            ],
        },
        Menu {
            name: "Xem".into(),
            items: vec![
                MenuItem::action("Hiện/ẩn trình soạn", ToggleEditor),
                MenuItem::action("Repo kế tiếp", NextRepository),
                MenuItem::action("Repo trước", PreviousRepository),
            ],
        },
    ]
}
