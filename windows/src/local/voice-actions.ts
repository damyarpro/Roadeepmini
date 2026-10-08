import {BridgeLocal,type DesktopVoiceReply} from "../core/bridge-local";
import {LocalRouteCancelled} from "./routing";
import {t} from "../core/i18n";
import "./messages";
/** Native parses the intent and owns execution. Cancellation never becomes a retry or API handoff. */
export async function voiceDesktopAction(options:{fromVoice:boolean;query:string;language:string;requestId:string;cancelled():boolean;bridge?:Pick<typeof BridgeLocal,"desktopVoiceTry">}):Promise<DesktopVoiceReply|null>{
 const check=()=>{if(options.cancelled())throw new LocalRouteCancelled();};
 check();if(!options.fromVoice)return null;
 try{const result=await(options.bridge??BridgeLocal).desktopVoiceTry(options.query,options.language,options.requestId);check();return result.handled?result:null;}
 catch(error){check();throw error;}
}
export function voiceTaskError(error:unknown):string{
 const text=error instanceof Error?error.message:String(error);
 if(text.includes("voice-proposal-"))return t(/expired|missing|stale/.test(text)?"adminVoice.proposalExpired":"adminVoice.proposalError");
 if(text.includes("voice-sensitive-action-blocked"))return t("local.voiceSensitiveBlocked");
 if(text.includes("desktop-launch")||text.includes("desktop-platform")||text.includes("desktop-unavailable"))return t("local.desktopFailed");
 return t("local.voiceTaskFailed");
}
