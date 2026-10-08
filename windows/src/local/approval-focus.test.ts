import {beforeEach,describe,expect,it,vi} from "vitest";
const state=vi.hoisted(()=>({pendingApproval:null as unknown,isPinned:true,view:"approval",defaultView:()=>"overview"}));
vi.mock("../core/state",()=>({State:state}));
import {approvalCleared,approvalPending,setVoiceApprovalProbe,voiceMayTakeFocus} from "./approval-focus";
import type {IslandViewName} from "../core/layout";
const island={alert:vi.fn(),setView:vi.fn(),dropPin:vi.fn()};
let voice:IslandViewName|null=null;
beforeEach(()=>{vi.clearAllMocks();voice=null;setVoiceApprovalProbe(()=>voice);Object.assign(state,{pendingApproval:null,isPinned:true,view:"approval"});});
describe("two approval cards never hide each other",()=>{
 it("brings back a waiting voice approval when the coding-agent card clears",()=>{
  voice="voiceApproval";approvalCleared(island,"approval");
  expect(island.alert).toHaveBeenCalledWith("voiceApproval");expect(state.isPinned).toBe(true);expect(island.dropPin).not.toHaveBeenCalled();
 });
 it("brings back a waiting coding-agent card when the voice approval clears",()=>{
  state.pendingApproval={requestId:"r"};state.view="voiceApproval";approvalCleared(island,"voiceApproval");
  expect(island.alert).toHaveBeenCalledWith("approval");expect(state.isPinned).toBe(true);
 });
 it("unpins and goes home only when nothing waits",()=>{
  approvalCleared(island,"approval");
  expect(state.isPinned).toBe(false);expect(island.dropPin).toHaveBeenCalledOnce();expect(island.setView).toHaveBeenCalledWith("overview");expect(island.alert).not.toHaveBeenCalled();
  state.view="tasks";approvalCleared(island,"voiceApproval");expect(island.setView).toHaveBeenCalledOnce();
 });
 it("opens a waiting planner draft on its own page and leaves a planner page in place when it clears",()=>{
  voice="tasks";approvalCleared(island,"approval");expect(island.alert).toHaveBeenCalledWith("tasks");
  voice=null;state.view="tasks";approvalCleared(island,"tasks");expect(state.isPinned).toBe(false);expect(island.dropPin).toHaveBeenCalledOnce();expect(island.setView).not.toHaveBeenCalled();
 });
 it("gives an on-screen coding-agent card priority over a new voice approval",()=>{
  expect(voiceMayTakeFocus()).toBe(true);state.pendingApproval={};state.view="approval";expect(voiceMayTakeFocus()).toBe(false);state.view="overview";expect(voiceMayTakeFocus()).toBe(true);
 });
 it("reports either pending card",()=>{
  expect(approvalPending()).toBe(false);voice="tasks";expect(approvalPending()).toBe(true);voice=null;state.pendingApproval={};expect(approvalPending()).toBe(true);
 });
});
