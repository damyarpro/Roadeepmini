import { describe, expect, it, vi } from "vitest";
import { ActivityStore } from "./store";
import { CODEX_TASK_ID, codexTask } from "./island-adapter";
import { routeCodexTarget } from "./codex-target";
const now=1_000_000;
describe("observed Codex target navigation",()=>{
 it("routes an explicit click to the active parent shown by the character, not a newer completed worker",()=>{
  const store=new ActivityStore();store.ingest({id:"start",sessionId:"parent",harness:"codex",kind:"tool",phase:"started",tool:"build",at:1,cwd:"D:/parent"});
  store.ingest({id:"done",sessionId:"worker",harness:"codex",kind:"finished",at:now,cwd:"D:/worker"});
  const select=vi.fn(),openActivity=vi.fn();expect(select).not.toHaveBeenCalled();expect(openActivity).not.toHaveBeenCalled();
  expect(routeCodexTarget(CODEX_TASK_ID,store.sessions(),{select,openActivity},now)).toBe(true);
  expect(select.mock.calls[0][0].id).toBe("parent");expect(select.mock.calls[0][0].cwd).toBe(codexTask(store.sessions(),now)?.sessionCwd);expect(openActivity).toHaveBeenCalledOnce();
 });
 it("leaves Claude, provider, custom-agent and URL target branches unchanged",()=>{
  for(const id of ["integration_claude","integration_n8n","integration_github","local:agent"]){
   const select=vi.fn(),openActivity=vi.fn();expect(routeCodexTarget(id,[],{select,openActivity},now)).toBe(false);expect(select).not.toHaveBeenCalled();expect(openActivity).not.toHaveBeenCalled();
  }
 });
 it("never falls through to another provider or Claude approval when evidence has disappeared",()=>{
  const select=vi.fn(),openActivity=vi.fn();expect(routeCodexTarget(CODEX_TASK_ID,[],{select,openActivity},now)).toBe(true);expect(select).not.toHaveBeenCalled();expect(openActivity).not.toHaveBeenCalled();
 });
});
