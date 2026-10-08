import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { IslandStateMachine } from "./fsm";
import {
  DEFAULT_LINE_DELAY, LINE_DELAY_OPTIONS, MAX_LINE_DELAY, MIN_LINE_DELAY, holdsLine, lineDelaySeconds,
} from "../core/idle";
import { devLineDelay } from "./dev-preview";

const MIN = 60_000;

/** A machine sitting in the compact island ("petit") with the mouse away. */
function compact(lineDelay: number): IslandStateMachine {
  const fsm = new IslandStateMachine();
  fsm.lineDelay = lineDelay;
  fsm.reveal();
  return fsm;
}

describe("idle line", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("shrinks the untouched compact island to the line after the delay", () => {
    const fsm = compact(60);
    const seen: string[] = [];
    fsm.onTransition = (_from, to) => seen.push(to);
    vi.advanceTimersByTime(MIN - 1);
    expect(fsm.state).toBe("petit");
    vi.advanceTimersByTime(1);
    expect(fsm.state).toBe("hidden");
    expect(seen).toEqual(["hidden"]);
  });

  it("starts counting when the launch greeting folds to compact, with no mouse involved", () => {
    const fsm = new IslandStateMachine();
    fsm.lineDelay = 60;
    fsm.launch();
    fsm.greetComplete();
    vi.advanceTimersByTime(fsm.greetAutoCollapseDelay * 1000);
    expect(fsm.state).toBe("petit");
    vi.advanceTimersByTime(MIN);
    expect(fsm.state).toBe("hidden");
  });

  it("defaults to five minutes", () => {
    const fsm = new IslandStateMachine();
    expect(fsm.lineDelay).toBe(300);
    fsm.reveal();
    vi.advanceTimersByTime(5 * MIN - 1);
    expect(fsm.state).toBe("petit");
    vi.advanceTimersByTime(1);
    expect(fsm.state).toBe("hidden");
  });

  it("never shrinks by itself when off", () => {
    const fsm = compact(0);
    fsm.mouseEntered();
    fsm.mouseLeft();
    vi.advanceTimersByTime(24 * 60 * MIN);
    expect(fsm.state).toBe("petit");
  });

  it("counts from the last interaction: hovering stops the count, leaving restarts it", () => {
    const fsm = compact(60);
    vi.advanceTimersByTime(50_000);
    fsm.mouseEntered();
    vi.advanceTimersByTime(10 * MIN);
    expect(fsm.state).toBe("petit");
    fsm.mouseLeft();
    vi.advanceTimersByTime(MIN - 1);
    expect(fsm.state).toBe("petit");
    vi.advanceTimersByTime(1);
    expect(fsm.state).toBe("hidden");
  });

  it("never runs while the island is open, however long", () => {
    const fsm = compact(60);
    fsm.mouseEntered();
    fsm.click();
    expect(fsm.state).toBe("home");
    fsm.pinned = true;
    fsm.mouseLeft();
    vi.advanceTimersByTime(60 * MIN);
    expect(fsm.state).toBe("home");

    // An alert or the shortcut opening it straight away.
    const other = compact(60);
    other.forceHome();
    other.cancelAutoClose();
    vi.advanceTimersByTime(60 * MIN);
    expect(other.state).toBe("home");
  });

  it("starts counting again once the island folds back to compact", () => {
    const fsm = compact(60);
    fsm.forceHome();
    fsm.forcePetit();
    fsm.mouseLeft();
    vi.advanceTimersByTime(MIN);
    expect(fsm.state).toBe("hidden");
  });

  it("waits another full period while something holds it (approval, drag, maximised chat…)", () => {
    const fsm = compact(60);
    let held = true;
    fsm.holdLine = () => held;
    vi.advanceTimersByTime(3 * MIN);
    expect(fsm.state).toBe("petit");
    held = false;
    vi.advanceTimersByTime(MIN);
    expect(fsm.state).toBe("hidden");
  });

  it("line → hover → normal, stays normal after leaving, then line again", () => {
    const fsm = compact(60);
    vi.advanceTimersByTime(MIN);
    expect(fsm.state).toBe("hidden");

    fsm.mouseEntered();
    expect(fsm.state).toBe("petit");
    fsm.mouseLeft();
    vi.advanceTimersByTime(MIN - 1000);
    expect(fsm.state).toBe("petit");
    vi.advanceTimersByTime(1000);
    expect(fsm.state).toBe("hidden");
  });

  it("hovering the line does nothing more than wake it (no timer runs while hidden)", () => {
    const fsm = compact(60);
    vi.advanceTimersByTime(MIN);
    expect(fsm.state).toBe("hidden");
    expect(vi.getTimerCount()).toBe(0);
  });

  it("a work event wakes the line exactly as before", () => {
    const fsm = compact(60);
    vi.advanceTimersByTime(MIN);
    fsm.reveal();
    expect(fsm.state).toBe("petit");
    vi.advanceTimersByTime(MIN);
    expect(fsm.state).toBe("hidden");
  });

  it("a changed setting restarts a running count, and 0 stops it", () => {
    const fsm = compact(300);
    vi.advanceTimersByTime(4 * MIN);
    fsm.setLineDelay(120);
    vi.advanceTimersByTime(2 * MIN - 1);
    expect(fsm.state).toBe("petit");
    vi.advanceTimersByTime(1);
    expect(fsm.state).toBe("hidden");

    const off = compact(60);
    off.setLineDelay(0);
    vi.advanceTimersByTime(60 * MIN);
    expect(off.state).toBe("petit");
    // Turned back on with the mouse away: the count starts.
    off.setLineDelay(60);
    vi.advanceTimersByTime(MIN);
    expect(off.state).toBe("hidden");
  });

  it("a changed setting never starts a count under the mouse or on an open island", () => {
    const fsm = compact(0);
    fsm.mouseEntered();
    fsm.setLineDelay(60);
    vi.advanceTimersByTime(10 * MIN);
    expect(fsm.state).toBe("petit");

    const open = compact(0);
    open.forceHome();
    open.cancelAutoClose();
    open.setLineDelay(60);
    vi.advanceTimersByTime(10 * MIN);
    expect(open.state).toBe("home");
  });

  it("an unchanged setting leaves a running count alone", () => {
    const fsm = compact(60);
    vi.advanceTimersByTime(50_000);
    fsm.setLineDelay(60);
    vi.advanceTimersByTime(10_000);
    expect(fsm.state).toBe("hidden");
  });
});

describe("auto-close delay from Settings (followed live)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  /** Opened by a click, the way the macOS tests open theirs. */
  function opened(delay: number): IslandStateMachine {
    const fsm = new IslandStateMachine();
    fsm.lineDelay = 0;
    fsm.homeToPetitDelay = delay;
    fsm.mouseEntered();
    fsm.click();
    expect(fsm.state).toBe("home");
    return fsm;
  }
  const seconds = (s: number) => vi.advanceTimersByTime(s * 1000);

  it("uses the configured delay instead of the default", () => {
    const fsm = opened(2);
    fsm.mouseLeft();
    seconds(1.9);
    expect(fsm.state).toBe("home");
    seconds(0.1);
    expect(fsm.state).toBe("petit");
  });

  it("restarts a running countdown with an edited delay, shorter or longer", () => {
    const shorter = opened(15);
    shorter.mouseLeft();
    shorter.homeToPetitDelay = 0.05;
    seconds(0.05);
    expect(shorter.state).toBe("petit");

    const longer = opened(0.05);
    longer.mouseLeft();
    longer.homeToPetitDelay = 0.25;
    seconds(0.1);
    expect(longer.state).toBe("home");
    seconds(0.15);
    expect(longer.state).toBe("petit");
  });

  it("starts no countdown for an edit while the island is hovered", () => {
    const fsm = opened(15);
    fsm.homeToPetitDelay = 0.05;
    seconds(600);
    expect(fsm.state).toBe("home");
    fsm.mouseLeft();
    seconds(0.05);
    expect(fsm.state).toBe("petit");
  });

  it("keeps a pinned alert open through an edit and through a timer armed before the pin", () => {
    const edited = opened(0.2);
    edited.mouseLeft();
    edited.pinned = true;
    edited.homeToPetitDelay = 0.01;
    seconds(600);
    expect(edited.state).toBe("home");
    edited.pinned = false;
    edited.mouseLeft();
    seconds(0.01);
    expect(edited.state).toBe("petit");

    const armed = opened(1);
    armed.mouseLeft();
    armed.pinned = true;
    seconds(600);
    expect(armed.state).toBe("home");
  });

  it("gives the countdown bar a deadline that follows the delay and clears with the timer", () => {
    const fsm = opened(15);
    expect(fsm.homeCollapseDueAt).toBeNull();
    fsm.mouseLeft();
    const first = fsm.homeCollapseDueAt;
    expect(first).not.toBeNull();
    fsm.homeToPetitDelay = 5;
    expect(fsm.homeCollapseDueAt!).toBeLessThan(first!);
    fsm.mouseEntered();
    expect(fsm.homeCollapseDueAt).toBeNull();
    fsm.mouseLeft();
    seconds(5);
    expect(fsm.state).toBe("petit");
    expect(fsm.homeCollapseDueAt).toBeNull();
  });

  it("ignores an unusable delay", () => {
    const fsm = new IslandStateMachine();
    for (const bad of [Number.NaN, -1, Number.POSITIVE_INFINITY]) fsm.homeToPetitDelay = bad;
    expect(fsm.homeToPetitDelay).toBe(15);
  });
});

describe("lineDelaySeconds", () => {
  it("keeps the offered choices, 0 meaning off", () => {
    for (const v of LINE_DELAY_OPTIONS) expect(lineDelaySeconds(v)).toBe(v);
    expect(LINE_DELAY_OPTIONS).toContain(DEFAULT_LINE_DELAY);
    expect(DEFAULT_LINE_DELAY).toBe(300);
  });

  it("brings anything else into range", () => {
    expect(lineDelaySeconds(undefined)).toBe(DEFAULT_LINE_DELAY);
    expect(lineDelaySeconds("300")).toBe(DEFAULT_LINE_DELAY);
    expect(lineDelaySeconds(Number.NaN)).toBe(DEFAULT_LINE_DELAY);
    expect(lineDelaySeconds(-1)).toBe(DEFAULT_LINE_DELAY);
    expect(lineDelaySeconds(5)).toBe(MIN_LINE_DELAY);
    expect(lineDelaySeconds(1e12)).toBe(MAX_LINE_DELAY);
    expect(lineDelaySeconds(450)).toBe(450);
  });
});

describe("devLineDelay", () => {
  it("reads ?idle=N for the browser preview only when sensible", () => {
    expect(devLineDelay("?idle=3")).toBe(3);
    expect(devLineDelay("")).toBeNull();
    expect(devLineDelay("?idle=0")).toBeNull();
    expect(devLineDelay("?idle=x")).toBeNull();
    expect(devLineDelay("?idle=99999")).toBeNull();
  });
});

describe("holdsLine", () => {
  const calm = { maximized: false, isPinned: false, pendingApproval: null, fileDragOver: false, tasks: [{ state: "idle" }, { state: "working" }] };
  const still = { dragging: false, menuOpen: false, uploading: false };

  it("lets an ordinary compact island become the line, busy or not", () => {
    expect(holdsLine(calm, still)).toBe(false);
  });

  it("holds it for anything that needs the island", () => {
    expect(holdsLine({ ...calm, maximized: true }, still)).toBe(true);
    expect(holdsLine({ ...calm, isPinned: true }, still)).toBe(true);
    expect(holdsLine({ ...calm, pendingApproval: { requestId: "r" } }, still)).toBe(true);
    expect(holdsLine({ ...calm, fileDragOver: true }, still)).toBe(true);
    expect(holdsLine({ ...calm, tasks: [{ state: "approval" }] }, still)).toBe(true);
    expect(holdsLine({ ...calm, tasks: [{ state: "idle" }, { state: "question" }] }, still)).toBe(true);
    expect(holdsLine(calm, { ...still, dragging: true })).toBe(true);
    expect(holdsLine(calm, { ...still, menuOpen: true })).toBe(true);
    expect(holdsLine(calm, { ...still, uploading: true })).toBe(true);
  });
});
