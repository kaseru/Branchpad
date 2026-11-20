//! Rebase tương tác: đổi thứ tự, gộp, sửa message, bỏ commit.

use crate::runner::{shell_quote, GitError, Repo, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TodoAction {
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
}

impl TodoAction {
    pub const ALL: [TodoAction; 5] = [
        TodoAction::Pick,
        TodoAction::Reword,
        TodoAction::Squash,
        TodoAction::Fixup,
        TodoAction::Drop,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TodoAction::Pick => "Giữ",
            TodoAction::Reword => "Sửa message",
            TodoAction::Squash => "Gộp",
            TodoAction::Fixup => "Gộp, bỏ message",
            TodoAction::Drop => "Bỏ",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|a| *a == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoItem {
    pub action: TodoAction,
    pub id: String,
    pub summary: String,
    /// Message mới khi `Reword`.
    pub message: Option<String>,
}

/// Dựng nội dung `git-rebase-todo`. `msg_file(i)` trả về đường dẫn file message
/// mới cho mục `i` (chỉ gọi với mục Reword có message).
pub fn build_todo(items: &[TodoItem], msg_file: impl Fn(usize) -> String) -> Result<String> {
    let first_kept = items.iter().find(|i| i.action != TodoAction::Drop);
    if matches!(
        first_kept.map(|i| i.action),
        Some(TodoAction::Squash | TodoAction::Fixup)
    ) {
        return Err(GitError::Invalid(
            "Commit đầu tiên không gộp được vào commit trước nó".into(),
        ));
    }
    let mut todo = String::new();
    for (i, item) in items.iter().enumerate() {
        let line = match item.action {
            TodoAction::Pick | TodoAction::Reword => "pick",
            TodoAction::Squash => "squash",
            TodoAction::Fixup => "fixup",
            TodoAction::Drop => "drop",
        };
        todo.push_str(&format!("{line} {}\n", item.id));
        if item.action == TodoAction::Reword && item.message.is_some() {
            todo.push_str(&format!(
                "exec git commit --amend --no-verify -F {}\n",
                shell_quote(&msg_file(i))
            ));
        }
    }
    Ok(todo)
}

impl Repo {
    /// Các commit sẽ được rebase lại (cũ trước), từ sau `base` tới HEAD.
    /// `base = None`: từ commit gốc.
    pub fn rebase_items(&self, base: Option<&str>) -> Result<Vec<TodoItem>> {
        let range = match base {
            Some(b) => format!("{b}..HEAD"),
            None => "HEAD".to_string(),
        };
        let text = self.run(&[
            "log",
            "--reverse",
            "--no-merges",
            "--format=%H%x00%s",
            &range,
        ])?;
        Ok(text
            .lines()
            .filter_map(|l| l.split_once('\0'))
            .map(|(id, summary)| TodoItem {
                action: TodoAction::Pick,
                id: id.to_string(),
                summary: summary.to_string(),
                message: None,
            })
            .collect())
    }

    pub fn rebase_interactive(&self, base: Option<&str>, items: &[TodoItem]) -> Result<()> {
        let dir = self.scratch_dir()?;
        for (i, item) in items.iter().enumerate() {
            if let (TodoAction::Reword, Some(m)) = (item.action, &item.message) {
                std::fs::write(dir.join(format!("reword-{i}.txt")), m)?;
            }
        }
        let todo = build_todo(items, |i| {
            dir.join(format!("reword-{i}.txt"))
                .to_string_lossy()
                .into_owned()
        })?;
        self.rebase_with_todo(base, &todo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(action: TodoAction, id: &str) -> TodoItem {
        TodoItem {
            action,
            id: id.into(),
            summary: String::new(),
            message: None,
        }
    }

    #[test]
    fn builds_todo_with_reword() {
        let mut r = item(TodoAction::Reword, "b");
        r.message = Some("new".into());
        let todo = build_todo(
            &[item(TodoAction::Pick, "a"), r, item(TodoAction::Fixup, "c"), item(TodoAction::Drop, "d")],
            |i| format!("/m/{i}"),
        )
        .unwrap();
        assert_eq!(
            todo,
            "pick a\npick b\nexec git commit --amend --no-verify -F '/m/1'\nfixup c\ndrop d\n"
        );
    }

    #[test]
    fn rejects_leading_squash() {
        assert!(build_todo(&[item(TodoAction::Drop, "a"), item(TodoAction::Squash, "b")], |_| String::new()).is_err());
    }
}
