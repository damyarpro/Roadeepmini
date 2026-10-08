import {Bridge,IS_TAURI} from "../core/bridge";
import {State} from "../core/state";
import {Sound} from "../core/sound";
import {t,getLanguage} from "../core/i18n";
import {h} from "../views/dom";
import {liveVoice,setLiveVoiceHost,type LiveSnapshot} from "../voice/live";
import {voiceError} from "../views/assistant-controls/messages";
import {IDLE_SNAPSHOT,liveActive,liveLevel,lastTranscript,micGlyph,phaseLabel} from "./live-ui";
import {approvalCleared,setVoiceApprovalProbe,voiceMayTakeFocus} from "./approval-focus";
import {previewOf} from "../views/planner/voice-draft";
import type {IslandViewName} from "../core/layout";
import {bindVoiceAsk,bindVoiceIsland,voiceIsland,voiceToolHost,type VoiceIslandApi} from "./voice-host";
import "./messages";
import "./style.css";

/**
 * The header microphone owns the app's single live conversation (src/voice/live.ts).
 * Everything here renders its snapshot; the island is told to open (pinned) while an
 * approval waits and to let go when it clears.
 */
let snapshot:LiveSnapshot=IDLE_SNAPSHOT;
setVoiceApprovalProbe(()=>voiceView(snapshot));
/** The page a pending approval is shown on: its planner page when it has a preview, else the approval card. */
function voiceView(s:LiveSnapshot):IslandViewName|null{return s.approval?previewOf(s.approval)?.view??"voiceApproval":null;}
let initialized=false;let renderKey="";let approvalTimer:ReturnType<typeof setInterval>|undefined;
const subscribers=new Set<()=>void>();
const levelSubscribers=new Set<()=>void>();

function receive(next:LiveSnapshot){
 const previous=snapshot;snapshot=next;
 syncApproval(previous,next);
 syncVisibility();
 for(const render of levelSubscribers)render();
 // Levels change many times a second; the island only redraws when something it shows changed.
 const key=JSON.stringify([next.phase,next.muted,next.error,next.notice,next.approval?.id,next.remembered?.at,lastTranscript(next)?.id,lastTranscript(next)?.text]);
 if(key===renderKey)return;renderKey=key;
 if(next.phase==="error"&&previous.phase!=="error"){void Bridge.log(`voice: live session error ${String(next.error??"").slice(0,120)}`);if(/Configure the voice API key/i.test(String(next.error)))void Bridge.openSettingsWindow("assistant");}
 for(const render of subscribers)render();
 State.notify();
}
/** Voice answers count only while the card is really on screen (not queued behind another card, not collapsed). */
// Keyed by card id: every new card starts hidden in live.ts, so it must be announced even when the previous one was shown.
let approvalShown:string|null=null;
function syncVisibility(){
 const shown=snapshot.approval&&State.mode==="expanded"&&State.view===voiceView(snapshot)?snapshot.approval.id:null;
 if(shown===approvalShown)return;approvalShown=shown;
 if(initialized)liveVoice().setApprovalVisible(shown!==null);
}
function syncApproval(previous:LiveSnapshot,next:LiveSnapshot){
 const island=voiceIsland();
 if(next.approval&&next.approval.id!==previous.approval?.id){
  // A coding-agent card on screen keeps priority; this one waits (the header says so) and opens when it clears.
  State.isPinned=true;Sound.play("approval");if(voiceMayTakeFocus())island?.alert(voiceView(next)!);
  clearInterval(approvalTimer);approvalTimer=setInterval(()=>{for(const render of subscribers)render();State.notify();},1000);
 }else if(!next.approval&&previous.approval){
  clearInterval(approvalTimer);approvalTimer=undefined;
  if(island)approvalCleared(island,voiceView(previous)!);
 }
}
/** live.ts reports Persian sentences: Persian shows them as-is; English gets a mapped sentence (generic when no cause is recognised). */
function liveErrorText(error?:string){const mapped=voiceError(error);return error&&mapped===t("assistant.callFailed")&&(getLanguage()==="fa"||!/[؀-ۿ]/.test(error))?error:mapped;}
export function globalVoiceSnapshot(){return snapshot;}
export function globalVoiceActive(){return liveActive(snapshot.phase);}
/** `id` is the card the user answered; a click on a card that was already replaced is ignored by live.ts. */
export function decideGlobalVoice(approve:boolean,id?:string){liveVoice().decide(approve,id);}
/** The chat turn behind the `ask_roadeep` tool. */
export function bindGlobalVoiceChat(send:(query:string)=>Promise<string>,stop?:()=>Promise<unknown>|void){bindVoiceAsk(send,stop);}
export function bindGlobalVoiceIsland(api:VoiceIslandApi){bindVoiceIsland(api);}
export async function stopGlobalVoice(){if(initialized&&liveActive(snapshot.phase))await liveVoice().end();}
async function toggle(){
 if(liveActive(snapshot.phase)){await liveVoice().end();return;}
 if(State.paused)return;
 if(!IS_TAURI){receive({...IDLE_SNAPSHOT,phase:"error",error:"ASSISTANT_NATIVE_REQUIRED"});return;}
 initializeGlobalVoice();
 await liveVoice().start();
}
export function initializeGlobalVoice(){
 if(initialized||!IS_TAURI)return;initialized=true;
 setLiveVoiceHost(voiceToolHost);
 const voice=liveVoice();voice.subscribe(receive);receive(voice.snapshot());
 State.subscribe(()=>{syncVisibility();if(State.paused&&liveActive(snapshot.phase))void stopGlobalVoice().catch(()=>void Bridge.log("voice: pause stop failed"));});
 window.addEventListener("pagehide",()=>void stopGlobalVoice().catch(()=>void Bridge.log("voice: cleanup failed")));
}

/** Drops a render subscription once its element, after being attached, leaves the document (view rebuilds). */
function whileAttached(el:HTMLElement,cleanup:()=>void){
 if(typeof MutationObserver==="undefined")return;
 let seen=el.isConnected;
 const observer=new MutationObserver(()=>{if(el.isConnected){seen=true;return;}if(seen){observer.disconnect();cleanup();}});
 observer.observe(document,{childList:true,subtree:true});
}
/** The shared header survives navigation; no view visibility handler owns the microphone. */
export function globalVoiceButton(showChat?:()=>void,preview?:LiveSnapshot){
 void showChat;
 const button=h("button",{type:"button",class:"global-voice-toggle"},h("span",{class:"global-voice-ring","aria-hidden":"true"}),micGlyph());
 const status=h("span",{class:"sr-only",role:"status"});button.append(status);
 const render=()=>{
  const display=preview??snapshot;const active=liveActive(display.phase);
  button.dataset.phase=display.phase;button.classList.toggle("is-listening",active);button.classList.toggle("is-muted",display.muted&&active);
  button.setAttribute("aria-pressed",String(active));button.setAttribute("aria-label",t(active?"liveVoice.end":"liveVoice.start"));
  button.title=display.phase==="error"?liveErrorText(display.error):active?`${phaseLabel(display)} · ${t("liveVoice.end")}`:t("liveVoice.start");
  status.textContent=display.phase==="idle"?"":display.phase==="error"?liveErrorText(display.error):phaseLabel(display);
 };
 const renderLevel=()=>button.style.setProperty("--voice-level",liveLevel(preview??snapshot).toFixed(3));
 if(!preview)button.onclick=()=>void toggle().catch(error=>{void Bridge.log("voice: toggle failed");receive({...snapshot,phase:"error",error:String(error)});});
 subscribers.add(render);levelSubscribers.add(renderLevel);render();renderLevel();
 whileAttached(button,()=>{subscribers.delete(render);levelSubscribers.delete(renderLevel);});
 return button;
}
/** «به خاطر سپردم: …» for 2.5 s when the live voice stores something about the user; never takes clicks or focus. */
export const REMEMBERED_MS=2500;
export function globalVoiceRemembered(){
 const text=h("span",{dir:"auto"});
 const el=h("div",{class:"live-remembered",role:"status","aria-live":"polite",hidden:true},text);
 let shown:number|undefined;let timer:ReturnType<typeof setTimeout>|undefined;
 const render=()=>{
  const note=snapshot.remembered;if(!note||note.at===shown)return;
  shown=note.at;text.textContent=t("liveVoice.remembered",{text:note.text});el.title=text.textContent;
  el.hidden=false;requestAnimationFrame(()=>el.classList.add("on"));
  clearTimeout(timer);timer=setTimeout(()=>{el.classList.remove("on");timer=setTimeout(()=>{el.hidden=true;},250);},REMEMBERED_MS);
 };
 subscribers.add(render);render();whileAttached(el,()=>{subscribers.delete(render);clearTimeout(timer);});
 return el;
}
/** Header and compact-island chip: phase or latest words, plus the microphone. */
export function globalVoiceIndicator(showChat?:()=>void,preview?:LiveSnapshot){
 const button=globalVoiceButton(showChat,preview);
 const label=h("span",{class:"global-voice-phase",dir:"auto","aria-live":"polite"});
 const el=h("div",{class:"global-voice-indicator"},label,button);
 const render=()=>{
  const display=preview??snapshot;const active=liveActive(display.phase);
  const line=active?lastTranscript(display):undefined;
  label.hidden=!active&&display.phase!=="error";
  // An approval waiting behind another card (or off screen) is announced here so it is never lost.
  const waiting=!!display.approval&&State.view!==voiceView(display);label.classList.toggle("is-waiting",waiting);
  label.textContent=display.phase==="error"?t("liveVoice.error"):waiting?t("liveVoice.draft.waiting"):line&&["listening","speaking"].includes(display.phase)&&!display.muted?line.text:phaseLabel(display);
  label.dataset.speaker=line?.speaker??"";
  el.title=display.phase==="error"?liveErrorText(display.error):phaseLabel(display);
 };
 subscribers.add(render);render();whileAttached(el,()=>subscribers.delete(render));return el;
}
