// Island docking, page side: how the island sits inside its window for each
// dock edge, its shape there, and the drag effects. Pure functions, unit-tested
// (dock.test.ts).
//
// Rust owns the dock (src-tauri/src/dock.rs): it places the window and sends
// the layout (window size, chat limit, maximised size). The window reaches
// EDGE_MARGIN past the screen edge, and the island is always centred along
// the edge inside it, so moving the island along the edge only moves the
// window. `previewLayout` repeats Rust's sizing for the plain-browser preview.

import { WAKE_STRIP_H, WAKE_STRIP_W, type IslandMode } from "../core/layout";

export type DockEdge = "top" | "left" | "right";
export const DOCK_EDGES: readonly DockEdge[] = ["top", "left", "right"];

/** Logical px between the window's edge-side border and the island (dock.rs EDGE_MARGIN). */
export const EDGE_MARGIN = 16;
/** Radius of the concave shoulders where the island meets the edge. */
export const SHOULDER = 20;
/** Same as the Rust hit test (src-tauri/src/island.rs). */
export const HIT_MARGIN = 14;

/** Window size, chat limit and maximised island for one dock, in logical px. */
export interface DockLayout {
  edge: DockEdge;
  panelW: number;
  panelH: number;
  chatMaxH: number;
  maxW: number;
  maxH: number;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export function parseEdge(value: unknown): DockEdge | null {
  return value === "top" || value === "left" || value === "right" ? value : null;
}

export const isSideEdge = (edge: DockEdge) => edge !== "top";

/**
 * The island's size on this edge: hidden and compact islands lie along the
 * edge, so on a side they stand up (width and height swap). Expanded views
 * keep their normal horizontal layout everywhere.
 */
export function dockedSize(edge: DockEdge, mode: IslandMode, size: { w: number; h: number }): { w: number; h: number } {
  return isSideEdge(edge) && mode !== "expanded" ? { w: size.h, h: size.w } : size;
}

/** The island's rect in its window: against the edge, centred along it. */
export function islandRectFor(edge: DockEdge, panel: { panelW: number; panelH: number }, w: number, h: number): Rect {
  switch (edge) {
    case "top":
      return { x: (panel.panelW - w) / 2, y: EDGE_MARGIN, w, h };
    case "left":
      return { x: EDGE_MARGIN, y: (panel.panelH - h) / 2, w, h };
    case "right":
      return { x: panel.panelW - EDGE_MARGIN - w, y: (panel.panelH - h) / 2, w, h };
  }
}

/** Centre of the island's rect in its window. */
export function islandCenter(edge: DockEdge, panel: { panelW: number; panelH: number }, w: number, h: number) {
  const r = islandRectFor(edge, panel, w, h);
  return { x: r.x + r.w / 2, y: r.y + r.h / 2 };
}

/**
 * How far the island moves inside the window when the layout changes (a new
 * edge or a new window size). Rust moves the window by the opposite, so the
 * island stays put on screen.
 */
export function layoutShift(
  from: { edge: DockEdge; panelW: number; panelH: number },
  to: { edge: DockEdge; panelW: number; panelH: number },
  w: number,
  h: number,
): { dx: number; dy: number } {
  const a = islandCenter(from.edge, from, w, h);
  const b = islandCenter(to.edge, to, w, h);
  return { dx: a.x - b.x, dy: a.y - b.y };
}

/**
 * CSS border-radius: the free corners are rounded; the two on the edge side
 * are square while docked (`attach` 1) and round off while the island floats
 * in a drag (`attach` 0).
 */
export function cornerRadii(edge: DockEdge, r: number, attach: number): string {
  const e = r * (1 - Math.max(0, Math.min(1, attach)));
  const px = (v: number) => `${Math.round(v * 100) / 100}px`;
  // top-left, top-right, bottom-right, bottom-left
  switch (edge) {
    case "top":
      return `${px(e)} ${px(e)} ${px(r)} ${px(r)}`;
    case "left":
      return `${px(e)} ${px(r)} ${px(r)} ${px(e)}`;
    case "right":
      return `${px(r)} ${px(e)} ${px(e)} ${px(r)}`;
  }
}

/** The shoulders shrink away with the island's depth and while it floats. */
export function shoulderSize(edge: DockEdge, w: number, h: number, attach: number): number {
  const depth = edge === "top" ? h : w;
  return Math.max(0, Math.min(SHOULDER, depth)) * Math.max(0, Math.min(1, attach));
}

/**
 * Where the wake strip sits in the full window: on the island's edge. While
 * the island is hidden it carries the line (style.css) and is what the mouse
 * finds; collapsed, the whole window is this strip (dock.rs strip_rect).
 */
export function wakeStripRect(edge: DockEdge, panel: { panelW: number; panelH: number }): Rect {
  const long = WAKE_STRIP_W;
  const thin = WAKE_STRIP_H;
  switch (edge) {
    case "top":
      return { x: (panel.panelW - long) / 2, y: EDGE_MARGIN, w: long, h: thin };
    case "left":
      return { x: EDGE_MARGIN, y: (panel.panelH - long) / 2, w: thin, h: long };
    case "right":
      return { x: panel.panelW - EDGE_MARGIN - thin, y: (panel.panelH - long) / 2, w: thin, h: long };
  }
}

/** The compact island's character and mini grid move to its top end when it stands up. */
export function sideBotPlacement<T extends { cx: number; cy: number }>(edge: DockEdge, mode: IslandMode, p: T): T {
  return isSideEdge(edge) && mode !== "expanded" ? { ...p, cx: p.cy, cy: p.cx } : p;
}

// ── Drag effects ──────────────────────────────────────────────────────────────

/** Speed (logical px/s) at which the stretch reaches its maximum. */
const STRETCH_SPEED = 2600;
const MAX_STRETCH = 0.22;
const MAX_TILT_DEG = 5;

/** Stretch vector (along the motion) and tilt for a drag velocity. */
export function dragEffect(vx: number, vy: number): { sx: number; sy: number; tilt: number } {
  let sx = vx / STRETCH_SPEED;
  let sy = vy / STRETCH_SPEED;
  const m = Math.hypot(sx, sy);
  if (m > MAX_STRETCH) {
    sx *= MAX_STRETCH / m;
    sy *= MAX_STRETCH / m;
  }
  const tilt = Math.max(-MAX_TILT_DEG, Math.min(MAX_TILT_DEG, (vx / 2200) * MAX_TILT_DEG));
  return { sx, sy, tilt };
}

/**
 * CSS transform for a stretch vector and tilt: longer along the motion,
 * thinner across it, leaning into it. "" when at rest.
 */
export function dragTransform(sx: number, sy: number, tilt: number): string {
  const m = Math.hypot(sx, sy);
  if (m < 0.002 && Math.abs(tilt) < 0.05) return "";
  const angle = (Math.atan2(sy, sx) * 180) / Math.PI;
  const r = (v: number) => Math.round(v * 1000) / 1000;
  return `rotate(${r(tilt)}deg) rotate(${r(angle)}deg) scale(${r(1 + m)}, ${r(1 - m * 0.6)}) rotate(${r(-angle)}deg)`;
}

/** Things a press must leave alone: it clicks, types, selects, scrolls or drops there. */
const NO_DRAG =
  'input, textarea, select, button, a, label, [contenteditable]:not([contenteditable="false"]), ' +
  '[role="button"], [role="slider"], [role="switch"], [role="radio"], [role="menuitem"], ' +
  ".chat-log, .hist, .upload-hit, #upload-layer, .md, .bubble, .reply, .code, .pill";

function isScrollable(el: Element): boolean {
  if (!(el instanceof HTMLElement)) return false;
  const style = getComputedStyle(el);
  const scrollY = /(auto|scroll)/.test(style.overflowY) && el.scrollHeight > el.clientHeight + 1;
  const scrollX = /(auto|scroll)/.test(style.overflowX) && el.scrollWidth > el.clientWidth + 1;
  return scrollY || scrollX;
}

function hasOwnText(el: Element): boolean {
  for (const node of el.childNodes) {
    if (node.nodeType === Node.TEXT_NODE && node.textContent?.trim()) return true;
  }
  return false;
}

/**
 * Whether a press on `target` inside the expanded island may start a drag:
 * the header bar or an empty margin of the card — never a control, text,
 * the chat log, anything that scrolls, or the drop zone.
 */
export function canDragFrom(target: EventTarget | null, island: Element): boolean {
  if (!(target instanceof Element) || !island.contains(target)) return false;
  if (target.closest(NO_DRAG)) return false;
  for (let el: Element | null = target; el && el !== island; el = el.parentElement) {
    if (isScrollable(el)) return false;
  }
  if (target.closest("#header")) return true;
  return !hasOwnText(target);
}

// ── Browser preview ───────────────────────────────────────────────────────────

const PANEL_W = 720;
const PANEL_H = 480;
const CHAT_MIN_H = 240;
const EXPANDED_W = 640;
const TOP_HALF = EXPANDED_W / 2 + SHOULDER;
const SIDE_HALF = PANEL_H / 2;
const CLEAR = 100;
const PAD = 10;

/** Centre along the edge, clamped like dock::dock_center (work-area logical px). */
export function previewCenter(edge: DockEdge, pos: number, work: { w: number; h: number }): number {
  const p = Number.isFinite(pos) ? Math.max(0, Math.min(1, pos)) : 0.5;
  const len = edge === "top" ? work.w : work.h;
  const half = edge === "top" ? TOP_HALF : SIDE_HALF;
  if (2 * half > len) return len / 2;
  return Math.max(half, Math.min(len - half, p * len));
}

/**
 * dock::layout for a work area of `work` logical px starting at the top of the
 * screen (the browser preview has no taskbar above it).
 */
export function previewLayout(edge: DockEdge, pos: number, work: { w: number; h: number }): DockLayout {
  if (edge === "top") {
    const chatMaxH = Math.max(CHAT_MIN_H, work.h - CLEAR);
    const maxW = Math.max(EXPANDED_W, Math.min(1100, work.w - 160));
    return {
      edge,
      panelW: Math.max(PANEL_W, maxW + 2 * (SHOULDER + HIT_MARGIN)),
      panelH: Math.max(PANEL_H, EDGE_MARGIN + chatMaxH + HIT_MARGIN + PAD),
      chatMaxH,
      maxW,
      maxH: chatMaxH,
    };
  }
  const c = previewCenter(edge, pos, work);
  const room = Math.min(c, work.h - c);
  const maxH = Math.max(CHAT_MIN_H, work.h - 2 * CLEAR);
  const chatMaxH = Math.max(CHAT_MIN_H, Math.min(maxH, 2 * (room - CLEAR)));
  const maxW = Math.max(EXPANDED_W, Math.min(1000, work.w - 120));
  return {
    edge,
    panelW: Math.max(PANEL_W, EDGE_MARGIN + maxW + HIT_MARGIN + PAD),
    panelH: Math.max(PANEL_H, maxH + 2 * (SHOULDER + HIT_MARGIN)),
    chatMaxH,
    maxW,
    maxH,
  };
}

/** dock::max_center for the preview. */
export function previewMaxCenter(edge: DockEdge, pos: number, work: { w: number; h: number }): number {
  if (edge !== "top") return work.h / 2;
  const half = previewLayout(edge, pos, work).maxW / 2 + SHOULDER;
  if (2 * half > work.w) return work.w / 2;
  return Math.max(half, Math.min(work.w - half, previewCenter(edge, pos, work)));
}

/** The default layout before Rust has said anything: today's top dock. */
export const DEFAULT_LAYOUT: DockLayout = {
  edge: "top", panelW: PANEL_W, panelH: PANEL_H, chatMaxH: 440, maxW: EXPANDED_W, maxH: 440,
};
