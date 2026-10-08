// Overview task ticker — port of TickerView (V2) from IslandViewContent.swift.
//
// Three rows: completed (A), current → completed (B), incoming (C). Every row
// position is recomputed from a single clock in `tick()`, driven by the island's
// frame loop — no CSS transitions and no timers. Chaining CSS transitions with a
// reset timer let two rows land on the same line when steps arrived in bursts,
// and any step that arrived mid-animation was dropped outright. Steps are now
// queued instead, so a burst scrolls past rather than vanishing.
//
// Diff steps (core/diff.ts) show as the file name followed by +N −M, and a
// click on one opens its diff. Once a turn is over the rows hold still on its
// final message: no shimmer, no chevron, the ✓ on every row (TickerRowView
// isActive = false on macOS).

import { h, svg } from "./dom";
import { ICONS } from "./icons";
import { cubicBezier, clamp, lerp } from "../core/anim";
import { parseDiffStep, type DiffStep } from "../core/diff";
import type { AgentTask } from "../core/state";
import { isRtl, t } from "../core/i18n";
import "../core/locales/r2-messages";

const ROW_H = 22;
/** One step transition, milliseconds. */
const DURATION = 380;
/** Beyond this many queued steps we stop trying to show them all. */
const MAX_QUEUE = 4;
const COMPLETED_SCALE = 11.5 / 13; // 0.885 — the completed font size
const EASE = cubicBezier(0.4, 0, 0.2, 1);

/** The current row's text once the turn is over (TickerRowView staticColor). */
const STILL_CURRENT = "#c9cdd4";
const DIM = "#6b7079";

/** What the island knows about the task's session beyond the task itself. */
export interface TickerLive {
  /**
   * Position of the newest step in the whole session. A hook-fed task keeps
   * only its last 20 steps, so its stepIndex stops moving once the list is
   * full; this keeps counting (island/live-session.ts).
   */
  seq?: number;
  /** The turn is over: the rows hold still on its final message. */
  still?: boolean;
}

interface Row {
  el: HTMLElement;
  chevron: SVGElement;
  check: SVGElement;
  body: HTMLElement;
  shimmer: HTMLElement;
  dim: HTMLElement;
  /** +N −M, attached to the row only while it shows a diff. */
  counts: HTMLElement;
  plus: HTMLElement;
  minus: HTMLElement;
  text: string;
  /** Set while the row shows a file diff. */
  diff: DiffStep | null;
}

function makeRow(): Row {
  const chevron = svg(ICONS.chevronRight, 9, { stroke: 2.4 });
  const check = svg(ICONS.check, 8, { stroke: 2.2 });
  check.style.color = "#454850"; // the completed tick is dimmer than the chevron
  check.style.position = "absolute";
  chevron.style.position = "absolute";
  chevron.classList.add("tick-chevron");
  // Steps can be Persian or English whatever the UI language, so each one
  // orders its own words (dir=auto); the row follows the UI direction.
  const shimmer = h("span", { class: "tick-text shimmer", dir: "auto" });
  const dim = h("span", {
    class: "tick-text",
    dir: "auto",
    // Pinned to the shimmer's box. Without a top it took its static position,
    // which wraps to a second line after a long step and lands on the row below.
    style: `position:absolute;inset:0;color:${DIM}`,
  });
  const body = h("span", { style: "position:relative;flex:1 1 auto;min-width:0" }, shimmer, dim);
  const plus = h("span", { class: "plus" });
  const minus = h("span", { class: "minus" });
  // Counts read left to right whatever the UI direction: "+3 −1", never "1− 3+".
  const counts = h("span", { class: "tick-count", dir: "ltr" }, plus, minus);
  const el = h(
    "div",
    { class: "ticker-row" },
    h("span", { class: "tick-icon", style: "position:relative" }, chevron, check),
    body,
  );
  return { el, chevron, check, body, shimmer, dim, counts, plus, minus, text: "", diff: null };
}

function setText(row: Row, text: string) {
  if (row.text === text) return;
  row.text = text;
  const diff = parseDiffStep(text);
  row.diff = diff;
  const shown = diff ? diff.filename : text;
  row.shimmer.textContent = shown;
  row.dim.textContent = shown;
  if (diff) {
    // The file name, then the counts right after it; the name truncates first.
    row.body.style.flex = "0 1 auto";
    row.plus.textContent = diff.added > 0 ? ` +${diff.added}` : "";
    row.minus.textContent = diff.removed > 0 ? ` −${diff.removed}` : "";
    if (row.counts.parentNode !== row.el) row.el.append(row.counts);
    row.el.classList.add("diff");
    row.el.title = t("live.showDiff");
  } else if (row.el.classList.contains("diff")) {
    // Back to an ordinary step: exactly the row it always was.
    row.body.style.flex = "1 1 auto";
    row.counts.remove();
    row.el.classList.remove("diff");
    row.el.removeAttribute("title");
  }
}

/**
 * Places a row. `phase` 0 = current (shimmering, full size), 1 = completed
 * (dim, shifted up-left and scaled down) — same crossfades as the Swift view.
 * `still` is the finished look: static text, ✓ only, nothing animating.
 */
function place(row: Row, y: number, phase: number, opacity: number, still: boolean) {
  const scale = 1 - phase * (1 - COMPLETED_SCALE);
  // A completed step slides towards the row's start edge: left, or right in RTL.
  const shift = (isRtl() ? 1 : -1) * phase * 10;
  row.el.style.transform = `translate(${shift}px, ${y}px) scale(${scale})`;
  row.el.style.opacity = String(opacity);
  if (still) {
    row.chevron.style.opacity = "0";
    row.check.style.opacity = "1";
    row.shimmer.style.opacity = "0";
    row.dim.style.opacity = "1";
    row.dim.style.color = phase < 0.5 ? STILL_CURRENT : DIM;
  } else {
    row.chevron.style.opacity = String(clamp(1 - phase * 2, 0, 1));
    row.check.style.opacity = String(clamp(phase * 2 - 1, 0, 1));
    row.shimmer.style.opacity = String(clamp(1 - phase * 1.6, 0, 1));
    row.dim.style.opacity = String(clamp(phase * 2 - 0.4, 0, 1));
    row.dim.style.color = DIM;
  }
  // An invisible shimmer must not keep the webview repainting.
  row.shimmer.style.animationPlayState = still ? "paused" : "";
}

export class Ticker {
  readonly el: HTMLElement;
  private a = makeRow(); // completed
  private b = makeRow(); // current
  private c = makeRow(); // incoming
  private queue: string[] = [];
  private startMs: number | null = null;
  private displayIndex = -1;
  private displayTaskId: string | undefined;
  private displaySessionId: string | undefined;
  private displayRevision: string | undefined;
  /** True once the turn is over: its final message holds still. */
  private still = false;

  /** `onDiffTap` receives the diff id of a clicked diff row. */
  constructor(onDiffTap?: (diffId: number) => void) {
    this.el = h("div", { class: "ticker" }, this.a.el, this.b.el, this.c.el);
    // The completed and the current row answer a click, as on macOS; the
    // incoming row is only ever seen mid-slide.
    for (const row of [this.a, this.b]) {
      row.el.addEventListener("click", () => {
        if (row.diff && onDiffTap) onDiffTap(row.diff.diffId);
      });
    }
    this.rest();
  }

  /** The state between transitions: completed on top, current below. */
  private rest() {
    place(this.a, 0, 1, 1, this.still);
    place(this.b, ROW_H, 0, 1, this.still);
    place(this.c, ROW_H * 2, 0, 0, this.still);
  }

  get animating(): boolean {
    return this.startMs != null || this.queue.length > 0;
  }

  sync(task: AgentTask | null, live: TickerLive = {}) {
    const steps = task?.steps ?? [];
    /** Index of the newest step in `steps`; -1 when there is none. */
    const last = Math.min(task?.stepIndex ?? 0, steps.length - 1);
    // Where the newest step sits in the whole session, not in `steps`: counting
    // inside that capped list stopped the ticker for good at a session's
    // twentieth step. An observed window (Codex logs) moves by its revision.
    const observed = task?.stepRevision !== undefined;
    const newest = last < 0 ? -1 : !observed && live.seq !== undefined ? live.seq : last;
    const identityChanged = task?.id !== this.displayTaskId || task?.stepSessionId !== this.displaySessionId;
    const revisionChanged = observed && task.stepRevision !== this.displayRevision;
    this.displayTaskId = task?.id;
    this.displaySessionId = task?.stepSessionId;
    this.displayRevision = task?.stepRevision;

    const still = live.still === true;
    if (still !== this.still) {
      this.still = still;
      // Mid-slide, the next frame places every row with the new look anyway.
      if (this.startMs == null) this.rest();
    }

    // First render, another task, or the session restarted (steps were
    // cleared): drop straight into place rather than scroll.
    if (this.displayIndex < 0 || identityChanged || newest < this.displayIndex) {
      this.queue = [];
      this.startMs = null;
      this.displayIndex = newest;
      setText(this.a, last > 0 ? steps[last - 1] : "…");
      setText(this.b, last >= 0 ? steps[last] : "…");
      setText(this.c, "");
      this.rest();
      return;
    }

    const fresh = Math.min(newest - this.displayIndex, last + 1);
    if (fresh > 0) this.queue.push(...steps.slice(last + 1 - fresh, last + 1));
    else if (revisionChanged) this.queue.push(steps[last]);
    this.displayIndex = newest;
    if (this.queue.length > MAX_QUEUE) {
      this.queue = this.queue.slice(-MAX_QUEUE);
    }
  }

  /** Called every frame by the island while the overview is on screen. */
  tick(nowMs: number) {
    if (this.startMs == null) {
      if (this.queue.length === 0) return;
      setText(this.c, this.queue[0]);
      place(this.c, ROW_H * 2, 0, 0, this.still);
      this.startMs = nowMs;
    }

    const p = clamp((nowMs - this.startMs) / DURATION, 0, 1);
    const e = EASE(p);

    // A leaves upwards and fades a little faster than it moves, as on macOS.
    place(this.a, lerp(0, -ROW_H, e), 1, clamp(1 - p * 1.35, 0, 1), this.still);
    place(this.b, lerp(ROW_H, 0, e), e, 1, this.still);
    place(this.c, lerp(ROW_H * 2, ROW_H, e), 0, e, this.still);

    if (p < 1) return;

    // Commit: the current row becomes the completed one, the incoming row the
    // current one. Texts move, elements stay put — no reordering, no overlap.
    setText(this.a, this.b.text);
    setText(this.b, this.c.text);
    this.queue.shift();
    this.startMs = null;
    this.rest();
  }
}
