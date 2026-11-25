//! [Branchpad] Upper bound for the frame rate. gpui draws once per display refresh while something
//! animates or scrolls; on a 144-175 Hz display that is 2-3x the frames (and CPU) of 60 Hz for a change
//! the eye barely sees. The platform frame callback skips display ticks that come sooner than the cap.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

static MAX_FPS: AtomicU32 = AtomicU32::new(0);

/// Draw at most `fps` frames per second (`None` or 0: every display refresh).
pub fn set_max_frame_rate(fps: Option<u32>) {
    MAX_FPS.store(fps.unwrap_or(0), Ordering::Relaxed);
}

/// Minimum time between two frames, a little under `1 / fps` so ticks that arrive a hair early (display
/// timing jitter) still draw instead of slipping a whole refresh.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn min_frame_interval() -> Option<Duration> {
    match MAX_FPS.load(Ordering::Relaxed) {
        0 => None,
        fps => Some(Duration::from_secs_f64(0.9 / fps as f64)),
    }
}
