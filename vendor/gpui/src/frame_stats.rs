//! [Branchpad] Frame cost counters for profiling, enabled with `BRANCHPAD_FRAME_STATS=<log file>`.
//! Once per second (while frames are drawn) a line is appended with the frame count, the average time
//! of each draw phase, primitive counts and which views had to be rendered.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Stats {
    file: Option<std::fs::File>,
    since: Option<Instant>,
    frames: u32,
    layout: Duration,
    prepaint: Duration,
    paint: Duration,
    finish: Duration,
    total: Duration,
    sprites: usize,
    quads: usize,
    layout_nodes: usize,
    view_reused: u32,
    renders: HashMap<&'static str, u32>,
}

thread_local! {
    static STATS: RefCell<Option<Stats>> = RefCell::new(
        std::env::var_os("BRANCHPAD_FRAME_STATS")
            .and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok())
            .map(|file| Stats { file: Some(file), ..Default::default() }),
    );
}

pub(crate) fn enabled() -> bool {
    STATS.with(|s| s.borrow().is_some())
}

fn with(f: impl FnOnce(&mut Stats)) {
    STATS.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            f(s)
        }
    })
}

/// A view's `render` ran this frame.
pub(crate) fn view_rendered(type_name: &'static str) {
    with(|s| *s.renders.entry(short(type_name)).or_default() += 1)
}

/// A cached view replayed its previous prepaint/paint.
pub(crate) fn view_reused() {
    with(|s| s.view_reused += 1)
}

pub(crate) fn layout_node() {
    with(|s| s.layout_nodes += 1)
}

pub(crate) fn layout_time(d: Duration) {
    with(|s| s.layout += d)
}

pub(crate) struct FrameTimes {
    pub prepaint: Duration,
    pub paint: Duration,
    pub finish: Duration,
    pub total: Duration,
    pub sprites: usize,
    pub quads: usize,
}

pub(crate) fn frame(t: FrameTimes) {
    with(|s| {
        let now = Instant::now();
        let since = *s.since.get_or_insert(now);
        s.frames += 1;
        s.prepaint += t.prepaint;
        s.paint += t.paint;
        s.finish += t.finish;
        s.total += t.total;
        s.sprites += t.sprites;
        s.quads += t.quads;
        if now.duration_since(since) < Duration::from_secs(1) {
            return;
        }
        let n = s.frames.max(1);
        let ms = |d: Duration| d.as_secs_f64() * 1000.0 / n as f64;
        let mut renders: Vec<_> = s.renders.drain().collect();
        renders.sort_by(|a, b| b.1.cmp(&a.1));
        let renders: Vec<String> = renders.iter().map(|(k, v)| format!("{k}:{v}")).collect();
        let line = format!(
            "{:?} frames={} total={:.2}ms (layout={:.2} prepaint={:.2} paint={:.2} finish={:.2}) sprites={} quads={} layout_nodes={} reused_views={} renders=[{}]\n",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default(),
            s.frames,
            ms(s.total),
            ms(s.layout),
            ms(s.prepaint),
            ms(s.paint),
            ms(s.finish),
            s.sprites / n as usize,
            s.quads / n as usize,
            s.layout_nodes / n as usize,
            s.view_reused / n,
            renders.join(" "),
        );
        if let Some(f) = s.file.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
        let file = s.file.take();
        *s = Stats { file, ..Default::default() };
    })
}

fn short(type_name: &'static str) -> &'static str {
    let base = type_name.split('<').next().unwrap_or(type_name);
    base.rsplit("::").next().unwrap_or(base)
}
