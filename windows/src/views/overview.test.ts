import { beforeEach,describe,expect,it,vi } from "vitest";
import { State,type AgentTask } from "../core/state";
import { setLanguage,t } from "../core/i18n";
import { buildOverview,type ViewActions } from "./views";
import { renderIntegrationCard } from "./integrations";
const actions=()=>({setView:vi.fn(),collapse:vi.fn(),setFocus:vi.fn(),openTerminal:vi.fn(),openTarget:vi.fn(),openUrl:vi.fn(),decide:vi.fn(),toggleSound:vi.fn(),setVolume:vi.fn(),setAutoClose:vi.fn(),openSettingsWindow:vi.fn(),blip:vi.fn(),toggleMaximize:vi.fn()} satisfies ViewActions);
const task=(state:AgentTask["state"],steps=["Codex","Read file"]):AgentTask=>({id:"integration_codex",name:"Codex · project",color:"#F5F6F8",state,steps,stepIndex:steps.length-1,source:"agent",isIntegration:true});
beforeEach(()=>{setLanguage("en");State.tasks=[];State.integrations={};State.focusId=null;});
describe("coding overview credential boundary",()=>{
 it.each(["working","thinking","finished","error","idle"] as const)("Codex %s displays its ticker without an API-key warning",state=>{
  const current=task(state);State.tasks=[current];State.focusId=current.id;const hooks=actions();const view=buildOverview(hooks);view.sync();
  expect(view.el.querySelector(".ticker")).not.toBeNull();expect(view.el.querySelector(".who .tool")?.textContent).toBe("Codex");expect(view.el.textContent).toContain("Read file");
  expect(view.el.textContent).not.toContain(t("int.keyNotConfigured"));expect(view.el.querySelector(".count")).toBeNull();expect(hooks.openTarget).not.toHaveBeenCalled();
  const jump=view.el.querySelector<HTMLButtonElement>(".jump")!;expect(jump.title).toBe("Codex activity details");jump.click();expect(hooks.openTarget).toHaveBeenCalledOnce();expect(hooks.decide).not.toHaveBeenCalled();
 });
 it("empty cancelled/idle Codex never falls into credential configuration",()=>{
  State.tasks=[task("idle",[])];State.focusId="integration_codex";const view=buildOverview(actions());view.sync();expect(view.el.querySelector(".ticker")).not.toBeNull();expect(view.el.textContent).not.toContain(t("int.keyNotConfigured"));
 });
 it("preserves the Claude ticker/count and unconfigured API integration warning",()=>{
  State.tasks=[{...task("working"),id:"integration_claude",source:"claudeCode"}];State.focusId="integration_claude";const view=buildOverview(actions());view.sync();expect(view.el.querySelector(".who .tool")?.textContent).toBe("Claude Code");expect(view.el.querySelector(".count")?.textContent).toBe("2/2");
  State.tasks=[{...task("idle",[]),id:"integration_github"}];State.focusId="integration_github";view.sync();expect(view.el.querySelector(".ticker")).toBeNull();expect(view.el.textContent).toContain(t("int.keyNotConfigured"));
 });
 it("defensive direct integration rendering gives Codex an evidence action, never a credential action",()=>{
  const openAgent=vi.fn(),openSettings=vi.fn();const card=renderIntegrationCard(task("idle",[]),{detailOpen:false,openDetail:vi.fn(),closeDetail:vi.fn(),openSettings,openAgent});
  expect(card.textContent).not.toContain(t("int.keyNotConfigured"));card.querySelector("button")!.click();expect(openAgent).toHaveBeenCalledOnce();expect(openSettings).not.toHaveBeenCalled();
 });
});
