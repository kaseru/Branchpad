//! Icon riêng của app (nhúng sẵn) + icon theo loại file; còn lại dùng bộ của gpui-component.

use std::borrow::Cow;

use gpui::{hsla, AssetSource, Hsla, Result, SharedString};
use gpui_component::{Icon, IconName};

macro_rules! bp_icons {
    ($($variant:ident => $file:literal),* $(,)?) => {
        #[derive(Clone, Copy)]
        pub enum BpIcon { $($variant),* }

        impl BpIcon {
            fn path(self) -> &'static str {
                match self { $(BpIcon::$variant => concat!("icons/bp/", $file, ".svg")),* }
            }
        }

        fn embedded(path: &str) -> Option<&'static str> {
            match path {
                $(concat!("icons/bp/", $file, ".svg") => Some(include_str!(concat!("../assets/icons/bp/", $file, ".svg"))),)*
                _ => None,
            }
        }
    };
}

bp_icons! {
    Fetch => "fetch",
    Pull => "pull",
    Push => "push",
    Stash => "stash",
    Patch => "patch",
    Refresh => "refresh",
    Branch => "branch",
    Undo => "undo",
    Settings => "settings",
    FileCode => "file-code",
    FileText => "file-text",
    FileBraces => "file-braces",
    FileImage => "file-image",
    FileLock => "file-lock",
    FileCog => "file-cog",
    FileTerminal => "file-terminal",
    Sparkles => "sparkles",
    Claude => "claude",
    Codex => "codex",
    Agy => "agy",
    History => "history",
}

impl From<BpIcon> for Icon {
    fn from(i: BpIcon) -> Self {
        Icon::empty().path(i.path())
    }
}

/// Nguồn asset: icon của app trước, không có thì dùng bộ của gpui-component.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match embedded(path) {
            Some(svg) => Ok(Some(Cow::Borrowed(svg.as_bytes()))),
            None => gpui_component_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_component_assets::Assets.list(path)
    }
}

fn color(h: f32) -> Hsla {
    hsla(h, 0.55, 0.58, 1.0)
}

/// Icon + màu đại diện cho một thư mục / file theo đuôi.
pub fn file_icon(name: &str, is_dir: bool, expanded: bool) -> (Icon, Hsla) {
    if is_dir {
        let i = if expanded { IconName::FolderOpen } else { IconName::FolderClosed };
        return (i.into(), hsla(0.58, 0.5, 0.62, 1.0));
    }
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let (icon, hue): (Icon, Option<f32>) = match (lower.as_str(), ext) {
        (_, "lock") | ("package-lock.json", _) => (BpIcon::FileLock.into(), Some(0.0)),
        ("gemfile" | "podfile" | "rakefile" | "fastfile", _) | (_, "rb" | "gemspec" | "podspec") => {
            (BpIcon::FileCode.into(), Some(0.0))
        }
        (_, "gradle" | "kts" | "groovy") => (BpIcon::FileCode.into(), Some(0.45)),
        (".bashrc" | ".zshrc" | ".profile" | ".zprofile" | "gradlew", _) => {
            (BpIcon::FileTerminal.into(), Some(0.33))
        }
        (_, "rs") => (BpIcon::FileCode.into(), Some(0.05)),
        (_, "js" | "jsx" | "mjs" | "cjs") => (BpIcon::FileCode.into(), Some(0.14)),
        (_, "ts" | "tsx") => (BpIcon::FileCode.into(), Some(0.6)),
        (_, "py") => (BpIcon::FileCode.into(), Some(0.3)),
        (_, "go") => (BpIcon::FileCode.into(), Some(0.5)),
        (_, "java" | "kt" | "swift" | "m" | "mm" | "c" | "h" | "cpp" | "hpp" | "cs") => {
            (BpIcon::FileCode.into(), Some(0.08))
        }
        (_, "html" | "htm" | "vue" | "xml" | "svg") => (BpIcon::FileCode.into(), Some(0.04)),
        (_, "css" | "scss" | "sass" | "less") => (BpIcon::FileCode.into(), Some(0.75)),
        (_, "json" | "jsonc") => (BpIcon::FileBraces.into(), Some(0.14)),
        (_, "md" | "markdown" | "txt" | "rst") => (BpIcon::FileText.into(), Some(0.55)),
        (_, "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" | "heic") => {
            (BpIcon::FileImage.into(), Some(0.82))
        }
        (_, "sh" | "zsh" | "bash" | "fish") => (BpIcon::FileTerminal.into(), Some(0.33)),
        (_, "toml" | "yml" | "yaml" | "ini" | "env" | "plist" | "properties")
        | (".gitignore" | ".gitattributes" | ".editorconfig" | "dockerfile" | "makefile", _) => {
            (BpIcon::FileCog.into(), None)
        }
        _ => (IconName::File.into(), None),
    };
    let c = hue.map_or(hsla(0.0, 0.0, 0.6, 1.0), color);
    (icon, c)
}
