import { beforeEach, describe, expect, it, vi } from "vitest";
import { Bridge } from "../core/bridge";
import { BridgeShortcuts } from "../core/bridge-shortcuts";
import { DEFAULT_SETTINGS, State, type AgentTask } from "../core/state";
import { ISLAND_KEYS } from "../settings/shortcut-keys";
import {
  cyclePill, islandKeyAction, pillByNumber, runGlobalShortcut, runIslandKey, type IslandKeyPress, type ShortcutHost,
} from "./shortcuts";

const task = (id: string, extra: Partial<AgentTask> = {}): AgentTask => ({
  id, name: id, color: "#FFFFFF", state: "idle", stepIndex: 0, steps: [], source: "agent", isIntegration: true, ...extra,
});

function fakeHost() {
  return {
    alert: vi.fn(), setView: vi.fn(), collapse: vi.fn(), emote: vi.fn(), setPinned: vi.fn(),
  } satisfies ShortcutHost;
}

const press = (key: string, code: string, mods: Partial<IslandKeyPress> = {}): IslandKeyPress =>
  ({ key, code, ctrlKey: true, altKey: false, shiftKey: false, metaKey: false, ...mods });

beforeEach(() => {
  vi.restoreAllMocks();
  State.tasks = [task("integration_claude", { sessionCwd: "C:\\code\\app", sessionId: "s-1" }), task("integration_n8n"), task("agent:local:a")];
  State.focusId = "integration_claude";
  State.mode = "hidden";
  State.view = "overview";
  State.pendingApproval = null;
  State.isPinned = false;
  State.settings = { ...DEFAULT_SETTINGS };
});

describe("pills by keyboard", () => {
  it("cycle with wrap-around and go by number", () => {
    const ids = ["a", "b", "c"];
    expect(cyclePill(ids, "a", 1)).toBe("b");
    expect(cyclePill(ids, "c", 1)).toBe("a");
    expect(cyclePill(ids, "a", -1)).toBe("c");
    expect(cyclePill(ids, null, 1)).toBe("b");
    expect(cyclePill(ids, "gone", -1)).toBe("c");
    expect(cyclePill([], "a", 1)).toBeNull();
    expect(pillByNumber(ids, 1)).toBe("a");
    expect(pillByNumber(ids, 3)).toBe("c");
    expect(pillByNumber(ids, 4)).toBeNull();
    expect(pillByNumber(ids, 0)).toBeNull();
  });
});

describe("keys inside the open island", () => {
  it("map Ctrl with an arrow, a digit, P and the comma", () => {
    expect(islandKeyAction(press("ArrowRight", "ArrowRight"), { inTextField: false })).toEqual({ kind: "cycle", delta: 1 });
    expect(islandKeyAction(press("ArrowLeft", "ArrowLeft"), { inTextField: false })).toEqual({ kind: "cycle", delta: -1 });
    expect(islandKeyAction(press("3", "Digit3"), { inTextField: true })).toEqual({ kind: "pill", number: 3 });
    expect(islandKeyAction(press("p", "KeyP"), { inTextField: true })).toEqual({ kind: "pin" });
    expect(islandKeyAction(press("ز", "KeyP"), { inTextField: false })).toEqual({ kind: "pin" });
    expect(islandKeyAction(press(",", "Comma"), { inTextField: false })).toEqual({ kind: "settings" });
  });

  it("leave word moves in a text field and every other combination alone", () => {
    expect(islandKeyAction(press("ArrowRight", "ArrowRight"), { inTextField: true })).toBeNull();
    expect(islandKeyAction(press("0", "Digit0"), { inTextField: false })).toBeNull();
    expect(islandKeyAction(press("k", "KeyK"), { inTextField: false })).toBeNull();
    expect(islandKeyAction(press("p", "KeyP", { shiftKey: true }), { inTextField: false })).toBeNull();
    expect(islandKeyAction(press("p", "KeyP", { altKey: true }), { inTextField: false })).toBeNull();
    expect(islandKeyAction(press("p", "KeyP", { ctrlKey: false }), { inTextField: false })).toBeNull();
  });

  it("are the ones Settings lists", () => {
    const ctrlCombos = ISLAND_KEYS.flatMap((k) => k.combos).filter((c) => c.startsWith("Ctrl+"));
    expect(ctrlCombos.length).toBeGreaterThan(0);
    for (const combo of ctrlCombos) {
      const code = combo.slice("Ctrl+".length);
      const key = code.startsWith("Arrow") ? code : code === "Comma" ? "," : code.replace(/^(Key|Digit)/, "").toLowerCase();
      expect(islandKeyAction(press(key, code), { inTextField: false }), combo).not.toBeNull();
    }
  });

  it("switch pills without opening, open Settings and pin unless a permission waits", () => {
    const host = fakeHost();
    const settings = vi.spyOn(Bridge, "openSettingsWindow").mockResolvedValue(null);
    runIslandKey(host, { kind: "cycle", delta: 1 });
    expect(State.focusId).toBe("integration_n8n");
    expect(host.setView).toHaveBeenCalledWith("overview");
    expect(host.alert).not.toHaveBeenCalled();
    runIslandKey(host, { kind: "pill", number: 3 });
    expect(State.focusId).toBe("agent:local:a");
    runIslandKey(host, { kind: "pill", number: 9 });
    expect(State.focusId).toBe("agent:local:a");
    runIslandKey(host, { kind: "settings" });
    expect(settings).toHaveBeenCalled();
    runIslandKey(host, { kind: "pin" });
    expect(host.setPinned).toHaveBeenCalledWith(true);
    State.pendingApproval = { requestId: "r", sessionId: "s", tool: "Bash", command: "ls" };
    runIslandKey(host, { kind: "pin" });
    expect(host.setPinned).toHaveBeenCalledTimes(1);
  });
});

describe("global shortcuts", () => {
  it("go to the waiting permission, then a question, else say nothing waits", () => {
    const host = fakeHost();
    const resume = vi.fn();
    State.pendingApproval = { taskId: "integration_n8n", requestId: "r", sessionId: "s", tool: "Bash", command: "ls" };
    runGlobalShortcut(host, "goToAlert", resume);
    expect(State.focusId).toBe("integration_n8n");
    expect(host.alert).toHaveBeenLastCalledWith("approval");
    expect(resume).toHaveBeenCalledTimes(1);

    State.pendingApproval = null;
    State.tasks[2].state = "question";
    runGlobalShortcut(host, "goToAlert", resume);
    expect(State.focusId).toBe("agent:local:a");
    expect(host.alert).toHaveBeenLastCalledWith("question");

    State.tasks[2].state = "idle";
    host.alert.mockClear();
    runGlobalShortcut(host, "goToAlert", resume);
    expect(host.alert).not.toHaveBeenCalled();
    expect(host.emote).toHaveBeenCalledWith("annoyed");
  });

  it("open or close the island", () => {
    const host = fakeHost();
    const resume = vi.fn();
    runGlobalShortcut(host, "toggleIsland", resume);
    expect(resume).toHaveBeenCalled();
    expect(host.alert).toHaveBeenCalledWith("overview");
    State.mode = "expanded";
    runGlobalShortcut(host, "toggleIsland", resume);
    expect(host.collapse).toHaveBeenCalledTimes(1);
  });

  it("bring the focused session's terminal forward and fold the island", () => {
    const host = fakeHost();
    const open = vi.spyOn(BridgeShortcuts, "openSession").mockResolvedValue(true);
    State.mode = "expanded";
    runGlobalShortcut(host, "jumpToTerminal", vi.fn());
    expect(open).toHaveBeenCalledWith("s-1", "C:\\code\\app");
    expect(host.collapse).toHaveBeenCalled();
    State.focusId = "integration_n8n";
    State.mode = "compact";
    runGlobalShortcut(host, "jumpToTerminal", vi.fn());
    expect(open).toHaveBeenLastCalledWith(null, null);
    expect(host.collapse).toHaveBeenCalledTimes(1);
  });

  it("move between pills and open on the one reached", () => {
    const host = fakeHost();
    runGlobalShortcut(host, "prevPill", vi.fn());
    expect(State.focusId).toBe("agent:local:a");
    expect(host.alert).toHaveBeenCalledWith("overview");
    runGlobalShortcut(host, "nextPill", vi.fn());
    expect(State.focusId).toBe("integration_claude");
  });

  it("turn sounds off and on, and save it", () => {
    const host = fakeHost();
    const save = vi.spyOn(Bridge, "saveSettings").mockResolvedValue(null);
    runGlobalShortcut(host, "muteToggle", vi.fn());
    expect(State.settings.soundEnabled).toBe(false);
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ soundEnabled: false }));
    expect(host.emote).toHaveBeenLastCalledWith("annoyed");
    runGlobalShortcut(host, "muteToggle", vi.fn());
    expect(State.settings.soundEnabled).toBe(true);
    expect(host.emote).toHaveBeenLastCalledWith("happy");
  });

  it("ignore an action they don't know", () => {
    const host = fakeHost();
    const log = vi.spyOn(Bridge, "log").mockResolvedValue(null);
    runGlobalShortcut(host, "wardrobeToggle", vi.fn());
    expect(Object.values(host).every((fn) => fn.mock.calls.length === 0)).toBe(true);
    expect(log).toHaveBeenCalled();
  });
});
