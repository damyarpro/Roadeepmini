import { beforeEach, describe, expect, it } from "vitest";
import { observeClaudeHook } from "./claude";
import { Activity } from "./store";

describe("Claude hook evidence", () => {
  beforeEach(() => Activity.clear());
  it("pairs genuine tool metadata and never invents successful test output", () => {
    observeClaudeHook({hook_event_name:"PreToolUse",session_id:"test-session",tool_use_id:"test-call",tool_name:"Bash",tool_input:{command:"npm test"}});
    observeClaudeHook({hook_event_name:"PostToolUse",session_id:"test-session",tool_use_id:"test-call",tool_name:"Bash"});
    const session = Activity.sessions()[0];
    expect(session.events.map(e => e.callId)).toEqual(["test-call","test-call"]);
    expect(session.test?.verdict).toBe("unknown");
    expect(session.events.every(e => e.output === undefined && e.exitCode === undefined)).toBe(true);
  });
  it("keeps successful edit filepath paired without manufacturing patch evidence", () => {
    observeClaudeHook({hook_event_name:"PreToolUse",session_id:"edit-session",tool_name:"Edit",tool_input:{file_path:"D:\\project\\file.ts"}});
    observeClaudeHook({hook_event_name:"PostToolUse",session_id:"edit-session",tool_name:"Edit"});
    const session = Activity.sessions()[0];
    expect(session.events[1].files).toEqual(["D:\\project\\file.ts"]);
    expect(session.events[0].callId).toBe(session.events[1].callId);
    expect(session.lastPatch).toBeUndefined();
  });
  it("does not retain prompt text or permission targets as evidence", () => {
    observeClaudeHook({hook_event_name:"PermissionRequest",session_id:"private",tool_name:"Bash",tool_input:{command:"private approval data"}});
    expect(Activity.sessions()).toEqual([]);
  });
});
