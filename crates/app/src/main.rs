//! Branchpad: git như Fork + soạn file như VSCode, cho macOS.
//!
//! Dùng: `branchpad [thư mục repo...]`

#[macro_use]
mod i18n;
mod actions;
mod editor;
mod icons;
mod repo_view;
mod settings_view;
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
    if std::env::var_os("BRANCHPAD_FOREGROUND").is_some() || !std::io::stdin().is_terminal() {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(paths)
        .env("BRANCHPAD_FOREGROUND", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let spawned = cmd.spawn();
    match spawned {
        Ok(_) => true,
        Err(e) => {
            eprintln!("branchpad: cannot detach from terminal: {e}");
            false
        }
    }
}

/// Workspace của cửa sổ chính, để nhận file mở từ Dock/Finder.
struct WorkspaceHandle(gpui::Entity<Workspace>);

impl gpui::Global for WorkspaceHandle {}

/// `file:///Users/a/My%20File.txt` -> `/Users/a/My File.txt`. URL khác file thì bỏ qua.
fn path_from_url(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&out).into_owned()))
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
    // macOS gửi file thả vào icon Dock / "Open With" qua sự kiện; chuyển qua kênh để xử lý trong app.
    let (url_tx, mut url_rx) = futures::channel::mpsc::unbounded::<Vec<String>>();
    app.on_open_urls(move |urls| {
        let _ = url_tx.unbounded_send(urls);
    });
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
            window_bounds: Some(WindowBounds::Maximized(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Branchpad".into()),
                // macOS: tab repo nằm trong thanh tiêu đề (kiểu Chrome).
                appears_transparent: cfg!(target_os = "macos"),
                traffic_light_position: cfg!(target_os = "macos").then(|| gpui::point(px(14.), px(13.))),
            }),
            ..Default::default()
        };
        let paths = paths.clone();
        if let Err(e) = cx.open_window(options, move |window, cx| {
            let workspace = cx.new(|cx| Workspace::new(paths, window, cx));
            cx.set_global(WorkspaceHandle(workspace.clone()));
            cx.new(|cx| Root::new(workspace, window, cx))
        }) {
            eprintln!("branchpad: cannot open window: {e:#}");
            cx.quit();
        }
        cx.spawn(async move |cx| {
            use futures::StreamExt;
            while let Some(urls) = url_rx.next().await {
                let paths: Vec<PathBuf> = urls.iter().filter_map(|u| path_from_url(u)).collect();
                if paths.is_empty() {
                    continue;
                }
                let result = cx.update(|cx| {
                    if !cx.has_global::<WorkspaceHandle>() {
                        return;
                    }
                    let workspace = cx.global::<WorkspaceHandle>().0.clone();
                    if let Some(win) = cx.windows().first().copied() {
                        let _ = win.update(cx, |_, window, cx| {
                            window.activate_window();
                            workspace.update(cx, |ws, cx| ws.open_dropped(&paths, window, cx));
                        });
                    }
                });
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();
    });
}
