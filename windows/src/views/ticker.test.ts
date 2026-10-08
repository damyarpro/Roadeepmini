import { describe,expect,it } from "vitest";
import { ActivityStore } from "../activity/store";
import { codexTask } from "../activity/island-adapter";
import type { AgentTask } from "../core/state";
import { Ticker } from "./ticker";
import { makeDiffStep } from "../core/diff";
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
 it("keeps scrolling a hook-fed pill past its twentieth step, by the session's own count",()=>{
  const ticker=new Ticker();const steps:string[]=[];let seq=-1;
  const push=(label:string)=>{steps.push(label);if(steps.length>20)steps.shift();seq++;return {id:"integration_claude",name:"Claude",color:"#fff",state:"working",stepIndex:steps.length-1,steps:[...steps],source:"claudeCode",isIntegration:true} as AgentTask;};
  for(let index=1;index<=30;index++){ticker.sync(push(`step-${index}`),{seq});drain(ticker,index*5000);expect(current(ticker)).toBe(`step-${index}`);}
 });
 it("queues every step that arrived between two syncs, not only the newest",()=>{
  const ticker=new Ticker();const task:AgentTask={id:"integration_claude",name:"Claude",color:"#fff",state:"working",stepIndex:19,steps:Array.from({length:20},(_,i)=>`s${i}`),source:"claudeCode",isIntegration:true};
  ticker.sync(task,{seq:19});
  const later={...task,steps:[...task.steps.slice(3),"n1","n2","n3"]};ticker.sync(later,{seq:22});
  const seen:string[]=[];let at=0;while(ticker.animating&&at<100000){ticker.tick(at);at+=400;ticker.tick(at);at+=1;seen.push(current(ticker)!);}
  expect(seen).toEqual(["n1","n2","n3"]);
 });
 it("shows the first step of an empty pill at once",()=>{
  const ticker=new Ticker();const task:AgentTask={id:"integration_claude",name:"Claude",color:"#fff",state:"idle",stepIndex:0,steps:[],source:"claudeCode",isIntegration:true};
  ticker.sync(task);expect(current(ticker)).toBe("…");
  ticker.sync({...task,state:"thinking",steps:["Fix the build"]},{seq:0});expect(current(ticker)).toBe("Fix the build");expect(ticker.animating).toBe(false);
 });
 it("shows a diff step as its file name and counts, and opens it on a click",()=>{
  const opened:number[]=[];const ticker=new Ticker(id=>opened.push(id));
  const task:AgentTask={id:"integration_claude",name:"Claude",color:"#fff",state:"working",stepIndex:1,steps:["Edits · app.ts",makeDiffStep("app.ts",3,1,42)],source:"claudeCode",isIntegration:true};
  ticker.sync(task,{seq:1});
  const row=ticker.el.children[1] as HTMLElement;
  expect(row.classList.contains("diff")).toBe(true);expect(row.querySelector(".tick-text")?.textContent).toBe("app.ts");
  expect(row.querySelector(".tick-count")?.textContent).toBe(" +3 −1");expect(row.querySelector(".tick-count")?.getAttribute("dir")).toBe("ltr");
  row.click();expect(opened).toEqual([42]);
  (ticker.el.children[0] as HTMLElement).click();expect(opened).toEqual([42]);
  ticker.sync({...task,stepIndex:2,steps:[...task.steps,"Runs · npm test"]},{seq:2});drain(ticker);
  expect((ticker.el.children[1] as HTMLElement).classList.contains("diff")).toBe(false);expect(ticker.el.children[1].querySelector(".tick-count")).toBeNull();
 });
 it("holds still on the final message once the turn is over",()=>{
  const ticker=new Ticker();const task:AgentTask={id:"integration_claude",name:"Claude",color:"#fff",state:"finished",stepIndex:1,steps:["Runs · tests","Done."],source:"claudeCode",isIntegration:true};
  ticker.sync(task,{seq:1,still:true});
  const row=ticker.el.children[1] as HTMLElement;const shimmer=row.querySelector<HTMLElement>(".shimmer")!;
  expect(shimmer.style.opacity).toBe("0");expect(shimmer.style.animationPlayState).toBe("paused");
  expect(row.querySelector<SVGElement>(".tick-chevron")!.style.opacity).toBe("0");
  ticker.sync({...task,state:"thinking"},{seq:1,still:false});
  expect(shimmer.style.opacity).toBe("1");expect(shimmer.style.animationPlayState).toBe("");
 });
 it("switching providers at the same index clears queued labels from the previous provider",()=>{
  const ticker=new Ticker();const claude:AgentTask={id:"integration_claude",name:"Claude",color:"#fff",state:"working",stepIndex:0,steps:["Claude read","Claude queued"],source:"claudeCode",isIntegration:true};
  ticker.sync(claude);ticker.sync({...claude,stepIndex:1});ticker.tick(0);expect(ticker.animating).toBe(true);
  ticker.sync({...claude,id:"integration_codex",stepIndex:1,steps:["Codex prompt","Codex tool"],stepRevision:"new",stepSessionId:"codex:live"});
  expect(current(ticker)).toBe("Codex tool");expect(ticker.animating).toBe(false);expect(ticker.el.textContent).not.toContain("Claude");
  ticker.sync({...claude,stepIndex:1});expect(current(ticker)).toBe("Claude queued");expect(ticker.el.textContent).not.toContain("Codex");
 });
});
