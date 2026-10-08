import "../src/settings/settings.css";
import { h } from "../src/views/dom";
import { setLanguage, t } from "../src/core/i18n";
import { assistantSettingsSection } from "../src/settings/assistant-settings";
import { BridgeAssistant } from "../src/core/bridge-assistant";
const language=new URLSearchParams(location.search).get("lang")==="fa"?"fa":"en";setLanguage(language);document.documentElement.lang=language;document.documentElement.dir=language==="fa"?"rtl":"ltr";
document.head.append(h("style",{text:"html,body{overflow:auto;height:auto;min-height:100%;margin:0}body{background:var(--bg);padding:24px;font-family:var(--font)}#preview{width:100%;max-width:720px;margin:20px auto}.preview-label{font-size:13px;color:var(--text-3);line-height:1.7}@media(max-width:450px){body{padding:12px}#preview{margin:0 auto}}"}));
const now=Date.now();
let memoryView={enabled:true,facts:[
 {id:"f1",text:"جواب‌های کوتاه و بدون مقدمه را ترجیح می‌دهد",category:"preference" as const,createdAt:now-86_400_000*3,updatedAt:now-86_400_000*3},
 {id:"f2",text:"صبح‌ها اول کارهای امروز را مرور می‌کند",category:"habit" as const,createdAt:now-86_400_000,updatedAt:now-86_400_000},
 {id:"f3",text:"رسمی و مؤدبانه صحبت شود",category:"style" as const,createdAt:now-3_600_000,updatedAt:now-3_600_000},
 {id:"f4",text:"تیم پروژه‌اش «آسمان» نام دارد",category:"fact" as const,createdAt:now-600_000,updatedAt:now-600_000},
],stats:{toolCounts:{add_task:14,list_notes:6,control_focus:5,app__github__list_issues:3},hourHistogram:Array.from({length:24},(_,h)=>h===9?8:h===10?5:h===21?3:0),weekdayHistogram:Array(7).fill(1),recent:[]}};
const bridge={...BridgeAssistant,voiceStatus:async()=>({configured:new URLSearchParams(location.search).get("scene")==="ready",model:"gpt-6-luna",voice:"marin",voices:[["marin","رها"],["quartz","آوا"],["ripple","آراد"],["vesper","آرش"],["willow","نیکا"],["stone","کاوه"],["gleam","نگار"],["meridian","سام"],["bossa","یلدا"],["tempo","نیما"],["beacon","سینا"],["delta","پریسا"],["cinder","مهراد"]].map(([id,label])=>({id,label})),models:["gpt-6-luna","gpt-6-sol"]}),voiceConfigure:async()=>{throw new Error("Preview only; no credential was saved.");},voiceClearKey:async()=>{},memoryGet:async()=>memoryView,memoryDelete:async(id:string)=>{memoryView={...memoryView,facts:memoryView.facts.filter(f=>f.id!==id)};},memoryClear:async()=>{memoryView={...memoryView,facts:[]};},memorySetEnabled:async(enabled:boolean)=>{memoryView={...memoryView,enabled};},computerSetup:async()=>({runtimePath:"C:/Example/Roadeep/computer-runtime",buildCommand:"docker build -t roadeep-computer:1 'C:/Example/Roadeep/computer-runtime'"})};
document.querySelector("#preview")!.append(h("p",{class:"preview-label",text:t("assistant.preview")}),assistantSettingsSection(bridge,{native:true}));
