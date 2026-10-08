// How long the compact island waits, untouched, before it shrinks to a thin
// line on its edge (Settings → General). Stored in `settings.absenceInterval`,
// in seconds; 0 turns it off. Rust brings stored values into range on load
// (src-tauri/src/settings.rs, `sanitize_absence`); this guards the page against
// anything else (a stale window, the browser preview).

/** Seconds; what new installs get. */
export const DEFAULT_LINE_DELAY = 300;
/** Shorter than this and the island would fold away while being read. */
export const MIN_LINE_DELAY = 60;
/** A day. Also keeps setTimeout far from its ~24.8-day overflow (it fires at once past it). */
export const MAX_LINE_DELAY = 86_400;

/** The choices the settings window offers, in seconds: off, 1, 2, 5, 10, 15, 30 min. */
export const LINE_DELAY_OPTIONS: readonly number[] = [0, 60, 120, 300, 600, 900, 1800];

/** A stored value as the island uses it: 0 (off) or seconds within the limits. */
export function lineDelaySeconds(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) return DEFAULT_LINE_DELAY;
  if (value === 0) return 0;
  return Math.min(MAX_LINE_DELAY, Math.max(MIN_LINE_DELAY, value));
}

/** What `holdsLine` reads from the app state (core/state.ts). */
export interface LineHoldState {
  maximized: boolean;
  isPinned: boolean;
  pendingApproval: unknown;
  fileDragOver: boolean;
  tasks: readonly { state: string }[];
}

/**
 * Things that keep the compact island up even when nobody touches it: a
 * decision or an answer Claude Code waits for, a pinned alert, the maximised
 * chat, a drag of the island, a file on its way in, the open context menu.
 * The idle count then waits another full period (fsm.ts holdLine).
 */
export function holdsLine(
  s: LineHoldState,
  island: { dragging: boolean; menuOpen: boolean; uploading: boolean },
): boolean {
  return s.maximized || s.isPinned || s.pendingApproval != null || s.fileDragOver ||
    island.dragging || island.menuOpen || island.uploading ||
    s.tasks.some((t) => t.state === "approval" || t.state === "question");
}
