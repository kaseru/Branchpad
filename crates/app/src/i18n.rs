//! UI text by key. Strings live in `locales/<lang>/<module>.json` (flat `"key": "text"`),
//! embedded at build time.
//!
//! Use `t!("module.key")` for plain text, `t!("module.key", name = value, n = 3)` to fill
//! `{name}` / `{n}` placeholders. A key missing in the active language falls back to English,
//! then to the key itself.
//!
//! Add a language: copy `locales/en/` to `locales/<code>/`, translate the values, and add one
//! line to `LANGUAGES` below. Pick it with the `BRANCHPAD_LANG` env var (e.g. `BRANCHPAD_LANG=vi`).

use std::collections::HashMap;
use std::sync::OnceLock;

macro_rules! locale_files {
    ($lang:literal: $($module:literal),* $(,)?) => {
        &[$(include_str!(concat!("../locales/", $lang, "/", $module, ".json"))),*]
    };
}

/// (language code, JSON files of that language).
const LANGUAGES: &[(&str, &[&str])] = &[(
    "en",
    locale_files!("en": "commands", "detail", "history", "repo", "overlay", "sidebar", "workspace", "actions", "editor", "common", "time", "state", "settings"),
)];

const FALLBACK: &str = "en";

type Table = HashMap<String, &'static str>;

fn load(files: &[&str]) -> Table {
    let mut table = Table::new();
    for json in files {
        match serde_json::from_str::<HashMap<String, String>>(json) {
            // Leaked once so lookups can hand out `&'static str`.
            Ok(map) => table.extend(map.into_iter().map(|(k, v)| (k, &*Box::leak(v.into_boxed_str())))),
            Err(e) => eprintln!("branchpad: invalid locale file: {e}"),
        }
    }
    table
}

struct Tables {
    active: Table,
    fallback: Table,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let find = |code: &str| LANGUAGES.iter().find(|(c, _)| *c == code).map(|(_, f)| load(f));
        let wanted = std::env::var("BRANCHPAD_LANG").unwrap_or_else(|_| FALLBACK.into());
        let active = find(&wanted).unwrap_or_else(|| {
            eprintln!("branchpad: unknown language {wanted:?}, using {FALLBACK}");
            find(FALLBACK).unwrap_or_default()
        });
        Tables { active, fallback: find(FALLBACK).unwrap_or_default() }
    })
}

/// Text for `key` in the active language.
pub fn tr(key: &'static str) -> &'static str {
    let t = tables();
    t.active.get(key).or_else(|| t.fallback.get(key)).copied().unwrap_or_else(|| {
        eprintln!("branchpad: missing translation key {key:?}");
        key
    })
}

/// Like [`tr`], replacing each `{name}` with its value.
pub fn tr_args(key: &'static str, args: &[(&str, String)]) -> String {
    let mut text = tr(key).to_string();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// Banner text for a repo in the middle of a multi-step operation.
pub fn repo_state_label(state: branchpad_core::git::RepoState) -> &'static str {
    use branchpad_core::git::RepoState as S;
    match state {
        S::Clean => "",
        S::Merging => crate::t!("state.merging"),
        S::Rebasing { .. } => crate::t!("state.rebasing"),
        S::ApplyingMailbox => crate::t!("state.applying_patch"),
        S::CherryPicking => crate::t!("state.cherry_picking"),
        S::Reverting => crate::t!("state.reverting"),
    }
}

/// Label of an interactive-rebase action.
pub fn todo_action_label(action: branchpad_core::git::TodoAction) -> &'static str {
    use branchpad_core::git::TodoAction as A;
    match action {
        A::Pick => crate::t!("rebase.action.pick"),
        A::Reword => crate::t!("rebase.action.reword"),
        A::Squash => crate::t!("rebase.action.squash"),
        A::Fixup => crate::t!("rebase.action.fixup"),
        A::Drop => crate::t!("rebase.action.drop"),
    }
}

/// "3 min ago" in the active language.
pub fn relative_time(unix: i64, now: i64) -> String {
    use branchpad_core::{relative_time_parts, RelativeUnit};
    match relative_time_parts(unix, now) {
        (RelativeUnit::JustNow, _) => crate::t!("time.just_now").to_string(),
        (RelativeUnit::Minutes, n) => crate::t!("time.minutes_ago", n = n),
        (RelativeUnit::Hours, n) => crate::t!("time.hours_ago", n = n),
        (RelativeUnit::Days, n) => crate::t!("time.days_ago", n = n),
        (RelativeUnit::Months, n) => crate::t!("time.months_ago", n = n),
        (RelativeUnit::Years, n) => crate::t!("time.years_ago", n = n),
    }
}

#[macro_export]
macro_rules! t {
    ($key:literal) => { $crate::i18n::tr($key) };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::tr_args($key, &[$((stringify!($name), ($value).to_string())),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `t!("key")` used in the source must exist in the fallback language,
    /// and every language must define the same keys as English.
    #[test]
    fn keys_are_complete() {
        let en = load(LANGUAGES.iter().find(|(c, _)| *c == FALLBACK).unwrap().1);
        let mut used = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") && path.file_name().is_some_and(|n| n != "i18n.rs") {
                    let src = std::fs::read_to_string(&path).unwrap();
                    for (at, _) in src.match_indices("t!(\"") {
                        // Skip `format!("`, `assert!(` and similar macros ending in `t!`.
                        let prev = src[..at].chars().next_back();
                        if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                            continue;
                        }
                        if let Some(key) = src[at + 4..].split('"').next() {
                            used.push((path.clone(), key.to_string()));
                        }
                    }
                }
            }
        }
        let missing: Vec<_> = used.iter().filter(|(_, k)| !en.contains_key(k)).collect();
        assert!(missing.is_empty(), "keys missing in en: {missing:?}");
        for (code, files) in LANGUAGES {
            let table = load(files);
            let lacking: Vec<_> = en.keys().filter(|k| !table.contains_key(*k)).collect();
            assert!(lacking.is_empty(), "language {code} lacks keys: {lacking:?}");
        }
    }
}
