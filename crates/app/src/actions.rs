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
        TogglePreview,
        DuplicateLine,
        CutLine,
        SaveAll,
        NextEditorTab,
        PreviousEditorTab,
        ToggleWordWrap,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        CopyLine,
        DeleteLine,
        MoveLineUp,
        MoveLineDown,
        CommitChanges,
        Refresh,
        Fetch,
        Pull,
        Push,
        Cancel,
        ToggleEditor,
        ToggleCenter,
        SelectAllChanges,
        ChangeUp,
        ChangeDown,
        ToggleStageSelected,
        DiscardSelected,
        StageAll,
        UnstageAll,
        NewBranch,
        StashChanges,
        ShowFilesTab,
        ShowChangesTab,
        ShowGitTab,
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
    Compare,
    Stage,
    Unstage,
    Discard,
}

#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = branchpad, no_json)]
pub struct OnFile {
    pub op: FileOp,
    pub path: PathBuf,
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-o", OpenRepository, Some("Workspace")),
        KeyBinding::new("secondary-shift-w", CloseRepository, Some("Workspace")),
        KeyBinding::new("secondary-alt-right", NextRepository, Some("Workspace")),
        KeyBinding::new("secondary-alt-left", PreviousRepository, Some("Workspace")),
        KeyBinding::new("secondary-s", Save, Some("EditorPane")),
        KeyBinding::new("secondary-w", CloseEditorTab, Some("EditorPane")),
        KeyBinding::new("secondary-/", ToggleComment, Some("EditorPane")),
        KeyBinding::new("secondary-shift-v", TogglePreview, Some("EditorPane")),
        KeyBinding::new("secondary-shift-d", DuplicateLine, Some("EditorPane")),
        // Không chọn gì thì cắt/chép cả dòng (như VSCode); có chọn thì chạy Cut/Copy gốc.
        // Phải đặt cùng độ sâu với bản của ô nhập (`Input`) và đăng ký sau nó.
        KeyBinding::new("secondary-shift-s", SaveAll, Some("EditorPane")),
        KeyBinding::new("ctrl-tab", NextEditorTab, Some("EditorPane")),
        KeyBinding::new("ctrl-shift-tab", PreviousEditorTab, Some("EditorPane")),
        KeyBinding::new("alt-z", ToggleWordWrap, Some("EditorPane")),
        KeyBinding::new("secondary-=", ZoomIn, Some("Workspace")),
        KeyBinding::new("secondary-shift-=", ZoomIn, Some("Workspace")),
        KeyBinding::new("secondary--", ZoomOut, Some("Workspace")),
        KeyBinding::new("secondary-0", ZoomReset, Some("Workspace")),
        KeyBinding::new("secondary-x", CutLine, Some("EditorPane > Input")),
        KeyBinding::new("secondary-c", CopyLine, Some("EditorPane > Input")),
        KeyBinding::new("secondary-shift-k", DeleteLine, Some("EditorPane")),
        KeyBinding::new("alt-up", MoveLineUp, Some("EditorPane")),
        KeyBinding::new("alt-down", MoveLineDown, Some("EditorPane")),
        KeyBinding::new("ctrl-g", GotoLine, Some("RepoView")),
        KeyBinding::new("secondary-p", QuickOpen, Some("RepoView")),
        KeyBinding::new("secondary-shift-f", FindInFiles, Some("RepoView")),
        KeyBinding::new("secondary-enter", CommitChanges, Some("RepoView")),
        KeyBinding::new("secondary-r", Refresh, Some("RepoView")),
        KeyBinding::new("secondary-shift-p", Pull, Some("RepoView")),
        KeyBinding::new("secondary-shift-u", Push, Some("RepoView")),
        KeyBinding::new("secondary-alt-b", ToggleEditor, Some("RepoView")),
        KeyBinding::new("secondary-alt-h", ToggleCenter, Some("RepoView")),
        KeyBinding::new("secondary-alt-f", Fetch, Some("RepoView")),
        KeyBinding::new("secondary-alt-s", StashChanges, Some("RepoView")),
        KeyBinding::new("secondary-b", NewBranch, Some("RepoView")),
        KeyBinding::new("secondary-1", ShowFilesTab, Some("RepoView")),
        KeyBinding::new("secondary-2", ShowChangesTab, Some("RepoView")),
        KeyBinding::new("secondary-3", ShowGitTab, Some("RepoView")),
        KeyBinding::new("secondary-shift-a", StageAll, Some("RepoView")),
        // Discard the selected / current file from anywhere in the repo view (not in text fields).
        KeyBinding::new("secondary-backspace", DiscardSelected, Some("RepoView")),
        KeyBinding::new("secondary-shift-z", UnstageAll, Some("RepoView")),
        // Changes tab (only when no text field has focus).
        KeyBinding::new("secondary-a", SelectAllChanges, Some("ChangesPane")),
        KeyBinding::new("up", ChangeUp, Some("ChangesPane")),
        KeyBinding::new("down", ChangeDown, Some("ChangesPane")),
        KeyBinding::new("space", ToggleStageSelected, Some("ChangesPane")),
        KeyBinding::new("backspace", DiscardSelected, Some("ChangesPane")),
        KeyBinding::new("delete", DiscardSelected, Some("ChangesPane")),
        KeyBinding::new("secondary-backspace", DiscardSelected, Some("ChangesPane")),
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
                MenuItem::action(t!("actions.file.save_all"), SaveAll),
                MenuItem::action(t!("actions.file.close_tab"), CloseEditorTab),
                MenuItem::action(t!("actions.file.close_repo"), CloseRepository),
            ],
        },
        Menu {
            name: t!("actions.menu.edit").into(),
            items: vec![
                MenuItem::action(t!("actions.edit.undo"), gpui_component::input::Undo),
                MenuItem::action(t!("actions.edit.redo"), gpui_component::input::Redo),
                MenuItem::separator(),
                MenuItem::action(t!("actions.edit.cut"), gpui_component::input::Cut),
                MenuItem::action(t!("actions.edit.copy"), gpui_component::input::Copy),
                MenuItem::action(t!("actions.edit.paste"), gpui_component::input::Paste),
                MenuItem::action(t!("actions.edit.select_all"), gpui_component::input::SelectAll),
                MenuItem::separator(),
                MenuItem::action(t!("actions.edit.find"), gpui_component::input::Search),
                MenuItem::action(t!("actions.edit.toggle_word_wrap"), ToggleWordWrap),
                MenuItem::action(t!("actions.edit.zoom_in"), ZoomIn),
                MenuItem::action(t!("actions.edit.zoom_out"), ZoomOut),
                MenuItem::action(t!("actions.edit.zoom_reset"), ZoomReset),
                MenuItem::action(t!("actions.edit.duplicate_line"), DuplicateLine),
                MenuItem::action(t!("actions.edit.delete_line"), DeleteLine),
                MenuItem::action(t!("actions.edit.move_line_up"), MoveLineUp),
                MenuItem::action(t!("actions.edit.move_line_down"), MoveLineDown),
                MenuItem::separator(),
                MenuItem::action(t!("actions.edit.find_in_files"), FindInFiles),
                MenuItem::action(t!("actions.edit.goto_line"), GotoLine),
                MenuItem::action(t!("actions.edit.toggle_comment"), ToggleComment),
            ],
        },
        Menu {
            name: "Git".into(),
            items: vec![
                MenuItem::action("Commit", CommitChanges),
                MenuItem::action(t!("actions.git.stage_all"), StageAll),
                MenuItem::action(t!("actions.git.unstage_all"), UnstageAll),
                MenuItem::action(t!("actions.git.discard_selected"), DiscardSelected),
                MenuItem::action(t!("actions.git.select_all_changes"), SelectAllChanges),
                MenuItem::separator(),
                MenuItem::action(t!("actions.git.new_branch"), NewBranch),
                MenuItem::action(t!("actions.git.stash"), StashChanges),
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
                MenuItem::action(t!("actions.view.toggle_center"), ToggleCenter),
                MenuItem::action(t!("actions.view.files_tab"), ShowFilesTab),
                MenuItem::action(t!("actions.view.changes_tab"), ShowChangesTab),
                MenuItem::action(t!("actions.view.git_tab"), ShowGitTab),
                MenuItem::action(t!("actions.view.next_repo"), NextRepository),
                MenuItem::action(t!("actions.view.previous_repo"), PreviousRepository),
            ],
        },
    ]
}
