// «تمرکز»: the Pomodoro timer. Rust keeps the clock (focus_* commands, the
// scheduler fires each phase end); this shows it: a big countdown, the phase,
// the rounds of the cycle and the buttons that fit the phase. A finished
// phase waits for the user, so the next one starts from here or from the alert.
//
// Also the compact island's `mm:ss` chip beside the character while a phase
// runs: one 1 s interval, only while the compact island is on screen.

import { h } from "../dom";
import type { ViewActions, ViewHost } from "../views";
import { PlannerBridge, type FocusState } from "../../core/bridge-planner";
import { Bridge } from "../../core/bridge";
import { State } from "../../core/state";
import { Sound } from "../../core/sound";
import { t } from "../../core/i18n";
import { openPalette } from "../palette";
import { Planner, attempt } from "./store";
import { fmtCountdown, num } from "./time";
import { plannerShell, solid } from "./ui";

/** Idle after a focus round: its break is what comes next (Rust's `next`). */
export const awaitingBreak = (f: FocusState | null | undefined): boolean =>
  !!f && f.phase === "idle" && (f.next === "break" || f.next === "longBreak");

/** Length of the phase the idle timer would start next, for the big clock. */
function nextLengthMs(f: FocusState): number {
  const s = State.settings;
  const minutes = f.next === "break" ? s.breakMinutes : f.next === "longBreak" ? s.longBreakMinutes : s.focusMinutes;
  return minutes * 60_000;
}

export const focusRunning = (f: FocusState | null | undefined): boolean =>
  !!f && f.endsAt != null && (f.phase === "focus" || f.phase === "break" || f.phase === "longBreak");

/** Milliseconds left in the phase (frozen while paused), or null when idle. */
export function focusRemaining(f: FocusState | null | undefined, now: number): number | null {
  if (!f) return null;
  if (f.phase === "paused") return f.remainingMs;
  if (focusRunning(f)) return Math.max(0, (f.endsAt as number) - now);
  return null;
}

/** Runs a focus command and takes its answer as the new state at once. */
export async function focusCommand(run: () => Promise<FocusState>): Promise<string | null> {
  const r = await attempt(run);
  if (!r.ok) return r.error;
  Planner.setFocus(r.value);
  return null;
}

/** A focus round, even when a break was next (Rust then counts it as the next round). */
export function startFocus(): Promise<string | null> {
  return focusCommand(() => PlannerBridge.focusStart("focus"));
}

/** Whatever comes next; after a focus round, its break (Rust picks short or long). */
export function startBreak(): Promise<string | null> {
  return focusCommand(() => PlannerBridge.focusResume());
}

export function buildFocus(actions: ViewActions): ViewHost {
  const shell = plannerShell({ icon: "focus", title: t("planner.focus.title"), onMenu: () => openPalette(actions), cls: "pl-focus" });
  const todayCount = h("span", { class: "pl-meta" });
  shell.tools.append(todayCount);

  const phase = h("div", { class: "fx-phase" });
  const clock = h("div", { class: "fx-clock", role: "timer", "aria-live": "off" });
  const dots = h("div", { class: "fx-dots", role: "img" });
  const controls = h("div", { class: "fx-controls" });
  // Said when the phase changes, not every second.
  const announcer = h("div", { class: "sr-only", "aria-live": "polite" });
  shell.body.append(h("div", { class: "fx" }, phase, clock, dots, controls), announcer);

  let controlsKey = "";
  let clockText = "";
  let renderedKey = "";

  const button = (glyph: Parameters<typeof solid>[0], label: string, kind: "primary" | "secondary", run: () => Promise<string | null>) =>
    h("button", {
      type: "button",
      class: `btn ${kind} fx-btn`,
      onclick: async () => {
        actions.blip();
        const err = await run();
        shell.setError(err);
      },
    }, solid(glyph, 11), h("span", { text: label }));

  function phaseLabel(f: FocusState): string {
    if (f.phase === "idle") return t(awaitingBreak(f) ? "planner.focus.breakNext" : "planner.focus.ready");
    return t(`planner.focus.phase.${f.phase}`);
  }

  function paintClock(f: FocusState, now: number) {
    const left = focusRemaining(f, now) ?? nextLengthMs(f);
    const text = fmtCountdown(left);
    if (text === clockText) return;
    clockText = text;
    clock.textContent = text;
    clock.setAttribute("aria-label", t("planner.focus.left", { time: text }));
  }

  function sync() {
    Planner.ensure();
    const f = Planner.data?.focus ?? null;
    if (!f) {
      phase.textContent = Planner.error ?? t("planner.loading");
      return;
    }
    const rounds = State.settings.roundsBeforeLongBreak;
    const waiting = awaitingBreak(f);
    const key = `${Planner.revision}|${rounds}|${State.settings.focusMinutes}`;
    paintClock(f, Date.now());
    if (key === renderedKey) return;
    renderedKey = key;

    const label = phaseLabel(f);
    if (phase.textContent !== label) {
      phase.textContent = label;
      announcer.textContent = label;
    }
    phase.dataset.phase = f.phase;
    clock.dataset.phase = f.phase;
    todayCount.textContent = t("planner.focus.today", { n: num(f.roundsDoneToday) });

    // Rounds of the cycle: the ones behind this one are full, this one is ringed.
    dots.replaceChildren();
    const current = Math.min(Math.max(1, f.round), rounds);
    for (let i = 1; i <= rounds; i++) {
      const done = i < current || (i === current && waiting);
      dots.append(h("i", { class: done ? "on" : i === current ? "now" : "" }));
    }
    dots.setAttribute("aria-label", t("planner.focus.round", { n: num(current), of: num(rounds) }));

    const ck = `${f.phase}|${waiting}`;
    if (ck !== controlsKey) {
      controlsKey = ck;
      const minutes = num(State.settings.focusMinutes);
      const stop = () => focusCommand(() => PlannerBridge.focusStop());
      switch (f.phase) {
        case "idle":
          controls.replaceChildren(...(waiting
            ? [
                button("play", t("planner.focus.startBreak"), "primary", startBreak),
                button("play", t("planner.focus.keepFocusing"), "secondary", startFocus),
              ]
            : [button("play", t("planner.focus.start", { n: minutes }), "primary", startFocus)]));
          break;
        case "paused":
          controls.replaceChildren(
            button("play", t("planner.focus.resume"), "primary", () => focusCommand(() => PlannerBridge.focusResume())),
            button("stop", t("planner.focus.stop"), "secondary", stop),
          );
          break;
        default:
          controls.replaceChildren(
            button("pause", t("planner.focus.pause"), "secondary", () => focusCommand(() => PlannerBridge.focusPause())),
            button("skip", t("planner.focus.skip"), "secondary", () => focusCommand(() => PlannerBridge.focusSkip())),
            button("stop", t("planner.focus.stop"), "secondary", stop),
          );
      }
    }
  }

  return {
    el: shell.el,
    sync,
    tick() {
      const f = Planner.data?.focus;
      if (f) paintClock(f, Date.now());
    },
    focus() {
      controls.querySelector<HTMLElement>("button")?.focus();
    },
  };
}

// ── Compact chip ──────────────────────────────────────────────────────────────

/** Expanded views that show the running clock and repaint with the 1 s tick. */
const CLOCK_VIEWS = new Set(["focus", "today"]);

/**
 * The `mm:ss` beside the compact character while a phase runs, and the
 * second hand of the focus and today views. One 1 s interval, only while the
 * island is on screen (compact or expanded) and a phase runs; hidden (the
 * line) costs nothing. The island's frame loop can't be the clock: it stops
 * once the character is still. Each time the island comes back it asks Rust
 * for the focus state again.
 */
export function createFocusChip() {
  const el = h("div", { id: "focus-chip", "aria-hidden": "true" });
  let timer: number | null = null;
  let wasVisible = false;
  let text = "";

  const paint = () => {
    const f = Planner.data?.focus;
    if (State.mode === "expanded") {
      if (CLOCK_VIEWS.has(State.view)) State.notify();
      return;
    }
    const left = focusRemaining(f, Date.now());
    const next = left == null ? "" : fmtCountdown(left);
    if (next !== text) {
      text = next;
      el.textContent = next;
    }
  };
  const stopTimer = () => {
    if (timer != null) window.clearInterval(timer);
    timer = null;
  };

  // Rust may have a focus round running from before (or from the chat's tools).
  Planner.ensure();

  return {
    el,
    /** Called on every island sync: shows, ticks or stops the chip. */
    update() {
      const visible = State.mode !== "hidden";
      if (visible && !wasVisible) {
        void PlannerBridge.focusGet().then(
          (f) => Planner.setFocus(f),
          () => void Bridge.log("planner: focus_get failed on show"),
        );
      }
      wasVisible = visible;
      const f = Planner.data?.focus;
      const shown = State.mode === "compact" && (focusRunning(f) || f?.phase === "paused");
      el.classList.toggle("on", shown);
      if (f) el.dataset.phase = f.phase;
      if (shown) paint();
      // A phase that ran out waits for Rust's event; the clock stops at 00:00.
      const ticking = visible && focusRunning(f) && (focusRemaining(f, Date.now()) ?? 0) > 0;
      if (ticking && timer == null) timer = window.setInterval(paint, 1000);
      if (!ticking) stopTimer();
    },
  };
}

/** The sound for the end of a focus or break phase (Settings → Planner → sounds). */
export function playFocusSound(phase: "focusDone" | "breakDone" | "longBreakDone") {
  if (!State.settings.focusSound) return;
  Sound.play(phase === "focusDone" ? "finish" : "pop");
}
