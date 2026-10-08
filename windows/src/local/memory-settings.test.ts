import {beforeEach,describe,expect,it,vi} from "vitest";
vi.mock("../core/bridge",()=>({IS_TAURI:true,onEvent:async()=>()=>{},Bridge:{log:async()=>{}}}));
import {memorySettings,topTools,usualHours} from "./memory-settings";
import {setLanguage} from "../core/i18n";
const flush=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
const hours=Array.from({length:24},(_,h)=>h===9?5:h===21?3:h===14?1:0);
function fixture(){
 let view:{enabled:boolean;facts:{id:string;text:string;category:string;createdAt:number;updatedAt:number}[];stats:{toolCounts:Record<string,number>;hourHistogram:number[];weekdayHistogram:number[];recent:{at:number;tool:string;summary:string}[]}}={enabled:true,facts:[
  {id:"f1",text:"جواب‌های کوتاه را ترجیح می‌دهد",category:"preference",createdAt:1_700_000_000_000,updatedAt:1_700_000_000_000},
  {id:"f2",text:"صبح‌ها ورزش می‌کند",category:"habit",createdAt:1_700_100_000_000,updatedAt:1_700_100_000_000},
 ],stats:{toolCounts:{add_task:7,list_notes:2,app__github__list_issues:4},hourHistogram:hours,weekdayHistogram:Array(7).fill(0),recent:[{at:1,tool:"add_task",summary:"x"}]}};
 const bridge={
  memoryGet:vi.fn(async()=>structuredClone(view)),
  memoryDelete:vi.fn(async(id:string)=>{view={...view,facts:view.facts.filter(f=>f.id!==id)};}),
  memoryClear:vi.fn(async()=>{view={...view,facts:[],stats:{...view.stats,toolCounts:{},hourHistogram:Array(24).fill(0),recent:[]}};}),
  memorySetEnabled:vi.fn(async(enabled:boolean)=>{view={...view,enabled};}),
 };
 return bridge;
}
beforeEach(()=>{setLanguage("fa");document.body.replaceChildren();});
describe("Roadeep memory settings",()=>{
 it("lists learned facts newest first with category, date and a delete button, plus a usage summary and privacy text",async()=>{
  const bridge=fixture();const card=memorySettings({native:true,bridge:bridge as never});document.body.append(card.el);await flush();
  const facts=[...card.el.querySelectorAll(".memory-fact")];
  expect(facts.map(f=>f.querySelector(".memory-text")?.textContent)).toEqual(["صبح‌ها ورزش می‌کند","جواب‌های کوتاه را ترجیح می‌دهد"]);
  expect(facts[0].querySelector(".memory-chip")?.textContent).toBe("عادت");expect(facts[1].querySelector(".memory-chip")?.textContent).toBe("ترجیح");
  expect(facts[0].querySelector(".memory-date")?.textContent).not.toBe("");
  expect(card.el.textContent).toContain("حافظهٔ رودیپ");expect(card.el.textContent).toContain("افزودن کار (۷)");expect(card.el.textContent).toContain("list issues (۴)");
  expect(card.el.textContent).toContain("۹ تا ۱۰");const privacy=card.el.querySelector(".memory-privacy")?.textContent??"";for(const part of ["خلاصهٔ کوتاه درخواست‌های اخیر","متن کارها و یادداشت‌ها","فایلی روی همین سیستم","در هر گفتگوی زنده","OpenAI","یادگیری تازه را متوقف","«پاک کردن همه» همه را حذف"])expect(privacy).toContain(part);
  setLanguage("en");const en=memorySettings({native:false,bridge:bridge as never}).el.querySelector(".memory-privacy")?.textContent??"";for(const part of ["short summaries of your recent requests","tasks and notes","file on this PC","every live conversation","OpenAI","stops new learning","erases"])expect(en).toContain(part);setLanguage("fa");
  expect(card.el.querySelector<HTMLInputElement>('input[role="switch"]')?.checked).toBe(true);
 });
 it("forgets one fact, toggles learning, and clears everything only after confirmation",async()=>{
  const bridge=fixture();const confirm=vi.fn(()=>false);const card=memorySettings({native:true,bridge:bridge as never,confirm});document.body.append(card.el);await flush();
  card.el.querySelector<HTMLButtonElement>(".memory-delete")!.click();await flush();
  expect(bridge.memoryDelete).toHaveBeenCalledWith("f2");expect(card.el.querySelectorAll(".memory-fact")).toHaveLength(1);
  const toggle=card.el.querySelector<HTMLInputElement>('input[role="switch"]')!;toggle.checked=false;toggle.dispatchEvent(new Event("change"));await flush();
  expect(bridge.memorySetEnabled).toHaveBeenCalledWith(false);expect(card.el.textContent).toContain("خاموش");
  const clear=[...card.el.querySelectorAll<HTMLButtonElement>("button")].find(b=>b.textContent==="پاک کردن همه")!;
  clear.click();await flush();expect(confirm).toHaveBeenCalledOnce();expect(bridge.memoryClear).not.toHaveBeenCalled();
  confirm.mockReturnValue(true);clear.click();await flush();expect(bridge.memoryClear).toHaveBeenCalledOnce();expect(card.el.textContent).toContain("هنوز چیزی نیست");
 });
 it("shows failures without breaking and stays read-only outside the app",async()=>{
  const bridge=fixture();bridge.memoryDelete.mockRejectedValueOnce(new Error("boom"));
  const card=memorySettings({native:true,bridge:bridge as never});document.body.append(card.el);await flush();
  card.el.querySelector<HTMLButtonElement>(".memory-delete")!.click();await flush();
  expect(card.el.querySelector<HTMLElement>(".notice")?.hidden).toBe(false);expect(card.el.querySelectorAll(".memory-fact")).toHaveLength(2);
  const preview=memorySettings({native:false,bridge:bridge as never});
  expect(preview.el.querySelector<HTMLInputElement>('input[role="switch"]')?.disabled).toBe(true);
 });
 it("summarises usage without noise",()=>{
  expect(usualHours(hours)).toEqual([9,21,14]);expect(usualHours(Array(24).fill(0))).toEqual([]);
  expect(topTools({a:1,b:5,c:0,d:3},2)).toEqual([["b",5],["d",3]]);
 });
});
