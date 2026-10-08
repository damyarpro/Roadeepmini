import { beforeEach, describe, expect, it, vi } from "vitest";
import { Bridge } from "../core/bridge";
import { buildActivity, preserveActivityView, requestActivitySession } from "./activity";
import { Activity } from "../activity/store";
import { State } from "../core/state";
import { Monitor } from "../activity/monitor";
import { setLanguage } from "../core/i18n";

describe("activity evidence view", () => {
  it("allows clearing an unreadable saved archive when no sessions were restored", () => {
    Monitor.history.error = true;
    const view = buildActivity(); view.sync();
    expect(view.el.querySelector<HTMLButtonElement>(".act-footer > .act-clear")?.disabled).toBe(false);
    Monitor.history.error = false;
  });

  it("preserves the exact focused project control and retention focus across live repaint", async () => {
    const agentList = vi.spyOn(Bridge, "codingHandoffAgents").mockResolvedValue([{id:"codex",name:"Codex",executable:"C:\\tools\\codex.exe"}]);
    Activity.ingest({id:"start-focus",sessionId:"reading-controls",harness:"codex",kind:"session",at:1,cwd:"D:\\project"});
    const view = buildActivity(); document.body.append(view.el); view.sync();
    const tools = view.el.querySelector<HTMLDetailsElement>(".act-project-tools")!;
    tools.open = true;
    view.el.querySelector<HTMLButtonElement>('[data-act-focus="prepare"]')!.click();
    await new Promise<void>(resolve => setTimeout(resolve,0));
    try {
      for (const [i, part] of ["summary", "git", "prepare", "agent", "launch"].entries()) {
        const focused = view.el.querySelector<HTMLElement>(`[data-act-focus="${part}"]`)!;
        focused.focus();
        Activity.ingest({id:`next-focus-${i}`,sessionId:"reading-controls",harness:"codex",kind:"tool",at:2+i,phase:"started",tool:"Read"});
        view.sync();
        expect(document.activeElement).toBe(focused);
        expect(tools.open).toBe(true);
      }
      const retained = view.el.querySelector<HTMLElement>(".act-history summary")!;
      retained.focus();
      Activity.ingest({id:"retention-focus",sessionId:"reading-controls",harness:"codex",kind:"tool",at:8,phase:"started",tool:"Read"});
      view.sync();
      expect(document.activeElement).toBe(retained);
    } finally { view.el.remove(); agentList.mockRestore(); }
  });

  it("keeps archived evidence idle and exposes local persistence as an explicit opt-in", () => {
    Activity.ingest({id:"saved",sessionId:"archive",harness:"codex",kind:"prompt",at:Date.now()});
    Activity.archiveHarness("codex");
    const view = buildActivity(); view.sync();
    expect(view.el.textContent).toContain("Archived");
    expect(State.activityBotState).toBe("idle");
    expect(view.el.querySelector(".act-history")?.textContent).toContain("coding-history-v1.json");
    expect(view.el.querySelector<HTMLDetailsElement>(".act-history")?.open).toBe(false);
  });
  beforeEach(() => { Activity.clear(); State.settings.observeCodex = true; State.settings.language = "en"; Monitor.error = false; Monitor.available = true; setLanguage("en"); });
  it("an explicit island request selects its active parent in an already mounted activity view",()=>{
    Activity.ingest({id:"parent",sessionId:"parent",harness:"codex",kind:"prompt",at:1,cwd:"D:/parent"});
    Activity.ingest({id:"worker",sessionId:"worker",harness:"codex",kind:"finished",at:2,cwd:"D:/worker"});
    const view=buildActivity();view.sync();const selector=view.el.querySelector("select")!;expect(selector.value).toBe("codex:worker");
    requestActivitySession({harness:"codex",id:"parent"});view.sync();expect(selector.value).toBe("codex:parent");expect(State.activityBotState).toBe("thinking");
    selector.value="codex:worker";selector.dispatchEvent(new Event("change"));
    Activity.ingest({id:"usage",sessionId:"parent",harness:"codex",kind:"usage",at:3});view.sync();expect(selector.value).toBe("codex:worker");
  });
  it("completed and failed tools return to thinking, ignoring subsequent usage counters",()=>{
    Activity.ingest({id:"tool",sessionId:"one",harness:"codex",kind:"tool",phase:"started",at:1});const view=buildActivity();view.sync();expect(State.activityBotState).toBe("working");
    Activity.ingest({id:"result",sessionId:"one",harness:"codex",kind:"tool",phase:"completed",at:2});view.sync();expect(State.activityBotState).toBe("thinking");
    Activity.ingest({id:"usage",sessionId:"one",harness:"codex",kind:"usage",at:3});view.sync();expect(State.activityBotState).toBe("thinking");
    Activity.ingest({id:"failed",sessionId:"one",harness:"codex",kind:"tool",phase:"failed",at:4});view.sync();expect(State.activityBotState).toBe("thinking");
  });
  it("cancelled turns display stopped, idle and never successful completion",()=>{
    Activity.ingest({id:"stop",sessionId:"one",harness:"codex",kind:"cancelled",at:1});setLanguage("fa");State.settings.language="fa";
    const view=buildActivity();view.sync();expect(State.activityBotState).toBe("idle");expect(view.el.querySelector(".act-status.cancelled")?.textContent).toBe("متوقف شد");
    expect(view.el.querySelector(".act-timeline")?.textContent).toContain("متوقف شد");expect(view.el.querySelector(".act-status.finished")).toBeNull();
  });
  it("shows intentional off state and an explicit opt-in control", () => {
    State.settings.observeCodex = false;
    const view = buildActivity(); view.sync();
    expect(view.el.textContent).toContain("Codex observation is off");
    expect(view.el.querySelector("button[aria-pressed='false']")?.textContent).toBe("Read Codex logs");
  });
  it("keeps compound harness identity and renders patch output as literal text", () => {
    Activity.ingest({id:"c",sessionId:"same",harness:"claude",kind:"session",at:1,cwd:"D:\\other"});
    Activity.ingest({id:"s",sessionId:"same",harness:"codex",kind:"session",at:2,cwd:"D:\\project"});
    Activity.ingest({id:"p",sessionId:"same",harness:"codex",kind:"tool",at:3,tool:"apply_patch",phase:"completed",exitCode:0,files:["file.ts"],patch:'+<img src=x onerror="alert(1)">'});
    const view = buildActivity(); view.sync();
    const select = view.el.querySelector("select")!;
    expect([...select.options].map(o => o.value)).toEqual(["codex:same","claude:same"]);
    expect(view.el.querySelector("img")).toBeNull();
    expect(view.el.textContent).toContain("<img src=x");
    expect(view.el.textContent).toContain("file.ts");
    select.value = "claude:same"; select.dispatchEvent(new Event("change"));
    expect(view.el.textContent).toContain("D:\\other");
    Activity.ingest({id:"later",sessionId:"same",harness:"codex",kind:"finished",at:4});
    view.sync(); expect(select.value).toBe("claude:same");
  });
  it("shows a retryable failure alongside retained evidence", () => {
    Monitor.error = true;
    const view = buildActivity(); view.sync();
    expect(view.el.textContent).toContain("Activity could not be loaded or saved");
    expect(view.el.querySelector(".act-alert button")?.textContent).toBe("Retry");
  });
  it("preserves recap expansion, evidence scroll and keyboard focus on live updates", () => {
    Activity.ingest({id:"start",sessionId:"reading",harness:"codex",kind:"session",at:1,cwd:"D:\\project"});
    const view = buildActivity(); document.body.append(view.el); view.sync();
    const recap = view.el.querySelector<HTMLDetailsElement>(".act-recap")!;
    const summary = recap.querySelector("summary")!;
    recap.open = true; summary.focus();
    const scroll = view.el.querySelector(".act-scroll")!; scroll.scrollTop = 90;
    Activity.ingest({id:"next",sessionId:"reading",harness:"codex",kind:"tool",at:2,phase:"started",tool:"Read"});
    view.sync();
    expect(view.el.querySelector<HTMLDetailsElement>(".act-recap")?.open).toBe(true);
    expect(document.activeElement).toBe(view.el.querySelector(".act-recap summary"));
    expect(scroll.scrollTop).toBe(90);
    const stableSummary = document.activeElement;
    Activity.ingest({id:"other",sessionId:"other",harness:"claude",kind:"tool",at:3,phase:"started",tool:"Read"});
    view.sync(); expect(document.activeElement).toBe(stableSummary);
    setLanguage("fa"); State.settings.language = "fa"; view.sync();
    expect(view.el.querySelector<HTMLPreElement>(".act-recap pre")?.dir).toBe("ltr");
    expect(view.el.querySelector<HTMLDetailsElement>(".act-recap")?.open).toBe(true);
    expect(document.activeElement).toBe(view.el.querySelector(".act-recap summary"));
    view.el.remove();
  });
  it("uses selected activity status without overriding existing alerts or focused tasks", () => {
    const oldView = State.view;
    State.view = "activity"; State.stateOverride = null;
    Activity.ingest({id:"a",sessionId:"active",harness:"codex",kind:"prompt",at:1});
    Activity.ingest({id:"b",sessionId:"done",harness:"codex",kind:"finished",at:2});
    const view = buildActivity(); view.sync();
    expect(State.effectiveState).toBe("finished");
    const select = view.el.querySelector("select")!;
    select.value="codex:active"; select.dispatchEvent(new Event("change"));
    expect(State.effectiveState).toBe("thinking");
    State.stateOverride="approval";
    expect(State.effectiveState).toBe("approval");
    State.stateOverride=null; State.view="overview";
    expect(State.effectiveState).toBe(State.focusTask?.state ?? "idle");
    State.view = oldView; State.activityBotState=null;
  });
  it("retains selection and open recap across the actual language DOM rebuild", () => {
    const oldView = State.view; State.view="activity";
    Activity.ingest({id:"old",sessionId:"reading",harness:"codex",kind:"session",at:1});
    const original = buildActivity(); document.body.append(original.el); original.sync();
    original.el.querySelector<HTMLDetailsElement>(".act-recap")!.open=true;
    original.el.querySelector<HTMLElement>(".act-recap summary")!.focus();
    preserveActivityView(); original.el.remove(); setLanguage("fa"); State.settings.language="fa";
    const rebuilt = buildActivity(); document.body.append(rebuilt.el); rebuilt.sync();
    expect(rebuilt.el.querySelector("select")?.value).toBe("codex:reading");
    expect(rebuilt.el.querySelector<HTMLDetailsElement>(".act-recap")?.open).toBe(true);
    expect(document.activeElement).toBe(rebuilt.el.querySelector(".act-recap summary"));
    rebuilt.el.remove(); State.view=oldView;
  });
});
