import {State} from "../core/state";
import type {IslandViewName} from "../core/layout";

/**
 * Two cards can wait for the user at once: a coding-agent permission (State.pendingApproval,
 * view "approval") and a live-voice change (view "voiceApproval"). Neither may hide the other
 * for good: whenever one clears, the other is shown again; the island stays pinned while any waits.
 */
export interface FocusIsland{alert(view:IslandViewName):void;setView(view:IslandViewName):void;dropPin():void}
let voiceView=():IslandViewName|null=>null;
/** global-voice.ts reports where a pending live-voice approval is shown ("voiceApproval" or a planner page), null when none (kept here to avoid an import cycle). */
export function setVoiceApprovalProbe(probe:()=>IslandViewName|null){voiceView=probe;}
export function approvalPending(){return !!State.pendingApproval||voiceView()!==null;}
/** Where a new live-voice approval may open now: never over a coding-agent card that is on screen (it keeps priority). */
export function voiceMayTakeFocus(){return !(State.pendingApproval&&(State.view==="approval"||State.view==="question"));}
/** One card cleared (`cleared` is the view it was shown on). Planner pages stay put so the result stays visible. */
export function approvalCleared(island:FocusIsland,cleared:IslandViewName){
 if(State.pendingApproval){State.isPinned=true;island.alert("approval");return;}
 const voice=voiceView();
 if(voice){State.isPinned=true;island.alert(voice);return;}
 State.isPinned=false;island.dropPin();
 if(State.view===cleared&&(cleared==="approval"||cleared==="voiceApproval"))island.setView(State.defaultView());
}
