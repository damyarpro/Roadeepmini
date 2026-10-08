// Island open/close FSM — port of IslandStateMachine.swift.
// No DOM, no Tauri: it only reports transitions.
//
// "hidden" is the line: after `lineDelay` seconds of the compact island going
// untouched (no hover; it was not open, dragged or anything else meanwhile)
// it shrinks to a thin line on its edge, so it is out of the way of other
// apps. Hovering the line brings the compact island back and the count starts
// over once the mouse leaves.

import { DEFAULT_LINE_DELAY } from "../core/idle";

export type FsmState = "hidden" | "petit" | "home" | "greeting";

export class IslandStateMachine {
  state: FsmState = "hidden";

  onTransition: ((from: FsmState, to: FsmState) => void) | null = null;

  /**
   * home → petit delay, seconds: the auto-close preference. Changing it while a
   * countdown runs starts that countdown again with the new delay, so an edit in
   * Settings applies at once (IslandStateMachine.homeToPetitDelay on macOS).
   */
  get homeToPetitDelay(): number {
    return this.homeDelay;
  }
  set homeToPetitDelay(seconds: number) {
    if (!Number.isFinite(seconds) || seconds < 0 || seconds === this.homeDelay) return;
    this.homeDelay = seconds;
    if (this.state === "home" && this.homeCollapse != null) this.scheduleHomeCollapse();
  }
  /** petit → hidden (the line) after this many untouched seconds; 0 = never. See setLineDelay. */
  lineDelay = DEFAULT_LINE_DELAY;
  /** greeting → petit once the greeting animation ends (no hover). */
  greetAutoCollapseDelay = 0.6;
  /** greeting → petit while the mouse hovers the greeting. */
  greetHoverCollapseDelay = 10;
  /** An alert waiting for an answer stays open, even when the mouse leaves. */
  pinned = false;
  /**
   * Asked when the idle count runs out: true while something still needs the
   * compact island (a pending approval, a drag…). The line then waits another
   * full period.
   */
  holdLine: (() => boolean) | null = null;

  /**
   * When the open island will fold, on the performance.now() clock, while the
   * mouse-leave countdown runs; null otherwise. The island draws its countdown
   * bar from it.
   */
  homeCollapseDueAt: number | null = null;

  private homeDelay = 15;
  /** The mouse is over the island, as last reported by mouseEntered / mouseLeft. */
  private inside = false;
  private petitHide: number | null = null;
  private homeCollapse: number | null = null;
  private greetCollapse: number | null = null;

  // ── Inputs ──────────────────────────────────────────────────────────────────

  launch() {
    this.cancelTimers();
    this.transition("greeting");
  }

  mouseEntered() {
    this.inside = true;
    switch (this.state) {
      case "hidden":
        this.cancelTimers();
        this.transition("petit");
        break;
      case "petit":
        this.clear("petitHide");
        break;
      case "home":
        this.clear("homeCollapse");
        break;
      case "greeting":
        this.scheduleGreetCollapse(this.greetHoverCollapseDelay);
        break;
    }
  }

  mouseLeft() {
    this.inside = false;
    switch (this.state) {
      case "hidden":
        break;
      case "petit":
        this.schedulePetitHide();
        break;
      case "home":
        this.scheduleHomeCollapse();
        break;
      case "greeting":
        this.clear("greetCollapse");
        this.transition("petit");
        break;
    }
  }

  click() {
    if (this.state !== "petit") return;
    this.cancelTimers();
    this.transition("home");
  }

  /** Greeting animation finished (T.end). Doesn't override a running hover timer. */
  greetComplete() {
    if (this.state !== "greeting") return;
    if (this.greetCollapse == null) this.scheduleGreetCollapse(this.greetAutoCollapseDelay);
  }

  /** Non-alert work event: show compact from hidden. */
  reveal() {
    if (this.state !== "hidden") return;
    this.cancelTimers();
    this.transition("petit");
    this.schedulePetitHide();
  }

  /** Alert or explicit request: open straight to expanded. */
  forceHome() {
    this.cancelTimers();
    this.transition("home");
  }

  /// Explicit close (OK button, Escape, an alert being answered).
  forcePetit() {
    this.cancelTimers();
    this.transition("petit");
  }

  forceHidden() {
    this.cancelTimers();
    this.transition("hidden");
  }

  /**
   * A new idle delay (the setting changed). A count already running starts
   * over with it; 0 stops it. Unchanged values leave the count alone.
   */
  setLineDelay(seconds: number) {
    if (seconds === this.lineDelay) return;
    this.lineDelay = seconds;
    if (this.state === "petit" && !this.inside) this.schedulePetitHide();
  }

  /** The open island must stay open (the maximised chat): drop a pending auto-close. */
  cancelAutoClose() {
    this.clear("homeCollapse");
  }

  // ── Timers ──────────────────────────────────────────────────────────────────

  private schedulePetitHide() {
    this.clear("petitHide");
    if (!(this.lineDelay > 0)) return;
    this.petitHide = window.setTimeout(() => {
      this.petitHide = null;
      if (this.state !== "petit") return;
      if (this.holdLine?.()) this.schedulePetitHide();
      else this.transition("hidden");
    }, this.lineDelay * 1000);
  }

  private scheduleHomeCollapse() {
    this.clear("homeCollapse");
    if (this.pinned) return;
    const ms = this.homeDelay * 1000;
    this.homeCollapseDueAt = performance.now() + ms;
    this.homeCollapse = window.setTimeout(() => {
      this.homeCollapse = null;
      this.homeCollapseDueAt = null;
      // An alert pinned while the countdown ran keeps the island open.
      if (this.state === "home" && !this.pinned) this.transition("petit");
    }, ms);
  }

  private scheduleGreetCollapse(delay: number) {
    this.clear("greetCollapse");
    this.greetCollapse = window.setTimeout(() => {
      this.greetCollapse = null;
      if (this.state === "greeting") this.transition("petit");
    }, delay * 1000);
  }

  private clear(which: "petitHide" | "homeCollapse" | "greetCollapse") {
    const id = this[which];
    if (id != null) window.clearTimeout(id);
    this[which] = null;
    if (which === "homeCollapse") this.homeCollapseDueAt = null;
  }

  cancelTimers() {
    this.clear("petitHide");
    this.clear("homeCollapse");
    this.clear("greetCollapse");
  }

  private transition(next: FsmState) {
    if (next === this.state) return;
    const from = this.state;
    this.state = next;
    // Every way back to the compact island (greeting done, auto-close, an
    // answered alert…) starts the count to the line, not just a mouse leave.
    if (next === "petit" && !this.inside) this.schedulePetitHide();
    this.onTransition?.(from, next);
  }
}
