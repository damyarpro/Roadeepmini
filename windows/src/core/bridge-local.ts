import { invoke } from "@tauri-apps/api/core";
import { IS_TAURI, onEvent } from "./bridge";
export interface LocalRuntimeStatus { ready:boolean;enabled:boolean;phase:string;component:string;downloaded:number;total:number;error:string|null }
export interface LocalHistory { role:"user"|"assistant";text:string }
export interface LocalBrainReply { route:"local"|"cloud";text:string;reason:string }
export interface VoiceProposal {id:string;kind:"task"|"note";text:string}
export interface VoiceAnalysis {route:"chat"|"silent"|"proposal"|"done"|"reply";text:string;proposal?:VoiceProposal}
export interface DesktopVoiceReply { handled:boolean;text:string;action?:string }
export interface SpeakerStatus { enrolled:boolean;enrolling:boolean }
async function call<T>(command:string,args?:Record<string,unknown>):Promise<T>{
 if(!IS_TAURI)throw new Error("local-native-required");
 try{return await invoke<T>(command,args);}catch(error){throw error instanceof Error?error:new Error(typeof error==="string"?error:"local-operation-failed");}
}
export const BridgeLocal={
 analyzeVoice:(query:string,mode:"manual"|"always",language:string,requestId:string)=>call<VoiceAnalysis>("voice_assistant_analyze",{query,mode,language,requestId}),
 proposalDecide:(id:string,accept:boolean,language:string)=>call<{text:string}>("voice_proposal_decide",{id,accept,language}),
 proposalPending:()=>call<{proposal:VoiceProposal|null}>("voice_proposal_pending"),
 proposalClear:()=>call<void>("voice_proposal_clear"),
 desktopVoiceTry:(query:string,language:string,requestId:string)=>call<DesktopVoiceReply>("desktop_voice_try",{query,language,requestId}),
 status:()=>call<LocalRuntimeStatus>("local_runtime_status"),
 install:()=>call<LocalRuntimeStatus>("local_runtime_install"),
 cancelInstall:()=>call<void>("local_runtime_cancel"),
 enable:(enabled:boolean)=>call<LocalRuntimeStatus>("local_runtime_enable",{enabled}),
 chat:(query:string,history:LocalHistory[],forceCloud:boolean,requestId:string)=>call<LocalBrainReply>("local_brain_chat",{query,history,forceCloud,requestId}),
 cancel:(requestId?:string)=>call<void>("local_brain_cancel",{requestId}),
 transcribe:(wavBase64:string,requestId:string,speakerTicket?:string)=>call<{text:string}>("local_speech_transcribe",{wavBase64,requestId,speakerTicket}),
 speak:(text:string,requestId:string)=>call<{wavBase64:string}>("local_speech_speak",{text,requestId}),
 speakerStatus:()=>call<SpeakerStatus>("speaker_status"),
 speakerEnroll:(wavBase64:string,requestId:string)=>call<SpeakerStatus>("speaker_enroll",{wavBase64,requestId}),
 speakerVerify:(wavBase64:string,requestId:string)=>call<{matched:boolean;score:number;ticket?:string;reason?:string}>("speaker_verify",{wavBase64,requestId}),
 speakerClear:()=>call<void>("speaker_clear"),
 speakerEnrollmentBegin:()=>call<SpeakerStatus>("speaker_enrollment_begin"),
 speakerEnrollmentEnd:()=>call<SpeakerStatus>("speaker_enrollment_end"),
};
export const onLocalRuntimeProgress=(handler:(status:LocalRuntimeStatus)=>void)=>onEvent("local-runtime-progress",handler);
