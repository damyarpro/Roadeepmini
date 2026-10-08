// Live conversation preview: the header microphone in every phase, the compact chip and the
// island approval card. No microphone, provider or native call is used.
// ?lang=en|fa  &scene=idle|connecting|listening|thinking|speaking|muted|error|approval
import "../src/style.css";
import {setLanguage,t} from "../src/core/i18n";
import {h} from "../src/views/dom";
import {washRGBA} from "../src/core/layout";
import {globalVoiceIndicator} from "../src/local/global-voice";
import {voiceApprovalCard,IDLE_SNAPSHOT} from "../src/local/live-ui";
import type {LivePhase,LiveSnapshot} from "../src/voice/live";
const params=new URLSearchParams(location.search);const language=params.get("lang")==="en"?"en":"fa";setLanguage(language);document.documentElement.lang=language;document.documentElement.dir=language==="fa"?"rtl":"ltr";
const fa=language==="fa";const scene=params.get("scene")??"approval";
const transcripts=[{id:"u1",speaker:"user" as const,text:fa?"یه کار اضافه کن: خرید نان برای فردا":"Add a task: buy bread tomorrow"},{id:"a1",speaker:"assistant" as const,text:fa?"باشه، کار «خرید نان» برای فردا اضافه شود؟":"Sure — add “buy bread” for tomorrow?"},{id:"u2",speaker:"user" as const,text:fa?"آره":"Yes"}];
const state=(phase:LivePhase,patch:Partial<LiveSnapshot>={}):LiveSnapshot=>({...IDLE_SNAPSHOT,phase,transcripts,inputLevel:.45,outputLevel:.6,...patch});
const approval={id:"preview",tool:"add_task",summary:"افزودن کار «خرید نان» با موعد ۱۴۰۵/۰۷/۱۶",expiresAt:Date.now()+42_000};
document.head.append(h("style",{text:"html,body{height:auto;min-height:100%;overflow:auto}body{margin:0;padding:24px;background:#202126;color:var(--ink);font-family:var(--font)}#preview{max-width:640px;margin:0 auto;display:grid;gap:16px}.pv-row{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:8px 16px;background:#000;border-radius:16px;min-height:48px}.pv-row span.pv-name{font-size:12px;color:var(--dim)}.pv-island{background:#000;border-radius:24px;padding:8px;height:236px}.pv-island .card{height:100%}.pv-note{font-size:12px;color:var(--dim);line-height:1.7}"}));
const root=document.querySelector<HTMLElement>("#preview")!;
root.append(h("p",{class:"pv-note",text:t("assistant.preview")}));
const phases:[string,LiveSnapshot][]=[["idle",state("idle",{transcripts:[]})],["connecting",state("connecting",{transcripts:[]})],["listening",state("listening")],["thinking",state("thinking")],["speaking",state("speaking",{transcripts:transcripts.slice(0,2)})],["muted",state("listening",{muted:true})],["error",state("error",{error:"دسترسی به میکروفون داده نشد."})]];
for(const [name,snapshot] of phases)if(scene==="approval"||scene===name)root.append(h("div",{class:"pv-row"},h("span",{class:"pv-name",text:name}),globalVoiceIndicator(undefined,snapshot)));
if(scene==="approval"){
 const card=voiceApprovalCard(approve=>{root.querySelector(".pv-note")!.textContent=approve?(fa?"پیش‌نمایش: تأیید شد":"Preview: approved"):(fa?"پیش‌نمایش: رد شد":"Preview: declined");});
 card.el.style.padding="8px 16px 8px 116px";
 const shell=h("div",{class:"card wash"},card.el);shell.style.setProperty("--wash",washRGBA("amber"));
 root.append(h("div",{class:"pv-island"},shell));
 const tick=()=>card.update(state("listening",{approval}));tick();setInterval(tick,1000);
}

// ── In-place planner drafts: ?scene=draft-task | draft-note | draft-complete (&outcome=yes|no plays the result) ──
if(scene.startsWith("draft-")){
 void (async()=>{
  const [{buildTasks},{buildNotes},{withVoiceDraft},{Planner},{EXPANDED_W,HEADER_EXTRA_H,VIEW_LAYOUTS}]=await Promise.all([import("../src/views/planner/tasks"),import("../src/views/planner/notes"),import("./../src/views/planner/voice-draft"),import("../src/views/planner/store"),import("../src/core/layout")]);
  const actions={setView(){},collapse(){},setFocus(){},openTerminal(){},openTarget(){},openUrl(){},decide(){},toggleSound(){},setVolume(){},setAutoClose(){},openSettingsWindow(){},blip(){},toggleMaximize(){}};
  const kind=scene.startsWith("draft-note")?"notes":"tasks";
  Planner.ensure();await new Promise(resolve=>setTimeout(resolve,300));
  const target=kind==="tasks"?Planner.data?.tasks.find(task=>!task.done&&/نان|bread/i.test(task.title)):undefined;
  const note=Planner.data?.notes.find(n=>!n.pinned);
  const preview=scene==="draft-note-update"?{view:"notes" as const,kind:"note" as const,action:"update" as const,targetId:note?.id,fields:[{label:fa?"متن":"Text",value:fa?"ایده: خلاصهٔ یک‌صفحه‌ای را تا جمعه بفرستم.":"Idea: send the one-page summary by Friday."},{label:fa?"متن قبلی":"Previous text",value:note?.text.split(String.fromCharCode(10))[0]??""}]}
   :scene==="draft-note"?{view:"notes" as const,kind:"note" as const,action:"create" as const,fields:[{label:fa?"متن":"Text",value:fa?"قرار دندان‌پزشکی پنجشنبه ساعت ۵":"Dentist on Thursday at 5"}]}
   :scene==="draft-complete"?{view:"tasks" as const,kind:"task" as const,action:"update" as const,targetId:target?.id,fields:[{label:fa?"کار":"Task",value:target?.title??""}]}
   :{view:"tasks" as const,kind:"task" as const,action:"create" as const,fields:[{label:fa?"عنوان":"Title",value:fa?"خرید هدیهٔ تولد سارا":"Buy Sara's birthday present"},{label:fa?"موعد":"Due",value:fa?"فردا":"Tomorrow"}]};
  const card={id:"draft-preview",tool:preview.action==="create"?(kind==="notes"?"add_note":"add_task"):kind==="notes"?"update_note":"complete_task",summary:kind==="notes"?(fa?"ویرایش یادداشت":"Edit note"):fa?`تکمیل کار «${target?.title??""}»`:`Complete “${target?.title??""}”`,expiresAt:Date.now()+60_000,preview};
  let snapshot=state("listening",{transcripts:transcripts.slice(0,1),approval:card});
  const host=(kind==="notes"?buildNotes:buildTasks)(actions as never);
  const view=withVoiceDraft(kind,host,{snapshot:()=>snapshot,decide:(approve)=>{snapshot=state("listening",{transcripts:transcripts.slice(0,1)});if(approve&&preview.action==="create"&&kind==="tasks")void import("../src/core/bridge-planner").then(({PlannerBridge})=>PlannerBridge.taskAdd(preview.fields[0].value,null)).then(()=>Planner.refresh());view.sync();}});
  view.el.classList.add("on");
  const frame=h("div",{id:"island",style:`position:relative;width:${EXPANDED_W}px;max-width:100%;height:${VIEW_LAYOUTS[kind].height+HEADER_EXTRA_H}px;background:#000;border-radius:24px;padding:8px;box-sizing:border-box`},h("div",{id:"content",style:"position:relative;width:100%;height:100%"},view.el));
  root.replaceChildren(h("p",{class:"pv-note",text:t("assistant.preview")}),frame);
  const tick=()=>view.sync();tick();setInterval(tick,250);
  const outcome=params.get("outcome");
  if(outcome)setTimeout(()=>{if(outcome==="no")snapshot=state("listening",{transcripts:[...transcripts.slice(0,1),{id:"u9",speaker:"user",text:fa?"نه":"no"}],approval:card});setTimeout(()=>view.el.querySelectorAll<HTMLButtonElement>(".pl-vd-actions button")[outcome==="yes"?1:0]?.click(),400);},1800);
 })();
}
