import { beforeAll, describe, expect, it, vi } from "vitest";
import type { PlannerFire } from "../../core/bridge-planner";

// The Rust side, as fire.ts sees it: the live event and the launch backlog.
const live: { handler: ((e: PlannerFire) => void) | null; pending: PlannerFire[] } = { handler: null, pending: [] };

vi.mock("../../core/bridge-planner", () => ({
  onPlannerFire: (handler: (e: PlannerFire) => void) => {
    live.handler = handler;
    return Promise.resolve(() => undefined);
  },
  PlannerBridge: { pendingFires: () => Promise.resolve(live.pending.splice(0)) },
}));

const { State } = await import("../../core/state");
const { currentFire, initPlannerAlerts } = await import("./fire");

const reminder = (id: string): PlannerFire => ({ kind: "reminder", id, title: `R ${id}` });

/** The island, reduced to what fire.ts drives. */
const island = {
  dropFlow: false,
  dropPins: 0,
  alert(view: typeof State.view) {
    State.view = view;
    State.mode = "expanded";
    State.notify();
  },
  collapse() {
    State.isPinned = false;
    State.mode = "compact";
    State.notify();
  },
  dropPin() {
    this.dropPins += 1;
  },
  inDropFlow() {
    return this.dropFlow;
  },
};

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("planner alerts", () => {
  beforeAll(async () => {
    live.pending = [reminder("missed-1"), reminder("missed-2")];
    initPlannerAlerts(island, { celebrate: () => undefined } as never);
    await flush();
  });

  it("shows the fires sent before the island listened, in order", () => {
    expect(currentFire()).toEqual(reminder("missed-1"));
    expect(State.view).toBe("plannerFire");
    expect(State.isPinned).toBe(true);
    // Folded by the user: done with, not shown again; the next one in line
    // shows on the next state change.
    island.collapse();
    expect(currentFire()).toBeNull();
    State.notify();
    expect(currentFire()).toEqual(reminder("missed-2"));
    island.collapse();
    expect(currentFire()).toBeNull();
  });

  it("an approval keeps its pin and the card comes back after it", () => {
    live.handler?.(reminder("r1"));
    expect(currentFire()).toEqual(reminder("r1"));
    const pinsBefore = island.dropPins;

    // hooks.ts PermissionRequest: the approval takes the island over.
    State.pendingApproval = { requestId: "q1", sessionId: "s", tool: "Bash", command: "ls" };
    State.isPinned = true;
    island.alert("approval");
    expect(State.view).toBe("approval");
    expect(State.isPinned).toBe(true);
    expect(island.dropPins).toBe(pinsBefore);
    expect(currentFire()).toBeNull();

    // Answered: the reminder shows again.
    State.pendingApproval = null;
    State.isPinned = false;
    State.view = "overview";
    State.notify();
    expect(currentFire()).toEqual(reminder("r1"));
    expect(State.view).toBe("plannerFire");
    island.collapse();
  });

  it("waits while a file is being dropped, then shows", () => {
    island.dropFlow = true;
    State.view = "upload";
    State.notify();
    live.handler?.(reminder("r2"));
    expect(currentFire()).toBeNull();
    expect(State.view).toBe("upload");

    island.dropFlow = false;
    State.view = "prompt";
    State.notify();
    expect(currentFire()).toEqual(reminder("r2"));
    island.collapse();
  });
});
