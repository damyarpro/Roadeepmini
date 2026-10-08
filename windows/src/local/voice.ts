import {BridgeLocal} from "../core/bridge-local";
import {UtteranceDetector} from "./vad";
export type LocalVoicePhase="idle"|"preparing"|"recording"|"verifying"|"transcribing"|"thinking"|"speaking"|"error";
export type LocalVoiceNotice="speaker-too-short"|"speaker-not-enrolled"|"speaker-uncertain"|"speaker-different"|"speaker-enrollment-active"|"speech-unrecognized";
export interface LocalVoiceState{phase:LocalVoicePhase;error?:unknown;caption?:string;notice?:LocalVoiceNotice}
export function speakerNotice(reason?:string):LocalVoiceNotice{
 switch(reason){case"too-short":return"speaker-too-short";case"not-enrolled":return"speaker-not-enrolled";case"uncertain":return"speaker-uncertain";case"enrollment-active":return"speaker-enrollment-active";default:return"speaker-different";}
}
export interface AudioInputLevel {rms:number;peak:number}
export function inputLevels(input:Float32Array):AudioInputLevel{
 let energy=0,peak=0;for(const value of input){if(!Number.isFinite(value))throw new Error("local-audio-invalid");const amplitude=Math.min(1,Math.abs(value));energy+=amplitude*amplitude;peak=Math.max(peak,amplitude);}
 return{rms:input.length?Math.sqrt(energy/input.length):0,peak};
}
export async function resumeCapture(context:AudioContext){
 let timeout:ReturnType<typeof setTimeout>|undefined;
 try{await Promise.race([context.resume(),new Promise<never>((_resolve,reject)=>{timeout=setTimeout(()=>reject(new Error("local-mic-start-timeout")),5000);})]);}
 finally{clearTimeout(timeout);}
}
export function pcmWav(samples:Float32Array,rate:number):Uint8Array{
 if(!Number.isFinite(rate)||rate<16000||rate>192000||samples.length/rate>30)throw new Error("local-duration");
 if(samples.some(sample=>!Number.isFinite(sample)))throw new Error("local-audio-invalid");
 const count=Math.floor(samples.length*16000/rate);const bytes=new Uint8Array(44+count*2);const v=new DataView(bytes.buffer);
 for(const [at,text]of [[0,"RIFF"],[8,"WAVE"],[12,"fmt "],[36,"data"]] as const)for(let i=0;i<text.length;i++)v.setUint8(at+i,text.charCodeAt(i));
 v.setUint32(4,36+count*2,true);v.setUint32(16,16,true);v.setUint16(20,1,true);v.setUint16(22,1,true);v.setUint32(24,16000,true);v.setUint32(28,32000,true);v.setUint16(32,2,true);v.setUint16(34,16,true);v.setUint32(40,count*2,true);
 for(let i=0;i<count;i++){const at=i*rate/16000;const left=Math.floor(at);const f=at-left;const sample=Math.max(-1,Math.min(1,samples[left]*(1-f)+(samples[Math.min(left+1,samples.length-1)]??0)*f));v.setInt16(44+i*2,Math.round(sample*(sample<0?32768:32767)),true);}return bytes;
}
export function encodeWav(bytes:Uint8Array):string{let output="";for(let i=0;i<bytes.length;i+=8192)output+=String.fromCharCode(...bytes.subarray(i,i+8192));return btoa(output);}
export interface LocalVoiceOptions{
 ask(query:string):Promise<string>;cancelAsk?():Promise<unknown>|void;
 onState(state:LocalVoiceState):void;
 onLevel?(level:AudioInputLevel):void;
 bridge?:Pick<typeof BridgeLocal,"transcribe"|"speak"|"cancel">;
 media?:Pick<MediaDevices,"getUserMedia">;
 context?():AudioContext;audio?():HTMLAudioElement;
 continuous?:boolean;
 rearm?():boolean;
 allowSilentReply?:boolean;
 verify?(wavBase64:string,requestId:string):Promise<{matched:boolean;ticket?:string;reason?:string}>;
}
export class LocalVoiceController{
 private generation=0;private stream?:MediaStream;private context?:AudioContext;private processor?:ScriptProcessorNode;private source?:MediaStreamAudioSourceNode;
 private timer?:ReturnType<typeof setTimeout>;private chunks:Float32Array[]=[];private length=0;private sampleRate=48000;private audio?:HTMLAudioElement;private playbackDone?:()=>void;
 private requestId?:string;private state:LocalVoiceState={phase:"idle"};private starting=false;private audioUrl?:string;
 private bridge:Pick<typeof BridgeLocal,"transcribe"|"speak"|"cancel">;
 private listening=false;private detector?:UtteranceDetector;
 private inputTimer?:ReturnType<typeof setInterval>;private lastInput=0;private lastLevel=-Infinity;
 private notice?:LocalVoiceNotice;private noticeTimer?:ReturnType<typeof setTimeout>;
 constructor(private options:LocalVoiceOptions){this.bridge=options.bridge??BridgeLocal;}
 get phase(){return this.state.phase;}get active(){return this.starting||!["idle","error"].includes(this.phase);}
 private emit(state:LocalVoiceState){this.state={...state,...(this.notice?{notice:this.notice}:{})};this.options.onState(this.state);}
 private notify(notice:LocalVoiceNotice){this.notice=notice;clearTimeout(this.noticeTimer);this.noticeTimer=setTimeout(()=>{this.notice=undefined;this.noticeTimer=undefined;const {notice:_notice,...state}=this.state;this.emit(state);},4000);this.emit({phase:"idle"});}
 private failCapture(error:unknown,revision:number){if(revision!==this.generation||this.phase!=="recording")return;this.listening=false;const cleanup=this.releaseMic();this.emit({phase:"error",error});void cleanup.catch(error=>console.error("Microphone cleanup failed",error));}
 private async releaseMic(){clearTimeout(this.timer);this.timer=undefined;clearInterval(this.inputTimer);this.inputTimer=undefined;this.options.onLevel?.({rms:0,peak:0});this.processor?.disconnect();this.source?.disconnect();if(this.processor)this.processor.onaudioprocess=null;this.processor=undefined;this.source=undefined;this.stream?.getTracks().forEach(track=>track.stop());this.stream=undefined;const context=this.context;this.context=undefined;if(context&&context.state!=="closed")await context.close();}
 async start(){
  if(this.active)return;const revision=++this.generation;this.listening=true;this.starting=true;this.chunks=[];this.length=0;this.emit({phase:"preparing"});
  try{
   const stream=await(this.options.media??navigator.mediaDevices).getUserMedia({audio:{channelCount:1,echoCancellation:true,noiseSuppression:true,autoGainControl:true},video:false});
   if(revision!==this.generation){stream.getTracks().forEach(track=>track.stop());return;}
   this.stream=stream;const context=(this.options.context??(()=>new AudioContext()))();this.context=context;this.sampleRate=context.sampleRate;await resumeCapture(context);
   if(revision!==this.generation){
    stream.getTracks().forEach(track=>track.stop());
    if(this.stream===stream)this.stream=undefined;if(this.context===context)this.context=undefined;
    if(context.state!=="closed")await context.close();return;
   }
   this.source=context.createMediaStreamSource(stream);this.processor=context.createScriptProcessor(2048,1,1);this.detector=new UtteranceDetector(this.sampleRate);
   this.processor.onaudioprocess=event=>{if(revision!==this.generation||this.phase!=="recording")return;const input=event.inputBuffer.getChannelData(0);if(input.length){let level:AudioInputLevel;try{level=inputLevels(input);}catch(error){this.failCapture(error,revision);return;}this.lastInput=performance.now();if(this.lastInput-this.lastLevel>=100){this.lastLevel=this.lastInput;this.options.onLevel?.(level);}}if(this.options.continuous){const utterance=this.detector!.push(input);if(utterance){this.chunks=[utterance];this.length=utterance.length;void this.finish();}return;}const remain=Math.floor(this.sampleRate*30)-this.length;if(remain<=0){void this.finish();return;}const chunk=input.slice(0,remain);this.chunks.push(chunk);this.length+=chunk.length;};
   this.source.connect(this.processor);this.processor.connect(context.destination);this.lastInput=performance.now();this.lastLevel=-Infinity;this.emit({phase:"recording"});if(revision!==this.generation)return;this.inputTimer=setInterval(()=>{if(revision===this.generation&&this.phase==="recording"&&performance.now()-this.lastInput>=3000){this.failCapture(new Error("local-mic-no-data"),revision);}},1000);if(revision!==this.generation)return;if(!this.options.continuous||(this.options.rearm&&!this.options.rearm()))this.timer=setTimeout(()=>void this.finish(),30000);
  }catch(error){if(revision===this.generation){await this.releaseMic();if(revision===this.generation)this.emit({phase:"error",error});}}
  finally{if(revision===this.generation)this.starting=false;}
 }
 async finish(){
  if(this.phase!=="recording")return;const revision=this.generation;this.emit({phase:"transcribing"});const requestId=`voice-${crypto.randomUUID()}`;this.requestId=requestId;
  try{
   await this.releaseMic();if(revision!==this.generation)return;
   const samples=new Float32Array(this.length);let offset=0;for(const chunk of this.chunks){samples.set(chunk,offset);offset+=chunk.length;}this.chunks=[];this.length=0;
   let energy=0;for(const sample of samples)energy+=sample*sample;
   if(samples.length<this.sampleRate*.25||Math.sqrt(energy/samples.length)<.003)throw new Error("local-silence");
   let speakerTicket:string|undefined;
   const inputWav=encodeWav(pcmWav(samples,this.sampleRate));
   if(this.options.verify){this.emit({phase:"verifying"});const match=await this.options.verify(inputWav,requestId);if(revision!==this.generation)return;if(!match.matched){this.notify(speakerNotice(match.reason));return;}speakerTicket=match.ticket;if(this.options.continuous&&!speakerTicket)throw new Error("local-speaker-ticket");}
   this.emit({phase:"transcribing"});
   const result=await this.bridge.transcribe(inputWav,requestId,speakerTicket);if(revision!==this.generation)return;
   const text=result.text.trim();if(!text||/^\[.*\]$/.test(text)||/^\(.*\)$/.test(text))throw new Error("local-silence");
   this.emit({phase:"thinking",caption:text});if(revision!==this.generation)return;const reply=await this.options.ask(text);if(revision!==this.generation)return;
   if(!reply.trim()){if(this.options.allowSilentReply)return;throw new Error("local-empty-reply");}
   const speech=await this.bridge.speak(Array.from(reply).slice(0,1500).join(""),requestId);if(revision!==this.generation)return;
   if(!/^[A-Za-z0-9+/=]+$/.test(speech.wavBase64)||speech.wavBase64.length>6*1024*1024)throw new Error("local-audio-invalid");
   const decoded=atob(speech.wavBase64);const wav=new Uint8Array(decoded.length);for(let i=0;i<decoded.length;i++)wav[i]=decoded.charCodeAt(i);
   if(String.fromCharCode(...wav.subarray(0,4))!=="RIFF"||String.fromCharCode(...wav.subarray(8,12))!=="WAVE")throw new Error("local-audio-invalid");
   this.emit({phase:"speaking",caption:text});if(revision!==this.generation)return;const audio=(this.options.audio??(()=>new Audio()))();this.audio=audio;this.audioUrl=URL.createObjectURL(new Blob([wav],{type:"audio/wav"}));audio.src=this.audioUrl;
   await new Promise<void>((resolve,reject)=>{this.playbackDone=resolve;audio.onended=()=>resolve();audio.onerror=()=>reject(new Error("local-audio-playback"));void audio.play().catch(reject);});
   if(revision===this.generation)this.emit({phase:"idle"});
  }catch(error){if(revision===this.generation){const message=error instanceof Error?error.message:String(error);if(this.options.continuous&&/speaker-insufficient|speaker-no-speech|speaker-too-short/.test(message))this.notify("speaker-too-short");else if(this.options.continuous&&message==="local-audio-no-speech")this.notify("speech-unrecognized");else if(this.options.continuous&&/local-silence|local-route-cancelled|local-cancelled|cancelled|Chat is busy|local-empty-reply/.test(message))this.emit({phase:"idle"});else this.emit({phase:"error",error});}}
  finally{if(revision===this.generation){this.requestId=undefined;this.clearAudio();if(this.options.continuous&&this.listening){if(this.options.rearm&&!this.options.rearm()){this.listening=false;this.emit({phase:"idle"});return;}if((this.state.phase as LocalVoicePhase)==="error"){this.listening=false;}else{this.emit({phase:"idle"});void this.start();}}}}
 }
 private clearAudio(){this.audio?.pause();if(this.audio){this.audio.onended=null;this.audio.onerror=null;this.audio.removeAttribute("src");this.audio.load();}this.audio=undefined;if(this.audioUrl)URL.revokeObjectURL(this.audioUrl);this.audioUrl=undefined;this.playbackDone?.();this.playbackDone=undefined;}
 async end(){
  const askActive=this.phase==="thinking";const requestId=this.requestId;const revision=++this.generation;this.listening=false;this.starting=false;this.requestId=undefined;this.chunks=[];this.length=0;this.detector?.reset();
  clearTimeout(this.noticeTimer);this.noticeTimer=undefined;this.notice=undefined;
  const cancellation=askActive?this.options.cancelAsk?.():undefined;
  const nativeCancellation=requestId?this.bridge.cancel(requestId):undefined;
  this.clearAudio();const cleanup=this.releaseMic();
  if(revision===this.generation)this.emit({phase:"idle"});
  const results=await Promise.allSettled([cleanup,cancellation,nativeCancellation]);
  const failed=results.find(result=>result.status==="rejected");if(failed?.status==="rejected")throw failed.reason;
 }
}
