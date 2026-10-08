import { describe, expect, it } from "vitest";
import raw from "../../src-tauri/src/default_agents.json?raw";
import { memoryTriggerWords } from "./memory-words";
import { DEFAULT_ISLAND_AGENTS, DEFAULT_SETTINGS, agentPillId, isDefaultAgent, parseAgentPill } from "./state";

// The built-in agents live in Rust (default_agents.json, seeded by
// default_agents.rs); these checks keep the window's side in step with them.

interface DefaultAgent {
  id: string;
  name: string;
  description: string;
  instructions: string;
  starterPrompts: string[];
}

const agents = (JSON.parse(raw) as { agents: DefaultAgent[] }).agents;

describe("built-in agents", () => {
  it("never use a phrase the server reads as a memory request", () => {
    expect(agents).toHaveLength(10);
    for (const a of agents) {
      for (const text of [a.name, a.description, a.instructions, ...a.starterPrompts]) {
        expect(memoryTriggerWords(text), `${a.id}: ${text}`).toEqual([]);
      }
    }
  });

  it("are recognised by their id", () => {
    for (const a of agents) expect(isDefaultAgent(a.id), a.id).toBe(true);
    expect(isDefaultAgent("3f1c2b9e-0000-4000-8000-123456789abc")).toBe(false);
  });

  it("put four of them on a new install's island and no service", () => {
    expect(DEFAULT_SETTINGS.activeIntegrations).toEqual(DEFAULT_ISLAND_AGENTS.map((id) => agentPillId("local", id)));
    for (const pill of DEFAULT_SETTINGS.activeIntegrations) {
      const parsed = parseAgentPill(pill);
      expect(parsed?.kind).toBe("local");
      expect(agents.some((a) => a.id === parsed?.id), pill).toBe(true);
    }
    expect(DEFAULT_SETTINGS.addedIntegrations).toEqual([]);
  });
});
