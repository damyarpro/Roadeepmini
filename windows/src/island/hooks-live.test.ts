// Live diff, the final line and the step count, end to end through the hook
// handler: file edits become ticker steps with their diff stored (and bounded),
// and Stop leaves the turn's final message on the card until the next turn.

import { beforeEach, describe, expect, it, vi } from "vitest";
import { handleHook, type HookPayload } from "./hooks";
import { DEFAULT_SETTINGS, State } from "../core/state";
import type { Island } from "./island";
import { parseDiffStep } from "../core/diff";
import {
  DIFF_TTL_MS, MAX_DIFFS_PER_PILL, clearSessionTrail, findDiff, finalLine, sessionDiffs, stepSeq,
} from "./live-session";
import { setLanguage } from "../core/i18n";

vi.mock("../core/bridge", () => ({
  IS_TAURI: false,
  onEvent: async () => () => {},
  Bridge: { approvalAck: vi.fn(), approvalDecline: vi.fn(), log: vi.fn() },
}));
vi.mock("../core/sound", () => ({ Sound: { play: vi.fn() } }));

const CLAUDE = "integration_claude";
const ui = {
  alert: vi.fn(), reveal: vi.fn(), setView: vi.fn(), dropPin: vi.fn(), cardCleared: vi.fn(), inDropFlow: false,
} as unknown as Island;
const hook = (payload: HookPayload) => handleHook(ui, { provider: "claude", ...payload });
const task = (id = CLAUDE) => State.tasks.find((t) => t.id === id)!;

const edit = (extra: HookPayload = {}) => hook({
  hook_event_name: "PostToolUse",
  cwd: "/p/proj",
  tool_name: "Edit",
  tool_input: { file_path: "/p/proj/src/app.ts", old_string: "a\nb\n", new_string: "a\nc\nd\n" },
  ...extra,
});

beforeEach(() => {
  vi.useFakeTimers();
  setLanguage("en");
  for (const id of [CLAUDE, "integration_gemini"]) clearSessionTrail(id);
  State.tasks = [{ id: CLAUDE, name: "Roadeep", color: "#fff", state: "idle", steps: [], stepIndex: 0, source: "claudeCode", isIntegration: true }];
  State.settings = { ...DEFAULT_SETTINGS };
  State.focusId = CLAUDE;
  State.pendingApproval = null;
  State.paused = false;
  State.mode = "compact";
});

describe("live diff", () => {
  it("adds a diff step with its counts after a finished Edit, and keeps the diff", () => {
    edit();
    const step = parseDiffStep(task().steps.at(-1)!)!;
    expect({ ...step, diffId: 0 }).toEqual({ filename: "app.ts", added: 2, removed: 1, diffId: 0 });
    const diff = findDiff(CLAUDE, step.diffId)!;
    expect(diff.path).toBe("/p/proj/src/app.ts");
    expect(diff.hunks.length).toBeGreaterThan(0);
  });

  it("diffs Write and MultiEdit too, never other tools or a starting tool", () => {
    hook({ hook_event_name: "PostToolUse", tool_name: "Write", tool_input: { file_path: "C:\\p\\new.md", content: "x\ny\n" } });
    expect(parseDiffStep(task().steps.at(-1)!)?.filename).toBe("new.md");
    hook({
      hook_event_name: "PostToolUse", tool_name: "MultiEdit",
      tool_input: { file_path: "/p/m.ts", edits: [{ old_string: "a", new_string: "b" }, { old_string: "c", new_string: "d" }] },
    });
    const multi = parseDiffStep(task().steps.at(-1)!)!;
    expect([multi.added, multi.removed]).toEqual([2, 2]);

    const before = task().steps.length;
    hook({ hook_event_name: "PostToolUse", tool_name: "Bash", tool_input: { command: "ls" } });
    hook({ hook_event_name: "PreToolUse", tool_name: "Edit", tool_input: { file_path: "/p/a.ts", old_string: "a", new_string: "b" } });
    expect(task().steps.length).toBe(before + 1); // only the PreToolUse label
    expect(parseDiffStep(task().steps.at(-1)!)).toBeNull();
    expect(sessionDiffs(CLAUDE)).toHaveLength(2);
  });

  it("adds nothing for an edit with no change, or one the relay had to cut", () => {
    hook({ hook_event_name: "PostToolUse", tool_name: "Edit", tool_input: { file_path: "/p/a.ts", old_string: "same", new_string: "same" } });
    edit({ roadeep_diff_truncated: true });
    expect(task().steps).toEqual([]);
    expect(sessionDiffs(CLAUDE)).toHaveLength(0);
  });

  it("caps the diffs per pill, oldest first, with ids that stay unique", () => {
    for (let i = 0; i < MAX_DIFFS_PER_PILL + 5; i++) edit();
    const kept = sessionDiffs(CLAUDE);
    expect(kept).toHaveLength(MAX_DIFFS_PER_PILL);
    expect(new Set(kept.map((d) => d.id)).size).toBe(MAX_DIFFS_PER_PILL);
    // The ticker keeps fewer steps than diffs; every step still finds its diff.
    for (const s of task().steps) expect(findDiff(CLAUDE, parseDiffStep(s)!.diffId)).not.toBeNull();
    expect(findDiff(CLAUDE, kept[0].id - 1)).toBeNull();
  });

  it("forgets the diffs an hour after the last one, and at the end of the session", () => {
    edit();
    vi.advanceTimersByTime(DIFF_TTL_MS - 1000);
    edit(); // re-arms the hour
    vi.advanceTimersByTime(2000);
    expect(sessionDiffs(CLAUDE)).toHaveLength(2);
    vi.advanceTimersByTime(DIFF_TTL_MS);
    expect(sessionDiffs(CLAUDE)).toHaveLength(0);

    edit();
    hook({ hook_event_name: "SessionEnd", cwd: "/p/proj" });
    expect(sessionDiffs(CLAUDE)).toHaveLength(0);
  });

  it("keeps each agent's diffs on its own pill", () => {
    handleHook(ui, { provider: "gemini", hook_event_name: "PostToolUse", tool_name: "Edit", tool_input: { file_path: "/g.ts", old_string: "a", new_string: "b" } });
    expect(sessionDiffs("integration_gemini")).toHaveLength(1);
    expect(sessionDiffs(CLAUDE)).toHaveLength(0);
  });
});

describe("the final line", () => {
  it("is the first paragraph of the turn's final message, on one line", () => {
    hook({ hook_event_name: "Stop", last_assistant_message: "**Done.** Fixed the `parser` and\nadded tests.\n\n## Details\n- a" });
    const line = "Done. Fixed the parser and added tests.";
    expect(finalLine(CLAUDE)).toBe(line);
    expect(task().steps.at(-1)).toBe(line);
  });

  it("falls back to `message`, and is nothing when both are empty", () => {
    hook({ hook_event_name: "Stop", message: "All good." });
    expect(finalLine(CLAUDE)).toBe("All good.");
    hook({ hook_event_name: "UserPromptSubmit", prompt: "again" });
    hook({ hook_event_name: "Stop", last_assistant_message: "\n\n---\n" });
    expect(finalLine(CLAUDE)).toBeNull();
    expect(task().steps.at(-1)).toBe("again");
  });

  it("is kept to 200 characters", () => {
    hook({ hook_event_name: "Stop", last_assistant_message: "word ".repeat(100) });
    expect(finalLine(CLAUDE)!.length).toBeLessThanOrEqual(200);
  });

  it.each([
    ["a new prompt", { hook_event_name: "UserPromptSubmit", prompt: "next" }],
    ["a tool starting", { hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "ls" } }],
    ["a new session", { hook_event_name: "SessionStart" }],
    ["the end of the session", { hook_event_name: "SessionEnd" }],
  ] as [string, HookPayload][])("stays until %s", (_what, event) => {
    hook({ hook_event_name: "Stop", last_assistant_message: "Done." });
    vi.advanceTimersByTime(5200); // back to idle: the line is still there
    expect(task().state).toBe("idle");
    expect(finalLine(CLAUDE)).toBe("Done.");
    hook({ cwd: "/p/proj", ...event });
    expect(finalLine(CLAUDE)).toBeNull();
  });
});

describe("the step count", () => {
  it("goes on past the 20 steps a pill keeps", () => {
    for (let i = 0; i < 25; i++) {
      hook({ hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: `c${i}` } });
    }
    expect(task().steps).toHaveLength(20);
    expect(task().stepIndex).toBe(19);
    expect(stepSeq(CLAUDE)).toBe(24);
    expect(task().steps.at(-1)).toBe("Runs · c24");
  });

  it("starts again with the session", () => {
    hook({ hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "a" } });
    hook({ hook_event_name: "SessionEnd" });
    expect(stepSeq(CLAUDE)).toBeUndefined();
    hook({ hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "b" } });
    expect(stepSeq(CLAUDE)).toBe(0);
  });
});
