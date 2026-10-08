import {Bridge,IS_TAURI,onEvent} from "../core/bridge";
import {BridgeLocal,type VoiceProposal} from "../core/bridge-local";
import {getLanguage,t} from "../core/i18n";
import {State} from "../core/state";
import {h} from "../views/dom";
import "./messages";
let pending:VoiceProposal|null=null;let busy=false;let feedback="";let failure=false;
const subscribers=new Set<()=>void>();
function notify(){for(const update of subscribers)update();State.notify();}
export function voiceProposal(){return pending;}
export function setVoiceProposal(proposal:VoiceProposal|null){pending=proposal;if(proposal){feedback="";failure=false;}notify();}
export function voiceProposalFeedback(text:string){feedback=text;failure=false;notify();}
export async function refreshVoiceProposal(){const result=await BridgeLocal.proposalPending();setVoiceProposal(result.proposal);}
export async function clearVoiceProposal(){await BridgeLocal.proposalClear();pending=null;feedback="";failure=false;notify();}
export function subscribeVoiceProposal(update:()=>void){subscribers.add(update);return()=>subscribers.delete(update);}
export async function decideVoiceProposal(accept:boolean){
 if(!pending||busy)return;const id=pending.id;busy=true;failure=false;feedback=t("adminVoice.proposalSaving");notify();
 try{const result=await BridgeLocal.proposalDecide(id,accept,getLanguage());if(pending?.id===id)pending=null;feedback=result.text;}
 catch(error){failure=true;const code=String(error);feedback=t(code.includes("expired")||code.includes("missing")||code.includes("not-found")?"adminVoice.proposalExpired":"adminVoice.proposalError");void Bridge.log("Voice proposal decision failed");try{const result=await BridgeLocal.proposalPending();pending=result.proposal;}catch{void Bridge.log("Voice proposal refresh failed");}}
 finally{busy=false;notify();}
}
export function voiceProposalCard(preview?:VoiceProposal){
 const title=h("strong"),text=h("p",{class:"voice-proposal-text",dir:"auto",tabindex:"0","aria-label":t("adminVoice.proposalTitle")});const status=h("p",{class:"voice-proposal-feedback",role:"status"});
 const save=h("button",{type:"button",class:"assistant-primary",text:t("adminVoice.proposalAccept")}),reject=h("button",{type:"button",class:"assistant-action",text:t("adminVoice.proposalReject")});
 const el=h("section",{class:"voice-proposal-card","aria-label":t("adminVoice.proposalTitle")},title,text,status,h("div",{class:"voice-proposal-actions"},save,reject));
 function render(){const proposal=preview??pending;el.hidden=!proposal&&!feedback;title.textContent=t(proposal?proposal.kind==="note"?"adminVoice.proposalNote":"adminVoice.proposalTask":"local.voice");text.hidden=!proposal;text.textContent=proposal?.text??"";status.textContent=preview?t("adminVoice.proposalHint"):feedback||t("adminVoice.proposalHint");status.classList.toggle("is-error",failure);save.hidden=reject.hidden=!proposal;save.disabled=reject.disabled=busy||!!preview;save.textContent=t(busy?"adminVoice.proposalSaving":"adminVoice.proposalAccept");}
 save.onclick=()=>void decideVoiceProposal(true);reject.onclick=()=>void decideVoiceProposal(false);const unsubscribe=preview?()=>{}:subscribeVoiceProposal(render);render();return{el,dispose:unsubscribe};
}
if(IS_TAURI){void onEvent<{proposal:VoiceProposal|null}>("voice-proposal-changed",result=>setVoiceProposal(result.proposal));void refreshVoiceProposal().catch(()=>void Bridge.log("Voice proposal state unavailable"));}
