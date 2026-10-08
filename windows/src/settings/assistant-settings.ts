import { BridgeAssistant, type VoiceStatus } from "../core/bridge-assistant";
import { Bridge, IS_TAURI, onRoadeepSession } from "../core/bridge";
import { t, isRtl, getLanguage } from "../core/i18n";
import { h } from "../views/dom";
import { sectionHead, helpDisclosure } from "./ui";
import { assistantError } from "../views/assistant-controls/messages";
import "./assistant-settings.css";
import {localSettings} from "../local/settings";
import "../local/messages";
import type {Settings} from "../core/state";
import {createAssistantControls} from "../views/assistant-controls";
import {memorySettings} from "../local/memory-settings";

/** Voices are descriptions («صدای رها»), never the assistant's name (always «رودیپ»). English adds the provider id. */
export function voiceLabel(option:{id:string;label:string}){
 const name=option.label.replace(/^صدای\s*/,"").trim()||option.id;
 return getLanguage()==="fa"?`صدای ${name}`:`Voice ${name} · ${option.id}`;
}
function fill(select:HTMLSelectElement,options:{id:string;label:string}[],value:string){
 const list=options.some(option=>option.id===value)||!value?options:[...options,{id:value,label:value}];
 select.replaceChildren(...list.map(option=>h("option",{value:option.id,text:option.label})));select.value=value;
}
function settingsError(error:unknown){return /End voice before/i.test(String(error))?t("assistant.endFirst"):assistantError(error);}

export function assistantSettingsSection(bridge=BridgeAssistant, options:{native?:boolean;settings?():Settings}={}): HTMLElement {
 const native=options.native??IS_TAURI;
 let signedIn=false;let identity="";
 const computerControls=createAssistantControls({preview:!native,
  snapshot:()=>({visible:section.isConnected&&!section.closest("[hidden]")&&!document.hidden,signedIn,busy:false,agentId:options.settings?.().chatAgent||"default",identity}),
  setup:()=>runtime.scrollIntoView({block:"nearest"}),changed:()=>{},bridge,
 });
 const syncControls=()=>computerControls.sync();
 let stopSession:(()=>void)|undefined;
 if(native){void Bridge.roadeepSession().then(session=>{if(disposed)return;signedIn=session.signedIn;identity=session.user?.id??"";syncControls();}).catch(()=>{if(!disposed)show(t("assistant.native"),true);});void onRoadeepSession(session=>{signedIn=session.signedIn;identity=session.user?.id??"";syncControls();}).then(stop=>{if(disposed)stop();else stopSession=stop;});}
 const card=h("div",{class:"card assistant-settings"});
 const section=h("section",{class:"sec","aria-labelledby":"sec-assistant-title"},sectionHead({id:"sec-assistant-title",icon:"agents",title:t("assistant.settingsTitle"),desc:t("assistant.settingsDesc")}),card);
 section.dir=isRtl()?"rtl":"ltr";
 const local=localSettings({native});
 const memory=memorySettings({native,bridge});
 const status=h("p",{class:"live-voice-status",role:"status"});
 const notice=h("p",{class:"notice",role:"status",hidden:true});
 const voice=h("select",{"aria-label":t("assistant.voiceName")});
 const model=h("select",{dir:"ltr","aria-label":t("assistant.model")});
 // Write-only: the field never receives the stored key and is emptied before every request leaves.
 const secret=h("input",{type:"password",dir:"ltr",autocomplete:"new-password","data-lpignore":"true","data-1p-ignore":"true",spellcheck:"false",maxlength:512,placeholder:t("assistant.apiKeyPlaceholder"),"aria-label":t("assistant.apiKey")});
 const label=(key:string,input:HTMLElement,help?:HTMLElement,note?:string)=>h("label",{class:"assistant-setting-field"},h("span",{class:"set-label-line"},h("span",{text:t(key)}),help??null),input,note?h("span",{class:"assistant-field-note",text:note}):null);
 const save=h("button",{type:"submit",class:"primary",text:t("assistant.save")});
 const remove=h("button",{type:"button",text:t("assistant.clearKey")});
 const form=h("form",{autocomplete:"off"},
  label("assistant.apiKey",secret,helpDisclosure(t("assistant.keyHint"),`${t("settings.help")}: ${t("assistant.apiKey")}`)),
  h("div",{class:"assistant-settings-grid"},label("assistant.voiceName",voice,undefined,t("assistant.nameFixed")),label("assistant.model",model)),
  h("p",{class:"hint live-voice-cost",text:t("assistant.costHint")}),
  h("div",{class:"actions"},save,remove));
 const live=h("div",{class:"live-voice-settings"},h("h3",{text:t("assistant.liveTitle")}),h("p",{class:"hint",text:t("assistant.liveDesc")}),status,form,notice);
 let busy=false;
 let disposed=false;
 let configured=false;
 let operation:"save"|"remove"|null=null;
 const show=(text:string,failed=false)=>{notice.textContent=text;notice.hidden=false;notice.classList.toggle("err",failed);};
 const controls=()=>{save.disabled=remove.disabled=busy||!native;remove.disabled=remove.disabled||!configured;model.disabled=voice.disabled=secret.disabled=busy||!native;save.textContent=t(operation==="save"?"assistant.saving":"assistant.save");remove.textContent=t(operation==="remove"?"assistant.removing":"assistant.clearKey");status.classList.toggle("is-ready",configured);};
 function apply(next:VoiceStatus){
  fill(voice,(next.voices??[]).map(option=>({id:option.id,label:voiceLabel(option)})),next.voice);
  fill(model,(next.models??[]).map(id=>({id,label:id})),next.model);
  configured=next.configured;status.textContent=t(configured?"assistant.keyPresent":"assistant.keyAbsent");
 }
 async function refresh(){
  if(!native){status.textContent=t("assistant.native");controls();return;}
  try{const next=await bridge.voiceStatus();if(disposed)return;apply(next);controls();}
  catch(error){if(!disposed)show(settingsError(error),true);}
 }
 form.addEventListener("submit",event=>{
  event.preventDefault();if(busy||!native)return;
  const key=secret.value.trim();secret.value="";busy=true;operation="save";controls();
  void bridge.voiceConfigure(model.value,voice.value,key||undefined).then(()=>{if(!disposed)show(t("assistant.saved"));return refresh();}).catch(error=>{if(!disposed)show(`${settingsError(error)}${key?` ${t("assistant.reenter")}`:""}`,true);}).finally(()=>{busy=false;operation=null;if(!disposed)controls();});
 });
 remove.addEventListener("click",()=>{
  if(busy||!native)return;secret.value="";busy=true;operation="remove";controls();
  void bridge.voiceClearKey().then(()=>{if(!disposed)show(t("assistant.keyRemoved"));return refresh();}).catch(error=>{if(!disposed)show(settingsError(error),true);}).finally(()=>{busy=false;operation=null;if(!disposed)controls();});
 });
 const command=h("pre",{class:"code",dir:"ltr",text:native?"…":"docker build -t roadeep-computer:1 computer-runtime"});
 const runtime=h("div",{class:"assistant-runtime"},h("h3",{text:t("assistant.computerSetup")}),h("p",{class:"hint",text:t("assistant.buildHint")}),command,helpDisclosure(t("assistant.isolationHint"), `${t("settings.help")}: ${t("assistant.computerSetup")}`));
 if(native)void bridge.computerSetup().then(setup=>{if(!disposed)command.textContent=setup.buildCommand;}).catch(error=>{if(!disposed)command.textContent=assistantError(error);});
 card.append(live,memory.el,local.el,runtime,computerControls.el);busy=true;status.textContent=t("assistant.loading");controls();void refresh().finally(()=>{busy=false;if(!disposed)controls();});
 let lastVisible:boolean|undefined;
 const visibilityObserver=new MutationObserver(()=>{const visible=!section.closest("[hidden]");if(visible!==lastVisible){lastVisible=visible;syncControls();}});queueMicrotask(()=>{if(section.isConnected){visibilityObserver.observe(document.body,{subtree:true,attributes:true,attributeFilter:["hidden"]});syncControls();}});
 // Redraw replaces this section; credentials and late responses never survive it.
 const dispose=()=>{disposed=true;local.dispose();memory.dispose();computerControls.dispose();stopSession?.();visibilityObserver.disconnect();secret.value="";observer.disconnect();};
 const observer=new MutationObserver(()=>{if(!section.isConnected)dispose();});
 queueMicrotask(()=>{if(section.isConnected)observer.observe(document.body,{childList:true,subtree:true});});
 window.addEventListener("pagehide",dispose,{once:true});
 return section;
}
