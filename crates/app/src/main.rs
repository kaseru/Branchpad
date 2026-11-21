//! Branchpad: git như Fork + soạn file như VSCode, cho macOS.
//!
//! Dùng: `branchpad [thư mục repo...]`

mod actions;
mod editor;
#[macro_use]
mod i18n;
mod icons;
mod repo_view;
mod ui;
mod workspace;

use std::path::PathBuf;

use gpui::{px, size, App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions};
use gpui_component::Root;

use crate::workspace::Workspace;

/// Chạy từ Terminal (`branchpad .`): tách app khỏi terminal để terminal dùng
/// tiếp được và ssh không bao giờ hỏi mật khẩu trên terminal.
fn detach_from_terminal(paths: &[PathBuf]) -> bool {
    use std::io::IsTerminal;
    use std::os::unix::process::CommandExt;
    if std::env::var_os("BRANCHPAD_FOREGROUND").is_some() || !std::io::stdin().is_terminal() {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let spawned = std::process::Command::new(exe)
        .args(paths)
        .env("BRANCHPAD_FOREGROUND", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    match spawned {
        Ok(_) => true,
        Err(e) => {
            eprintln!("branchpad: không tách được khỏi terminal: {e}");
            false
        }
    }
}

fn main() {
    let paths: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .filter(|a| !a.to_string_lossy().starts_with('-'))
        .map(|a| {
            let p = PathBuf::from(a);
            std::fs::canonicalize(&p).unwrap_or(p)
        })
        .collect();

    if detach_from_terminal(&paths) {
        return;
    }

    let app = Application::new().with_assets(icons::Assets);
    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        actions::bind_keys(cx);
        cx.on_action(|_: &actions::Quit, cx| cx.quit());
        cx.set_menus(actions::menus());
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);

        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Branchpad".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let paths = paths.clone();
        if let Err(e) = cx.open_window(options, move |window, cx| {
            let workspace = cx.new(|cx| Workspace::new(paths, window, cx));
            cx.new(|cx| Root::new(workspace, window, cx))
        }) {
            eprintln!("branchpad: không mở được cửa sổ: {e:#}");
            cx.quit();
        }
    });
}
