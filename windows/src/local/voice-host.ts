import {Bridge} from "../core/bridge";
import {State,type Settings} from "../core/state";
import type {IslandViewName} from "../core/layout";
import {approvalPending} from "./approval-focus";
import type {CharacterOptionsList,CharacterPatch,IslandVoiceView,ToolHost} from "../voice/tools";

/** The few island operations the live voice needs; island.ts binds itself at construction. */
export interface VoiceIslandApi{alert(view:IslandViewName):void;setView(view:IslandViewName):void;dropPin():void;reveal():void}
export interface VoiceCharacterApi{options():CharacterOptionsList;apply(settings:Settings,patch:CharacterPatch):Settings}

const VIEWS:Record<IslandVoiceView,IslandViewName>={chat:"prompt",planner:"today",sessions:"activity",integrations:"overview",
 tasks:"tasks",notes:"notes",reminders:"reminders",habits:"habits",today:"today",week:"week",focus:"focus"};
export const VIEW_BLOCKED="ابتدا به درخواست تأیید باز پاسخ دهید.";
let island:VoiceIslandApi|undefined;
let ask:((query:string)=>Promise<string>)|undefined;
let cancelAsk:(()=>Promise<unknown>|void)|undefined;
let character:VoiceCharacterApi|undefined;

export function bindVoiceIsland(api:VoiceIslandApi){island=api;}
export function voiceIsland(){return island;}
/** `stop` cancels the chat turn `send` started (the session ended or the request timed out). */
export function bindVoiceAsk(send:(query:string)=>Promise<string>,stop?:()=>Promise<unknown>|void){ask=send;cancelAsk=stop;}
/** Wired from the character module (CHARACTER_OPTIONS + applyCharacterPatch). Until then the character tools answer "unavailable". */
export function bindVoiceCharacter(api:VoiceCharacterApi){character=api;}

export const voiceToolHost:ToolHost={
 showView(view){
  const target=VIEWS[view];if(!target)throw new Error("voice-view-unknown");
  if(!island)throw new Error("voice-island-unavailable");
  // A card waiting for the user's answer is never navigated away from; the model relays this sentence.
  if(approvalPending())throw new Error(VIEW_BLOCKED);
  island.setView(target);
 },
 async ask(query,signal){
  if(!ask)throw new Error("voice-chat-unavailable");
  if(signal?.aborted)throw new Error("voice-ask-cancelled");
  // Show the chat so its tool cards are visible, but never over a card that waits for the user.
  if(!approvalPending())island?.setView("prompt");await Promise.resolve();
  const stop=()=>{void Promise.resolve(cancelAsk?.()).catch(()=>void Bridge.log("voice: ask cancellation failed"));};
  signal?.addEventListener("abort",stop,{once:true});
  try{return await ask(query);}finally{signal?.removeEventListener("abort",stop);}
 },
 async setCharacter(patch){
  if(!character)throw new Error("voice-character-unavailable");
  const next=character.apply(State.settings,patch);
  await Bridge.saveSettingsChecked(next);State.settings=next;State.notify();
 },
 characterOptions(){return character?.options()??{shapes:[],colors:[],expressions:[]};},
};
