import {beforeEach,describe,expect,it,vi} from "vitest";
import {handleHook} from "./hooks";
import {State,DEFAULT_SETTINGS} from "../core/state";
import {Bridge} from "../core/bridge";
import type {Island} from "./island";
import {registerCodingIsland} from "../activity/island-adapter";
vi.mock("../core/bridge",()=>({onEvent:async()=>()=>{},Bridge:{approvalAck:vi.fn(),approvalDecline:vi.fn()}}));
vi.mock("../core/sound",()=>({Sound:{play:vi.fn()}}));
const island=()=>({alert:vi.fn(),reveal:vi.fn(),setView:vi.fn(),dropPin:vi.fn(),inDropFlow:false}) as unknown as Island;
beforeEach(()=>{vi.useFakeTimers();vi.clearAllMocks();State.tasks=[{id:"integration_claude",name:"Claude",color:"#fff",state:"idle",steps:[],stepIndex:0,source:"claudeCode",isIntegration:true}];State.settings={...DEFAULT_SETTINGS};State.focusId="integration_claude";State.pendingApproval=null;State.paused=false;State.mode="hidden";});
describe("coding hook UI routing",()=>{
 it("updates each provider's own task, character and source without relabeling Claude",()=>{
  const ui=island();handleHook(ui,{provider:"cursor",hook_event_name:"PreToolUse",session_id:"same",tool_name:"Shell",cwd:"D:/App"});
  expect(State.tasks.find(t=>t.id==="integration_cursor")?.state).toBe("working");expect(State.tasks[0].state).toBe("idle");expect(ui.reveal).toHaveBeenCalled();
 });
 it("acknowledges a Codex permission without granting it and keeps its task identity",()=>{
  handleHook(island(),{provider:"codex",hook_event_name:"PermissionRequest",request_id:"123-1",tool_name:"Bash",tool_input:{command:"npm test"}});
  expect(Bridge.approvalAck).toHaveBeenCalledWith("123-1");expect(State.pendingApproval?.taskId).toBe("integration_codex");expect(State.pendingApproval?.command).toBe("Bash · npm test");
 });
 it("returns requests to the provider when paused, unsupported or another card is pending",()=>{
  const ui=island();State.paused=true;handleHook(ui,{provider:"codex",hook_event_name:"PermissionRequest",request_id:"1-1"});State.paused=false;
  handleHook(ui,{provider:"cursor",hook_event_name:"PermissionRequest",request_id:"1-2"});
  handleHook(ui,{provider:"claude",hook_event_name:"PermissionRequest",request_id:"1-3"});handleHook(ui,{provider:"codex",hook_event_name:"PermissionRequest",request_id:"1-4"});
  expect(vi.mocked(Bridge.approvalDecline).mock.calls.map(c=>c[0])).toEqual(["1-1","1-2","1-4"]);expect(State.pendingApproval?.requestId).toBe("1-3");
 });
 it("does not clear a new turn when an older finish reset timer expires",()=>{
  const ui=island();handleHook(ui,{provider:"gemini",hook_event_name:"Stop"});vi.advanceTimersByTime(1000);handleHook(ui,{provider:"gemini",hook_event_name:"PreToolUse",tool_name:"Shell"});vi.advanceTimersByTime(6000);
  expect(State.tasks.find(t=>t.id==="integration_gemini")?.state).toBe("working");
 });
 it("shows direct Codex hooks with log observation disabled",()=>{
  State.settings.observeCodex=false;const ui=island();const stop=registerCodingIsland(ui);
  try{handleHook(ui,{provider:"codex",hook_event_name:"PreToolUse",session_id:"direct",tool_use_id:"call",tool_name:"exec_command"});expect(State.tasks.find(t=>t.id==="integration_codex")?.state).toBe("working");expect(State.focusId).toBe("integration_codex");}finally{stop();}
 });
});
