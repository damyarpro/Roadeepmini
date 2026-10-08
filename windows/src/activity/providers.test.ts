import {beforeEach,describe,expect,it} from "vitest";
import {Activity} from "./store";
import {observeCodingHook,clearClaudePending} from "./claude";
beforeEach(()=>{Activity.clear();clearClaudePending();});
describe("provider hook evidence",()=>{
 it("keeps equal session/call IDs isolated by coding provider",()=>{
  for(const provider of ["codex","cursor","gemini","kiro"]){
   observeCodingHook({provider,hook_event_name:"PreToolUse",session_id:"same",tool_use_id:"same",tool_name:"Shell",tool_input:{command:`echo ${provider}`}});
   observeCodingHook({provider,hook_event_name:"PostToolUse",session_id:"same",tool_use_id:"same",tool_name:"Shell"});
  }
  expect(Activity.sessions()).toHaveLength(4);
  for(const s of Activity.sessions()){expect(s.events[1].command).toBe(`echo ${s.harness}`);expect(s.events[1].output).toBeUndefined();}
 });
 it("merges matching hook/log call phases but retains richer result evidence",()=>{
  observeCodingHook({provider:"codex",hook_event_name:"PreToolUse",session_id:"one",tool_use_id:"call",tool_name:"exec_command",tool_input:{command:"npm test"}});
  Activity.ingest({id:"log-start",sessionId:"one",harness:"codex",kind:"tool",at:Date.now(),phase:"started",callId:"call",command:"npm test"});
  observeCodingHook({provider:"codex",hook_event_name:"PostToolUse",session_id:"one",tool_use_id:"call",tool_name:"exec_command"});
  Activity.ingest({id:"log-end",sessionId:"one",harness:"codex",kind:"tool",at:Date.now(),phase:"completed",callId:"call",output:"Tests  3 passed (3)",exitCode:0});
  const s=Activity.sessions()[0];expect(s.events).toHaveLength(2);expect(s.counts.tools).toBe(1);expect(s.events[1].output).toContain("3 passed");
 });
 it("does not mistake simultaneous identical commands for the same call",()=>{
  for(const call of ["one","two"])observeCodingHook({provider:"codex",hook_event_name:"PreToolUse",session_id:"one",tool_use_id:call,tool_name:"exec_command",tool_input:{command:"npm test"}});
  expect(Activity.sessions()[0].counts.tools).toBe(2);
 });
 it("rejects unknown providers and does not create activity from a permission request",()=>{
  observeCodingHook({provider:"unknown",hook_event_name:"SessionStart"});observeCodingHook({provider:"codex",hook_event_name:"PermissionRequest",session_id:"private"});expect(Activity.sessions()).toHaveLength(0);
 });
 it("pausing log reading preserves direct hooks without retaining rich log-only results",()=>{
  observeCodingHook({provider:"codex",hook_event_name:"PreToolUse",session_id:"one",tool_use_id:"call",tool_name:"exec_command"});
  Activity.ingest({id:"log",sessionId:"one",harness:"codex",kind:"tool",at:Date.now(),phase:"started",callId:"call",output:"log-only output"});
  Activity.ingest({id:"other",sessionId:"other",harness:"codex",kind:"prompt",at:Date.now()});
  Activity.clearHarness("codex",true);
  expect(Activity.sessions()).toHaveLength(1);expect(Activity.sessions()[0].events[0].source).toBe("hook");expect(Activity.sessions()[0].events[0].output).toBeUndefined();
 });
});
