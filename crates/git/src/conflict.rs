//! Đọc và giải quyết khối xung đột (`<<<<<<<` / `|||||||` / `=======` / `>>>>>>>`).

use crate::runner::{Repo, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictBlock {
    pub ours: String,
    pub base: Option<String>,
    pub theirs: String,
    pub ours_label: String,
    pub base_label: String,
    pub theirs_label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictPart {
    Common(String),
    Conflict(ConflictBlock),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Ours,
    Theirs,
    OursThenTheirs,
    TheirsThenOurs,
}

fn marker<'a>(line: &'a str, m: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(m)?;
    let rest = rest.trim_end_matches(['\n', '\r']);
    if rest.is_empty() {
        Some("")
    } else {
        rest.strip_prefix(' ')
    }
}

enum Section {
    Ours,
    Base,
    Theirs,
}

/// Tách file thành phần chung và khối xung đột. File không có xung đột trả về
/// một phần `Common`. Khối mở mà không đóng được giữ nguyên dạng chữ.
pub fn parse_conflicts(text: &str) -> Vec<ConflictPart> {
    let mut parts = Vec::new();
    let mut common = String::new();
    let mut block: Option<(ConflictBlock, Section, String)> = None;

    for line in text.split_inclusive('\n') {
        if let Some((b, section, raw)) = block.as_mut() {
            raw.push_str(line);
            if let Some(label) = marker(line, "|||||||") {
                b.base = Some(String::new());
                b.base_label = label.to_string();
                *section = Section::Base;
            } else if marker(line, "=======").is_some() {
                *section = Section::Theirs;
            } else if let Some(label) = marker(line, ">>>>>>>") {
                b.theirs_label = label.to_string();
                let (b, _, _) = block.take().expect("đang trong khối");
                if !common.is_empty() {
                    parts.push(ConflictPart::Common(std::mem::take(&mut common)));
                }
                parts.push(ConflictPart::Conflict(b));
            } else {
                match section {
                    Section::Ours => b.ours.push_str(line),
                    Section::Base => b.base.get_or_insert_with(String::new).push_str(line),
                    Section::Theirs => b.theirs.push_str(line),
                }
            }
            continue;
        }
        if let Some(label) = marker(line, "<<<<<<<") {
            block = Some((
                ConflictBlock {
                    ours: String::new(),
                    base: None,
                    theirs: String::new(),
                    ours_label: label.to_string(),
                    base_label: String::new(),
                    theirs_label: String::new(),
                },
                Section::Ours,
                line.to_string(),
            ));
        } else {
            common.push_str(line);
        }
    }
    if let Some((_, _, raw)) = block {
        common.push_str(&raw);
    }
    if !common.is_empty() {
        parts.push(ConflictPart::Common(common));
    }
    parts
}

fn ensure_newline(s: &str) -> String {
    if s.is_empty() || s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// Viết lại khối xung đột chưa giải quyết.
pub fn render_markers(b: &ConflictBlock) -> String {
    let mut s = format!("<<<<<<< {}\n", b.ours_label);
    s.push_str(&ensure_newline(&b.ours));
    if let Some(base) = &b.base {
        s.push_str(&format!("||||||| {}\n", b.base_label));
        s.push_str(&ensure_newline(base));
    }
    s.push_str("=======\n");
    s.push_str(&ensure_newline(&b.theirs));
    s.push_str(&format!(">>>>>>> {}\n", b.theirs_label));
    s
}

/// Ghép lại file. `choices[i]` ứng với khối xung đột thứ i; `None` = giữ dấu xung đột.
pub fn resolve(parts: &[ConflictPart], choices: &[Option<Resolution>]) -> String {
    let mut out = String::new();
    let mut i = 0;
    for part in parts {
        match part {
            ConflictPart::Common(c) => out.push_str(c),
            ConflictPart::Conflict(b) => {
                match choices.get(i).copied().flatten() {
                    Some(Resolution::Ours) => out.push_str(&b.ours),
                    Some(Resolution::Theirs) => out.push_str(&b.theirs),
                    Some(Resolution::OursThenTheirs) => {
                        out.push_str(&ensure_newline(&b.ours));
                        out.push_str(&b.theirs);
                    }
                    Some(Resolution::TheirsThenOurs) => {
                        out.push_str(&ensure_newline(&b.theirs));
                        out.push_str(&b.ours);
                    }
                    None => out.push_str(&render_markers(b)),
                }
                i += 1;
            }
        }
    }
    out
}

pub fn conflict_count(parts: &[ConflictPart]) -> usize {
    parts
        .iter()
        .filter(|p| matches!(p, ConflictPart::Conflict(_)))
        .count()
}

impl Repo {
    /// Đánh dấu file đã xử lý xung đột.
    pub fn mark_resolved(&self, path: &str) -> Result<()> {
        self.run(&["add", "--", path]).map(drop)
    }

    /// Lấy nguyên bản của một phía cho cả file.
    pub fn checkout_side(&self, path: &str, theirs: bool) -> Result<()> {
        self.run(&[
            "checkout",
            if theirs { "--theirs" } else { "--ours" },
            "--",
            path,
        ])
        .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "a\n<<<<<<< HEAD\nours\n||||||| base\norig\n=======\ntheirs\n>>>>>>> feature\nz\n";

    #[test]
    fn parses_diff3_block() {
        let p = parse_conflicts(TEXT);
        assert_eq!(p.len(), 3);
        let ConflictPart::Conflict(b) = &p[1] else { panic!() };
        assert_eq!(b.ours, "ours\n");
        assert_eq!(b.base.as_deref(), Some("orig\n"));
        assert_eq!(b.theirs, "theirs\n");
        assert_eq!(b.theirs_label, "feature");
    }

    #[test]
    fn resolves_and_roundtrips() {
        let p = parse_conflicts(TEXT);
        assert_eq!(resolve(&p, &[Some(Resolution::Theirs)]), "a\ntheirs\nz\n");
        assert_eq!(resolve(&p, &[Some(Resolution::OursThenTheirs)]), "a\nours\ntheirs\nz\n");
        assert_eq!(resolve(&p, &[None]), TEXT);
    }

    #[test]
    fn unterminated_block_kept() {
        let t = "x\n<<<<<<< HEAD\ny\n";
        assert_eq!(parse_conflicts(t), vec![ConflictPart::Common(t.into())]);
    }
}
