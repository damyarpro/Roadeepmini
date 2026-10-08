// Moving the island window: the drag, the spring into a dock, and the slide
// to and from the maximised chat's spot.
//
// The page only says "drag starts" (and later answers one layout handshake);
// the window itself is moved from here, at 60 Hz, with the cursor read straight
// from Win32 — no IPC round trip per frame. Each move runs on its own thread
// that exists only while the island moves, so a resting or hidden island costs
// nothing.
//
// Inside the window the island is always centred along the dock edge
// (dock.rs), so moving it along the edge only moves the window. Changing edge
// or display changes the page layout too: the page hides the island, lays
// itself out for the new dock and answers with the shift that keeps the island
// where it is on screen (`dock_layout_ready`); the window jumps by that shift
// and the island reappears, so it never shows in two places.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewWindow};

use crate::dock::{self, Edge, Layout, Rect, Screen, Spring};
use crate::island::{self, WINDOW_LABEL};
use crate::settings::{self, Dock, Settings};
use crate::Shared;

const TICK: Duration = Duration::from_millis(16);
/// How long the drop waits for the page to re-lay itself out.
const HANDSHAKE_WAIT: Duration = Duration::from_millis(250);
/// The spring into a dock: a little bouncier than the island's own growth.
const SETTLE_RESPONSE: f64 = 0.5;
const SETTLE_DAMPING: f64 = 0.62;
const SETTLE_MAX: Duration = Duration::from_millis(1600);
/// The maximise slide uses the island's open spring and close curve
/// (Tracked in src/core/anim.ts), so window and island move as one.
const OPEN_RESPONSE: f64 = 0.5;
const OPEN_DAMPING: f64 = 0.72;
const CLOSE_MS: f64 = 340.0;
/// Longest a maximise waits for a move in progress.
const MAX_WAIT: Duration = Duration::from_secs(3);
/// A throw faster than this (logical px/s) is not carried into the settle.
const MAX_THROW: f64 = 5000.0;
/// How quickly the grab point slides inside the compact island (per tick).
const GRAB_EASE: f64 = 0.3;

/// The island's moves, one at a time: a drag, a dock change or a maximise slide.
pub struct Motion {
    active: AtomicBool,
    /// The chat is maximised: the window sits at dock::max_center.
    pub maximized: AtomicBool,
    swap: Mutex<Swap>,
    cv: Condvar,
}

/// The layout handshake of a drop: where the window is, the size it takes for
/// the new dock, and whether the page answered.
#[derive(Default)]
struct Swap {
    waiting: bool,
    done: bool,
    base: (f64, f64),
    size: (u32, u32),
}

impl Motion {
    pub fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            maximized: AtomicBool::new(false),
            swap: Mutex::new(Swap::default()),
            cv: Condvar::new(),
        }
    }

    fn begin(&self) -> bool {
        self.active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }

    fn end(&self) {
        self.active.store(false, Ordering::Release);
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DragFrame {
    /// Island velocity, logical px/s (drives the squash and tilt).
    vx: f64,
    vy: f64,
    /// The edge a drop would dock to.
    edge: &'static str,
    /// After the release, while springing into the dock.
    settling: bool,
}

#[derive(Serialize, Clone)]
struct MovePayload {
    layout: Layout,
    /// The page must answer with `dock_layout_ready`.
    wait: bool,
}

/// Stores a new dock, persists it and tells every window.
fn save_dock(app: &AppHandle, dock: Dock) -> Settings {
    let shared = app.state::<Shared>();
    let updated = {
        let mut current = shared.settings.lock().unwrap();
        current.dock = settings::sanitize_dock(&dock);
        current.clone()
    };
    if let Err(err) = settings::save(&updated) {
        crate::log::line(format!("dock: could not save settings: {err}"));
    }
    let _ = app.emit("settings-changed", updated.clone());
    updated
}

fn finite(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}

fn set_pos(win: &WebviewWindow, p: (f64, f64)) {
    let _ = win.set_position(PhysicalPosition::new(p.0.round() as i32, p.1.round() as i32));
}

/// Starts a drag of the island. `cx`, `cy` is the compact island's centre in
/// the window and `half_w`, `half_h` its half size (logical px), as the page
/// lays it out for the current dock.
#[tauri::command]
pub fn drag_start(
    app: AppHandle,
    shared: State<Shared>,
    cx: f64,
    cy: f64,
    half_w: f64,
    half_h: f64,
    reduced_motion: bool,
) -> bool {
    let sane = finite(&[cx, cy, half_w, half_h])
        && (0.0..=4096.0).contains(&cx)
        && (0.0..=4096.0).contains(&cy)
        && half_w > 0.0
        && half_h > 0.0
        && half_w <= 2048.0
        && half_h <= 2048.0;
    if !sane {
        crate::log::line("dock: drag refused (bad geometry from the page)");
        return false;
    }
    let motion = shared.motion.clone();
    if shared.gate.collapsed.load(Ordering::Relaxed) || motion.maximized.load(Ordering::Relaxed) {
        return false;
    }
    if !motion.begin() {
        return false;
    }
    let settings = shared.settings.lock().unwrap().clone();
    let grab = Grab { cx, cy, half_w, half_h, reduced: reduced_motion };
    std::thread::spawn(move || {
        let started = Instant::now();
        run_drag(&app, &motion, &settings, grab);
        motion.end();
        crate::log::line(format!("dock: drag finished in {} ms", started.elapsed().as_millis()));
    });
    true
}

struct Grab {
    cx: f64,
    cy: f64,
    half_w: f64,
    half_h: f64,
    reduced: bool,
}

enum Outcome {
    Drop,
    Cancel,
}

fn run_drag(app: &AppHandle, motion: &Motion, settings: &Settings, g: Grab) {
    let Some(win) = island::window(app) else { return };
    let (screens, primary) = island::screens(app);
    if screens.is_empty() {
        return;
    }
    let (Ok(origin), Some(cursor0)) = (win.outer_position(), island::cursor_physical()) else { return };
    let mut scale = win.scale_factor().unwrap_or(1.0);
    let mut wpos = (origin.x as f64, origin.y as f64);
    let mut pill = (wpos.0 + g.cx * scale, wpos.1 + g.cy * scale);
    // Where the cursor holds the island; it slides inside the compact island
    // (the press may have been on the wide expanded card).
    let mut offset = (cursor0.0 - pill.0, cursor0.1 - pill.1);
    let start_edge = island::dock_edge(settings);
    let mut preview = start_edge;
    let mut vel = (0.0, 0.0);
    let mut last = Instant::now();

    let outcome = loop {
        std::thread::sleep(TICK);
        let now = Instant::now();
        let dt = (now - last).as_secs_f64().max(0.001);
        last = now;
        if island::escape_down() {
            break Outcome::Cancel;
        }
        if !island::primary_button_down() {
            break Outcome::Drop;
        }
        let Some(c) = island::cursor_physical() else { continue };
        scale = win.scale_factor().unwrap_or(scale);
        let lim = ((g.half_w - 4.0).max(0.0) * scale, (g.half_h - 4.0).max(0.0) * scale);
        offset.0 += (offset.0.clamp(-lim.0, lim.0) - offset.0) * GRAB_EASE;
        offset.1 += (offset.1.clamp(-lim.1, lim.1) - offset.1) * GRAB_EASE;
        let next = (c.0 - offset.0, c.1 - offset.1);
        vel.0 = vel.0 * 0.6 + (next.0 - pill.0) / dt * 0.4;
        vel.1 = vel.1 * 0.6 + (next.1 - pill.1) / dt * 0.4;
        pill = next;
        let w = (pill.0 - g.cx * scale, pill.1 - g.cy * scale);
        if (w.0 - wpos.0).abs() >= 0.5 || (w.1 - wpos.1).abs() >= 0.5 {
            wpos = w;
            set_pos(&win, wpos);
        }
        if let Some(i) = dock::screen_at(&screens, pill.0, pill.1) {
            let s = &screens[i];
            preview = dock::nearest_edge(&s.work, pill.0, pill.1, Some(preview), dock::PREVIEW_HYSTERESIS * s.scale);
        }
        let _ = app.emit_to(
            WINDOW_LABEL,
            "dock-drag",
            DragFrame { vx: vel.0 / scale, vy: vel.1 / scale, edge: preview.as_str(), settling: false },
        );
    };

    let (screen, dock) = match outcome {
        Outcome::Drop => {
            let Some(i) = dock::screen_at(&screens, pill.0, pill.1) else { return };
            let s = screens[i].clone();
            let pos = dock::pos_at(&s, preview, pill.0, pill.1);
            // Dropped on the display the preference picks anyway: remember no
            // display, so the preference keeps working.
            let cursor = island::cursor_physical();
            let preferred = dock::resolve_screen(&screens, primary.as_deref(), &settings.screen, "", cursor);
            let monitor = if preferred == Some(i) { String::new() } else { s.id.clone() };
            let dock = Dock { edge: preview.as_str().to_string(), pos, monitor };
            save_dock(app, dock.clone());
            crate::log::line(format!("dock: dropped on the {} edge at {:.2}", preview.as_str(), pos));
            (s, dock)
        }
        Outcome::Cancel => {
            let cursor = island::cursor_physical();
            let i = dock::resolve_screen(&screens, primary.as_deref(), &settings.screen, &settings.dock.monitor, cursor)
                .unwrap_or(0);
            crate::log::line("dock: drag cancelled");
            (screens[i].clone(), settings.dock.clone())
        }
    };
    let edge = Edge::parse(&dock.edge).unwrap_or(Edge::Top);
    let throw = (vel.0.clamp(-MAX_THROW * scale, MAX_THROW * scale), vel.1.clamp(-MAX_THROW * scale, MAX_THROW * scale));
    settle(app, motion, &win, &screen, edge, dock.pos, wpos, throw, g.reduced);
}

/// Springs the window from `wpos` into the dock, after the layout handshake.
fn settle(
    app: &AppHandle,
    motion: &Motion,
    win: &WebviewWindow,
    s: &Screen,
    edge: Edge,
    pos: f64,
    wpos: (f64, f64),
    vel: (f64, f64),
    reduced: bool,
) {
    let target = dock::panel_rect(s, edge, pos, false);
    let wpos = handshake(app, motion, dock::layout(s, edge, pos), wpos, target);
    if !reduced {
        let mut sx = Spring::new(wpos.0, vel.0, target.x, SETTLE_RESPONSE, SETTLE_DAMPING);
        let mut sy = Spring::new(wpos.1, vel.1, target.y, SETTLE_RESPONSE, SETTLE_DAMPING);
        let start = Instant::now();
        let mut last = start;
        while start.elapsed() < SETTLE_MAX && !(sx.settled(0.5, 20.0) && sy.settled(0.5, 20.0)) {
            std::thread::sleep(TICK);
            let now = Instant::now();
            let dt = (now - last).as_secs_f64();
            last = now;
            sx.step(dt);
            sy.step(dt);
            set_pos(win, (sx.value, sy.value));
            let _ = app.emit_to(
                WINDOW_LABEL,
                "dock-drag",
                DragFrame { vx: sx.velocity / s.scale, vy: sy.velocity / s.scale, edge: edge.as_str(), settling: true },
            );
        }
    }
    land(app, win, target);
}

/// The exact final placement, and the page told the move is over.
fn land(app: &AppHandle, win: &WebviewWindow, target: Rect) {
    place_final(app, win, target);
    let _ = app.emit_to(WINDOW_LABEL, "dock-settled", ());
}

/// Puts the window at `target`, unless the island was hidden meanwhile: then
/// it lives in its wake strip.
fn place_final(app: &AppHandle, win: &WebviewWindow, target: Rect) {
    let shared = app.state::<Shared>();
    if shared.gate.collapsed.load(Ordering::Relaxed) {
        let settings = shared.settings.lock().unwrap().clone();
        island::apply_geometry(app, &settings, true, false);
    } else {
        island::place(win, target);
    }
}

/// Tells the page the new layout and waits (briefly) for its answer, which
/// moves and resizes the window (`dock_layout_ready`). Returns where the
/// window is afterwards.
fn handshake(app: &AppHandle, motion: &Motion, layout: Layout, wpos: (f64, f64), target: Rect) -> (f64, f64) {
    {
        let mut sw = motion.swap.lock().unwrap();
        *sw = Swap {
            waiting: true,
            done: false,
            base: wpos,
            size: (target.w.round().max(1.0) as u32, target.h.round().max(1.0) as u32),
        };
    }
    let _ = app.emit_to(WINDOW_LABEL, "dock-move", MovePayload { layout, wait: true });
    let sw = motion.swap.lock().unwrap();
    let (mut sw, _) = motion
        .cv
        .wait_timeout_while(sw, HANDSHAKE_WAIT, |sw| !sw.done)
        .unwrap();
    if !sw.done {
        crate::log::line("dock: the page did not answer the layout handshake in time");
    }
    sw.waiting = false;
    sw.base
}

/// The page has laid itself out for the new dock (island hidden). `dx`, `dy`
/// (logical px) is how far the island moved inside the window; the window
/// moves the other way so the island stays put on screen, and takes the new
/// dock's size.
#[tauri::command]
pub fn dock_layout_ready(app: AppHandle, shared: State<Shared>, dx: f64, dy: f64) {
    if !finite(&[dx, dy]) || dx.abs() > 8192.0 || dy.abs() > 8192.0 {
        return;
    }
    let motion = &shared.motion;
    let mut sw = motion.swap.lock().unwrap();
    if !sw.waiting || sw.done {
        return;
    }
    if let Some(win) = island::window(&app) {
        let scale = win.scale_factor().unwrap_or(1.0);
        let base = (sw.base.0 + dx * scale, sw.base.1 + dy * scale);
        let current = win.inner_size().map(|s| (s.width, s.height)).unwrap_or((0, 0));
        if current != sw.size {
            let _ = win.set_size(PhysicalSize::new(sw.size.0, sw.size.1));
        }
        set_pos(&win, base);
        sw.base = base;
    }
    sw.done = true;
    motion.cv.notify_all();
}

/// Docks the island from the menu or the settings window: an edge, a position
/// along it, and with `reset` back on the display the preference picks.
#[tauri::command]
pub fn dock_set(app: AppHandle, shared: State<Shared>, edge: String, pos: f64, reset: bool, reduced_motion: bool) -> bool {
    let Some(edge) = Edge::parse(&edge) else { return false };
    let pos = if pos.is_finite() { pos.clamp(0.0, 1.0) } else { 0.5 };
    let motion = shared.motion.clone();
    if !motion.begin() {
        return false;
    }
    let mut dock = shared.settings.lock().unwrap().dock.clone();
    dock.edge = edge.as_str().to_string();
    dock.pos = pos;
    if reset {
        dock.monitor.clear();
    }
    let settings = save_dock(&app, dock);
    motion.maximized.store(false, Ordering::Relaxed);
    crate::log::line(format!("dock: set to the {} edge at {:.2}", edge.as_str(), pos));
    if shared.gate.collapsed.load(Ordering::Relaxed) {
        island::apply_geometry(&app, &settings, true, false);
        motion.end();
        return true;
    }
    std::thread::spawn(move || {
        if let (Some(win), Some(s)) = (island::window(&app), island::target_screen(&app, &settings)) {
            if let Ok(origin) = win.outer_position() {
                let from = (origin.x as f64, origin.y as f64);
                settle(&app, &motion, &win, &s, edge, settings.dock.pos, from, (0.0, 0.0), reduced_motion);
            }
        }
        motion.end();
    });
    true
}

/// Slides the window to the maximised chat's spot (or back), in step with the
/// island growing (or shrinking) inside it. A slide still running finishes
/// first; the window then goes wherever the latest call says.
#[tauri::command]
pub fn set_maximized(app: AppHandle, shared: State<Shared>, on: bool, reduced_motion: bool) {
    let motion = shared.motion.clone();
    let on = on && !shared.gate.collapsed.load(Ordering::Relaxed);
    motion.maximized.store(on, Ordering::Relaxed);
    std::thread::spawn(move || {
        let waited = Instant::now();
        while !motion.begin() {
            if waited.elapsed() > MAX_WAIT {
                crate::log::line("dock: maximise gave up waiting for another move");
                return;
            }
            std::thread::sleep(TICK);
        }
        let on = motion.maximized.load(Ordering::Relaxed);
        let settings = app.state::<Shared>().settings.lock().unwrap().clone();
        if let (Some(win), Some(s)) = (island::window(&app), island::target_screen(&app, &settings)) {
            let edge = island::dock_edge(&settings);
            let target = dock::panel_rect(&s, edge, settings.dock.pos, on);
            if let Ok(origin) = win.outer_position() {
                let from = (origin.x as f64, origin.y as f64);
                if !reduced_motion && (from.0 - target.x).abs() + (from.1 - target.y).abs() >= 1.0 {
                    slide(&win, from, (target.x, target.y), on);
                }
            }
            place_final(&app, &win, target);
        }
        motion.end();
    });
}

/// Growing: the open spring. Shrinking: the 340 ms close curve.
fn slide(win: &WebviewWindow, from: (f64, f64), to: (f64, f64), growing: bool) {
    let start = Instant::now();
    if growing {
        let mut sx = Spring::new(from.0, 0.0, to.0, OPEN_RESPONSE, OPEN_DAMPING);
        let mut sy = Spring::new(from.1, 0.0, to.1, OPEN_RESPONSE, OPEN_DAMPING);
        let mut last = start;
        while start.elapsed() < SETTLE_MAX && !(sx.settled(0.5, 20.0) && sy.settled(0.5, 20.0)) {
            std::thread::sleep(TICK);
            let now = Instant::now();
            let dt = (now - last).as_secs_f64();
            last = now;
            sx.step(dt);
            sy.step(dt);
            set_pos(win, (sx.value, sy.value));
        }
    } else {
        loop {
            std::thread::sleep(TICK);
            let p = (start.elapsed().as_secs_f64() * 1000.0 / CLOSE_MS).min(1.0);
            let e = close_curve(p);
            set_pos(win, (from.0 + (to.0 - from.0) * e, from.1 + (to.1 - from.1) * e));
            if p >= 1.0 {
                break;
            }
        }
    }
}

/// cubic-bezier(.45, 0, .2, 1) — the island's close curve (closeCurve in
/// src/core/anim.ts), solved for x by bisection.
pub fn close_curve(x: f64) -> f64 {
    let (x1, y1, x2, y2) = (0.45, 0.0, 0.2, 1.0);
    let bx = |t: f64| 3.0 * (1.0 - t) * (1.0 - t) * t * x1 + 3.0 * (1.0 - t) * t * t * x2 + t * t * t;
    let by = |t: f64| 3.0 * (1.0 - t) * (1.0 - t) * t * y1 + 3.0 * (1.0 - t) * t * t * y2 + t * t * t;
    let (mut lo, mut hi, mut t) = (0.0, 1.0, x);
    for _ in 0..20 {
        if bx(t) < x {
            lo = t;
        } else {
            hi = t;
        }
        t = (lo + hi) / 2.0;
    }
    by(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_curve_runs_from_0_to_1_without_overshoot() {
        assert!(close_curve(0.0).abs() < 1e-6);
        assert!((close_curve(1.0) - 1.0).abs() < 1e-6);
        let mut prev = 0.0;
        for i in 1..=100 {
            let v = close_curve(i as f64 / 100.0);
            assert!(v >= prev - 1e-9 && v <= 1.0 + 1e-9);
            prev = v;
        }
        // Ease-in-out: slow at first, past halfway by the middle.
        assert!(close_curve(0.1) < 0.1 && close_curve(0.5) > 0.5);
    }

    #[test]
    fn only_one_move_at_a_time() {
        let m = Motion::new();
        assert!(m.begin());
        assert!(!m.begin(), "a second drag or slide is refused while one runs");
        m.end();
        assert!(m.begin());
    }
}
