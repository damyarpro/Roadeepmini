import { describe, expect, it } from "vitest";
import { toolsEn } from "./tools-en";
import { toolsFa } from "./tools-fa";

describe("chat MCP tool strings", () => {
  it("exist in both languages with the same placeholders", () => {
    expect(Object.keys(toolsFa).sort()).toEqual(Object.keys(toolsEn).sort());
    const vars = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
    for (const key of Object.keys(toolsEn)) {
      expect(vars(toolsFa[key]), key).toEqual(vars(toolsEn[key]));
      expect(toolsFa[key].trim(), key).not.toBe("");
    }
  });

  it("cover every tool-step state and TOOL_* error the chat can show", () => {
    for (const state of ["waiting", "running", "done", "error", "declined", "stopped"]) {
      expect(toolsEn[`tools.state.${state}`], state).toBeTruthy();
    }
    for (const code of ["TOOL_MEMORY_TRIGGER", "TOOL_STEP_LIMIT", "TOOL_APPROVAL_TIMEOUT"]) {
      expect(toolsEn[`rerr.${code}`], code).toBeTruthy();
    }
  });
});
