import { describe,expect,it } from "vitest";
import { ActivityStore } from "../activity/store";
import { codexTask } from "../activity/island-adapter";
import type { AgentTask } from "../core/state";
import { Ticker } from "./ticker";
const current=(ticker:Ticker)=>ticker.el.children[1]?.querySelector(".tick-text")?.textContent;
function drain(ticker:Ticker,start=0){let frames=0;while(ticker.animating&&frames<10){ticker.tick(start);start+=400;ticker.tick(start);start+=1;frames++;}return frames;}
function step(store:ActivityStore,index:number,label=`tool-${index}`,sessionId="parent"){
 store.ingest({id:`event-${index}`,sessionId,harness:"codex",kind:"tool",phase:"started",tool:label,at:1000+index});return codexTask(store.sessions(),2000)!;
}
describe("bounded observed ticker revisions",()=>{
 it("continues updating beyond25 tools although the retained index stays19",()=>{
  const store=new ActivityStore(),ticker=new Ticker();for(let index=1;index<=25;index++){const task=step(store,index);ticker.sync(task);drain(ticker,index*5000);expect(current(ticker)).toBe(`tool-${index}`);}
  expect(codexTask(store.sessions(),2000)?.steps).toHaveLength(20);expect(codexTask(store.sessions(),2000)?.stepIndex).toBe(19);
 });
 it("repeated tool labels animate only for a new evidence ID, not usage or completion",()=>{
  const store=new ActivityStore(),ticker=new Ticker();for(let index=1;index<=20;index++){ticker.sync(step(store,index,"functions.exec"));drain(ticker,index*5000);}
  ticker.sync(step(store,21,"functions.exec"));expect(ticker.animating).toBe(true);drain(ticker,200000);expect(current(ticker)).toBe("functions.exec");
  store.ingest({id:"result",sessionId:"parent",harness:"codex",kind:"tool",phase:"completed",tool:"functions.exec",at:1100});
  store.ingest({id:"quota",sessionId:"parent",harness:"codex",kind:"usage",at:1101});const task=codexTask(store.sessions(),2000)!;
  expect(task.steps).toHaveLength(20);expect(task.stepRevision).toBe("event-21");ticker.sync(task);expect(ticker.animating).toBe(false);
 });
 it("bounds a long burst to four transitions and lands on the newest observed step",()=>{
  const store=new ActivityStore(),ticker=new Ticker();for(let index=1;index<=100;index++)ticker.sync(step(store,index));
  expect(drain(ticker)).toBe(4);expect(current(ticker)).toBe("tool-100");expect(ticker.animating).toBe(false);
 });
 it("switching observed sessions resets rather than showing the previous project's steps",()=>{
  const store=new ActivityStore(),ticker=new Ticker();ticker.sync(step(store,1,"parent-tool"));ticker.sync(step(store,2,"worker-tool","worker"));
  expect(current(ticker)).toBe("worker-tool");expect(ticker.animating).toBe(false);
 });
 it("preserves Claude's index-driven ticker without requiring revision metadata",()=>{
  const ticker=new Ticker();const task:AgentTask={id:"integration_claude",name:"Claude",color:"#fff",state:"working",stepIndex:0,steps:["Read","Write"],source:"claudeCode",isIntegration:true};
  ticker.sync(task);expect(current(ticker)).toBe("Read");ticker.sync({...task,stepIndex:1});expect(ticker.animating).toBe(true);drain(ticker);expect(current(ticker)).toBe("Write");ticker.sync({...task,stepIndex:1});expect(ticker.animating).toBe(false);
 });
 it("switching providers at the same index clears queued labels from the previous provider",()=>{
  const ticker=new Ticker();const claude:AgentTask={id:"integration_claude",name:"Claude",color:"#fff",state:"working",stepIndex:0,steps:["Claude read","Claude queued"],source:"claudeCode",isIntegration:true};
  ticker.sync(claude);ticker.sync({...claude,stepIndex:1});ticker.tick(0);expect(ticker.animating).toBe(true);
  ticker.sync({...claude,id:"integration_codex",stepIndex:1,steps:["Codex prompt","Codex tool"],stepRevision:"new",stepSessionId:"codex:live"});
  expect(current(ticker)).toBe("Codex tool");expect(ticker.animating).toBe(false);expect(ticker.el.textContent).not.toContain("Claude");
  ticker.sync({...claude,stepIndex:1});expect(current(ticker)).toBe("Claude queued");expect(ticker.el.textContent).not.toContain("Codex");
 });
});
