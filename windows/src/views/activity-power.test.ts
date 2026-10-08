import { describe, expect, it, vi } from "vitest";
import "../core/locales/activity";
import { setLanguage } from "../core/i18n";
import type { ActivitySession, GitInspection, HandoffAgent } from "../activity/types";
import { createCodingActions, observedUsage } from "./activity-power";

const session = (extra: Partial<ActivitySession> = {}): ActivitySession => ({
  id:"test",harness:"codex",cwd:"D:\\project",title:"Test",status:"finished",updatedAt:1,
  events:[],changedFiles:[],conflicts:[],counts:{tools:0,errors:0},...extra,
});
const snapshot = (extra: Partial<GitInspection> = {}): GitInspection => ({
  root:"D:\\project",at:Date.now(),files:[{path:"file.ts",status:"M"}],patch:'+<img src=x onerror="alert(1)">',truncated:false,...extra,
});
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
function setup() {
  setLanguage("en");
  const host = {
    git: vi.fn(async () => snapshot()),
    agents: vi.fn(async (): Promise<HandoffAgent[]> => [{id:"codex",name:"Codex",executable:"C:\\tools\\codex.exe"}]),
    handoff: vi.fn(async () => ({path:"D:\\note.md",agent:"codex"})), changed: vi.fn(), log: vi.fn(),
  };
  const actions = createCodingActions(host);
  actions.sync(session());
  return {host,actions,buttons:actions.el.querySelectorAll<HTMLButtonElement>("button")};
}

describe("explicit coding project actions", () => {
  it("makes no Git or terminal calls while rendering; inspection is literal project-wide evidence", async () => {
    const {host,actions,buttons} = setup();
    expect(host.git).not.toHaveBeenCalled();
    expect(host.agents).not.toHaveBeenCalled();
    buttons[0].click();
    await flush();
    expect(host.git).toHaveBeenCalledExactlyOnceWith("D:\\project");
    expect(actions.el.querySelector("img")).toBeNull();
    expect(actions.el.textContent).toContain("<img src=x");
    expect(actions.el.textContent).toContain("not attributed to this session");
    actions.sync(session({updatedAt:2}));
    expect(host.git).toHaveBeenCalledTimes(1);
  });

  it("discovers agents without launching; only a second explicit click opens the terminal", async () => {
    const {host,actions,buttons} = setup();
    buttons[1].click();
    await flush();
    expect(host.agents).toHaveBeenCalledTimes(1);
    expect(host.handoff).not.toHaveBeenCalled();
    expect(actions.el.querySelector<HTMLElement>(".act-terminal")?.hidden).toBe(false);
    buttons[2].click();
    await flush();
    expect(host.handoff).toHaveBeenCalledTimes(1);
    expect(host.handoff.mock.calls[0].slice(0,2)).toEqual(["D:\\project","codex"]);
    expect(actions.el.textContent).toContain("Terminal opened");
  });

  it("drops stale Git responses after selecting another project and disables actions without a path", async () => {
    const {host,actions,buttons} = setup();
    let finish!: (result: GitInspection) => void;
    host.git.mockImplementationOnce(() => new Promise((resolve) => { finish=resolve; }));
    buttons[0].click();
    actions.sync(session({cwd:"D:\\other"}));
    finish(snapshot());
    await flush();
    expect(actions.el.querySelector(".act-git-result")?.textContent).toBe("");
    actions.sync(session({cwd:undefined}));
    expect(buttons[0].disabled).toBe(true);
    expect(buttons[1].disabled).toBe(true);
  });

  it("reports failure without exposing native error text and can retry", async () => {
    const {host,actions,buttons} = setup();
    host.git.mockRejectedValueOnce(new Error("private-file-path"));
    buttons[0].click();
    await flush();
    expect(actions.el.textContent).toContain("Git inspection failed");
    expect(actions.el.textContent).not.toContain("private-file-path");
    expect(buttons[0].disabled).toBe(false);
    buttons[0].click();
    await flush();
    expect(actions.el.textContent).toContain("Git snapshot ready");
  });
});

describe("observed coding usage", () => {
  it("does not synthesize quota from absent evidence and preserves measured zero", () => {
    setLanguage("en");
    expect(observedUsage(session())).toBeNull();
    const el = observedUsage(session({context:{usedPercent:0},usage:{primary:{usedPercent:0}}}))!;
    expect(el.textContent).toContain("Context · 0%");
    expect(el.textContent).toContain("Primary quota · 0% used");
    expect(el.textContent).not.toContain("Secondary quota");
  });
});
