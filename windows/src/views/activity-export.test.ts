import { beforeEach, describe, expect, it, vi } from "vitest";
const bridge = vi.hoisted(() => ({codingExport:vi.fn(),log:vi.fn()}));
vi.mock("../core/bridge", () => ({ Bridge:bridge, IS_TAURI:true, onEvent:async()=>()=>{} }));
import { buildActivity } from "./activity";
import { Activity } from "../activity/store";
import { State } from "../core/state";
import { Monitor } from "../activity/monitor";
import { setLanguage } from "../core/i18n";

describe("native evidence export", () => {
  beforeEach(() => {
    vi.clearAllMocks(); Activity.clear(); State.settings.observeCodex=true; State.settings.language="en"; setLanguage("en"); Monitor.available=true; Monitor.error=false;
    Activity.ingest({id:"fixture",sessionId:"export",harness:"codex",kind:"session",at:1});
  });
  it("waits for a successful native write and blocks repeat exports during live events", async () => {
    let complete!: (path:string)=>void;
    bridge.codingExport.mockReturnValue(new Promise<string>(resolve=>{complete=resolve;}));
    const view=buildActivity(); view.sync();
    const button=view.el.querySelector<HTMLButtonElement>(".act-footer button:nth-child(2)")!;
    button.click();
    expect(button.disabled).toBe(true); expect(view.el.textContent).not.toContain("Markdown saved");
    Activity.ingest({id:"live",sessionId:"export",harness:"codex",kind:"tool",at:2,tool:"Read",phase:"started"});
    view.sync(); expect(button.disabled).toBe(true); button.click(); expect(bridge.codingExport).toHaveBeenCalledTimes(1);
    complete("D:\\fixture\\exports\\handoff.md");
    await vi.waitFor(()=>expect(view.el.textContent).toContain("Markdown saved: D:\\fixture\\exports\\handoff.md"));
    expect(button.disabled).toBe(false);
  });
  it("shows an honest export failure without raw backend error details", async () => {
    bridge.codingExport.mockRejectedValue(new Error("secret personal path"));
    const view=buildActivity(); view.sync(); view.el.querySelector<HTMLButtonElement>(".act-footer button:nth-child(2)")!.click();
    await vi.waitFor(()=>expect(view.el.textContent).toContain("Export failed"));
    expect(view.el.textContent).not.toContain("secret personal path");
    expect(bridge.log).toHaveBeenCalledWith("coding activity handoff export failed");
  });
});
