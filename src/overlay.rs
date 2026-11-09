//! Animations drawn over the desktop (overlay_macos.m): the countdown at a screen edge
//! and the highlight that shows where the cursor is. The effects themselves are the
//! table of drawing functions there, one per `EdgeAnimStyle` / `LocateStyle`.

use crate::config::{parse_color, Edge, Edges, Locate, DEFAULT_EFFECT_COLOR};
use crate::edge::Rect;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

extern "C" {
    #[allow(clippy::too_many_arguments)]
    fn fs_overlay_edge_begin(
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        edge: i32,
        style: i32,
        direction: i32,
        shrink: bool,
        elapsed: f64,
        duration: f64,
        preview: bool,
        red: f64,
        green: f64,
        blue: f64,
    );
    fn fs_overlay_edge_end();
    fn fs_overlay_locate(style: i32, red: f64, green: f64, blue: f64);
}

static READY: AtomicBool = AtomicBool::new(false);

/// Says an AppKit event loop is running on the main thread. Without one (`flockswitch
/// run`) the overlay's work would queue up for ever, so nothing is drawn.
pub fn set_ready() {
    READY.store(true, Ordering::Relaxed);
}

/// A colour from the settings as red, green and blue from 0 to 1.
fn components(text: &str) -> (f64, f64, f64) {
    let [r, g, b] = parse_color(text).or(parse_color(DEFAULT_EFFECT_COLOR)).unwrap_or([255, 204, 0]);
    (f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0)
}

fn ready() -> bool {
    READY.load(Ordering::Relaxed)
}

/// Starts the countdown on `edge` of `display`, `elapsed` into the wait of `duration`.
pub fn edge_begin(edges: &Edges, edge: Edge, display: &Rect, elapsed: Duration, duration: Duration, preview: bool) {
    if !ready() {
        return;
    }
    let (red, green, blue) = components(&edges.anim_color);
    // Safety: plain values; the call only queues work for the main thread.
    unsafe {
        fs_overlay_edge_begin(
            display.x,
            display.y,
            display.w,
            display.h,
            edge as i32,
            edges.anim_style.resolved() as i32,
            edges.anim_direction as i32,
            edges.anim_shrink,
            elapsed.as_secs_f64(),
            duration.as_secs_f64(),
            preview,
            red,
            green,
            blue,
        );
    }
}

/// Takes the countdown away at once.
pub fn edge_end() {
    if ready() {
        // Safety: takes nothing; only queues work for the main thread.
        unsafe { fs_overlay_edge_end() }
    }
}

/// Shows where the cursor is, for a moment.
pub fn locate(locate: &Locate) {
    if ready() {
        let (red, green, blue) = components(&locate.color);
        // Safety: plain values; the call only queues work for the main thread.
        unsafe { fs_overlay_locate(locate.style.resolved() as i32, red, green, blue) }
    }
}
