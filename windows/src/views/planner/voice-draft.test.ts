import {afterEach,beforeEach,describe,expect,it,vi} from "vitest";
const planner=vi.hoisted(()=>({revision:1}));
vi.mock("./store",()=>({Planner:planner}));
vi.mock("../../core/bridge",()=>({IS_TAURI:false,Bridge:{log:async()=>{}}}));
import {withVoiceDraft,typedFields,draftEnd,TYPE_MS} from "./voice-draft";
import {h} from "../dom";
import {setLanguage} from "../../core/i18n";
import type {LiveSnapshot} from "../../voice/live";
const idle:LiveSnapshot={phase:"listening",muted:false,transcripts:[],inputLevel:0,outputLevel:0};
let clock=0;let snapshot:LiveSnapshot=idle;
const decide=vi.fn();
function page(rows:string[]=[]){
 const body=h("div",{class:"pl-body"},h("ul",{},...rows.map(title=>h("li",{class:"pl-row"},h("span",{class:"pl-row-title",text:title})))));
 const host={el:h("div",{class:"view"},body),sync:vi.fn()};
 const view=withVoiceDraft("tasks",host,{snapshot:()=>snapshot,decide,now:()=>clock});
 document.body.append(view.el);return{view,host};
}
const approval=(tool:string,fields:{label:string;value:string}[],view="tasks",targetId?:string)=>({id:`ap-${tool}`,tool,summary:"تکمیل «خرید نان»",expiresAt:60_000,preview:{view,kind:"task",action:tool.startsWith("add")?"create":tool.startsWith("delete")?"delete":"update",targetId,fields}}) as unknown as LiveSnapshot["approval"];
const draft=(el:HTMLElement)=>el.querySelector<HTMLElement>(".pl-voice-draft")!;
beforeEach(()=>{setLanguage("fa");clock=0;snapshot=idle;decide.mockClear();planner.revision=1;vi.useFakeTimers();});
afterEach(()=>{vi.useRealTimers();document.body.replaceChildren();});
describe("live voice drafts on planner pages",()=>{
 it("types a new task in at the top with badge, countdown, hint and buttons",()=>{
  const {view,host}=page(["کار قبلی"]);
  snapshot={...idle,transcripts:[{id:"u0",speaker:"user",text:"یه کار اضافه کن"}],approval:approval("add_task",[{label:"عنوان",value:"خرید نان"}])};
  view.sync();expect(host.sync).toHaveBeenCalled();
  const el=draft(view.el);expect(el.hidden).toBe(false);expect(el.parentElement?.firstElementChild).toBe(el);
  expect(el.textContent).toContain("در انتظار تأیید شما");expect(el.textContent).toContain("کار جدید");expect(el.textContent).toContain("بگویید «تأیید» یا «رد»");
  expect(el.querySelector(".pl-vd-countdown")?.textContent).toBe("۶۰ ثانیه");
  expect(el.querySelector("dd")?.textContent).toBe("");
  clock=4*TYPE_MS;vi.advanceTimersByTime(4*TYPE_MS);expect(el.querySelector("dd")?.textContent).toBe("خرید");
  clock=100*TYPE_MS;vi.advanceTimersByTime(TYPE_MS);expect(el.querySelector("dd")?.textContent).toBe("خرید نان");
  expect(el.querySelector<HTMLElement>(".pl-vd-heard")?.hidden).toBe(true);
  [...el.querySelectorAll<HTMLButtonElement>("button")].find(b=>b.textContent==="تأیید")!.click();expect(decide).toHaveBeenCalledWith(true,"ap-add_task");
 });
 it("shows success when approved, then leaves once the real item arrives",()=>{
  const {view}=page();snapshot={...idle,approval:approval("add_task",[{label:"عنوان",value:"خرید"}])};view.sync();
  [...draft(view.el).querySelectorAll<HTMLButtonElement>("button")].find(b=>b.textContent==="تأیید")!.click();
  expect(decide).toHaveBeenCalledWith(true,"ap-add_task");
  snapshot={...idle,lastDecision:{id:"ap-add_task",outcome:"approved"}};view.sync();
  expect(draft(view.el).classList.contains("is-working")).toBe(true);expect(draft(view.el).textContent).toContain("در حال انجام");
  snapshot={...idle,lastDecision:{id:"ap-add_task",outcome:"approved",ok:true}};view.sync();
  expect(draft(view.el).classList.contains("is-approved")).toBe(true);expect(draft(view.el).textContent).toContain("انجام شد");
  planner.revision=2;view.sync();vi.advanceTimersByTime(1500);expect(draft(view.el).hidden).toBe(true);
 });
 it("shows a short error when the approved change failed, never «انجام شد»",()=>{
  const {view}=page();snapshot={...idle,approval:approval("add_task",[{label:"عنوان",value:"خرید"}])};view.sync();
  snapshot={...idle,lastDecision:{id:"ap-add_task",outcome:"approved",ok:false}};view.sync();
  expect(draft(view.el).classList.contains("is-failed")).toBe(true);expect(draft(view.el).querySelector(".pl-vd-done")?.textContent).toBe("انجام نشد.");
  vi.advanceTimersByTime(3000);expect(draft(view.el).hidden).toBe(true);
 });
 it("fades out when the user says no or the time runs out",()=>{
  const {view}=page();snapshot={...idle,approval:approval("add_task",[{label:"عنوان",value:"خرید"}])};view.sync();
  snapshot={...snapshot,transcripts:[{id:"u1",speaker:"user",text:"نه"}]};view.sync();expect(draft(view.el).querySelector(".pl-vd-heard")?.textContent).toBe("شنیدم: نه");
  // Hearing «نه» decides nothing here: the controller's decision does.
  expect(draft(view.el).hidden).toBe(false);
  snapshot={...idle,lastDecision:{id:"ap-add_task",outcome:"rejected"}};view.sync();expect(draft(view.el).classList.contains("is-leaving")).toBe(true);expect(draft(view.el).textContent).not.toContain("انجام شد");vi.advanceTimersByTime(400);expect(draft(view.el).hidden).toBe(true);
 });
 it("highlights the existing row for complete/delete instead of drafting",()=>{
  const {view}=page(["نوشتن گزارش","خرید نان"]);
  snapshot={...idle,approval:approval("complete_task",[{label:"کار",value:"خرید نان"}])};view.sync();
  expect(view.el.querySelectorAll(".pl-voice-target")).toHaveLength(1);expect(view.el.querySelector(".pl-voice-target")?.textContent).toBe("خرید نان");
  expect(draft(view.el).textContent).toContain("تکمیل «خرید نان»");
  snapshot=idle;view.sync();expect(view.el.querySelector(".pl-voice-target")).toBeNull();
 });
 it("finds the row by its item id first",()=>{
  const {view}=page();const list=view.el.querySelector("ul")!;
  for(const [id,title] of [["t1","خرید نان"],["t2","خرید نان"]]){const check=h("button");check.dataset.fk=`check:${id}`;list.append(h("li",{class:"pl-row"},check,h("span",{class:"pl-row-title",text:title})));}
  snapshot={...idle,approval:approval("delete_task",[{label:"کار",value:"خرید نان"}],"tasks","t2")};view.sync();
  expect(view.el.querySelector<HTMLElement>(".pl-voice-target button")?.dataset.fk).toBe("check:t2");
 });
 it("on the notes page: highlights the note card and types its new text for an update; delete is instant",()=>{
  const body=h("div",{class:"pl-body"},h("ul",{},...[["n1","خرید شیر"],["n2","جلسهٔ شنبه"]].map(([id,text])=>{const pin=h("button");pin.dataset.fk=`pin:${id}`;return h("li",{class:"pl-note-card"},h("p",{class:"pl-note-text",text}),pin);})));
  const view=withVoiceDraft("notes",{el:h("div",{class:"view"},body),sync:vi.fn()},{snapshot:()=>snapshot,decide,now:()=>clock});document.body.append(view.el);
  const note=(action:"update"|"delete",targetId:string,value:string)=>({id:`ap-${action}`,tool:`${action}_note`,summary:"ویرایش یادداشت",expiresAt:60_000,preview:{view:"notes",kind:"note",action,targetId,fields:[{label:"متن",value}]}}) as unknown as LiveSnapshot["approval"];
  snapshot={...idle,approval:{...note("update","n2","جلسهٔ یکشنبه")!,preview:{view:"notes",kind:"note",action:"update",targetId:"n2",fields:[{label:"متن",value:"جلسهٔ یکشنبه"},{label:"متن قبلی",value:"جلسهٔ شنبه"}]}}};view.sync();
  expect(view.el.querySelectorAll("dd")[1]?.textContent).toBe("جلسهٔ شنبه");
  expect(view.el.querySelector(".pl-voice-target .pl-note-text")?.textContent).toBe("جلسهٔ شنبه");
  expect(view.el.querySelector("dd")?.textContent).toBe("");clock=100*TYPE_MS;vi.advanceTimersByTime(TYPE_MS);expect(view.el.querySelector("dd")?.textContent).toBe("جلسهٔ یکشنبه");
  snapshot={...idle,approval:note("delete","n1","خرید شیر")};view.sync();
  expect(view.el.querySelector(".pl-voice-target .pl-note-text")?.textContent).toBe("خرید شیر");expect(view.el.querySelector("dd")?.textContent).toBe("خرید شیر");
 });
 it("ignores approvals for other pages and approvals without a preview",()=>{
  const {view}=page();
  snapshot={...idle,approval:approval("add_note",[{label:"متن",value:"x"}],"notes")};view.sync();expect(draft(view.el).hidden).toBe(true);
  snapshot={...idle,approval:{id:"plain",tool:"update_settings",summary:"x",expiresAt:1}};view.sync();expect(draft(view.el).hidden).toBe(true);
 });
 it("types field after field and is instant when asked; ends only on the controller decision",()=>{
  const fields=[{label:"a",value:"ab"},{label:"b",value:"cd"}];
  expect(typedFields(fields,3*TYPE_MS,false).map(f=>f.value)).toEqual(["ab","c"]);
  expect(typedFields(fields,0,true).map(f=>f.value)).toEqual(["ab","cd"]);expect(typedFields(fields,TYPE_MS,false,1).map(f=>f.value)).toEqual(["a","cd"]);
  expect(draftEnd(undefined,"a")).toBe("gone");expect(draftEnd({id:"b",outcome:"approved",ok:true},"a")).toBe("gone");
  for(const outcome of ["rejected","expired","cancelled"] as const)expect(draftEnd({id:"a",outcome},"a")).toBe("gone");
  expect(draftEnd({id:"a",outcome:"approved"},"a")).toBe("working");expect(draftEnd({id:"a",outcome:"approved",ok:true},"a")).toBe("done");expect(draftEnd({id:"a",outcome:"approved",ok:false},"a")).toBe("failed");
 });
});
