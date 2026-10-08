// Plain-browser preview of the dock (`npm run dev`, no Tauri). The page can't
// move a native window there, so a simulated screen stands in for it:
//
//   ?dock=top|left|right   the edge (default: top)
//   ?pos=0..1              where along it (default: 0.5)
//   ?wa=1920x1040          the work area in logical px (default: the viewport)
//   ?zoom=1                show it at this scale, panned to the dock, instead of
//                          scaled to fit (for close-ups)
//   ?idle=N                the compact island becomes the line after N untouched
//                          seconds (1…3600), instead of the setting's minutes
//   ?view=tasks            opens that view after the greeting (menu, tasks, notes,
//                          reminders, habits, today, week, focus, settings, prompt…)
//
// The island's window (#root) is placed in the simulated screen the way Rust
// places the real one, and the screen is scaled down to fit the viewport.
// Never used inside the app.

import type { IslandViewName } from "../core/layout";
import {
  EDGE_MARGIN, parseEdge, previewCenter, previewLayout, previewMaxCenter,
  type DockEdge, type DockLayout,
} from "./dock";

export interface DevDock {
  edge: DockEdge;
  pos: number;
  work: { w: number; h: number };
  /** Fixed scale, panned to the dock point; null = fit the viewport. */
  zoom?: number | null;
}

/** The simulated dock from the query string. */
export function devDockFromUrl(search: string, viewport: { w: number; h: number }): DevDock {
  const q = new URLSearchParams(search);
  const edge = parseEdge(q.get("dock")) ?? "top";
  const pos = Number(q.get("pos") ?? "0.5");
  const m = /^(\d{3,5})x(\d{3,5})$/.exec(q.get("wa") ?? "");
  const work = m ? { w: Number(m[1]), h: Number(m[2]) } : { w: Math.round(viewport.w), h: Math.round(viewport.h) };
  const zoom = Number(q.get("zoom"));
  const dock: DevDock = { edge, pos: Number.isFinite(pos) ? Math.max(0, Math.min(1, pos)) : 0.5, work };
  if (q.has("zoom") && zoom > 0 && zoom <= 4) dock.zoom = zoom;
  return dock;
}

/** `?idle=N`: seconds before the line in the preview, or null to use the setting. */
export function devLineDelay(search: string): number | null {
  const raw = new URLSearchParams(search).get("idle");
  const n = raw == null ? NaN : Number(raw);
  return Number.isFinite(n) && n >= 1 && n <= 3600 ? n : null;
}

/** Views `?view=` may open: the ones that stand on their own without a session or a file. */
const DEV_VIEWS: readonly IslandViewName[] = [
  "activity",
  "overview", "empty", "prompt", "settings", "note",
  "menu", "tasks", "notes", "reminders", "habits", "today", "week", "focus",
];

/** `?view=NAME`: the view to open once the greeting is over, or null. */
export function devView(search: string): IslandViewName | null {
  const v = new URLSearchParams(search).get("view");
  return v != null && (DEV_VIEWS as readonly string[]).includes(v) ? (v as IslandViewName) : null;
}

export function devLayout(d: DevDock): DockLayout {
  return previewLayout(d.edge, d.pos, d.work);
}

/** Where the simulated window goes in the simulated screen (logical px). */
export function devWindowOrigin(d: DevDock, layout: DockLayout, maximized: boolean): { x: number; y: number } {
  const c = maximized ? previewMaxCenter(d.edge, d.pos, d.work) : previewCenter(d.edge, d.pos, d.work);
  switch (d.edge) {
    case "top":
      return { x: c - layout.panelW / 2, y: -EDGE_MARGIN };
    case "left":
      return { x: -EDGE_MARGIN, y: c - layout.panelH / 2 };
    case "right":
      return { x: d.work.w - layout.panelW + EDGE_MARGIN, y: c - layout.panelH / 2 };
  }
}

/** The simulated screen around #root; returns the function that re-places it. */
export function mountDevScreen(root: HTMLElement, dock: DevDock) {
  const screen = document.createElement("div");
  screen.id = "dev-screen";
  screen.style.width = `${dock.work.w}px`;
  screen.style.height = `${dock.work.h}px`;
  root.replaceWith(screen);
  screen.append(root);
  root.style.position = "absolute";
  root.style.inset = "auto";

  let scale = 1;
  const fit = () => {
    if (dock.zoom) {
      // A close-up: the dock point in view, the docked edge against the viewport's.
      scale = dock.zoom;
      const c = previewCenter(dock.edge, dock.pos, dock.work) * scale;
      const vw = window.innerWidth;
      const vh = window.innerHeight;
      const tx = dock.edge === "top" ? vw / 2 - c : dock.edge === "left" ? 0 : vw - dock.work.w * scale;
      const ty = dock.edge === "top" ? 0 : vh / 2 - c;
      screen.style.transform = `translate(${tx}px, ${ty}px) scale(${scale})`;
      return;
    }
    scale = Math.min(1, window.innerWidth / dock.work.w, window.innerHeight / dock.work.h);
    screen.style.transform = `scale(${scale})`;
  };
  fit();
  window.addEventListener("resize", fit);

  return {
    place(layout: DockLayout, maximized: boolean) {
      const o = devWindowOrigin(dock, layout, maximized);
      root.style.left = `${o.x}px`;
      root.style.top = `${o.y}px`;
      root.style.width = `${layout.panelW}px`;
      root.style.height = `${layout.panelH}px`;
    },
    /** Viewport coordinates → coordinates in the simulated window. */
    toWindow(x: number, y: number) {
      const r = root.getBoundingClientRect();
      return { x: (x - r.left) / scale, y: (y - r.top) / scale };
    },
  };
}

/**
 * `?chat=N`: a signed-in chat with N sample exchanges, so the chat's height
 * and the maximised island can be looked at without an account.
 */
export function seedDemoChat(search: string, lang: "fa" | "en", state: {
  roadeep: { signedIn: boolean | null };
  chatHistory: { id: number; role: "user" | "assistant"; content: string }[];
  notify(): void;
}) {
  const n = Math.max(0, Math.min(40, Number(new URLSearchParams(search).get("chat")) || 0));
  if (!n) return;
  const q = lang === "fa"
    ? ["این گزارش را خلاصه کن", "سه ایده برای عنوان بده", "متن را رسمی‌تر کن", "یک جدول از هزینه‌ها بساز"]
    : ["Summarise this report", "Give me three title ideas", "Make the text more formal", "Build a table of the costs"];
  const a = lang === "fa"
    ? "حتماً. نکته‌های اصلی این‌ها هستند: هدف پروژه روشن است، زمان‌بندی کمی فشرده است و بودجه برای مرحلهٔ دوم باید دوباره بررسی شود. اگر بخواهی، برای هر بخش یک پیشنهاد کوتاه هم می‌نویسم."
    : "Sure. The main points are: the project goal is clear, the schedule is a little tight, and the budget for the second phase should be reviewed again. If you like, I can add a short suggestion for each part.";
  state.roadeep.signedIn = true;
  state.chatHistory = [];
  for (let i = 0; i < n; i++) {
    state.chatHistory.push({ id: 10_000 + 2 * i, role: "user", content: q[i % q.length] });
    state.chatHistory.push({ id: 10_001 + 2 * i, role: "assistant", content: a });
  }
  state.notify();
}
