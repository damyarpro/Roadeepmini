import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ActivityStore } from "./store";
import { State } from "../core/state";
import { Activity } from "./store";
import { codexTask, registerCodingIsland, CODEX_TASK_ID } from "./island-adapter";
const now = Date.now();
function event(id: string, sessionId: string, kind: string, delta = 0, extra = {}) { return { id, sessionId, kind, harness: "codex", at: now + delta, ...extra }; }
describe("Codex observed island state", () => {
  beforeEach(() => { Activity.clear(); State.tasks = []; State.focusId = null; State.pendingApproval = null; State.stateOverride = null; State.paused = false; State.settings.observeCodex = true; State.view = "overview"; State.mode = "hidden"; State.loadIntegrationTasks(); });
  afterEach(() => vi.useRealTimers());
  it.each(["completed","failed"] as const)("a %s tool result returns to thinking until a real terminal event",phase=>{
    const store=new ActivityStore();store.ingest(event("start","live","tool",0,{phase:"started",tool:"exec"}));expect(codexTask(store.sessions(),now)?.state).toBe("working");
    store.ingest(event("result","live","tool",1,{phase,tool:"exec"}));expect(codexTask(store.sessions(),now+1)?.state).toBe("thinking");
    store.ingest(event("quota","live","usage",2));expect(codexTask(store.sessions(),now+2)?.state).toBe("thinking");
    store.ingest(event("error","live","error",3));expect(codexTask(store.sessions(),now+3)?.state).toBe("error");
  });
  it("cancelled work is idle, terminal and expires without a successful badge",()=>{
    const store=new ActivityStore();store.ingest(event("stop","live","cancelled"));const task=codexTask(store.sessions(),now)!;
    expect(task.state).toBe("idle");expect(task.pillBadge).toBeNull();expect(codexTask(store.sessions(),now+300_001)).toBeNull();
  });
  it("a completed worker cannot mask an active parent", () => {
    const store = new ActivityStore(); store.ingest(event("p", "parent", "prompt", -100)); store.ingest(event("w", "worker", "finished"));
    expect(codexTask(store.sessions(), now)?.state).toBe("thinking");
    store.ingest(event("tool", "parent", "tool", -50, {phase:"started", tool:"functions.exec", command:"private command", cwd:"D:/work/project"}));
    const task = codexTask(store.sessions(), now)!; expect(task.state).toBe("working"); expect(task.name).toBe("Codex \u00b7 project"); expect(task.steps).not.toContain("private command");
  });
  it("archived usage never presents as active work and active silence does not expire", () => {
    const store = new ActivityStore(); store.ingest(event("u", "one", "usage")); expect(codexTask(store.sessions(), now)).toBeNull();
    store.ingest(event("old", "old", "prompt", -400_000)); expect(codexTask(store.sessions(), now)?.state).toBe("thinking");
    store.ingest(event("tool", "old", "tool", -399_000, {phase:"started",tool:"long build"})); expect(codexTask(store.sessions(), now + 3_600_000)?.state).toBe("working");
    expect(codexTask(store.sessions().map(session=>({...session,status:"archived" as const})), now + 3_600_000)).toBeNull();
  });
  it("terminal sessions expire after five minutes while old active parents remain", () => {
    const store = new ActivityStore(); store.ingest(event("done", "worker", "finished"));
    expect(codexTask(store.sessions(),now+300_000)?.state).toBe("finished");expect(codexTask(store.sessions(),now+300_001)).toBeNull();
    store.ingest(event("parent", "parent", "tool", -3_600_000,{phase:"started",tool:"build"}));
    expect(codexTask(store.sessions(),now+300_001)?.state).toBe("working");
  });
  it("the polling lifecycle keeps a silent tool and removes only its later terminal display",()=>{
    vi.useFakeTimers();vi.setSystemTime(now);const stop=registerCodingIsland({reveal:vi.fn()});try {
      Activity.ingest(event("start","live","tool",0,{phase:"started",tool:"build"}));vi.advanceTimersByTime(3_600_000);
      expect(State.tasks.find(task=>task.id===CODEX_TASK_ID)?.state).toBe("working");
      Activity.ingest(event("done","live","finished",3_600_000));expect(State.tasks.find(task=>task.id===CODEX_TASK_ID)?.state).toBe("finished");
      vi.advanceTimersByTime(330_000);expect(State.tasks.some(task=>task.id===CODEX_TASK_ID)).toBe(false);expect(State.focusId).toBe("integration_claude");
    } finally {stop();}
  });
  it("live evidence drives character and clears on opt-out without approvals", () => {
    const reveal = vi.fn(); const stop = registerCodingIsland({reveal});
    Activity.ingest(event("p", "live", "prompt")); expect(State.focusId).toBe(CODEX_TASK_ID); expect(State.effectiveState).toBe("thinking"); expect(reveal).toHaveBeenCalled();
    Activity.ingest(event("t", "live", "tool", 1, {phase:"started", tool:"exec"})); expect(State.effectiveState).toBe("working"); expect(State.pendingApproval).toBeNull();
    State.settings.observeCodex = false; State.notify(); expect(State.tasks.some(t=>t.id===CODEX_TASK_ID)).toBe(false); stop();
  });
  it("active Claude, selected agent and chat focus are respected", () => {
    State.tasks[0].state = "working"; const stop = registerCodingIsland({reveal:vi.fn()}); Activity.ingest(event("p", "live", "prompt")); expect(State.focusId).toBe("integration_claude"); stop();
  });
  it.each(["finished","error"] as const)("%s Claude yields to already active Codex without another event", state => {
    State.tasks[0].state="working";const stop=registerCodingIsland({reveal:vi.fn()});
    try {Activity.ingest(event("p","live","prompt"));expect(State.focusId).toBe("integration_claude");State.updateTask("integration_claude",state);expect(State.focusId).toBe(CODEX_TASK_ID);} finally {stop();}
  });
  it("never focuses a completed Codex session just because Claude is idle",()=>{
    const stop=registerCodingIsland({reveal:vi.fn()});try {Activity.ingest(event("done","done","finished"));expect(State.focusId).toBe("integration_claude");} finally {stop();}
  });
  it("keeps explicit nonintegration selection and interaction guards",()=>{
    State.tasks.push({...State.tasks[0],id:"chosen",state:"idle",isIntegration:false});State.focusId="chosen";
    const stop=registerCodingIsland({reveal:vi.fn()});try {
      Activity.ingest(event("p","live","prompt"));expect(State.focusId).toBe("chosen");
      State.focusId="integration_claude";State.view="prompt";State.notify();expect(State.focusId).toBe("integration_claude");
      State.view="overview";State.stateOverride="thinking";State.notify();expect(State.focusId).toBe("integration_claude");
      State.stateOverride=null;State.pendingApproval={tool:"exec"} as NonNullable<typeof State.pendingApproval>;State.notify();expect(State.focusId).toBe("integration_claude");
      State.pendingApproval=null;State.notify();expect(State.focusId).toBe(CODEX_TASK_ID);
    } finally {stop();}
  });
});
