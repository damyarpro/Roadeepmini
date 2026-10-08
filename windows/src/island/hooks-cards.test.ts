// The waiting cards (permission requests and Claude Code's questions) and the
// Stop timers, driven through the hook handler the way Rust emits the events.

import { beforeEach, describe, expect, it, vi } from "vitest";
import { handleHook, type HookPayload } from "./hooks";
import { DEFAULT_SETTINGS, State } from "../core/state";
import { Bridge } from "../core/bridge";
import type { Island } from "./island";
import { cardView, endApproval, pendingQuestions } from "./approvals";
import { setLanguage } from "../core/i18n";

vi.mock("../core/bridge", () => ({
  IS_TAURI: false,
  onEvent: async () => () => {},
  Bridge: { approvalAck: vi.fn(), approvalDecline: vi.fn(), log: vi.fn() },
}));
vi.mock("../core/sound", () => ({ Sound: { play: vi.fn() } }));

const CLAUDE = "integration_claude";
const task = (id: string) => State.tasks.find((t) => t.id === id);

function island() {
  return {
    alert: vi.fn(), reveal: vi.fn(), setView: vi.fn(), dropPin: vi.fn(), cardCleared: vi.fn(), inDropFlow: false,
  };
}
type FakeIsland = ReturnType<typeof island>;
const hook = (ui: FakeIsland, payload: HookPayload) => handleHook(ui as unknown as Island, payload);

const QUESTIONS = [
  { question: "Which database?", header: "DB", options: [{ label: "Postgres", description: "SQL" }, { label: "SQLite" }] },
  { question: "Extras?", multiSelect: true, options: [{ label: "Tests" }, { label: "Docs" }, { label: "CI" }] },
];
const ask = (extra: HookPayload = {}): HookPayload => ({
  provider: "claude", hook_event_name: "PermissionRequest", request_id: "q-1", session_id: "s1",
  tool_name: "AskUserQuestion", tool_input: { questions: QUESTIONS }, ...extra,
});
const permission = (extra: HookPayload = {}): HookPayload => ({
  provider: "claude", hook_event_name: "PermissionRequest", request_id: "p-1", session_id: "s1",
  tool_name: "Bash", tool_input: { command: "npm test" }, ...extra,
});

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  setLanguage("en");
  endApproval();
  State.tasks = [{ id: CLAUDE, name: "Roadeep", color: "#fff", state: "idle", steps: [], stepIndex: 0, source: "claudeCode", isIntegration: true }];
  State.settings = { ...DEFAULT_SETTINGS };
  State.focusId = CLAUDE;
  State.pendingApproval = null;
  State.isPinned = false;
  State.paused = false;
  State.mode = "hidden";
  State.view = "overview";
});

describe("question cards", () => {
  it("turns Claude Code's AskUserQuestion into a question card with its options", () => {
    const ui = island();
    hook(ui, ask());
    expect(ui.alert).toHaveBeenCalledWith("question");
    expect(cardView()).toBe("question");
    expect(pendingQuestions()?.map((q) => [q.question, q.multiSelect, q.options.length])).toEqual([
      ["Which database?", false, 2],
      ["Extras?", true, 3],
    ]);
    expect(task(CLAUDE)?.state).toBe("question");
    expect(Bridge.approvalAck).toHaveBeenCalledWith("q-1");
    expect(State.isPinned).toBe(true);
  });

  it("keeps Deny / Allow for anything else: another tool, or Codex asking", () => {
    const ui = island();
    hook(ui, ask({ provider: "codex", request_id: "c-1" }));
    expect(ui.alert).toHaveBeenCalledWith("approval");
    expect(pendingQuestions()).toBeNull();
    endApproval();
    hook(ui, ask({ tool_input: { questions: [{ question: "Only one option?", options: [{ label: "A" }] }] } }));
    expect(cardView()).toBe("approval");
  });

  it("takes the card down when the question was answered in the terminal", () => {
    const ui = island();
    hook(ui, ask());
    hook(ui, { provider: "claude", hook_event_name: "PostToolUse", session_id: "other", tool_name: "AskUserQuestion" });
    expect(State.pendingApproval).not.toBeNull();
    hook(ui, { provider: "claude", hook_event_name: "PostToolUse", session_id: "s1", tool_name: "AskUserQuestion" });
    expect(State.pendingApproval).toBeNull();
    expect(ui.cardCleared).toHaveBeenCalledOnce();
    // Answered there: nothing to hand back.
    expect(Bridge.approvalDecline).not.toHaveBeenCalled();
  });
});

describe("the waiting card", () => {
  it("always comes up, bringing its pill to the front, and gives the front back after", () => {
    const ui = island();
    hook(ui, { provider: "codex", hook_event_name: "PermissionRequest", request_id: "c-1", session_id: "x", tool_name: "Bash", tool_input: { command: "ls" } });
    expect(State.focusId).toBe("integration_codex");
    expect(ui.alert).toHaveBeenCalledWith("approval");
    expect(ui.reveal).not.toHaveBeenCalled();
    endApproval();
    expect(State.focusId).toBe(CLAUDE);
    expect(task("integration_codex")?.state).toBe("working");
  });

  it("leaves the pill you moved to while it waited", () => {
    const ui = island();
    State.tasks.push({ id: "integration_n8n", name: "n8n", color: "#F29B38", state: "idle", steps: [], stepIndex: 0, source: "n8n", isIntegration: true });
    hook(ui, permission());
    State.focusId = "integration_n8n";
    endApproval();
    expect(State.focusId).toBe("integration_n8n");
  });

  it("goes when its turn is over in the same session, and the relay is released", () => {
    for (const event of ["Stop", "StopFailure", "UserPromptSubmit", "SessionEnd"]) {
      const ui = island();
      hook(ui, permission());
      hook(ui, { provider: "claude", hook_event_name: event, session_id: "another" });
      expect(State.pendingApproval?.requestId).toBe("p-1");
      hook(ui, { provider: "claude", hook_event_name: event, session_id: "s1" });
      expect(State.pendingApproval).toBeNull();
      expect(Bridge.approvalDecline).toHaveBeenLastCalledWith("p-1");
      expect(ui.cardCleared).toHaveBeenCalledOnce();
    }
  });

  it("is never covered by another alert while it waits", () => {
    const ui = island();
    hook(ui, permission());
    ui.alert.mockClear();
    hook(ui, { provider: "claude", hook_event_name: "Stop", session_id: "another" });
    expect(ui.alert).not.toHaveBeenCalled();
    expect(ui.setView).not.toHaveBeenCalled();
    expect(task(CLAUDE)?.pillBadge).toBe("finished");
  });

  it("is withdrawn once the relay has given up (110 s), never decided", () => {
    const ui = island();
    hook(ui, permission());
    vi.advanceTimersByTime(109_000);
    expect(State.pendingApproval).not.toBeNull();
    vi.advanceTimersByTime(1_000);
    expect(State.pendingApproval).toBeNull();
    expect(ui.cardCleared).toHaveBeenCalledOnce();
    expect(Bridge.approvalDecline).not.toHaveBeenCalled();
  });

  it("hands a second request straight back while one waits", () => {
    const ui = island();
    hook(ui, permission());
    hook(ui, permission({ request_id: "p-2" }));
    expect(Bridge.approvalDecline).toHaveBeenCalledWith("p-2");
    expect(State.pendingApproval?.requestId).toBe("p-1");
  });
});

describe("Stop timers", () => {
  const gemini = (event: string, extra: HookPayload = {}) => ({ provider: "gemini", hook_event_name: event, ...extra });

  it("a new turn cancels the return to idle and takes the finished badge with it", () => {
    const ui = island();
    hook(ui, gemini("Stop"));
    expect(task("integration_gemini")?.pillBadge).toBe("finished");
    hook(ui, gemini("PreToolUse", { tool_name: "Shell" }));
    expect(task("integration_gemini")?.pillBadge).toBeNull();
    vi.advanceTimersByTime(6000);
    expect(task("integration_gemini")?.state).toBe("working");
  });

  it("an event that changes nothing leaves the return to idle running", () => {
    const ui = island();
    hook(ui, gemini("Stop"));
    hook(ui, gemini("Notification", { message: "Just so you know." }));
    hook(ui, gemini("SubagentStop"));
    vi.advanceTimersByTime(5200);
    expect(task("integration_gemini")?.state).toBe("idle");
    expect(task("integration_gemini")?.pillBadge).toBeNull();
  });

  it("the end of the session cancels it, so a new session within 5.2 s is left alone", () => {
    const ui = island();
    hook(ui, gemini("Stop"));
    hook(ui, gemini("SessionEnd"));
    hook(ui, gemini("SessionStart"));
    hook(ui, gemini("UserPromptSubmit", { prompt: "again" }));
    vi.advanceTimersByTime(6000);
    expect(task("integration_gemini")?.state).toBe("thinking");
    expect(task("integration_gemini")?.name).toBe("Gemini CLI · Session");
  });

  it("gives an agent's pill its own name back when the session ends", () => {
    const ui = island();
    hook(ui, gemini("SessionStart", { cwd: "D:\\work\\app" }));
    expect(task("integration_gemini")?.name).toBe("Gemini CLI · app");
    hook(ui, gemini("SessionEnd"));
    expect(task("integration_gemini")?.name).toBe("Gemini CLI");
  });
});
