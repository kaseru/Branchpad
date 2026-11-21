//! Xử lý thao tác từ menu chuột phải, nút bấm và ô nhập.

use std::path::{Path, PathBuf};

use branchpad_core::{
    file_tree,
    git::{LogQuery, RefKind, ResetMode},
};
use gpui::{ClipboardItem, Context, PathPromptOptions, Window};

use super::{overlay::PromptKind, sidebar::confirm_then, Detail, RebaseView, RepoView, RepoViewEvent};
use crate::actions::{CommitOp, FileOp, OnCommit, OnFile, OnRef, RefOp};
use crate::ui::confirm;

impl RepoView {
    fn ref_kind(&self, name: &str) -> Option<RefKind> {
        self.refs.iter().find(|r| r.name == name).map(|r| r.kind)
    }

    fn rel_path(&self, path: &Path) -> String {
        path.strip_prefix(self.repo.workdir())
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }

    // ---- Commit ----

    pub(super) fn on_commit_action(&mut self, a: &OnCommit, window: &mut Window, cx: &mut Context<Self>) {
        let id = a.id.clone();
        let short = id[..id.len().min(8)].to_string();
        match a.op {
            CommitOp::Checkout => {
                let msg = t!("commands.done.checkout_detached", short = short);
                self.run_op(t!("commands.busy.checkout"), Some(&msg), cx, move |r| r.checkout_detached(&id));
            }
            CommitOp::CreateBranch => {
                self.open_prompt(PromptKind::CreateBranch { start: id }, &t!("commands.prompt.new_branch_at", short = short), "", window, cx)
            }
            CommitOp::CreateTag => self.open_prompt(
                PromptKind::CreateTag { rev: id },
                &t!("commands.prompt.new_tag_at", short = short),
                "",
                window,
                cx,
            ),
            CommitOp::CherryPick => {
                self.run_op(t!("commands.busy.cherry_pick"), Some(t!("commands.done.cherry_pick")), cx, move |r| r.cherry_pick(&id))
            }
            CommitOp::Revert => {
                let is_merge = self
                    .commits
                    .iter()
                    .find(|c| c.id == id)
                    .is_some_and(|c| c.parents.len() > 1);
                self.run_op(t!("commands.busy.revert"), Some(t!("commands.done.revert")), cx, move |r| r.revert(&id, is_merge))
            }
            CommitOp::ResetSoft => {
                self.run_op(t!("commands.busy.reset"), Some(t!("commands.done.reset_soft")), cx, move |r| r.reset(&id, ResetMode::Soft))
            }
            CommitOp::ResetMixed => {
                self.run_op(t!("commands.busy.reset"), Some(t!("commands.done.reset_mixed")), cx, move |r| r.reset(&id, ResetMode::Mixed))
            }
            CommitOp::ResetHard => confirm_then(
                window,
                cx,
                &t!("commands.reset_hard.title", short = short),
                t!("commands.reset_hard.detail"),
                t!("commands.reset_hard.ok"),
                move |this, cx| {
                    this.run_op(t!("commands.busy.reset"), Some(t!("commands.done.reset_hard")), cx, move |r| r.reset(&id, ResetMode::Hard))
                },
            ),
            CommitOp::FormatPatch => {
                let rx = cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some(t!("commands.patch.save_prompt").into()),
                });
                cx.spawn_in(window, async move |this, cx| {
                    if let Ok(Ok(Some(paths))) = rx.await {
                        if let Some(dir) = paths.into_iter().next() {
                            this.update(cx, |this, cx| {
                                this.spawn_git(
                                    Some(t!("commands.busy.format_patch")),
                                    cx,
                                    move |r| r.format_patch(&id, &dir),
                                    |this, file, _| {
                                        this.info = Some(t!("commands.done.patch_created", file = file.display()));
                                    },
                                );
                            })
                            .ok();
                        }
                    }
                })
                .detach();
            }
            CommitOp::CopyHash => {
                cx.write_to_clipboard(ClipboardItem::new_string(id));
                self.info = Some(t!("commands.done.copy_hash", short = short));
                cx.notify();
            }
            CommitOp::RebaseInteractive => {
                if self.current_branch().is_none() {
                    self.error = Some(t!("commands.error.rebase_needs_branch").into());
                    cx.notify();
                    return;
                }
                let base = id.clone();
                self.spawn_git(
                    Some(t!("commands.busy.read_commits")),
                    cx,
                    move |r| r.rebase_items(Some(&base)),
                    move |this, items, _| {
                        if items.is_empty() {
                            this.error = Some(t!("commands.error.no_commits_after").into());
                        } else {
                            this.detail = Detail::Rebase(RebaseView { base: Some(id), items });
                        }
                    },
                );
            }
        }
    }

    // ---- Nhánh / tag / stash / worktree / remote ----

    pub(crate) fn on_ref_action(&mut self, a: &OnRef, window: &mut Window, cx: &mut Context<Self>) {
        let name = a.name.clone();
        match a.op {
            RefOp::Checkout => {
                let detached = self.ref_kind(&name) != Some(RefKind::Local);
                if !detached {
                    self.ensure_branch_folder_open(&name, "local:");
                }
                let msg = t!("commands.done.checkout", name = name);
                self.run_op(t!("commands.busy.checkout"), Some(&msg), cx, move |r| {
                    if detached {
                        r.checkout_detached(&name)
                    } else {
                        r.checkout(&name)
                    }
                });
            }
            RefOp::Merge => {
                let msg = t!("commands.done.merge", name = name);
                self.run_op(t!("commands.busy.merge"), Some(&msg), cx, move |r| r.merge(&name, false));
            }
            RefOp::Rebase => {
                let msg = t!("commands.done.rebase", name = name);
                self.run_op(t!("commands.busy.rebase"), Some(&msg), cx, move |r| r.rebase_onto(&name));
            }
            RefOp::Rename => self.open_prompt(PromptKind::RenameBranch { old: name.clone() }, t!("commands.prompt.new_name"), &name, window, cx),
            RefOp::Delete => confirm(
                window,
                cx,
                &t!("commands.delete_branch.title", name = name),
                t!("commands.delete_branch.detail"),
                &[t!("commands.cancel"), t!("commands.delete"), t!("commands.force_delete")],
                move |this: &mut Self, answer, _, cx| {
                    let force = answer == 2;
                    this.run_op(t!("commands.busy.delete_branch"), Some(t!("commands.done.delete_branch")), cx, move |r| r.delete_branch(&name, force));
                },
            ),
            RefOp::Push => self.push_branch(name, false, cx),
            RefOp::ForcePush => confirm_then(
                window,
                cx,
                &t!("commands.force_push.title", name = name),
                t!("commands.force_push.detail"),
                t!("commands.force_push.ok"),
                move |this, cx| this.push_branch(name, true, cx),
            ),
            RefOp::Track => {
                let local = name.split_once('/').map(|(_, b)| b.to_string()).unwrap_or_else(|| name.clone());
                self.open_prompt(
                    PromptKind::TrackBranch { remote_branch: name.clone() },
                    &t!("commands.prompt.track_branch", name = name),
                    &local,
                    window,
                    cx,
                );
            }
            RefOp::DeleteRemoteBranch => {
                let Some((remote, branch)) = name.split_once('/').map(|(r, b)| (r.to_string(), b.to_string())) else {
                    return;
                };
                confirm_then(
                    window,
                    cx,
                    &t!("commands.delete_remote_branch.title", branch = branch, remote = remote),
                    t!("commands.delete_remote_branch.detail"),
                    t!("commands.delete_remote_branch.ok"),
                    move |this, cx| {
                        this.run_op(t!("commands.busy.delete_remote_branch"), Some(t!("commands.done.delete_remote_branch")), cx, move |r| {
                            r.delete_remote_branch(&remote, &branch)
                        })
                    },
                );
            }
            RefOp::TagPush => {
                let Some(remote) = self.default_remote() else {
                    self.error = Some(t!("commands.error.no_remote").into());
                    cx.notify();
                    return;
                };
                let msg = t!("commands.done.push_tag", name = name, remote = remote);
                self.run_op(t!("commands.busy.push_tag"), Some(&msg), cx, move |r| r.push_tag(&remote, &name));
            }
            RefOp::TagDelete => {
                let remote = self.default_remote();
                let mut buttons = vec![t!("commands.cancel"), t!("commands.delete_tag.local")];
                if remote.is_some() {
                    buttons.push(t!("commands.delete_tag.both"));
                }
                confirm(
                    window,
                    cx,
                    &t!("commands.delete_tag.title", name = name),
                    t!("commands.delete_tag.detail"),
                    &buttons,
                    move |this: &mut Self, answer, _, cx| {
                        this.run_op(t!("commands.busy.delete_tag"), Some(t!("commands.done.delete_tag")), cx, move |r| {
                            r.delete_tag(&name)?;
                            match (answer, remote) {
                                (2, Some(remote)) => r.delete_remote_tag(&remote, &name),
                                _ => Ok(()),
                            }
                        })
                    },
                );
            }
            RefOp::StashApply => {
                self.run_op(t!("commands.busy.stash_apply"), Some(t!("commands.done.stash_apply")), cx, move |r| r.stash_apply(&name))
            }
            RefOp::StashPop => {
                self.detail = Detail::Empty;
                self.run_op(t!("commands.busy.stash_pop"), Some(t!("commands.done.stash_pop")), cx, move |r| r.stash_pop(&name))
            }
            RefOp::StashDrop => confirm_then(
                window,
                cx,
                &t!("commands.delete_named.title", name = name),
                t!("commands.stash_drop.detail"),
                t!("commands.stash_drop.ok"),
                move |this, cx| {
                    this.detail = Detail::Empty;
                    this.run_op(t!("commands.busy.stash_drop"), Some(t!("commands.done.stash_drop")), cx, move |r| r.stash_drop(&name))
                },
            ),
            RefOp::WorktreeOpen => cx.emit(RepoViewEvent::OpenRepository(PathBuf::from(name))),
            RefOp::WorktreeRemove => confirm(
                window,
                cx,
                &t!("commands.remove_worktree.title", name = name),
                t!("commands.remove_worktree.detail"),
                &[t!("commands.cancel"), t!("commands.delete"), t!("commands.force_delete")],
                move |this: &mut Self, answer, _, cx| {
                    let force = answer == 2;
                    this.run_op(t!("commands.busy.remove_worktree"), Some(t!("commands.done.remove_worktree")), cx, move |r| {
                        r.remove_worktree(&name, force)
                    });
                },
            ),
            RefOp::RemoteFetch => {
                let msg = t!("commands.done.fetch", name = name);
                self.run_op(t!("commands.busy.fetch"), Some(&msg), cx, move |r| r.fetch_remote(&name));
            }
            RefOp::RemoteRemove => confirm_then(
                window,
                cx,
                &t!("commands.remove_remote.title", name = name),
                t!("commands.remove_remote.detail"),
                t!("commands.remove_remote.ok"),
                move |this, cx| this.run_op(t!("commands.busy.remove_remote"), Some(t!("commands.done.remove_remote")), cx, move |r| r.remove_remote(&name)),
            ),
        }
    }

    // ---- File ----

    pub(crate) fn on_file_action(&mut self, a: &OnFile, window: &mut Window, cx: &mut Context<Self>) {
        let path = a.path.clone();
        let rel = self.rel_path(&path);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match a.op {
            FileOp::NewFile | FileOp::NewFolder => {
                let dir = if path.is_dir() {
                    path
                } else {
                    path.parent().map(Path::to_path_buf).unwrap_or_else(|| self.repo.workdir().to_path_buf())
                };
                if a.op == FileOp::NewFile {
                    self.open_prompt(PromptKind::NewFile { dir }, t!("commands.prompt.new_file"), "", window, cx);
                } else {
                    self.open_prompt(PromptKind::NewFolder { dir }, t!("commands.prompt.new_folder"), "", window, cx);
                }
            }
            FileOp::Stage => self.stage_paths(vec![rel], cx),
            FileOp::Unstage => self.unstage_paths(vec![rel], cx),
            FileOp::Discard => {
                let untracked = self
                    .status
                    .entries
                    .iter()
                    .any(|e| e.path == rel && e.kind == branchpad_core::git::EntryKind::Untracked);
                self.discard_paths(vec![rel], untracked, window, cx);
            }
            FileOp::Rename => self.open_prompt(PromptKind::RenameFile { path }, t!("commands.prompt.new_name"), &name, window, cx),
            FileOp::Delete => confirm_then(
                window,
                cx,
                &t!("commands.delete_named.title", name = name),
                t!("commands.delete_file.detail"),
                t!("commands.delete"),
                move |this, cx| {
                    match file_tree::delete(&path) {
                        Ok(()) => {
                            if let Some(tree) = this.tree.as_mut() {
                                tree.refresh_paths(&this.repo, [&path]);
                            }
                        }
                        Err(e) => this.error = Some(t!("commands.error.delete_failed", path = path.display(), error = e)),
                    }
                    cx.notify();
                },
            ),
            FileOp::RevealInFinder => {
                if let Err(e) = file_tree::reveal_in_finder(&path) {
                    self.error = Some(e.to_string());
                    cx.notify();
                }
            }
            FileOp::CopyPath => {
                cx.write_to_clipboard(ClipboardItem::new_string(path.to_string_lossy().into_owned()));
                self.info = Some(t!("commands.done.copy_path", rel = rel));
                cx.notify();
            }
            FileOp::History => {
                let r2 = rel.clone();
                self.spawn_git(
                    Some(t!("commands.busy.file_history")),
                    cx,
                    move |r| {
                        r.log(&LogQuery {
                            skip: 0,
                            limit: 2000,
                            path: Some(r2),
                            ..Default::default()
                        })
                    },
                    move |this, commits, _| this.detail = Detail::FileHistory { path: rel, commits },
                );
            }
            FileOp::Blame => {
                let r2 = rel.clone();
                self.spawn_git(
                    Some(t!("commands.busy.blame")),
                    cx,
                    move |r| r.blame(&r2, None),
                    move |this, lines, _| this.detail = Detail::Blame { path: rel, lines },
                );
            }
            FileOp::OpenInEditor => self.open_in_editor(path, None, window, cx),
        }
    }

    // ---- Ô nhập ----

    pub(super) fn handle_prompt(&mut self, kind: PromptKind, value: String, window: &mut Window, cx: &mut Context<Self>) {
        let v = value.trim().to_string();
        let needs_value = !matches!(kind, PromptKind::Stash);
        if needs_value && v.is_empty() {
            return;
        }
        match kind {
            PromptKind::CreateBranch { start } => {
                let msg = t!("commands.done.create_branch", v = v);
                self.run_op(t!("commands.busy.create_branch"), Some(&msg), cx, move |r| r.create_branch(&v, Some(&start), true));
            }
            PromptKind::CreateTag { rev } => {
                let (name, message) = match v.split_once(" : ") {
                    Some((n, m)) => (n.trim().to_string(), Some(m.trim().to_string())),
                    None => (v, None),
                };
                let msg = t!("commands.done.create_tag", name = name);
                self.run_op(t!("commands.busy.create_tag"), Some(&msg), cx, move |r| r.create_tag(&name, &rev, message.as_deref()));
            }
            PromptKind::RenameBranch { old } => {
                self.run_op(t!("commands.busy.rename_branch"), Some(t!("commands.done.rename_branch")), cx, move |r| r.rename_branch(&old, &v))
            }
            PromptKind::TrackBranch { remote_branch } => {
                let msg = t!("commands.done.track_branch", v = v, remote_branch = remote_branch);
                self.run_op(t!("commands.busy.create_branch"), Some(&msg), cx, move |r| r.track_remote_branch(&remote_branch, &v));
            }
            PromptKind::AddRemote => {
                let mut parts = v.split_whitespace();
                match (parts.next(), parts.next()) {
                    (Some(name), Some(url)) => {
                        let (name, url) = (name.to_string(), url.to_string());
                        let msg = t!("commands.done.add_remote", name = name);
                        self.run_op(t!("commands.busy.add_remote"), Some(&msg), cx, move |r| {
                            r.add_remote(&name, &url)?;
                            r.fetch_remote(&name)
                        });
                    }
                    _ => {
                        self.error = Some(t!("commands.error.add_remote_format").into());
                    }
                }
            }
            PromptKind::NewFile { dir } => match file_tree::create_file(&dir, &v) {
                Ok(path) => {
                    if let Some(tree) = self.tree.as_mut() {
                        tree.refresh_paths(&self.repo, [&path]);
                    }
                    self.open_in_editor(path, None, window, cx);
                }
                Err(e) => self.error = Some(t!("commands.error.create_file", error = e)),
            },
            PromptKind::NewFolder { dir } => match file_tree::create_dir(&dir, &v) {
                Ok(path) => {
                    if let Some(tree) = self.tree.as_mut() {
                        tree.refresh_paths(&self.repo, [&path]);
                    }
                }
                Err(e) => self.error = Some(t!("commands.error.create_folder", error = e)),
            },
            PromptKind::RenameFile { path } => match file_tree::rename(&path, &v) {
                Ok(new_path) => {
                    if let Some(tree) = self.tree.as_mut() {
                        tree.refresh_paths(&self.repo, [&path, &new_path]);
                    }
                }
                Err(e) => self.error = Some(t!("commands.error.rename", error = e)),
            },
            PromptKind::Stash => {
                self.run_op(t!("commands.busy.stash"), Some(t!("commands.done.stash")), cx, move |r| r.stash_push(&v))
            }
            PromptKind::AddWorktree => {
                let mut parts = v.split_whitespace();
                let Some(p) = parts.next() else { return };
                let target = self.repo.workdir().join(p);
                let target = target.to_string_lossy().into_owned();
                let branch = parts.next().map(str::to_string);
                let msg = t!("commands.done.add_worktree", target = target);
                self.run_op(t!("commands.busy.add_worktree"), Some(&msg), cx, move |r| {
                    r.add_worktree(&target, "HEAD", branch.as_deref())
                });
            }
            PromptKind::GotoLine => match v.parse::<u32>() {
                Ok(line) if line > 0 => {
                    self.editor.update(cx, |e, cx| e.goto_line(line, window, cx));
                }
                _ => self.error = Some(t!("commands.error.not_a_line", v = v)),
            },
            PromptKind::Reword(i) => {
                if let Detail::Rebase(view) = &mut self.detail {
                    if let Some(item) = view.items.get_mut(i) {
                        item.message = Some(value.trim_end().to_string());
                        item.action = branchpad_core::git::TodoAction::Reword;
                    }
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn prompt_stash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_prompt(PromptKind::Stash, t!("commands.prompt.stash_message"), "", window, cx);
    }

    pub(crate) fn apply_patch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("commands.patch.apply_prompt").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(file) = paths.into_iter().next() {
                    this.update(cx, |this, cx| {
                        let name = file
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let msg = t!("commands.done.apply_patch", name = name);
                        this.run_op(t!("commands.busy.apply_patch"), Some(&msg), cx, move |r| r.apply_patch_file(&file));
                    })
                    .ok();
                }
            }
        })
        .detach();
    }
}
