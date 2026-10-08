import {t,getLanguage} from "../core/i18n";
import {h,svg} from "../views/dom";
import type {LivePhase,LiveSnapshot} from "../voice/live";
import "./messages";
import "./style.css";

export const MIC_ICON="M12 14a3 3 0 0 0 3-3V5a3 3 0 0 0-6 0v6a3 3 0 0 0 3 3z M5 10v1a7 7 0 0 0 14 0v-1 M12 18v4 M8 22h8";
export const IDLE_SNAPSHOT:LiveSnapshot={phase:"idle",muted:false,transcripts:[],inputLevel:0,outputLevel:0};
export const liveActive=(phase:LivePhase)=>phase!=="idle"&&phase!=="error";
const number=(value:number)=>getLanguage()==="fa"?value.toLocaleString("fa-IR"):String(value);
export function phaseLabel(snapshot:LiveSnapshot){return snapshot.muted&&liveActive(snapshot.phase)?t("liveVoice.muted"):t(`liveVoice.${snapshot.phase}`);}
/** The ring follows whoever is talking: the user while listening, Roadeep while speaking. */
export function liveLevel(snapshot:LiveSnapshot){
 const level=snapshot.phase==="speaking"?snapshot.outputLevel:snapshot.phase==="listening"&&!snapshot.muted?snapshot.inputLevel:0;
 return Number.isFinite(level)?Math.max(0,Math.min(1,level)):0;
}
export const lastTranscript=(snapshot:LiveSnapshot,speaker?:"user"|"assistant")=>[...snapshot.transcripts].reverse().find(line=>line.text.trim()&&(!speaker||line.speaker===speaker));
export const secondsLeft=(expiresAt:number,now=Date.now())=>Math.max(0,Math.ceil((expiresAt-now)/1000));

/** The user's latest words not yet present in `baseline` (a line that kept growing contributes only its new part). */
export function heardSince(snapshot:LiveSnapshot,baseline:ReadonlyMap<string,string>){
 for(const line of [...snapshot.transcripts].reverse()){
  if(line.speaker!=="user")continue;
  const before=baseline.get(line.id);
  const text=(before===undefined?line.text:line.text.startsWith(before)?line.text.slice(before.length):"").trim();
  if(text)return text;
 }
 return "";
}

/**
 * The island's voice-approval card: what will be done (Persian summary from the tool runtime),
 * a countdown, the two buttons and the user's latest words. The decision itself belongs to
 * the live controller; this only renders and forwards clicks.
 */
export function voiceApprovalCard(decide:(approve:boolean,id:string)=>void){
 const countdown=h("span",{class:"voice-approval-countdown",role:"timer","aria-live":"off"});
 const who=h("div",{class:"who-row"},h("span",{class:"voice-approval-dot","aria-hidden":"true"}),h("span",{class:"n",text:t("liveVoice.approvalTitle")}),countdown);
 const summary=h("div",{class:"title voice-approval-summary",dir:"auto"});
 const heard=h("div",{class:"sub voice-approval-heard",dir:"auto","aria-live":"polite"});
 const hint=h("div",{class:"sub voice-approval-hint",text:t("liveVoice.approvalHint")});
 // The full arguments (tools without a planner preview): scrollable and reachable by keyboard.
 const details=h("pre",{class:"voice-approval-details",dir:"ltr",tabindex:0,role:"region","aria-label":t("liveVoice.details"),hidden:true});
 const reject=h("button",{type:"button",class:"btn secondary"},h("span",{text:t("liveVoice.reject")}));
 const approve=h("button",{type:"button",class:"btn primary"},h("span",{text:t("liveVoice.approve")}));
 // Built once: replacing buttons between mouse-down and mouse-up would swallow the click.
 let current:string|undefined;
 // What was already said when this card appeared: only words heard after it count as an answer.
 let baseline=new Map<string,string>();
 reject.onclick=()=>{if(current)decide(false,current);};approve.onclick=()=>{if(current)decide(true,current);};
 const el=h("div",{class:"stack voice-approval",role:"alertdialog","aria-label":t("liveVoice.approvalTitle")},who,summary,details,heard,hint,h("div",{class:"actions"},reject,approve));
 return{el,update(snapshot:LiveSnapshot,now=Date.now()){
  const approval=snapshot.approval;
  if(approval?.id!==current)baseline=new Map(snapshot.transcripts.map(line=>[line.id,line.text]));
  current=approval?.id;approve.disabled=reject.disabled=!approval;
  if(!approval)return;
  summary.textContent=approval.summary;summary.title=approval.summary;
  if(details.textContent!==(approval.details??""))details.textContent=approval.details??"";details.hidden=!approval.details;
  countdown.textContent=t("liveVoice.approvalLeft",{value:number(secondsLeft(approval.expiresAt,now))});
  const said=heardSince(snapshot,baseline);heard.textContent=said?t("liveVoice.heard",{text:said}):"";heard.hidden=!said;
 }};
}

export function micGlyph(){return svg(MIC_ICON,16,{stroke:1.7});}
