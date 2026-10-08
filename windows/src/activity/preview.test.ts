import { afterEach, describe, expect, it } from "vitest";
import { Activity } from "./store";
import { seedActivityPreview } from "./preview";
import { State } from "../core/state";

describe("saved-history preview fidelity", () => {
  afterEach(() => { Activity.clear(); State.settings.retainCodingHistory = false; });
  it("uses the persisted evidence format, omits raw content, and preserves normalized test results", () => {
    Activity.clear();
    seedActivityPreview("?view=activity&activity=history");
    const sessions = Activity.sessions();
    expect(sessions.every(session => session.status === "archived")).toBe(true);
    const codex = sessions.find(session => session.harness === "codex")!;
    expect(codex.test?.verdict).toBe("passed");
    expect(codex.context?.usedPercent).toBe(25);
    expect(codex.lastPatch).toBeUndefined();
    for (const session of sessions) for (const event of session.events) {
      expect(event.command).toBeUndefined();
      expect(event.output).toBeUndefined();
      expect(event.patch).toBeUndefined();
      expect(event.title).toBeUndefined();
    }
  });
});
