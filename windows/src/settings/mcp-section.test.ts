import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLanguage } from "../core/i18n";
import { BridgeMcp, type McpClient, type McpStatus } from "../core/bridge-mcp";
import { mcpSection } from "./mcp-section";

vi.mock("../core/bridge", () => ({ IS_TAURI: true, Bridge: { log: vi.fn() } }));
vi.mock("../core/bridge-mcp", () => ({ BridgeMcp: { status: vi.fn(), preview: vi.fn(), apply: vi.fn(), legacyCleanup: vi.fn() } }));

const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
const client = (over: Partial<McpClient>): McpClient => ({
  id: "claude-code", name: "Claude Code", detected: true, installed: false, conflict: false, configPath: "C:/Users/Example/.claude.json", ...over,
});
const status = (clients: McpClient[]): McpStatus => ({ exeReady: true, exePath: "C:/Roadeep/roadeep-mcp.exe", signedIn: true, clients });

beforeEach(() => { vi.clearAllMocks(); document.body.replaceChildren(); setLanguage("en"); });

describe("mcp clients", () => {
  it("marks an entry from the previous version as needing an update, with a reviewed update action", async () => {
    vi.mocked(BridgeMcp.status).mockResolvedValue(status([client({ legacyRelay: true })]));
    vi.mocked(BridgeMcp.preview).mockResolvedValue({ diff: "+ new relay", backup: "b", configPath: "C:/Users/Example/.claude.json", fingerprint: "f" });
    const section = mcpSection(); document.body.append(section); await flush();
    const row = section.querySelector(".mcp-client")!;
    expect(row.textContent).toContain("Needs update — installed by the previous version");
    const buttons = [...row.querySelectorAll<HTMLButtonElement>("button")];
    expect(buttons.map((b) => b.textContent)).toEqual(["Update…"]);
    buttons[0].click(); await flush();
    expect(BridgeMcp.preview).toHaveBeenCalledWith("claude-code", true);
    expect(BridgeMcp.apply).not.toHaveBeenCalled();
  });

  it("keeps the normal states when nothing is left from the previous version", async () => {
    vi.mocked(BridgeMcp.status).mockResolvedValue(status([client({ installed: true })]));
    const section = mcpSection(); document.body.append(section); await flush();
    expect(section.textContent).not.toContain("Needs update");
    expect(section.textContent).toContain("Connected");
  });
  it("removes old helper files only on click and shows what was removed and kept", async () => {
    vi.mocked(BridgeMcp.status).mockResolvedValue(status([client({ installed: true })]));
    vi.mocked(BridgeMcp.legacyCleanup).mockResolvedValue({ referenced: false, removed: ["C:/Old/bin/hook.exe"], kept: ["C:/Old/bin/mcp.exe"] });
    const section = mcpSection(); document.body.append(section); await flush();
    expect(BridgeMcp.legacyCleanup).not.toHaveBeenCalled();
    const button = [...section.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Remove old helper files")!;
    button.click(); await flush();
    expect(BridgeMcp.legacyCleanup).toHaveBeenCalledTimes(1);
    expect(section.textContent).toContain("C:/Old/bin/hook.exe");
    expect(section.textContent).toContain("Kept (in use");
    expect(section.textContent).toContain("C:/Old/bin/mcp.exe");
    expect(button.disabled).toBe(false);
  });

  it("explains that nothing was removed while an old relay is still registered, and surfaces failures", async () => {
    vi.mocked(BridgeMcp.status).mockResolvedValue(status([client({ legacyRelay: true })]));
    vi.mocked(BridgeMcp.legacyCleanup).mockResolvedValueOnce({ referenced: true, removed: [], kept: ["C:/Old/bin"] })
      .mockRejectedValueOnce(new Error("disk busy"));
    const section = mcpSection(); document.body.append(section); await flush();
    const button = [...section.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Remove old helper files")!;
    button.click(); await flush();
    expect(section.textContent).toContain("Nothing was removed");
    button.click(); await flush();
    expect(section.querySelector('[role="alert"]')?.textContent).toContain("could not be removed");
  });
});
