// Planner alerts ("planner-fire" from the Rust scheduler): a reminder going
// off, a focus or break phase ending, a habit nudge. Each opens the island on
// a small card with the answers that fit; a reminder or a finished phase stays
// up until answered (pinned), a nudge folds away like any other card.
//
// Alerts queue: one card at a time, none over a pending Claude Code approval,
// none during a file drop and none while the island is paused from the tray;
// they show once that ends. A card pushed aside by an approval or a drop
// shows again afterwards.
// The card never takes keyboard focus (it isn't one of the island's keyboard
// views), so a reminder can't swallow what the user is typing elsewhere.

import { h } from "../dom";
import type { ViewActions, ViewHost } from "../views";
import type { IslandViewName } from "../../core/layout";
import { Bridge } from "../../core/bridge";
import { PlannerBridge, onPlannerFire, type PlannerFire } from "../../core/bridge-planner";
import { State } from "../../core/state";
import { Sound } from "../../core/sound";
import { washRGBA, type Wash } from "../../core/layout";
import { t } from "../../core/i18n";
import { attempt } from "./store";
import { focusCommand, playFocusSound, startBreak, startFocus } from "./focus";
import { dayKeyOf, num } from "./time";
import { habitGlyph, icon } from "./ui";

export interface FireHost {
  alert(view: IslandViewName): void;
  collapse(): void;
  dropPin(): void;
  /** A file is being dropped or waits for a choice: opening a card would kill the drop. */
  inDropFlow(): boolean;
}

/** Minutes a reminder's «بعداً» puts it off. */
export const SNOOZE_MINUTES = 10;
/** Alerts kept waiting at most (a long absence shouldn't stack a wall of cards). */
const QUEUE_MAX = 8;

let host: FireHost | null = null;
let actionsRef: ViewActions | null = null;
const queue: PlannerFire[] = [];
let current: PlannerFire | null = null;
/** True while the island is being opened on the card (its own view changes are expected). */
let opening = false;
/** We pinned the island for the current card. */
let pinnedByUs = false;
let listening = false;

const sameFire = (a: PlannerFire, b: PlannerFire) =>
  a.kind === b.kind && (a.kind === "focus" ? (b as typeof a).phase === a.phase : (b as { id: string }).id === a.id);

/** The current alert, for the card. */
export function currentFire(): PlannerFire | null {
  return current;
}

export function initPlannerAlerts(islandHost: FireHost, actions: ViewActions) {
  host = islandHost;
  actionsRef = actions;
  if (listening) return;
  listening = true;
  void onPlannerFire(onFire)
    .then(async () => {
      // Fired before this listener existed — the launch catch-ups: Tauri drops
      // an event nobody listens to, and Rust has already marked them fired.
      const missed = await PlannerBridge.pendingFires();
      missed.forEach((e, i) => onFire(e, i > 0));
    })
    .catch((err) => {
      console.error("[roadeep] planner alerts: missed alerts not read", err);
      void Bridge.log("planner: missed alerts could not be read");
    });
  // Re-checked on every state change: an approval answered, the pause lifted,
  // the drop finished, the card navigated away from.
  State.subscribe(onState);
}

/** Claude Code is waiting on the user: its card goes first, and keeps the pin. */
function claudeWaiting(): boolean {
  return State.pendingApproval != null ||
    (State.mode === "expanded" && (State.view === "approval" || State.view === "question"));
}

function onFire(e: PlannerFire, quiet = false) {
  void Bridge.log(`planner: alert ${e.kind}${e.kind === "focus" ? ` ${e.phase}` : ` ${e.id}`}`);
  // `quiet`: a batch of launch catch-ups gets one sound, not one each.
  if (!quiet) {
    if (e.kind === "focus") playFocusSound(e.phase);
    else if (State.settings.reminderSound) Sound.play(e.kind === "reminder" ? "approval" : "pop");
  }
  if ((current && sameFire(current, e)) || queue.some((q) => sameFire(q, e))) return;
  queue.push(e);
  if (queue.length > QUEUE_MAX) queue.shift();
  showNext();
}

function showNext() {
  if (current || !queue.length || !host) return;
  if (claudeWaiting() || State.paused || host.inDropFlow()) return;
  current = queue.shift() as PlannerFire;
  pinnedByUs = current.kind !== "habit";
  if (pinnedByUs) State.isPinned = true;
  opening = true;
  try {
    host.alert("plannerFire");
  } finally {
    opening = false;
  }
  if (current.kind === "focus" && current.phase === "focusDone") actionsRef?.celebrate?.("focus");
  State.notify();
}

function onState() {
  if (opening) return;
  if (current && (State.view !== "plannerFire" || State.mode !== "expanded")) {
    // A Claude Code approval or a file drop took the island over: the card
    // comes back once that is over (Rust has already marked it fired, so it
    // would otherwise be lost). Navigated away or folded by the user: done
    // with, unanswered.
    release(claudeWaiting() || (host?.inDropFlow() ?? false));
    return;
  }
  if (!current && queue.length) showNext();
}

/** `requeue`: the card was pushed aside unanswered, it shows again first. */
function release(requeue = false) {
  const fire = current;
  current = null;
  if (requeue && fire) {
    queue.unshift(fire);
    if (queue.length > QUEUE_MAX) queue.splice(1, 1);
  }
  if (pinnedByUs) {
    pinnedByUs = false;
    // The pin is the approval's now (hooks.ts set it): clearing it would let
    // Esc or the cursor leaving fold the card while Claude Code waits.
    if (!claudeWaiting()) {
      State.isPinned = false;
      host?.dropPin();
    }
  }
}

/** The card was answered: the next alert, or the island folds back. */
function finish() {
  release();
  if (queue.length) showNext();
  else host?.collapse();
}

export function buildFire(actions: ViewActions): ViewHost {
  const who = h("div", { class: "who-row fire-who" });
  const title = h("div", { class: "title", dir: "auto" });
  const row = h("div", { class: "actions" });
  const error = h("div", { class: "detail", role: "alert", hidden: true });
  const stack = h("div", { class: "stack fire-stack" }, who, title, error, row);
  const card = h("div", { class: "card wash fire-card" }, stack);
  const el = h("div", { class: "view" }, card);
  let shownFor: PlannerFire | null = null;

  const btn = (label: string, kind: "primary" | "secondary", run: () => Promise<string | null> | void) =>
    h("button", {
      type: "button",
      class: `btn ${kind}`,
      text: label,
      onclick: async () => {
        actions.blip();
        const err = await run();
        if (err) {
          error.hidden = false;
          error.textContent = err;
          return;
        }
        finish();
      },
    });

  function paint(fire: PlannerFire) {
    error.hidden = true;
    let wash: Wash = "amber";
    switch (fire.kind) {
      case "reminder":
        who.replaceChildren(h("span", { class: "fire-ico" }, icon("reminders", 12)), h("span", { text: t("planner.fire.reminder") }));
        title.textContent = fire.title;
        row.replaceChildren(
          btn(t("planner.fire.done"), "primary", () => undefined),
          btn(t("planner.fire.snooze", { n: num(SNOOZE_MINUTES) }), "secondary", async () => {
            const r = await attempt(() => PlannerBridge.reminderSnooze(fire.id, SNOOZE_MINUTES));
            return r.ok ? null : r.error;
          }),
        );
        break;
      case "focus":
        if (fire.phase === "focusDone") {
          wash = "indigo";
          who.replaceChildren(h("span", { class: "fire-ico" }, icon("focus", 12)),
            h("span", { text: t("planner.fire.focusRound", { n: num(fire.round) }) }));
          title.textContent = t("planner.fire.focusDone");
          row.replaceChildren(
            btn(t("planner.focus.startBreak"), "primary", startBreak),
            btn(t("planner.focus.keepFocusing"), "secondary", startFocus),
            btn(t("planner.focus.stop"), "secondary", () => focusCommand(() => PlannerBridge.focusStop())),
          );
        } else {
          wash = "green";
          who.replaceChildren(h("span", { class: "fire-ico" }, icon("focus", 12)),
            h("span", { text: t(fire.phase === "longBreakDone" ? "planner.fire.longBreakOver" : "planner.fire.breakOver") }));
          title.textContent = t("planner.fire.breakDone");
          row.replaceChildren(
            btn(t("planner.fire.start"), "primary", startFocus),
            btn(t("planner.fire.later"), "secondary", () => undefined),
          );
        }
        break;
      case "habit": {
        wash = "cyan";
        const glyph = h("span", { class: "fire-ico" }, habitGlyph(fire.icon, 12));
        who.replaceChildren(glyph, h("span", { dir: "auto", text: fire.title }));
        title.textContent = fire.icon === "custom"
          ? t("planner.fire.habit.custom", { title: fire.title })
          : t(`planner.fire.habit.${fire.icon}`);
        row.replaceChildren(
          btn(t("planner.fire.didIt"), "primary", async () => {
            const r = await attempt(() => PlannerBridge.habitCheck(fire.id, dayKeyOf(new Date()), true));
            if (!r.ok) return r.error;
            actions.celebrate?.("task");
            return null;
          }),
          btn(t("planner.fire.later"), "secondary", () => undefined),
        );
        break;
      }
    }
    card.style.setProperty("--wash", washRGBA(wash));
  }

  return {
    el,
    sync() {
      const fire = current;
      if (!fire || fire === shownFor) return;
      shownFor = fire;
      paint(fire);
    },
  };
}
