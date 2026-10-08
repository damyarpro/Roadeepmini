import {helpDisclosure} from "../settings/help";
import {BridgeLocal,type SpeakerStatus} from "../core/bridge-local";
import {h} from "../views/dom";
import {t} from "../core/i18n";
import {encodeWav,pcmWav,inputLevels,resumeCapture,type AudioInputLevel} from "./voice";
import {inputMeter} from "./input-meter";
import {localError} from "./messages";

export function adminVoiceSettings(native:boolean,bridge=BridgeLocal,previewStatus?:SpeakerStatus&{recordedSeconds?:number;level?:AudioInputLevel}){
 let status:SpeakerStatus=previewStatus??{enrolled:false,enrolling:false};let busy=previewStatus?.recordedSeconds!==undefined;let recording=busy;let preparing=false;let disposed=false;let revision=0;let recordedSeconds=previewStatus?.recordedSeconds??0;
 let stream:MediaStream|undefined;let context:AudioContext|undefined;let source:MediaStreamAudioSourceNode|undefined;let processor:ScriptProcessorNode|undefined;let timer:ReturnType<typeof setTimeout>|undefined;
 let beginning:Promise<SpeakerStatus>|undefined;let inputTimer:ReturnType<typeof setInterval>|undefined;let lastInput=0;let lastLevel=-Infinity;
 const meter=inputMeter();if(previewStatus?.level)meter.update(previewStatus.level);
 let chunks:Float32Array[]=[];let count=0;let requestId:string|undefined;
 const state=h("p",{class:"admin-voice-status",role:"status"});const notice=h("p",{class:"notice err",role:"alert",hidden:true});
 const record=h("button",{type:"button",class:"primary"});const remove=h("button",{type:"button",text:t("adminVoice.remove")});const cancel=h("button",{type:"button",text:t("adminVoice.cancelEnrollment"),hidden:true,disabled:!native});
 const progress=h("progress",{max:12,value:0,"aria-label":t("adminVoice.recordingProgress")});const progressCount=h("span");
 const progressLabel=h("p",{class:"admin-voice-progress-label","aria-live":"off"},progressCount);
 const progressBlock=h("div",{class:"admin-voice-progress"},h("span",{class:"input-meter-label",text:t("adminVoice.recordingTime")}),progress,progressLabel);
 const el=h("div",{class:"admin-voice-settings"},h("div",{class:"set-label-line"},h("h3",{text:t("adminVoice.title")}),helpDisclosure(`${t("adminVoice.description")} ${t("adminVoice.privacy")}`,`${t("settings.help")}: ${t("adminVoice.title")}`)),state,h("p",{class:"hint",text:t("adminVoice.enrollmentHint")}),progressBlock,meter.el,notice,h("div",{class:"actions"},record,cancel,remove));
 function show(error:unknown){if(!disposed){notice.textContent=localError(error);notice.hidden=false;}}
 function render(){record.textContent=t(busy?recording?"adminVoice.enrolling":preparing?"local.preparing":"adminVoice.saving":status.enrolled?"adminVoice.rerecord":"adminVoice.enroll");record.hidden=recording;remove.hidden=recording;record.disabled=busy||!native;remove.disabled=busy||!native||!status.enrolled;cancel.hidden=!busy;meter.el.hidden=!recording;state.textContent=t(!native&&!previewStatus?"local.preview":recording?"adminVoice.enrolling":busy?preparing?"local.preparing":"adminVoice.saving":status.enrolled?"adminVoice.registered":"adminVoice.missing");el.setAttribute("aria-busy",String(busy));renderProgress();}
 function renderProgress(){progressBlock.hidden=!recording;progress.value=recordedSeconds;progressCount.textContent=t("adminVoice.recordedCount",{value:recordedSeconds});progress.setAttribute("aria-valuetext",progressCount.textContent??"");}
 async function release(){clearTimeout(timer);timer=undefined;clearInterval(inputTimer);inputTimer=undefined;processor?.disconnect();source?.disconnect();if(processor)processor.onaudioprocess=null;processor=undefined;source=undefined;stream?.getTracks().forEach(track=>track.stop());stream=undefined;const old=context;context=undefined;if(old&&old.state!=="closed")await old.close();}
 async function stop(){const ending=++revision;recording=false;preparing=false;busy=true;meter.update();chunks=[];count=0;const id=requestId;requestId=undefined;render();await beginning?.catch(show);const results=await Promise.allSettled([release(),id?bridge.cancel(id):undefined,bridge.speakerEnrollmentEnd()]);if(ending===revision){busy=false;render();}for(const result of results)if(result.status==="rejected")throw result.reason;}
 async function save(current:number,rate:number){
  if(current!==revision||!recording)return;recording=false;render();const samples=new Float32Array(count);let offset=0;for(const chunk of chunks){samples.set(chunk,offset);offset+=chunk.length;}chunks=[];count=0;
  try{await release();if(current!==revision)return;if(samples.length<rate*8)throw new Error("local-mic-no-data");if(inputLevels(samples).rms<.003)throw new Error("local-silence");requestId=`enroll-${crypto.randomUUID()}`;const next=await bridge.speakerEnroll(encodeWav(pcmWav(samples,rate)),requestId);if(current===revision&&!disposed){status=next;notice.hidden=true;}}
  catch(error){if(current===revision)show(error);}
  finally{samples.fill(0);if(current===revision){requestId=undefined;await bridge.speakerEnrollmentEnd().catch(show);if(current===revision){busy=false;if(!disposed)render();}}}
 }
 record.onclick=()=>{if(busy||!native)return;const current=++revision;busy=true;preparing=true;recordedSeconds=0;meter.update();notice.hidden=true;render();void(async()=>{
  try{beginning=bridge.speakerEnrollmentBegin();await beginning;beginning=undefined;if(current!==revision)return;const next=await navigator.mediaDevices.getUserMedia({audio:{channelCount:1,echoCancellation:true,noiseSuppression:true},video:false});if(current!==revision){next.getTracks().forEach(track=>track.stop());return;}stream=next;const audio=new AudioContext();context=audio;await resumeCapture(audio);if(current!==revision){next.getTracks().forEach(track=>track.stop());if(audio.state!=="closed")await audio.close();return;}source=audio.createMediaStreamSource(next);processor=audio.createScriptProcessor(2048,1,1);chunks=[];count=0;recording=true;preparing=false;lastInput=performance.now();lastLevel=-Infinity;render();processor.onaudioprocess=event=>{if(current!==revision||!recording)return;const data=event.inputBuffer.getChannelData(0);if(!data.length)return;let level:AudioInputLevel;try{level=inputLevels(data);}catch(error){void stop().then(()=>show(error)).catch(show);return;}lastInput=performance.now();if(lastInput-lastLevel>=100){lastLevel=lastInput;meter.update(level);}const chunk=data.slice(0,Math.max(0,Math.floor(audio.sampleRate*12)-count));chunks.push(chunk);count+=chunk.length;const seconds=Math.floor(count/audio.sampleRate);if(seconds!==recordedSeconds){recordedSeconds=seconds;renderProgress();}if(count>=audio.sampleRate*12)void save(current,audio.sampleRate);};source.connect(processor);processor.connect(audio.destination);inputTimer=setInterval(()=>{if(current===revision&&recording&&performance.now()-lastInput>=3000)void stop().then(()=>show(new Error("local-mic-no-data"))).catch(show);},1000);timer=setTimeout(()=>{if(current===revision&&recording)void stop().then(()=>show(new Error("local-mic-no-data"))).catch(show);},16000);}
  catch(error){if(current===revision){await stop().catch(show);show(error);if(!disposed)render();}}
 })();};
 cancel.onclick=()=>void stop().catch(show);
 remove.onclick=()=>{if(busy||!native)return;busy=true;render();void bridge.speakerClear().then(()=>{if(!disposed)status={enrolled:false,enrolling:false};}).catch(show).finally(()=>{busy=false;if(!disposed)render();});};
 if(native)void bridge.speakerStatus().then(next=>{if(!disposed&&revision===0){status=next;render();}}).catch(show);
 render();return{el,dispose(){disposed=true;if(busy&&native)void stop().catch(error=>console.error("Voice enrollment cleanup failed",error));}};
}
