// Island window: placement on the chosen display and dock (dock.rs), the two
// window sizes (full panel / the wake strip with the hidden island's line),
// click-through and the cursor poll.
//
// There is no notch on a PC, so the island is a black shape drawn against an
// edge of the screen (the top centre by default) inside a borderless,
// transparent, always-on-top window that never takes focus.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewWindow};

use windows::Win32::Foundation::{HWND, POINT};
use windows::core::BOOL;
use windows::Win32::Foundation::LPARAM;
use windows::core::{w, Interface};
use windows::Win32::System::Ole::{IDropTarget, RegisterDragDrop, RevokeDragDrop};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetClassNameW, GetParent, GetPropW, GetSystemMetrics, SM_SWAPBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};

use crate::dock::{self, Edge, Layout, Rect, Screen};
use crate::settings::Settings;

/// Smallest logical size of the full window. The real size is the envelope of
/// the largest island on the display (dock::layout); only the island rect
/// takes the mouse, the rest is click-through. Keep in step with PANEL_W /
/// PANEL_H in src/core/layout.ts.
pub const PANEL_W: f64 = 720.0;
pub const PANEL_H: f64 = 480.0;
/// Logical size of the strip that is the whole window while the island is
/// hidden: it shows the island's line (src/style.css) and wakes it on hover.
/// It takes the mouse over whatever is beneath, so it stays thin: as long as
/// the compact island plus its two shoulders, 8 px deep for the 5 px line.
/// Same as WAKE_STRIP_W / WAKE_STRIP_H in src/core/layout.ts.
pub const STRIP_W: f64 = 304.0;
pub const STRIP_H: f64 = 8.0;

pub const WINDOW_LABEL: &str = "island";

/// Margin around the island that still counts as "on the island", in logical px.
/// Wider than the macOS 6 pt because a click must never be swallowed.
pub const HIT_MARGIN: f64 = 14.0;
/// While a mouse button is held this close to the island, the window takes the
/// mouse so a file dragged from elsewhere finds its drop target. Bounded, so
/// the large transparent window never swallows drags meant for other apps.
const DROP_MARGIN: f64 = 96.0;

#[derive(Serialize, Clone)]
pub struct CursorPayload {
    pub x: f64,
    pub y: f64,
}

#[derive(Serialize, Clone)]
pub struct ScreenInfo {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/// The island shape in window-logical coordinates, pushed by the front end.
/// The poll thread owns the click-through decision so it lands in the same 16 ms
/// tick as the cursor read — an IPC round trip here loses clicks.
#[derive(Clone, Copy, Default)]
pub struct IslandRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Wakes / parks the cursor poll thread so a hidden island costs literally nothing.
pub struct PollGate {
    active: Mutex<bool>,
    cv: Condvar,
    pub collapsed: AtomicBool,
    pub rect: Mutex<IslandRect>,
    /// Mirrors the window flag so we only call into Win32 when it changes.
    ignoring: AtomicBool,
}

impl PollGate {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(false),
            cv: Condvar::new(),
            collapsed: AtomicBool::new(true),
            rect: Mutex::new(IslandRect::default()),
            ignoring: AtomicBool::new(false),
        }
    }

    pub fn set_rect(&self, rect: IslandRect) {
        *self.rect.lock().unwrap() = rect;
    }

    /// Forces the next poll tick to re-apply the flag (after a window resize).
    pub fn forget_ignore_state(&self) {
        self.ignoring.store(false, Ordering::Relaxed);
    }

    pub fn set_active(&self, on: bool) {
        let mut guard = self.active.lock().unwrap();
        *guard = on;
        self.cv.notify_all();
    }

    fn wait_until_active(&self) {
        let mut guard = self.active.lock().unwrap();
        while !*guard {
            guard = self.cv.wait(guard).unwrap();
        }
    }

    fn is_active(&self) -> bool {
        *self.active.lock().unwrap()
    }
}

pub fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(WINDOW_LABEL)
}

pub fn cursor_physical() -> Option<(f64, f64)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok()? };
    Some((p.x as f64, p.y as f64))
}

/// Lets dropped files reach the app again.
///
/// wry installs its drop target by walking the webview's child windows **once**,
/// when the webview is created. WebView2 creates `Chrome_RenderWidgetHostHWND`
/// later and registers its own target on it; being the innermost window, that one
/// wins, and since the page has no HTML5 drop handler it refuses everything — the
/// "no drop" cursor, with nothing reaching Tauri. So that target is replaced with
/// wry's own (taken from the nearest ancestor that has one), which feeds Tauri's
/// drag events. Merely revoking it used to be enough, OLE then fell through to
/// the parent's target; since WebView2 154 it no longer does.
///
/// Cheap and idempotent, so it is simply re-run whenever a drag might be starting.
pub fn unblock_webview_drops(app: &AppHandle) {
    for label in [WINDOW_LABEL, "settings"] {
        let Some(win) = app.get_webview_window(label) else { continue };
        let Some(hwnd) = hwnd_of(&win) else { continue };
        unsafe {
            let _ = EnumChildWindows(Some(hwnd), Some(revoke_render_widget), LPARAM(0));
        }
    }
}

unsafe extern "system" fn revoke_render_widget(hwnd: HWND, _: LPARAM) -> BOOL {
    let mut name = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut name) };
    if len > 0 {
        let class = String::from_utf16_lossy(&name[..len as usize]);
        if class == "Chrome_RenderWidgetHostHWND" {
            let _ = unsafe { RevokeDragDrop(hwnd) };
            if let Some(target) = unsafe { ancestor_drop_target(hwnd) } {
                if let Err(err) = unsafe { RegisterDragDrop(hwnd, &target) } {
                    crate::log::line(format!("files: could not take over the webview drop target: {err}"));
                }
            }
        }
    }
    true.into()
}

/// wry's drop target: the one OLE keeps for the nearest ancestor that has one.
/// OLE stores a registered target as an `IDropTarget` pointer in this window
/// property (same process); a borrowed clone keeps it alive while registered.
unsafe fn ancestor_drop_target(hwnd: HWND) -> Option<IDropTarget> {
    let mut cur = unsafe { GetParent(hwnd) }.ok()?;
    loop {
        let raw = unsafe { GetPropW(cur, w!("OleDropTargetInterface")) };
        if !raw.0.is_null() {
            return unsafe { IDropTarget::from_raw_borrowed(&raw.0) }.cloned();
        }
        cur = unsafe { GetParent(cur) }.ok()?;
    }
}

fn key_down(vk: i32) -> bool {
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

/// True while the left mouse button is held — the only signal we get that a
/// drag might be in flight before it reaches the window.
fn left_button_down() -> bool {
    key_down(VK_LBUTTON.0 as i32)
}

/// GetAsyncKeyState reads the physical buttons; with the buttons swapped for a
/// left hand, the primary one (the page's button 0) is the right one.
pub fn primary_button_down() -> bool {
    let swapped = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
    key_down(if swapped { VK_RBUTTON.0 as i32 } else { VK_LBUTTON.0 as i32 })
}

pub fn escape_down() -> bool {
    key_down(VK_ESCAPE.0 as i32)
}

/// Windows' device name (`\\.\DISPLAY2`), or the position when there is none.
pub fn monitor_id(m: &Monitor) -> String {
    match m.name() {
        Some(name) if !name.is_empty() => name.clone(),
        _ => format!("{},{}", m.position().x, m.position().y),
    }
}

fn screen_of(m: &Monitor) -> Screen {
    let p = m.position();
    let s = m.size();
    let wa = m.work_area();
    Screen {
        id: monitor_id(m),
        bounds: Rect { x: p.x as f64, y: p.y as f64, w: s.width as f64, h: s.height as f64 },
        work: Rect {
            x: wa.position.x as f64,
            y: wa.position.y as f64,
            w: wa.size.width as f64,
            h: wa.size.height as f64,
        },
        scale: m.scale_factor(),
    }
}

/// Every display, and the id of the primary one.
pub fn screens(app: &AppHandle) -> (Vec<Screen>, Option<String>) {
    let list = app
        .available_monitors()
        .map(|ms| ms.iter().map(screen_of).collect())
        .unwrap_or_default();
    let primary = app.primary_monitor().ok().flatten().map(|m| monitor_id(&m));
    (list, primary)
}

/// The display the island lives on (dock::resolve_screen).
pub fn target_screen(app: &AppHandle, settings: &Settings) -> Option<Screen> {
    let (list, primary) = screens(app);
    let cursor = if settings.screen == "cursor" { cursor_physical() } else { None };
    let i = dock::resolve_screen(&list, primary.as_deref(), &settings.screen, &settings.dock.monitor, cursor)?;
    list.into_iter().nth(i)
}

pub fn dock_edge(settings: &Settings) -> Edge {
    Edge::parse(&settings.dock.edge).unwrap_or(Edge::Top)
}

/// Stand-in when Windows reports no display at all.
fn fallback_screen() -> Screen {
    let full = Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 };
    Screen { id: String::new(), bounds: full, work: full, scale: 1.0 }
}

pub fn screen_info(app: &AppHandle, settings: &Settings) -> ScreenInfo {
    let s = target_screen(app, settings).unwrap_or_else(fallback_screen);
    ScreenInfo {
        x: s.bounds.x / s.scale,
        y: s.bounds.y / s.scale,
        width: s.bounds.w / s.scale,
        height: s.bounds.h / s.scale,
        scale: s.scale,
    }
}

/// The page layout for the stored dock (sizes, chat limit, maximised island).
pub fn current_layout(app: &AppHandle, settings: &Settings) -> Layout {
    let s = target_screen(app, settings).unwrap_or_else(fallback_screen);
    dock::layout(&s, dock_edge(settings), settings.dock.pos)
}

/// Moves and sizes the window to a physical rect.
pub fn place(win: &WebviewWindow, r: Rect) {
    let size = PhysicalSize::new(r.w.round().max(1.0) as u32, r.h.round().max(1.0) as u32);
    let _ = win.set_size(size);
    let _ = win.set_position(PhysicalPosition::new(r.x.round() as i32, r.y.round() as i32));
    // Moving across displays can rescale the window: re-assert the physical size.
    let _ = win.set_size(size);
}

/// Places and sizes the window for the stored dock: the wake strip when
/// `collapsed`, else the panel (at its maximised spot when `maximized`). The
/// page is told the layout that goes with it ("dock-layout").
pub fn apply_geometry(app: &AppHandle, settings: &Settings, collapsed: bool, maximized: bool) {
    let Some(win) = window(app) else { return };
    let Some(s) = target_screen(app, settings) else { return };
    let edge = dock_edge(settings);
    let pos = settings.dock.pos;
    let rect = if collapsed {
        dock::strip_rect(&s, edge, pos)
    } else {
        dock::panel_rect(&s, edge, pos, maximized)
    };
    place(&win, rect);
    let _ = win.set_always_on_top(true);
    let _ = app.emit_to(WINDOW_LABEL, "dock-layout", dock::layout(&s, edge, pos));
}

fn hwnd_of(win: &WebviewWindow) -> Option<HWND> {
    let raw = win.hwnd().ok()?.0 as isize;
    if raw == 0 {
        return None;
    }
    Some(HWND(raw as *mut _))
}

/// WS_EX_NOACTIVATE keeps clicks from stealing focus; WS_EX_TOOLWINDOW keeps the
/// island out of Alt-Tab.
pub fn make_non_activating(win: &WebviewWindow) {
    let Some(hwnd) = hwnd_of(win) else { return };
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let want = ex | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
    }
}

/// Temporarily allow activation so a text field inside the island can be typed in.
pub fn set_activating(win: &WebviewWindow, activating: bool) {
    let Some(hwnd) = hwnd_of(win) else { return };
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let want = if activating {
            ex & !(WS_EX_NOACTIVATE.0 as isize)
        } else {
            ex | WS_EX_NOACTIVATE.0 as isize
        };
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
    }
}

/// Bounds, work area and scale of the display the island lives on. Any change
/// here (a display plugged in, rescaled, the taskbar moved) means the island
/// has to be placed again.
fn current_screen_key(app: &AppHandle) -> Option<Screen> {
    let settings = app.try_state::<crate::Shared>()?.settings.lock().unwrap().clone();
    target_screen(app, &settings)
}

/// Whether a cursor at window-logical (x, y) is within `margin` of the island.
/// A hidden island is a line (zero width or height) and still counts.
fn near_island(r: &IslandRect, x: f64, y: f64, margin: f64) -> bool {
    (r.w > 0.0 || r.h > 0.0)
        && x >= r.x - margin
        && x <= r.x + r.w + margin
        && y >= r.y - margin
        && y <= r.y + r.h + margin
}

/// Whether a cursor at window-logical (x, y) is over the island (plus margin).
/// Everything else in the panel — the transparent area around the island —
/// stays click-through.
fn on_island(r: &IslandRect, x: f64, y: f64) -> bool {
    near_island(r, x, y, HIT_MARGIN)
}

/// Emits `cursor` (window-logical coordinates) at ~60 Hz while the island is
/// visible. Parked on a condvar the rest of the time.
pub fn spawn_cursor_poll(app: AppHandle, gate: Arc<PollGate>) {
    std::thread::spawn(move || {
        let mut was_down = false;
        // Remembered across wakes so a display change while hidden is noticed the
        // moment the island comes back.
        let mut last_screen: Option<Screen> = None;
        loop {
            gate.wait_until_active();
            let mut last = (f64::MIN, f64::MIN);
            let mut ticks: u32 = 0;
            while gate.is_active() {
                std::thread::sleep(Duration::from_millis(16));

                // Monitors get plugged in, unplugged, rearranged and rescaled, and
                // an island pinned to coordinates that no longer exist is an island
                // nobody can reach. Checked about twice a second — the cursor poll
                // is already running, so this costs one monitor query.
                ticks = ticks.wrapping_add(1);
                if ticks % 30 == 0 {
                    let now = current_screen_key(&app);
                    if now.is_some() && now != last_screen {
                        let first = last_screen.is_none();
                        last_screen = now;
                        if !first {
                            crate::log::line("display layout changed — repositioning".to_string());
                            let _ = app.emit_to(WINDOW_LABEL, "screen-changed", ());
                        }
                    }
                }

                let Some(win) = window(&app) else { continue };
                let Ok(origin) = win.outer_position() else { continue };
                let scale = win.scale_factor().unwrap_or(1.0);
                let Some((cx, cy)) = cursor_physical() else { continue };
                let x = (cx - origin.x as f64) / scale;
                let y = (cy - origin.y as f64) / scale;
                if (x - last.0).abs() < 1.0 && (y - last.1).abs() < 1.0 {
                    continue;
                }
                last = (x, y);

                // Click-through: the window only takes the mouse over the island
                // shape. A small entry margin means the flag is already off by the
                // time a moving cursor reaches a button.
                let rect = *gate.rect.lock().unwrap();
                let on_island = on_island(&rect, x, y);

                // A file being dragged has to be able to find us. WS_EX_TRANSPARENT
                // — what click-through is on Windows — hides the window from
                // WindowFromPoint, so OLE finds no drop target and shows the "no
                // drop" cursor. macOS has no such problem: AppKit delivers drags to
                // registered destinations whatever ignoresMouseEvents says. So while
                // a button is held near the island, the window takes the mouse,
                // which also makes the drop zone as forgiving as the Mac's. Only
                // near it: the window is as large as the biggest island can get,
                // and drags meant for the apps under it must still reach them.
                // A press may be the start of a drag: make sure the drop target is
                // ours before the file arrives.
                let down = left_button_down();
                if down && !was_down {
                    let handle = app.clone();
                    let _ = app.run_on_main_thread(move || unblock_webview_drops(&handle));
                }
                was_down = down;

                let dragging = down && near_island(&rect, x, y, DROP_MARGIN);

                let accept = on_island || dragging;
                if gate.ignoring.load(Ordering::Relaxed) == accept {
                    gate.ignoring.store(!accept, Ordering::Relaxed);
                    // Applied on the main thread, and only if the island has not
                    // collapsed meanwhile: set_collapsed runs there too, so the two
                    // cannot interleave. Applied from here instead, a tick already
                    // under way when the island collapsed would land after
                    // set_collapsed and leave the wake strip click-through — a line
                    // the mouse can never reach, with nothing left running to fix it.
                    let handle = app.clone();
                    let gate = Arc::clone(&gate);
                    let _ = app.run_on_main_thread(move || {
                        if gate.collapsed.load(Ordering::Relaxed) {
                            return;
                        }
                        if let Some(win) = window(&handle) {
                            let _ = win.set_ignore_cursor_events(!accept);
                        }
                    });
                }

                let _ = win.emit("cursor", CursorPayload { x, y });
            }
        }
    });
}

pub fn set_ignore_cursor(app: &AppHandle, ignore: bool) {
    if let Some(win) = window(app) {
        let _ = win.set_ignore_cursor_events(ignore);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tallest normal chat and the maximised island fit the window of
    /// every dock, with the hit margin, on ordinary and large displays.
    #[test]
    fn panel_holds_the_largest_island() {
        assert!(STRIP_H < PANEL_H && STRIP_W < PANEL_W);
        for (w, h, scale) in [(1920.0, 1032.0, 1.0), (3840.0, 2100.0, 1.5), (1366.0, 728.0, 1.0)] {
            let work = Rect { x: 0.0, y: 0.0, w, h };
            let s = Screen { id: "d".into(), bounds: Rect { h: h + 48.0, ..work }, work, scale };
            let top = dock::layout(&s, Edge::Top, 0.5);
            assert!(top.panel_h >= dock::EDGE_MARGIN + top.chat_max_h.max(top.max_h) + HIT_MARGIN);
            assert!(top.panel_w >= top.max_w + 2.0 * HIT_MARGIN);
            for edge in [Edge::Left, Edge::Right] {
                let side = dock::layout(&s, edge, 0.5);
                assert!(side.panel_w >= dock::EDGE_MARGIN + side.max_w + HIT_MARGIN);
                assert!(side.panel_h >= side.max_h + 2.0 * HIT_MARGIN);
            }
        }
    }

    #[test]
    fn only_the_island_takes_the_mouse() {
        // Top dock: a 300 px chat centred in a 720 px panel, EDGE_MARGIN down.
        let r = IslandRect { x: 40.0, y: 16.0, w: 640.0, h: 300.0 };
        assert!(on_island(&r, 360.0, 150.0));
        assert!(on_island(&r, 40.0 - HIT_MARGIN, 316.0 + HIT_MARGIN));
        // Below the island, inside the taller panel: click-through.
        assert!(!on_island(&r, 360.0, 316.0 + HIT_MARGIN + 1.0));
        assert!(!on_island(&r, 360.0, PANEL_H - 1.0));
        // Beside it.
        assert!(!on_island(&r, 10.0, 150.0));
        // Hidden island (zero size) never takes the mouse.
        let hidden = IslandRect { x: 268.0, y: 0.0, w: 0.0, h: 0.0 };
        assert!(!on_island(&hidden, 268.0, 0.0));
    }

    #[test]
    fn side_docked_islands_take_the_mouse_on_their_side() {
        // Left dock: a vertical compact island against the left edge of a
        // 1040 x 900 panel.
        let pill = IslandRect { x: 16.0, y: 306.0, w: 32.0, h: 288.0 };
        assert!(on_island(&pill, 20.0, 450.0));
        assert!(on_island(&pill, 16.0 - HIT_MARGIN, 306.0 - HIT_MARGIN));
        assert!(!on_island(&pill, 48.0 + HIT_MARGIN + 1.0, 450.0), "right of it is click-through");
        assert!(!on_island(&pill, 30.0, 306.0 - HIT_MARGIN - 1.0), "above it too");
        // Right dock: an overview card against the right edge.
        let right = IslandRect { x: 1040.0 - 16.0 - 640.0, y: 370.0, w: 640.0, h: 160.0 };
        assert!(on_island(&right, 1040.0 - 20.0, 450.0));
        assert!(!on_island(&right, 1040.0 - 16.0 - 640.0 - HIT_MARGIN - 1.0, 450.0));
        // A hidden side island is a vertical line and still counts, like the top one.
        let hidden = IslandRect { x: 16.0, y: 358.0, w: 0.0, h: 184.0 };
        assert!(on_island(&hidden, 16.0 + HIT_MARGIN, 400.0));
        assert!(!on_island(&hidden, 16.0 + HIT_MARGIN + 1.0, 400.0));
    }

    #[test]
    fn a_held_button_only_takes_the_mouse_near_the_island() {
        let r = IslandRect { x: 264.0, y: 16.0, w: 640.0, h: 160.0 };
        assert!(near_island(&r, 264.0 - DROP_MARGIN, 176.0 + DROP_MARGIN, DROP_MARGIN));
        // Far below the island in the big transparent window: drags go to other apps.
        assert!(!near_island(&r, 584.0, 176.0 + DROP_MARGIN + 1.0, DROP_MARGIN));
        assert!(!near_island(&r, 584.0, 900.0, DROP_MARGIN));
    }
}
