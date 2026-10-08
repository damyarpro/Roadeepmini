// Custom right-click menu for both windows. Replaces WebView2's own menu
// (Back / Reload / Inspect…), which makes no sense in a desktop app.
//
// The native menu is kept where it is actually useful:
// - on editable targets (input, textarea, select, contenteditable) for
//   cut / copy / paste;
// - while text is selected, so Copy keeps working. This was chosen over a
//   custom "Copy" item: the native menu already does it right (formats,
//   clipboard permissions, selection inside shadow trees) with no code here.

import { isRtl, t } from "./i18n";
import { Bridge } from "./bridge";
import "./context-menu.css";

export type MenuIcon = "settings" | "power" | "minimize" | "dock";

export interface MenuAction {
  id: string;
  label: string;
  icon?: MenuIcon;
  /** Tinted red; used for Quit. */
  danger?: boolean;
  run: () => void;
}

export type MenuEntry = MenuAction | "separator";

/** Window-coordinate rectangle, as pushed to Rust for the click-through test. */
export interface MenuRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface ContextMenuOptions {
  /**
   * Entries for a right-click on `target`. Returning null swallows the event
   * and shows nothing (e.g. the island is collapsed to its wake strip).
   */
  entries: (target: Element) => MenuEntry[] | null;
  /** The menu is on screen at `rect` (after flipping and clamping). */
  onOpen?: (rect: MenuRect) => void;
  /** The menu is gone. Called exactly once per onOpen. */
  onClose?: () => void;
}

/** Gap kept between the menu and the window edge. */
const EDGE = 6;

const EDITABLE =
  'input, textarea, select, [contenteditable]:not([contenteditable="false"])';

const ICONS: Record<MenuIcon, string> = {
  settings:
    '<path d="M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z"/>',
  power: '<path d="M12 2v10"/><path d="M18.36 6.64a9 9 0 1 1-12.73 0"/>',
  minimize: '<path d="M4 14h6v6"/><path d="M20 10h-6V4"/><path d="M14 10l7-7"/><path d="M3 21l7-7"/>',
  // An arrow back up to the top edge.
  dock: '<path d="M4 4h16"/><path d="M12 20V9"/><path d="M7 13l5-5 5 5"/>',
};

function svgIcon(name: MenuIcon): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", "16");
  svg.setAttribute("height", "16");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.8");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("ctx-icon");
  // Static, module-owned markup — never user input.
  svg.innerHTML = ICONS[name];
  return svg;
}

/** True where the native menu (cut / copy / paste) should stay. */
function wantsNativeMenu(target: EventTarget | null): boolean {
  if (target instanceof Element && target.closest(EDITABLE)) return true;
  if (target instanceof HTMLElement && target.isContentEditable) return true;
  const selection = window.getSelection();
  return !!selection && !selection.isCollapsed && selection.toString().trim().length > 0;
}

/**
 * Places a `w`×`h` menu for a click at (x, y): it opens toward the inline end
 * (right in LTR, left in RTL) and downwards, flips when that side has no room,
 * and is finally clamped inside the window.
 */
function place(x: number, y: number, w: number, h: number, rtl: boolean): { left: number; top: number } {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  let left = rtl ? x - w : x;
  if (rtl && left < EDGE) left = x;
  if (!rtl && left + w > vw - EDGE) left = x - w;
  let top = y;
  if (top + h > vh - EDGE) top = y - h;
  left = Math.min(Math.max(EDGE, left), Math.max(EDGE, vw - w - EDGE));
  top = Math.min(Math.max(EDGE, top), Math.max(EDGE, vh - h - EDGE));
  return { left, top };
}

/**
 * Takes over `contextmenu` for the whole document. Returns a function that
 * removes the listener and closes any open menu.
 */
export function installContextMenu(opts: ContextMenuOptions): () => void {
  let menu: HTMLElement | null = null;
  let returnFocus: HTMLElement | null = null;
  let teardown: (() => void) | null = null;

  const items = () =>
    menu ? [...menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')] : [];

  function close(restoreFocus: boolean) {
    if (!menu) return;
    const el = menu;
    menu = null;
    teardown?.();
    teardown = null;
    el.remove();
    if (restoreFocus && returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
    returnFocus = null;
    try {
      opts.onClose?.();
    } catch (err) {
      console.error("[roadeep] context menu onClose failed", err);
      void Bridge.log(`context menu onClose failed: ${String(err)}`);
    }
  }

  function activate(entry: MenuAction) {
    close(true);
    try {
      entry.run();
    } catch (err) {
      console.error(`[roadeep] context menu action ${entry.id} failed`, err);
      void Bridge.log(`context menu action ${entry.id} failed: ${String(err)}`);
    }
  }

  function onKey(e: KeyboardEvent) {
    const list = items();
    const at = list.indexOf(document.activeElement as HTMLButtonElement);
    const move = (i: number) => list[(i + list.length) % list.length]?.focus();
    switch (e.key) {
      case "ArrowDown":
        move(at < 0 ? 0 : at + 1);
        break;
      case "ArrowUp":
        move(at < 0 ? list.length - 1 : at - 1);
        break;
      case "Home":
        move(0);
        break;
      case "End":
        move(list.length - 1);
        break;
      case "Escape":
        close(true);
        break;
      case "Tab":
        close(true);
        break;
      default:
        return; // Enter / Space reach the focused <button> as a click.
    }
    e.preventDefault();
    // The island collapses on Escape from a window-level listener.
    e.stopPropagation();
  }

  function open(x: number, y: number, entries: MenuEntry[]) {
    const rtl = isRtl();
    const el = document.createElement("div");
    el.className = "ctx-menu";
    el.setAttribute("role", "menu");
    el.setAttribute("aria-label", t("ctx.menu"));
    el.dir = rtl ? "rtl" : "ltr";
    el.tabIndex = -1;

    for (const entry of entries) {
      if (entry === "separator") {
        const sep = document.createElement("div");
        sep.className = "ctx-sep";
        sep.setAttribute("role", "separator");
        el.append(sep);
        continue;
      }
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = entry.danger ? "ctx-item danger" : "ctx-item";
      btn.setAttribute("role", "menuitem");
      btn.tabIndex = -1;
      btn.dataset.id = entry.id;
      if (entry.icon) btn.append(svgIcon(entry.icon));
      const label = document.createElement("span");
      label.className = "ctx-label";
      label.textContent = entry.label;
      btn.append(label);
      btn.addEventListener("click", () => activate(entry));
      // Hover and keyboard share one highlight: the focused item.
      btn.addEventListener("pointermove", () => {
        if (document.activeElement !== btn) btn.focus({ preventScroll: true });
      });
      el.append(btn);
    }
    el.addEventListener("keydown", onKey);
    el.addEventListener("contextmenu", (e) => e.preventDefault());

    // Measured off-screen first, so flipping uses the real size.
    el.style.left = "-9999px";
    el.style.top = "-9999px";
    document.body.append(el);
    const { width, height } = el.getBoundingClientRect();
    const { left, top } = place(x, y, width, height, rtl);
    el.style.left = `${left}px`;
    el.style.top = `${top}px`;

    menu = el;
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;

    const onPointerDown = (e: PointerEvent) => {
      if (!el.contains(e.target as Node)) close(false);
    };
    const onLeave = () => close(false);
    const onHidden = () => {
      if (document.hidden) close(false);
    };
    // Registered after this event finishes, so the opening click can't close it.
    const timer = window.setTimeout(() => {
      document.addEventListener("pointerdown", onPointerDown, true);
    }, 0);
    document.addEventListener("scroll", onLeave, true);
    document.addEventListener("wheel", onLeave, { capture: true, passive: true });
    window.addEventListener("resize", onLeave);
    window.addEventListener("blur", onLeave);
    document.addEventListener("visibilitychange", onHidden);
    teardown = () => {
      window.clearTimeout(timer);
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("scroll", onLeave, true);
      document.removeEventListener("wheel", onLeave, true);
      window.removeEventListener("resize", onLeave);
      window.removeEventListener("blur", onLeave);
      document.removeEventListener("visibilitychange", onHidden);
    };

    try {
      opts.onOpen?.({ x: left, y: top, w: width, h: height });
    } catch (err) {
      console.error("[roadeep] context menu onOpen failed", err);
      void Bridge.log(`context menu onOpen failed: ${String(err)}`);
    }
    items()[0]?.focus({ preventScroll: true });
  }

  const onContextMenu = (e: MouseEvent) => {
    if (menu && menu.contains(e.target as Node)) {
      e.preventDefault();
      return;
    }
    if (wantsNativeMenu(e.target)) {
      close(false);
      return;
    }
    e.preventDefault();
    close(false);
    const target = e.target instanceof Element ? e.target : document.body;
    const entries = opts.entries(target);
    if (!entries || !entries.some((x) => x !== "separator")) return;
    open(e.clientX, e.clientY, entries);
  };

  document.addEventListener("contextmenu", onContextMenu);
  return () => {
    document.removeEventListener("contextmenu", onContextMenu);
    close(false);
  };
}

/** The Quit entry both windows share. */
export function quitEntry(): MenuAction {
  return {
    id: "quit",
    label: t("ctx.quit"),
    icon: "power",
    danger: true,
    run: () => {
      void Bridge.log("context menu: quit");
      void Bridge.quit();
    },
  };
}
