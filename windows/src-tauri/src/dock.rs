// Island docking: which screen edge the island hugs (top, left or right) and
// where along it, and the window rectangles that follow from that.
//
// Pure geometry on physical pixels — island.rs feeds it the real monitors — so
// every rule here is unit-tested. The front end lays the island out inside the
// window with the same constants (src/island/dock.ts).
//
// The window sits EDGE_MARGIN logical px *beyond* the screen edge it docks to.
// The island inside it starts at that margin, so on screen it touches the edge
// exactly; the margin only gives a dragged island room to squash and tilt
// without being cut off by its own window. Along the edge the island is always
// centred in its window, so moving the island along the edge (a drag, the
// maximised chat sliding to the middle) only ever moves the window — the page
// never has to re-lay itself out under a moving window.
//
// The window's size depends only on the display and the edge: it is the
// envelope of the largest island possible there (the maximised chat). It never
// changes with the island's state, so the page is never resized under a
// visible island; everything outside the island is click-through.

use serde::Serialize;

use crate::island::{HIT_MARGIN, PANEL_H, PANEL_W, STRIP_H, STRIP_W};

/// Logical px between the window's edge-side border and the island. Keep in
/// step with EDGE_MARGIN in src/island/dock.ts.
pub const EDGE_MARGIN: f64 = 16.0;
/// Radius of the concave shoulders where the island meets the edge.
pub const SHOULDER: f64 = 20.0;
/// Width of every expanded view; the chat's shortest height.
pub const EXPANDED_W: f64 = 640.0;
pub const CHAT_MIN_H: f64 = 240.0;
/// Shortest distance from the island's centre to either end of a top edge:
/// half the expanded width plus a shoulder.
const TOP_HALF: f64 = EXPANDED_W / 2.0 + SHOULDER;
/// Same for a side edge: room for the vertical compact island and for a chat
/// of 280 px that keeps SIDE_CLEAR free at both ends.
const SIDE_HALF: f64 = PANEL_H / 2.0;
/// Free space the chat leaves below it on a top dock, and at each end on a side dock.
pub const TOP_CLEAR: f64 = 100.0;
pub const SIDE_CLEAR: f64 = 100.0;
/// Maximised island: at most this wide, and this far from the screen sides
/// (top dock: half on each side; side dock: on the inner side).
pub const TOP_MAX_W: f64 = 1100.0;
pub const TOP_MAX_GAP: f64 = 160.0;
pub const SIDE_MAX_W: f64 = 1000.0;
pub const SIDE_MAX_GAP: f64 = 120.0;
/// Transparent room kept past the hit margin, so nothing touches the window border.
const PAD: f64 = 10.0;
/// While dragging, the edge already previewed keeps the lead by this many
/// logical px, so the preview does not flicker on a diagonal.
pub const PREVIEW_HYSTERESIS: f64 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Left,
    Right,
}

impl Edge {
    pub fn parse(s: &str) -> Option<Edge> {
        match s {
            "top" => Some(Edge::Top),
            "left" => Some(Edge::Left),
            "right" => Some(Edge::Right),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Edge::Top => "top",
            Edge::Left => "left",
            Edge::Right => "right",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    fn distance_sq(&self, px: f64, py: f64) -> f64 {
        let dx = (self.x - px).max(0.0).max(px - self.right());
        let dy = (self.y - py).max(0.0).max(py - self.bottom());
        dx * dx + dy * dy
    }
}

/// One display, in physical pixels of the virtual desktop.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    /// Stable name (`\\.\DISPLAY2`), or "x,y" when Windows gives none.
    pub id: String,
    pub bounds: Rect,
    /// The bounds minus the taskbar and other app bars.
    pub work: Rect,
    pub scale: f64,
}

fn sanitize_pos(pos: f64) -> f64 {
    if pos.is_finite() {
        pos.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

/// Physical coordinate of the island's centre along `edge` (x for the top,
/// y for the sides), clamped so the largest island stays inside the work area.
/// A work area too small for it gets its middle.
pub fn dock_center(s: &Screen, edge: Edge, pos: f64) -> f64 {
    let pos = sanitize_pos(pos);
    let (start, len, half) = match edge {
        Edge::Top => (s.work.x, s.work.w, TOP_HALF * s.scale),
        Edge::Left | Edge::Right => (s.work.y, s.work.h, SIDE_HALF * s.scale),
    };
    let lo = start + half;
    let hi = start + len - half;
    if lo > hi {
        start + len / 2.0
    } else {
        (start + pos * len).clamp(lo, hi)
    }
}

/// What the page needs to lay the island out for one dock, in logical px:
/// the window size, how tall the chat may grow, and the maximised island.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub edge: &'static str,
    pub panel_w: f64,
    pub panel_h: f64,
    pub chat_max_h: f64,
    pub max_w: f64,
    pub max_h: f64,
}

/// The layout for a dock. Only the side-dock chat limit depends on `pos`: a
/// chat centred on the dock keeps SIDE_CLEAR free at both ends of the work
/// area, so near an end it is shorter.
pub fn layout(s: &Screen, edge: Edge, pos: f64) -> Layout {
    let ww = s.work.w / s.scale;
    let wh = s.work.h / s.scale;
    match edge {
        Edge::Top => {
            // The island hangs from the top of the monitor itself.
            let room = (s.work.bottom() - s.bounds.y) / s.scale;
            let chat_max_h = (room - TOP_CLEAR).max(CHAT_MIN_H);
            let max_w = (ww - TOP_MAX_GAP).min(TOP_MAX_W).max(EXPANDED_W);
            Layout {
                edge: edge.as_str(),
                panel_w: (max_w + 2.0 * (SHOULDER + HIT_MARGIN)).max(PANEL_W),
                panel_h: (EDGE_MARGIN + chat_max_h + HIT_MARGIN + PAD).max(PANEL_H),
                chat_max_h,
                max_w,
                max_h: chat_max_h,
            }
        }
        Edge::Left | Edge::Right => {
            let c = dock_center(s, edge, pos);
            let room = ((c - s.work.y).min(s.work.bottom() - c)) / s.scale;
            let max_h = (wh - 2.0 * SIDE_CLEAR).max(CHAT_MIN_H);
            let chat_max_h = (2.0 * (room - SIDE_CLEAR)).min(max_h).max(CHAT_MIN_H);
            let max_w = (ww - SIDE_MAX_GAP).min(SIDE_MAX_W).max(EXPANDED_W);
            Layout {
                edge: edge.as_str(),
                panel_w: (EDGE_MARGIN + max_w + HIT_MARGIN + PAD).max(PANEL_W),
                panel_h: (max_h + 2.0 * (SHOULDER + HIT_MARGIN)).max(PANEL_H),
                chat_max_h,
                max_w,
                max_h,
            }
        }
    }
}

/// Where along the edge the maximised island is centred: on the dock when it
/// fits there (top), else pulled in; on a side, always the middle of the work
/// area, so it keeps SIDE_CLEAR free at both ends.
pub fn max_center(s: &Screen, edge: Edge, pos: f64) -> f64 {
    let c = dock_center(s, edge, pos);
    match edge {
        Edge::Top => {
            let half = (layout(s, edge, pos).max_w / 2.0 + SHOULDER) * s.scale;
            let (lo, hi) = (s.work.x + half, s.work.right() - half);
            if lo > hi {
                s.work.x + s.work.w / 2.0
            } else {
                c.clamp(lo, hi)
            }
        }
        Edge::Left | Edge::Right => s.work.y + s.work.h / 2.0,
    }
}

/// The island window for a dock, centred on `center` along the edge (from
/// `dock_center` or `max_center`). The top edge keeps today's placement —
/// against the top of the monitor itself — while the sides use the work area,
/// so a side taskbar is never covered.
pub fn panel_rect_at(s: &Screen, edge: Edge, pos: f64, center: f64) -> Rect {
    let l = layout(s, edge, pos);
    let (w, h, m) = (l.panel_w * s.scale, l.panel_h * s.scale, EDGE_MARGIN * s.scale);
    match edge {
        Edge::Top => Rect { x: center - w / 2.0, y: s.bounds.y - m, w, h },
        Edge::Left => Rect { x: s.work.x - m, y: center - h / 2.0, w, h },
        Edge::Right => Rect { x: s.work.right() - w + m, y: center - h / 2.0, w, h },
    }
}

/// The island window for a dock, normal or maximised.
pub fn panel_rect(s: &Screen, edge: Edge, pos: f64, maximized: bool) -> Rect {
    let center = if maximized { max_center(s, edge, pos) } else { dock_center(s, edge, pos) };
    panel_rect_at(s, edge, pos, center)
}

/// The wake strip of a hidden island, which shows its line: a thin band along
/// the docked edge, centred on the dock. It has to be on screen to meet the
/// cursor, so it has no margin.
pub fn strip_rect(s: &Screen, edge: Edge, pos: f64) -> Rect {
    let c = dock_center(s, edge, pos);
    let (long, thin) = (STRIP_W * s.scale, STRIP_H * s.scale);
    match edge {
        Edge::Top => Rect { x: c - long / 2.0, y: s.bounds.y, w: long, h: thin },
        Edge::Left => Rect { x: s.work.x, y: c - long / 2.0, w: thin, h: long },
        Edge::Right => Rect { x: s.work.right() - thin, y: c - long / 2.0, w: thin, h: long },
    }
}

/// The dock edge nearest to a point. The bottom is never a target: the taskbar
/// lives there. `current` (the edge already previewed) wins ties within
/// `hysteresis` physical px.
pub fn nearest_edge(work: &Rect, px: f64, py: f64, current: Option<Edge>, hysteresis: f64) -> Edge {
    let mut best = (Edge::Top, f64::INFINITY);
    for (edge, d) in [
        (Edge::Top, py - work.y),
        (Edge::Left, px - work.x),
        (Edge::Right, work.right() - px),
    ] {
        let d = if Some(edge) == current { d - hysteresis } else { d };
        if d < best.1 {
            best = (edge, d);
        }
    }
    best.0
}

/// The `pos` (0…1 along the work area) that puts the island's centre as close
/// to (px, py) as the clamp in `dock_center` allows.
pub fn pos_at(s: &Screen, edge: Edge, px: f64, py: f64) -> f64 {
    let (start, len, along) = match edge {
        Edge::Top => (s.work.x, s.work.w, px),
        Edge::Left | Edge::Right => (s.work.y, s.work.h, py),
    };
    if len <= 0.0 {
        return 0.5;
    }
    let raw = ((along - start) / len).clamp(0.0, 1.0);
    let c = dock_center(s, edge, raw);
    ((c - start) / len).clamp(0.0, 1.0)
}

/// The screen holding a point, or the nearest one when it sits in a gap
/// between displays.
pub fn screen_at(screens: &[Screen], px: f64, py: f64) -> Option<usize> {
    if let Some(i) = screens.iter().position(|s| s.bounds.contains(px, py)) {
        return Some(i);
    }
    screens
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.bounds.distance_sq(px, py).total_cmp(&b.1.bounds.distance_sq(px, py)))
        .map(|(i, _)| i)
}

/// The display the island lives on: the one the user dragged it to while it
/// is still connected, else the `screen` preference ("primary", or "cursor" =
/// under the cursor), else the first display.
pub fn resolve_screen(
    screens: &[Screen],
    primary: Option<&str>,
    pref: &str,
    dock_monitor: &str,
    cursor: Option<(f64, f64)>,
) -> Option<usize> {
    if !dock_monitor.is_empty() {
        if let Some(i) = screens.iter().position(|s| s.id == dock_monitor) {
            return Some(i);
        }
    }
    if pref == "cursor" {
        if let Some((cx, cy)) = cursor {
            if let Some(i) = screens.iter().position(|s| s.bounds.contains(cx, cy)) {
                return Some(i);
            }
        }
    }
    if let Some(p) = primary {
        if let Some(i) = screens.iter().position(|s| s.id == p) {
            return Some(i);
        }
    }
    (!screens.is_empty()).then_some(0)
}

/// SwiftUI-style spring (ω₀ = 2π / response, ζ = damping), sub-stepped like
/// `Spring` in src/core/anim.ts so the window lands with the island's feel.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f64,
    pub velocity: f64,
    pub target: f64,
    omega: f64,
    zeta: f64,
}

impl Spring {
    pub fn new(value: f64, velocity: f64, target: f64, response: f64, damping: f64) -> Self {
        Self { value, velocity, target, omega: 2.0 * std::f64::consts::PI / response, zeta: damping }
    }

    pub fn step(&mut self, dt: f64) {
        let steps = (dt / (1.0 / 240.0)).ceil().max(1.0) as u32;
        let h = dt / steps as f64;
        for _ in 0..steps {
            let acc = self.omega * self.omega * (self.target - self.value) - 2.0 * self.zeta * self.omega * self.velocity;
            self.velocity += acc * h;
            self.value += self.velocity * h;
        }
    }

    pub fn settled(&self, eps: f64, eps_v: f64) -> bool {
        (self.target - self.value).abs() < eps && self.velocity.abs() < eps_v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1920×1080 at 100 %, taskbar 48 px at the bottom.
    fn fhd() -> Screen {
        Screen {
            id: r"\\.\DISPLAY1".into(),
            bounds: Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 },
            work: Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1032.0 },
            scale: 1.0,
        }
    }

    /// A 4K display at 150 % to the right of the first one, taskbar on its left.
    fn uhd_right() -> Screen {
        Screen {
            id: r"\\.\DISPLAY2".into(),
            bounds: Rect { x: 1920.0, y: 0.0, w: 3840.0, h: 2160.0 },
            work: Rect { x: 1920.0 + 72.0, y: 0.0, w: 3840.0 - 72.0, h: 2160.0 },
            scale: 1.5,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn edge_names_round_trip() {
        for e in [Edge::Top, Edge::Left, Edge::Right] {
            assert_eq!(Edge::parse(e.as_str()), Some(e));
        }
        assert_eq!(Edge::parse("bottom"), None);
        assert_eq!(Edge::parse("TOP"), None);
    }

    #[test]
    fn default_top_dock_is_todays_placement() {
        let s = fhd();
        let l = layout(&s, Edge::Top, 0.5);
        let r = panel_rect(&s, Edge::Top, 0.5, false);
        // Centred on the screen, margin above its top, as large as the envelope.
        assert!(close(r.x + r.w / 2.0, 960.0));
        assert!(close(r.y, -EDGE_MARGIN));
        assert!(close(r.w, l.panel_w) && close(r.h, l.panel_h));
        // So the island (EDGE_MARGIN inside the window) touches y = 0.
        assert!(close(r.y + EDGE_MARGIN, 0.0));
        let strip = strip_rect(&s, Edge::Top, 0.5);
        assert_eq!(strip, Rect { x: (1920.0 - STRIP_W) / 2.0, y: 0.0, w: STRIP_W, h: STRIP_H });
    }

    #[test]
    fn top_layout_grows_the_chat_to_100_px_above_the_work_area() {
        let s = fhd();
        let l = layout(&s, Edge::Top, 0.5);
        assert_eq!(l.edge, "top");
        assert!(close(l.chat_max_h, 1032.0 - TOP_CLEAR));
        assert!(close(l.max_h, l.chat_max_h));
        assert!(close(l.max_w, TOP_MAX_W));
        // The window holds the maximised island, its shoulders and the hit margin.
        assert!(l.panel_w >= l.max_w + 2.0 * (SHOULDER + HIT_MARGIN));
        assert!(l.panel_h >= EDGE_MARGIN + l.max_h + HIT_MARGIN);
        assert!(close(l.panel_w, 1168.0) && close(l.panel_h, 972.0));
        // A narrow display: 160 px kept free at the sides.
        let narrow = Screen {
            work: Rect { x: 0.0, y: 0.0, w: 1024.0, h: 720.0 },
            bounds: Rect { x: 0.0, y: 0.0, w: 1024.0, h: 768.0 },
            ..fhd()
        };
        let n = layout(&narrow, Edge::Top, 0.5);
        assert!(close(n.max_w, 1024.0 - TOP_MAX_GAP) && close(n.chat_max_h, 620.0));
    }

    #[test]
    fn side_docks_hug_the_work_area() {
        let s = fhd();
        let l = layout(&s, Edge::Left, 0.5);
        assert_eq!(l.edge, "left");
        assert!(close(l.max_w, SIDE_MAX_W) && close(l.max_h, 1032.0 - 2.0 * SIDE_CLEAR));
        assert!(close(l.chat_max_h, l.max_h), "centred: the chat may use the whole height");
        assert!(l.panel_w >= EDGE_MARGIN + l.max_w + HIT_MARGIN);
        assert!(l.panel_h >= l.max_h + 2.0 * SHOULDER);

        let left = panel_rect(&s, Edge::Left, 0.5, false);
        assert!(close(left.x, -EDGE_MARGIN));
        assert!(close(left.y + left.h / 2.0, 516.0), "centred on the work area, not the monitor");
        let right = panel_rect(&s, Edge::Right, 0.5, false);
        assert!(close(right.right() - EDGE_MARGIN, 1920.0));
        assert!(close(right.y, left.y) && close(right.w, l.panel_w));

        let ls = strip_rect(&s, Edge::Left, 0.5);
        assert_eq!(ls, Rect { x: 0.0, y: 516.0 - STRIP_W / 2.0, w: STRIP_H, h: STRIP_W });
        let rs = strip_rect(&s, Edge::Right, 0.5);
        assert_eq!(rs, Rect { x: 1920.0 - STRIP_H, y: 516.0 - STRIP_W / 2.0, w: STRIP_H, h: STRIP_W });
    }

    #[test]
    fn the_island_never_leaves_the_work_area_along_the_edge() {
        let s = fhd();
        for pos in [0.0, 0.01, 0.2, 0.5, 0.9, 1.0, -3.0, 7.0, f64::NAN] {
            for edge in [Edge::Left, Edge::Right] {
                let l = layout(&s, edge, pos);
                let c = dock_center(&s, edge, pos);
                // The chat, centred on the dock, keeps 100 px free at both ends...
                assert!(c - l.chat_max_h / 2.0 >= s.work.y + SIDE_CLEAR - 1e-9, "{edge:?} {pos}");
                assert!(c + l.chat_max_h / 2.0 <= s.work.bottom() - SIDE_CLEAR + 1e-9, "{edge:?} {pos}");
                // ...and so does the maximised island, centred on the work area.
                let m = max_center(&s, edge, pos);
                assert!(close(m - l.max_h / 2.0, s.work.y + SIDE_CLEAR));
                assert!(close(m + l.max_h / 2.0, s.work.bottom() - SIDE_CLEAR));
                // The window is centred on the island, normal or maximised.
                let r = panel_rect(&s, edge, pos, true);
                assert!(close(r.y + r.h / 2.0, m));
            }
            let l = layout(&s, Edge::Top, pos);
            let c = dock_center(&s, Edge::Top, pos);
            assert!(c - EXPANDED_W / 2.0 - SHOULDER >= s.work.x - 1e-9, "{pos}");
            assert!(c + EXPANDED_W / 2.0 + SHOULDER <= s.work.right() + 1e-9, "{pos}");
            let m = max_center(&s, Edge::Top, pos);
            assert!(m - l.max_w / 2.0 - SHOULDER >= s.work.x - 1e-9, "{pos}");
            assert!(m + l.max_w / 2.0 + SHOULDER <= s.work.right() + 1e-9, "{pos}");
        }
        assert!(close(dock_center(&s, Edge::Top, f64::NAN), 960.0), "NaN means the middle");
    }

    #[test]
    fn a_side_chat_near_an_end_is_shorter() {
        let s = fhd();
        let near_top = layout(&s, Edge::Left, 0.0);
        // Centre clamped to 240 px from the top: 140 px of chat each side of it.
        assert!(close(near_top.chat_max_h, 2.0 * (SIDE_HALF - SIDE_CLEAR)));
        assert!(near_top.chat_max_h >= CHAT_MIN_H);
        // The window size never depends on pos: no resize when the island moves.
        let mid = layout(&s, Edge::Left, 0.5);
        assert!(close(near_top.panel_w, mid.panel_w) && close(near_top.panel_h, mid.panel_h));
    }

    #[test]
    fn maximised_top_island_is_pulled_in_from_the_corner() {
        let s = fhd();
        // Dock at the far left: the 1100 px island is centred 570 px in.
        assert!(close(max_center(&s, Edge::Top, 0.0), 550.0 + SHOULDER));
        assert!(close(max_center(&s, Edge::Top, 0.5), 960.0));
        let r = panel_rect(&s, Edge::Top, 0.0, true);
        assert!(close(r.x + r.w / 2.0, 570.0) && close(r.y, -EDGE_MARGIN));
    }

    #[test]
    fn a_tiny_work_area_gets_its_middle_and_minimum_sizes() {
        let s = Screen {
            id: "x".into(),
            bounds: Rect { x: 0.0, y: 0.0, w: 600.0, h: 400.0 },
            work: Rect { x: 0.0, y: 0.0, w: 600.0, h: 360.0 },
            scale: 1.0,
        };
        assert!(close(dock_center(&s, Edge::Top, 0.0), 300.0));
        assert!(close(dock_center(&s, Edge::Left, 1.0), 180.0));
        let l = layout(&s, Edge::Top, 0.5);
        assert!(close(l.chat_max_h, 260.0) && close(l.max_w, EXPANDED_W));
        assert!(close(l.panel_w, PANEL_W) && close(l.panel_h, PANEL_H));
        let side = layout(&s, Edge::Right, 0.5);
        assert!(close(side.chat_max_h, CHAT_MIN_H) && close(side.max_h, CHAT_MIN_H));
        assert!(close(max_center(&s, Edge::Top, 0.0), 300.0));
    }

    #[test]
    fn dpi_scale_applies_to_every_size_and_margin() {
        let s = uhd_right();
        let l = layout(&s, Edge::Left, 0.5);
        // Logical limits from the 2512 x 1440 logical work area.
        assert!(close(l.max_h, 1440.0 - 2.0 * SIDE_CLEAR) && close(l.max_w, SIDE_MAX_W));
        let r = panel_rect(&s, Edge::Left, 0.5, false);
        assert!(close(r.w, l.panel_w * 1.5) && close(r.h, l.panel_h * 1.5));
        assert!(close(r.x, 1920.0 + 72.0 - EDGE_MARGIN * 1.5), "left of the work area, past the side taskbar");
        let t = panel_rect(&s, Edge::Top, 0.0, false);
        // Clamped by 340 logical px = 510 physical from the work area's left.
        assert!(close(t.x + t.w / 2.0, 1920.0 + 72.0 + 510.0));
        assert!(close(t.y, -EDGE_MARGIN * 1.5));
        let top = layout(&s, Edge::Top, 0.0);
        assert!(close(top.chat_max_h, 1440.0 - TOP_CLEAR));
        let strip = strip_rect(&s, Edge::Right, 1.0);
        assert!(close(strip.w, STRIP_H * 1.5) && close(strip.h, STRIP_W * 1.5));
        assert!(close(strip.right(), 1920.0 + 3840.0));
        assert!(close(strip.y + strip.h / 2.0, 2160.0 - SIDE_HALF * 1.5));
    }

    #[test]
    fn layout_serializes_for_the_page() {
        let json = serde_json::to_value(layout(&fhd(), Edge::Right, 0.5)).unwrap();
        assert_eq!(json["edge"], "right");
        for key in ["panelW", "panelH", "chatMaxH", "maxW", "maxH"] {
            assert!(json[key].is_number(), "{key}");
        }
    }

    #[test]
    fn nearest_edge_ignores_the_bottom() {
        let w = fhd().work;
        assert_eq!(nearest_edge(&w, 960.0, 40.0, None, 0.0), Edge::Top);
        assert_eq!(nearest_edge(&w, 30.0, 500.0, None, 0.0), Edge::Left);
        assert_eq!(nearest_edge(&w, 1900.0, 500.0, None, 0.0), Edge::Right);
        // Bottom-centre: the sides are nearer than the top, never the bottom.
        assert_eq!(nearest_edge(&w, 900.0, 1020.0, None, 0.0), Edge::Left);
        assert_eq!(nearest_edge(&w, 1020.0, 1020.0, None, 0.0), Edge::Right);
        // Over the taskbar or off the left of the screen still works.
        assert_eq!(nearest_edge(&w, -40.0, 700.0, None, 0.0), Edge::Left);
    }

    #[test]
    fn the_previewed_edge_keeps_the_lead_within_the_hysteresis() {
        let w = fhd().work;
        // Diagonal near the top-left corner: top is 2 px nearer.
        assert_eq!(nearest_edge(&w, 100.0, 98.0, None, 0.0), Edge::Top);
        assert_eq!(nearest_edge(&w, 100.0, 98.0, Some(Edge::Left), PREVIEW_HYSTERESIS), Edge::Left);
        assert_eq!(nearest_edge(&w, 100.0, 60.0, Some(Edge::Left), PREVIEW_HYSTERESIS), Edge::Top);
    }

    #[test]
    fn pos_follows_the_point_and_is_clamped() {
        let s = fhd();
        assert!(close(pos_at(&s, Edge::Top, 960.0, 10.0), 0.5));
        assert!(close(pos_at(&s, Edge::Top, 1440.0, 10.0), 0.75));
        // Too near the corner: pulled in to 340 px.
        assert!(close(pos_at(&s, Edge::Top, 5.0, 10.0), 340.0 / 1920.0));
        assert!(close(pos_at(&s, Edge::Left, 10.0, 0.0), 240.0 / 1032.0));
        assert!(close(pos_at(&s, Edge::Right, 1910.0, 5000.0), (1032.0 - 240.0) / 1032.0));
        // Round trip: the pos lands the centre back on the point.
        let p = pos_at(&s, Edge::Left, 0.0, 700.0);
        assert!(close(dock_center(&s, Edge::Left, p), 700.0));
    }

    #[test]
    fn screens_are_found_by_point_and_by_nearest() {
        let screens = [fhd(), uhd_right()];
        assert_eq!(screen_at(&screens, 100.0, 100.0), Some(0));
        assert_eq!(screen_at(&screens, 2500.0, 1500.0), Some(1));
        // Below the short first display: nearest is the first one.
        assert_eq!(screen_at(&screens, 500.0, 1500.0), Some(0));
        assert_eq!(screen_at(&[], 0.0, 0.0), None);
    }

    #[test]
    fn the_dragged_to_display_wins_while_it_is_connected() {
        let screens = [fhd(), uhd_right()];
        let primary = Some(r"\\.\DISPLAY1");
        assert_eq!(resolve_screen(&screens, primary, "primary", "", None), Some(0));
        assert_eq!(resolve_screen(&screens, primary, "primary", r"\\.\DISPLAY2", None), Some(1));
        // Unplugged: back to the primary display.
        assert_eq!(resolve_screen(&screens, primary, "primary", r"\\.\DISPLAY9", None), Some(0));
        assert_eq!(resolve_screen(&screens, primary, "cursor", "", Some((3000.0, 10.0))), Some(1));
        assert_eq!(resolve_screen(&screens, primary, "cursor", "", None), Some(0));
        assert_eq!(resolve_screen(&screens, None, "primary", "", None), Some(0));
        assert_eq!(resolve_screen(&[], primary, "primary", "", None), None);
    }

    #[test]
    fn the_settle_spring_overshoots_then_lands() {
        let mut s = Spring::new(0.0, 0.0, 100.0, 0.5, 0.62);
        let mut peak: f64 = 0.0;
        for _ in 0..240 {
            s.step(1.0 / 60.0);
            peak = peak.max(s.value);
        }
        assert!(peak > 100.5, "springy: it overshoots ({peak})");
        assert!(s.settled(0.5, 30.0), "and settles within 4 s");
    }
}
