import { describe, expect, it, vi } from "vitest";
import { HistoryPersistence } from "./history";
import { ActivityStore, normalizeEvent } from "./store";

const event = (id: string, extra = {}) => ({ id, sessionId: "fixture", harness: "codex", at: Date.now(), kind: "tool", ...extra });
describe("bounded retained coding evidence", () => {
  it("normalizes context and usage and discards invalid values", () => {
    expect(normalizeEvent(event("e", { context: { usedTokens: 10, limitTokens: 100, usedPercent: 10 }, usage: { primary: { usedPercent: 42 } } }))).toMatchObject({ context: { usedTokens: 10 }, usage: { primary: { usedPercent: 42 } } });
    expect(normalizeEvent(event("e", { context: { usedTokens: 101, limitTokens: 100 }, usage: { primary: { usedPercent: Infinity }, secondary: { usedPercent: -1 } } }))).toMatchObject({ context: undefined, usage: undefined });
  });
  it("usage does not reopen a finished session or reset its files", () => {
    const store = new ActivityStore(); store.ingest(event("e", { kind: "finished" }));
    store.ingest(event("u", { kind: "usage", phase: "started", context: { usedPercent: 90 } }));
    expect(store.sessions()[0]).toMatchObject({ status: "finished", context: { usedPercent: 90 } });
  });
  it("restores archives with unknown test freshness and ignores live collisions", () => {
    const store = new ActivityStore();
    store.restore([event("old", { phase: "completed", exitCode: 0, testSummary: { verdict: "passed", passed: 4, failed: 0, skipped: 0 } })]);
    expect(store.sessions()[0]).toMatchObject({ status: "archived", conflicts: [], test: { freshness: "unknown", verdict: "passed" } });
    store.ingest(event("new", { kind: "prompt" }));
    store.restore([event("other", { kind: "finished" })]);
    expect(store.sessions()[0].status).toBe("active");
    store.clearArchived(); expect(store.sessions()).toHaveLength(1);
  });
  it("omits process output, prompts and old evidence from persistence", () => {
    const store = new ActivityStore();
    store.ingest(event("old", { at: Date.now() - 8 * 86400000 }));
    store.ingest(event("e", { title: "private prompt", command: "private command", output: "private output", patch: "private patch", context: { model: "Bearer secret" } }));
    const saved = JSON.stringify(store.retainedEvents());
    expect(saved).not.toMatch(/private|old|Bearer secret/);
    expect(saved).toContain("redacted");
  });
  it("archives a retained live harness without losing evidence or fresh-test claims", () => {
    const store = new ActivityStore();
    store.ingest(event("t", { phase: "completed", command: "npm test", exitCode: 0, output: "Tests 2 passed" }));
    store.archiveHarness("codex");
    expect(store.sessions()[0].status).toBe("archived");
    expect(store.sessions()[0].conflicts).toEqual([]);
    expect(store.sessions()[0].test?.freshness).toBe("unknown");
    expect(store.retainedEvents()).toHaveLength(1);
    store.ingest(event("p", { kind: "prompt" }));
    expect(store.sessions()[0].status).toBe("active");
  });
  it("does not expose nested context or quota objects to snapshot mutation", () => {
    const store = new ActivityStore(); store.ingest(event("u", { context: { usedPercent: 20 }, usage: { primary: { usedPercent: 30 } } }));
    const session = store.sessions()[0];
    session.usage!.primary!.usedPercent = 100; session.events[0].context!.usedPercent = 100;
    expect(store.sessions()[0]).toMatchObject({ context: { usedPercent: 20 }, usage: { primary: { usedPercent: 30 } } });
  });
  it("requires a known successful exit for retained passing summaries", () => {
    const summary = { verdict: "passed", passed: 4, failed: 0, skipped: 0 };
    expect(normalizeEvent(event("unknown", { phase: "completed", testSummary: summary }))?.testSummary?.verdict).toBe("unknown");
    expect(normalizeEvent(event("failed", { phase: "completed", exitCode: 1, testSummary: summary }))?.testSummary?.verdict).toBe("failed");
    expect(normalizeEvent(event("passed", { phase: "completed", exitCode: 0, testSummary: summary }))?.testSummary?.verdict).toBe("passed");
    expect(normalizeEvent(event("failedphase", { phase: "failed", exitCode: 0, testSummary: summary }))?.testSummary?.verdict).toBe("failed");
  });
  it("never persists an unfinished test as a normalized result", () => {
    const store = new ActivityStore();
    store.ingest(event("running", { phase: "started", command: "npm test" }));
    expect(store.retainedEvents()[0].testSummary).toBeUndefined();
  });
});
describe("history persistence races", () => {
  function setup() {
    const deps = { snapshot: vi.fn().mockResolvedValue({ revision: 2, events: [] }), save: vi.fn().mockResolvedValue(true), events: vi.fn(() => [event("e")]), restore: vi.fn(), changed: vi.fn(), log: vi.fn() };
    return { deps, history: new HistoryPersistence(deps) };
  }
  it("does not restore or save after clear/disable cancels a pending read", async () => {
    const { deps, history } = setup();
    let done!: (value: unknown) => void;
    deps.snapshot.mockReturnValue(new Promise(resolve => { done = resolve; }));
    const loading = history.load(); history.cancel(); done({ revision: 1, events: [event("stale")] }); await loading; await history.flush();
    expect(deps.restore).not.toHaveBeenCalled(); expect(deps.save).not.toHaveBeenCalled();
  });
  it("debounces and serializes saves with native revision", async () => {
    vi.useFakeTimers();
    try {
      const { deps, history } = setup(); await history.load(); history.schedule(); history.schedule();
      await vi.advanceTimersByTimeAsync(500);
      expect(deps.save).toHaveBeenCalledTimes(1); expect(deps.save.mock.calls[0][1]).toBe(2);
      history.cancel(); history.schedule(); await vi.advanceTimersByTimeAsync(1000); expect(deps.save).toHaveBeenCalledTimes(1);
    } finally { vi.useRealTimers(); }
  });
  it("rejects corrupt snapshots and visibly stops stale writes", async () => {
    const { deps, history } = setup(); deps.snapshot.mockResolvedValue({ revision: NaN, events: [] }); await history.load();
    expect(history.error).toBe(true); expect(deps.restore).not.toHaveBeenCalled();
    deps.snapshot.mockResolvedValue({ revision: 2, events: [] }); await history.load(); deps.save.mockResolvedValue(false); await history.flush();
    expect(history.error).toBe(true); expect(history.loaded).toBe(false);
  });
  it("saves continuously changing evidence within the first-change deadline", async () => {
    vi.useFakeTimers();
    try {
      const { deps, history } = setup(); await history.load();
      for (let i = 0; i < 12; i++) {
        deps.events.mockReturnValue([event(`e${i}`)]); history.schedule();
        await vi.advanceTimersByTimeAsync(100);
      }
      expect(deps.save).toHaveBeenCalledTimes(2);
      expect(deps.save.mock.calls[0][0][0]).toMatchObject({ id: "e4" });
      expect(deps.save.mock.calls[1][0][0]).toMatchObject({ id: "e9" });
      history.cancel(); await vi.advanceTimersByTimeAsync(1000);
      expect(deps.save).toHaveBeenCalledTimes(2);
    } finally { vi.useRealTimers(); }
  });
});
