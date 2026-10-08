import { beforeEach, describe, expect, it, vi } from "vitest";

const f = vi.hoisted(() => ({
  events: new Map<string, (event: unknown) => void>(),
  state: { settings: { observeCodex: false, retainCodingHistory: false }, paused: false, notify: vi.fn() },
  bridge: { codingStatus: vi.fn(), codingSnapshot: vi.fn(), codingClear: vi.fn(), codingHistorySnapshot: vi.fn(), codingHistorySave: vi.fn(), saveSettingsChecked: vi.fn(), log: vi.fn() },
  activity: { subscribe: vi.fn(), ingest: vi.fn(), clear: vi.fn(), clearHarness:vi.fn(), archiveHarness: vi.fn(), clearArchived: vi.fn(), restore: vi.fn(), retainedEvents: vi.fn(() => []) },
}));
vi.mock("../core/bridge", () => ({ IS_TAURI: true, Bridge: f.bridge, onEvent: async (name: string, cb: (e: unknown) => void) => { f.events.set(name, cb); return () => f.events.delete(name); } }));
vi.mock("../core/state", () => ({ State: f.state }));
vi.mock("./store", () => ({ Activity: f.activity }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
describe("local activity observer lifecycle", () => {
  beforeEach(() => {
    vi.resetModules(); vi.clearAllMocks(); f.events.clear();
    f.state.settings.observeCodex = false; f.state.paused = false;
    f.state.settings.retainCodingHistory = false;
    f.bridge.codingStatus.mockResolvedValue({ enabled: true, available: true });
    f.bridge.codingSnapshot.mockResolvedValue([]);
    f.bridge.codingHistorySnapshot.mockResolvedValue({ revision: 1, events: [] });
    f.bridge.codingHistorySave.mockResolvedValue(true);
    f.bridge.codingClear.mockResolvedValue(undefined);
    f.bridge.saveSettingsChecked.mockResolvedValue(undefined);
  });
  it("subscribes before snapshot and accepts a live event while replay is pending", async () => {
    f.state.settings.observeCodex = true;
    const replay = deferred<unknown[]>(); f.bridge.codingSnapshot.mockReturnValue(replay.promise);
    const monitor = await import("./monitor");
    const boot = monitor.startMonitor();
    await vi.waitFor(() => expect(f.bridge.codingSnapshot).toHaveBeenCalled());
    const event = { id: "fabricated-live" };
    f.events.get("coding-activity")!(event);
    expect(f.activity.ingest).toHaveBeenCalledWith(event);
    replay.resolve([event]); await boot;
    expect(f.activity.ingest).toHaveBeenCalledTimes(2); // The bounded store owns stable-ID dedupe.
  });
  it("does not rehydrate from an older snapshot after disable", async () => {
    f.state.settings.observeCodex = true;
    const replay = deferred<unknown[]>(); f.bridge.codingSnapshot.mockReturnValue(replay.promise);
    const monitor = await import("./monitor"); const boot = monitor.startMonitor();
    await vi.waitFor(() => expect(f.bridge.codingSnapshot).toHaveBeenCalled());
    f.state.settings.observeCodex = false; await monitor.refreshMonitor();
    f.events.get("coding-activity")!({ id: "late-live" });
    replay.resolve([{ id: "old-snapshot" }]); await boot;
    expect(f.activity.ingest).not.toHaveBeenCalled(); expect(f.activity.clearHarness).toHaveBeenCalledWith("codex",true); expect(f.activity.clear).not.toHaveBeenCalled();
  });
  it("clear cancels replay and ignores events during the backend clear", async () => {
    f.state.settings.observeCodex = true;
    const replay = deferred<unknown[]>(); f.bridge.codingSnapshot.mockReturnValue(replay.promise);
    const cleared = deferred<void>(); f.bridge.codingClear.mockReturnValue(cleared.promise);
    const monitor = await import("./monitor"); const boot = monitor.startMonitor();
    await vi.waitFor(() => expect(f.bridge.codingSnapshot).toHaveBeenCalled());
    const clearing = monitor.clearActivity();
    f.events.get("coding-activity")!({ id: "during-clear" });
    cleared.resolve(); await clearing;
    replay.resolve([{ id: "stale-replay" }]); await boot;
    expect(f.activity.ingest).not.toHaveBeenCalled(); expect(f.activity.clear).toHaveBeenCalled();
  });
  it("retains the preference on a failed save and exposes a safe error state", async () => {
    f.bridge.saveSettingsChecked.mockRejectedValue(new Error("secret raw failure"));
    const monitor = await import("./monitor"); await monitor.toggleMonitor();
    expect(f.state.settings.observeCodex).toBe(false); expect(monitor.Monitor.error).toBe(true);
    expect(monitor.Monitor.busy).toBe(false);
    expect(f.bridge.log).toHaveBeenCalledWith("coding activity preference save failed");
  });
  it("receives live status transitions without recording raw error details", async () => {
    f.state.settings.observeCodex = true;
    const monitor = await import("./monitor"); await monitor.startMonitor();
    f.events.get("coding-status")!({ enabled:true, available:false, error:"raw error omitted" });
    expect(monitor.Monitor).toMatchObject({ available:false, error:true });
    expect(f.activity.ingest).not.toHaveBeenCalled();
  });
  it("archives live Codex evidence when observation is off with retention enabled", async () => {
    f.state.settings.retainCodingHistory = true;
    const monitor = await import("./monitor"); await monitor.refreshMonitor();
    expect(f.activity.archiveHarness).toHaveBeenCalledWith("codex",true);
    expect(f.activity.clearHarness).not.toHaveBeenCalled();
  });
  it("cancels retained history restored by another window after retention is disabled", async () => {
    f.state.settings.retainCodingHistory = true;
    const pending = deferred<{ revision: number; events: unknown[] }>(); f.bridge.codingHistorySnapshot.mockReturnValue(pending.promise);
    const monitor = await import("./monitor"); const loading = monitor.refreshMonitor();
    f.state.settings.retainCodingHistory = false; await monitor.refreshMonitor();
    pending.resolve({ revision: 1, events: [{ id: "stale" }] }); await loading;
    expect(f.activity.restore).not.toHaveBeenCalled(); expect(f.activity.clearArchived).toHaveBeenCalled();
    expect(monitor.Monitor.history.active).toBe(false);
  });
  it("shows maintenance failures without logging the event payload and releases the listener", async () => {
    f.state.settings.retainCodingHistory = true;
    const monitor = await import("./monitor"); await monitor.startMonitor();
    f.events.get("coding-history-error")!("untrusted raw error detail");
    expect(monitor.Monitor.history.error).toBe(true);
    expect(f.bridge.log).toHaveBeenCalledWith("coding history maintenance failed");
    expect(f.bridge.log).not.toHaveBeenCalledWith("untrusted raw error detail");
    window.dispatchEvent(new Event("beforeunload"));
    expect(f.events.has("coding-history-error")).toBe(false);
  });
});
