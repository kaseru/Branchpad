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
                let msg = format!("Đã checkout {short} (detached HEAD)");
                self.run_op("Đang checkout…", Some(&msg), cx, move |r| r.checkout_detached(&id));
            }
            CommitOp::CreateBranch => {
                self.open_prompt(PromptKind::CreateBranch { start: id }, &format!("Tên nhánh mới tại {short}"), "", window, cx)
            }
            CommitOp::CreateTag => self.open_prompt(
                PromptKind::CreateTag { rev: id },
                &format!("Tên tag tại {short} (thêm ' : chú thích' nếu muốn)"),
                "",
                window,
                cx,
            ),
            CommitOp::CherryPick => {
                self.run_op("Đang cherry-pick…", Some("Đã cherry-pick"), cx, move |r| r.cherry_pick(&id))
            }
            CommitOp::Revert => {
                let is_merge = self
                    .commits
                    .iter()
                    .find(|c| c.id == id)
                    .is_some_and(|c| c.parents.len() > 1);
                self.run_op("Đang revert…", Some("Đã revert"), cx, move |r| r.revert(&id, is_merge))
            }
            CommitOp::ResetSoft => {
                self.run_op("Đang reset…", Some("Đã reset (soft)"), cx, move |r| r.reset(&id, ResetMode::Soft))
            }
            CommitOp::ResetMixed => {
                self.run_op("Đang reset…", Some("Đã reset (mixed)"), cx, move |r| r.reset(&id, ResetMode::Mixed))
            }
            CommitOp::ResetHard => confirm_then(
                window,
                cx,
                &format!("Reset hard về {short}?"),
                "Mọi thay đổi chưa commit và các commit sau đó trên nhánh hiện tại sẽ mất.",
                "Reset hard",
                move |this, cx| {
                    this.run_op("Đang reset…", Some("Đã reset (hard)"), cx, move |r| r.reset(&id, ResetMode::Hard))
                },
            ),
            CommitOp::FormatPatch => {
                let rx = cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some("Lưu patch vào thư mục".into()),
                });
                cx.spawn_in(window, async move |this, cx| {
                    if let Ok(Ok(Some(paths))) = rx.await {
                        if let Some(dir) = paths.into_iter().next() {
                            this.update(cx, |this, cx| {
                                this.spawn_git(
                                    Some("Đang tạo patch…"),
                                    cx,
                                    move |r| r.format_patch(&id, &dir),
                                    |this, file, _| {
                                        this.info = Some(format!("Đã tạo {}", file.display()));
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
                self.info = Some(format!("Đã copy {short}"));
                cx.notify();
            }
            CommitOp::RebaseInteractive => {
                if self.current_branch().is_none() {
                    self.error = Some("Hãy checkout một nhánh trước khi rebase".into());
                    cx.notify();
                    return;
                }
                let base = id.clone();
                self.spawn_git(
                    Some("Đang đọc danh sách commit…"),
                    cx,
                    move |r| r.rebase_items(Some(&base)),
                    move |this, items, _| {
                        if items.is_empty() {
                            this.error = Some("Không có commit nào sau commit này trên nhánh hiện tại".into());
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
                let msg = format!("Đã checkout {name}");
                self.run_op("Đang checkout…", Some(&msg), cx, move |r| {
                    if detached {
                        r.checkout_detached(&name)
                    } else {
                        r.checkout(&name)
                    }
                });
            }
            RefOp::Merge => {
                let msg = format!("Đã merge {name}");
                self.run_op("Đang merge…", Some(&msg), cx, move |r| r.merge(&name, false));
            }
            RefOp::Rebase => {
                let msg = format!("Đã rebase lên {name}");
                self.run_op("Đang rebase…", Some(&msg), cx, move |r| r.rebase_onto(&name));
            }
            RefOp::Rename => self.open_prompt(PromptKind::RenameBranch { old: name.clone() }, "Tên mới", &name, window, cx),
            RefOp::Delete => confirm(
                window,
                cx,
                &format!("Xoá nhánh {name}?"),
                "\"Xoá\" chỉ xoá khi nhánh đã được merge. \"Xoá bắt buộc\" xoá luôn cả commit chưa merge.",
                &["Huỷ", "Xoá", "Xoá bắt buộc"],
                move |this: &mut Self, answer, _, cx| {
                    let force = answer == 2;
                    this.run_op("Đang xoá nhánh…", Some("Đã xoá nhánh"), cx, move |r| r.delete_branch(&name, force));
                },
            ),
            RefOp::Push => self.push_branch(name, false, cx),
            RefOp::ForcePush => confirm_then(
                window,
                cx,
                &format!("Push bắt buộc {name}?"),
                "Ghi đè nhánh trên remote (an toàn: chỉ khi remote chưa có commit mới từ người khác).",
                "Push bắt buộc",
                move |this, cx| this.push_branch(name, true, cx),
            ),
            RefOp::Track => {
                let local = name.split_once('/').map(|(_, b)| b.to_string()).unwrap_or_else(|| name.clone());
                self.open_prompt(
                    PromptKind::TrackBranch { remote_branch: name.clone() },
                    &format!("Tên nhánh local theo dõi {name}"),
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
                    &format!("Xoá nhánh {branch} trên {remote}?"),
                    "Nhánh sẽ bị xoá trên máy chủ cho mọi người.",
                    "Xoá trên remote",
                    move |this, cx| {
                        this.run_op("Đang xoá nhánh remote…", Some("Đã xoá nhánh remote"), cx, move |r| {
                            r.delete_remote_branch(&remote, &branch)
                        })
                    },
                );
            }
            RefOp::TagPush => {
                let Some(remote) = self.default_remote() else {
                    self.error = Some("Repo chưa có remote nào".into());
                    cx.notify();
                    return;
                };
                let msg = format!("Đã push tag {name} lên {remote}");
                self.run_op("Đang push tag…", Some(&msg), cx, move |r| r.push_tag(&remote, &name));
            }
            RefOp::TagDelete => {
                let remote = self.default_remote();
                let mut buttons = vec!["Huỷ", "Xoá ở máy"];
                if remote.is_some() {
                    buttons.push("Xoá cả trên remote");
                }
                confirm(
                    window,
                    cx,
                    &format!("Xoá tag {name}?"),
                    "Tag trên remote chỉ bị xoá nếu chọn \"Xoá cả trên remote\".",
                    &buttons,
                    move |this: &mut Self, answer, _, cx| {
                        this.run_op("Đang xoá tag…", Some("Đã xoá tag"), cx, move |r| {
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
                self.run_op("Đang áp dụng stash…", Some("Đã áp dụng stash"), cx, move |r| r.stash_apply(&name))
            }
            RefOp::StashPop => {
                self.detail = Detail::Empty;
                self.run_op("Đang pop stash…", Some("Đã áp dụng và xoá stash"), cx, move |r| r.stash_pop(&name))
            }
            RefOp::StashDrop => confirm_then(
                window,
                cx,
                &format!("Xoá {name}?"),
                "Thay đổi trong stash sẽ mất.",
                "Xoá stash",
                move |this, cx| {
                    this.detail = Detail::Empty;
                    this.run_op("Đang xoá stash…", Some("Đã xoá stash"), cx, move |r| r.stash_drop(&name))
                },
            ),
            RefOp::WorktreeOpen => cx.emit(RepoViewEvent::OpenRepository(PathBuf::from(name))),
            RefOp::WorktreeRemove => confirm(
                window,
                cx,
                &format!("Xoá worktree {name}?"),
                "\"Xoá bắt buộc\" xoá cả khi worktree còn thay đổi chưa commit.",
                &["Huỷ", "Xoá", "Xoá bắt buộc"],
                move |this: &mut Self, answer, _, cx| {
                    let force = answer == 2;
                    this.run_op("Đang xoá worktree…", Some("Đã xoá worktree"), cx, move |r| {
                        r.remove_worktree(&name, force)
                    });
                },
            ),
            RefOp::RemoteFetch => {
                let msg = format!("Đã fetch {name}");
                self.run_op("Đang fetch…", Some(&msg), cx, move |r| r.fetch_remote(&name));
            }
            RefOp::RemoteRemove => confirm_then(
                window,
                cx,
                &format!("Xoá remote {name}?"),
                "Chỉ xoá cấu hình remote trên máy, không đụng tới máy chủ.",
                "Xoá remote",
                move |this, cx| this.run_op("Đang xoá remote…", Some("Đã xoá remote"), cx, move |r| r.remove_remote(&name)),
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
                    self.open_prompt(PromptKind::NewFile { dir }, "Tên file mới", "", window, cx);
                } else {
                    self.open_prompt(PromptKind::NewFolder { dir }, "Tên thư mục mới", "", window, cx);
                }
            }
            FileOp::Rename => self.open_prompt(PromptKind::RenameFile { path }, "Tên mới", &name, window, cx),
            FileOp::Delete => confirm_then(
                window,
                cx,
                &format!("Xoá {name}?"),
                "File/thư mục được chuyển vào Thùng rác.",
                "Xoá",
                move |this, cx| {
                    match file_tree::delete(&path) {
                        Ok(()) => {
                            if let Some(tree) = this.tree.as_mut() {
                                tree.refresh_paths(&this.repo, [&path]);
                            }
                        }
                        Err(e) => this.error = Some(format!("Không xoá được {}: {e}", path.display())),
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
                self.info = Some(format!("Đã copy đường dẫn {rel}"));
                cx.notify();
            }
            FileOp::History => {
                let r2 = rel.clone();
                self.spawn_git(
                    Some("Đang tải lịch sử file…"),
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
                    Some("Đang blame…"),
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
                let msg = format!("Đã tạo và checkout nhánh {v}");
                self.run_op("Đang tạo nhánh…", Some(&msg), cx, move |r| r.create_branch(&v, Some(&start), true));
            }
            PromptKind::CreateTag { rev } => {
                let (name, message) = match v.split_once(" : ") {
                    Some((n, m)) => (n.trim().to_string(), Some(m.trim().to_string())),
                    None => (v, None),
                };
                let msg = format!("Đã tạo tag {name}");
                self.run_op("Đang tạo tag…", Some(&msg), cx, move |r| r.create_tag(&name, &rev, message.as_deref()));
            }
            PromptKind::RenameBranch { old } => {
                self.run_op("Đang đổi tên nhánh…", Some("Đã đổi tên nhánh"), cx, move |r| r.rename_branch(&old, &v))
            }
            PromptKind::TrackBranch { remote_branch } => {
                let msg = format!("Đã tạo nhánh {v} theo dõi {remote_branch}");
                self.run_op("Đang tạo nhánh…", Some(&msg), cx, move |r| r.track_remote_branch(&remote_branch, &v));
            }
            PromptKind::AddRemote => {
                let mut parts = v.split_whitespace();
                match (parts.next(), parts.next()) {
                    (Some(name), Some(url)) => {
                        let (name, url) = (name.to_string(), url.to_string());
                        let msg = format!("Đã thêm remote {name}");
                        self.run_op("Đang thêm remote…", Some(&msg), cx, move |r| {
                            r.add_remote(&name, &url)?;
                            r.fetch_remote(&name)
                        });
                    }
                    _ => {
                        self.error = Some("Nhập tên và URL cách nhau bởi dấu cách".into());
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
                Err(e) => self.error = Some(format!("Không tạo được file: {e}")),
            },
            PromptKind::NewFolder { dir } => match file_tree::create_dir(&dir, &v) {
                Ok(path) => {
                    if let Some(tree) = self.tree.as_mut() {
                        tree.refresh_paths(&self.repo, [&path]);
                    }
                }
                Err(e) => self.error = Some(format!("Không tạo được thư mục: {e}")),
            },
            PromptKind::RenameFile { path } => match file_tree::rename(&path, &v) {
                Ok(new_path) => {
                    if let Some(tree) = self.tree.as_mut() {
                        tree.refresh_paths(&self.repo, [&path, &new_path]);
                    }
                }
                Err(e) => self.error = Some(format!("Không đổi tên được: {e}")),
            },
            PromptKind::Stash => {
                self.run_op("Đang stash…", Some("Đã cất thay đổi vào stash"), cx, move |r| r.stash_push(&v))
            }
            PromptKind::AddWorktree => {
                let mut parts = v.split_whitespace();
                let Some(p) = parts.next() else { return };
                let target = self.repo.workdir().join(p);
                let target = target.to_string_lossy().into_owned();
                let branch = parts.next().map(str::to_string);
                let msg = format!("Đã thêm worktree {target}");
                self.run_op("Đang thêm worktree…", Some(&msg), cx, move |r| {
                    r.add_worktree(&target, "HEAD", branch.as_deref())
                });
            }
            PromptKind::GotoLine => match v.parse::<u32>() {
                Ok(line) if line > 0 => {
                    self.editor.update(cx, |e, cx| e.goto_line(line, window, cx));
                }
                _ => self.error = Some(format!("\"{v}\" không phải số dòng")),
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
        self.open_prompt(PromptKind::Stash, "Ghi chú cho stash (có thể bỏ trống)", "", window, cx);
    }

    pub(crate) fn apply_patch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Áp patch".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(file) = paths.into_iter().next() {
                    this.update(cx, |this, cx| {
                        let name = file
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let msg = format!("Đã áp {name}");
                        this.run_op("Đang áp patch…", Some(&msg), cx, move |r| r.apply_patch_file(&file));
                    })
                    .ok();
                }
            }
        })
        .detach();
    }
}
