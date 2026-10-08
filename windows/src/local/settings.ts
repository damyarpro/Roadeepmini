import {helpDisclosure} from "../settings/help";
import {BridgeLocal,onLocalRuntimeProgress,type LocalRuntimeStatus} from "../core/bridge-local";
import {IS_TAURI} from "../core/bridge";
import {t,isRtl} from "../core/i18n";
import {h} from "../views/dom";
import "./messages";
import "./style.css";
export interface LocalSettingsOptions{bridge?:typeof BridgeLocal;native?:boolean;initial?:LocalRuntimeStatus;preview?:boolean}
export function localSettings(options:LocalSettingsOptions={}){
 const bridge=options.bridge??BridgeLocal;const native=options.native??IS_TAURI;let status=options.initial;let busy=false;let actionKind:"install"|"cancel"|"toggle"|undefined;let loading=!options.initial&&native&&!options.preview;let loadFailed=false;let displayError:string|undefined;let revision=0;let disposed=false;let stop:(()=>void)|undefined;
 const title=h("h3",{text:t("local.title")});const readinessText=h("span");const readiness=h("div",{class:"local-readiness",role:"status"},h("span",{class:"local-readiness-dot","aria-hidden":"true"}),readinessText);const description=helpDisclosure([t("local.description"),t("local.apiModelHint"),t("local.downloadHint")].join("\n\n"),`${t("settings.help")}: ${t("local.title")}`);
 const progress=h("progress",{max:1,value:0,"aria-label":t("local.downloading")});const progressLabel=h("span");const progressNumbers=h("bdi",{dir:"ltr",class:"local-progress-count"});const progressText=h("p",{class:"hint",role:"status"},progressLabel,h("span",{text:" · ","aria-hidden":"true"}),progressNumbers);const error=h("p",{class:"notice err",role:"alert",hidden:true});
 const install=h("button",{type:"button",class:"primary",text:t("local.install")});const cancel=h("button",{type:"button",text:t("local.cancel"),hidden:true});const toggle=h("button",{type:"button",text:t("local.enable"),hidden:true});
 const retryStatus=h("button",{type:"button",text:t("local.reloadStatus"),hidden:true});
 const sources=h("details",{class:"local-sources"},h("summary",{text:t("local.sources")}));
 const list=h("ul");for(const [label,url] of [["Qwen3.5‑2B Q4_K_M · Apache‑2.0","https://huggingface.co/unsloth/Qwen3.5-2B-GGUF"],["llama.cpp · MIT","https://github.com/ggml-org/llama.cpp"],["Whisper Small Q5_1 · MIT","https://github.com/ggml-org/whisper.cpp"],["Piper · MIT / eSpeak NG · GPL‑3.0","https://github.com/rhasspy/piper"],["Persian Amir voice · CC0 dataset","https://huggingface.co/rhasspy/piper-voices/tree/main/fa/fa_IR/amir/medium"]])list.append(h("li",{},h("a",{href:url,target:"_blank",rel:"noopener noreferrer",dir:"ltr",text:label})));sources.append(list);
 const el=h("div",{class:"local-settings",dir:isRtl()?"rtl":"ltr"},h("div",{class:"set-label-line"},title,description),readiness,progress,progressText,error,h("div",{class:"actions"},install,cancel,toggle,retryStatus),sources);
 function render(){
  const working=!!status&&["downloading","installing","verifying"].includes(status.phase);el.setAttribute("aria-busy",String(busy||working||loading));
  readinessText.textContent=status?.ready?t(status.enabled?"local.enabled":"local.disabled"):t(working?`local.${status!.phase}`:status?.phase==="cancelled"?"local.cancelled":"local.notInstalled");
  if(loading)readinessText.textContent=t("local.loadingStatus");
  if(status?.ready)readinessText.textContent+=` · ${t("local.ready")}`;
  install.hidden=!!status?.ready||working;install.disabled=busy||loading||loadFailed||!native||!!options.preview;install.textContent=t(busy&&actionKind==="install"?"local.startingInstall":status?.phase==="error"||status?.phase==="cancelled"?"local.retry":"local.install");
  cancel.hidden=!working;cancel.disabled=busy||!native||!!options.preview;toggle.hidden=!status?.ready;toggle.disabled=busy||!native||!!options.preview;toggle.textContent=t(busy&&actionKind==="toggle"?status?.enabled?"local.disabling":"local.enabling":status?.enabled?"local.disable":"local.enable");cancel.textContent=t(busy&&actionKind==="cancel"?"local.cancelling":"local.cancel");readiness.classList.toggle("is-ready",!!status?.ready&&status.enabled);toggle.setAttribute("aria-pressed",String(!!status?.enabled));
  progress.hidden=progressText.hidden=!working;progress.max=status?.total||1;progress.value=status?.downloaded??0;
  progressLabel.textContent=working?`${t(`local.${status!.phase}`)}${status?.component?` · ${t(`local.component.${status.component}`)}`:""}`:"";
  progressNumbers.textContent=working?`${Math.floor((status!.downloaded||0)/1048576).toLocaleString(isRtl()?"fa":"en")} / ${Math.ceil(status!.total/1048576).toLocaleString(isRtl()?"fa":"en")} MB`:"";
  error.hidden=!status?.error&&!displayError;error.textContent=displayError??(status?.error?t("local.error"):"");
  retryStatus.hidden=!loadFailed;retryStatus.disabled=busy||loading;
  if(!native&&!options.preview){readinessText.textContent=t("local.preview");}
 }
 async function action(kind:"install"|"cancel"|"toggle",run:()=>Promise<LocalRuntimeStatus|void>){if(busy||disposed||!native||options.preview)return;busy=true;actionKind=kind;displayError=undefined;render();try{const next=await run();if(!disposed&&next)status=next;}catch{if(!disposed){displayError=t(kind==="toggle"?"local.toggleError":kind==="cancel"?"local.cancelError":"local.error");if(kind==="install")status={ready:status?.ready??false,enabled:status?.enabled??false,phase:"error",component:"",downloaded:0,total:status?.total??0,error:"operation-failed"};}}finally{busy=false;actionKind=undefined;if(!disposed)render();}}
 install.onclick=()=>void action("install",()=>bridge.install());cancel.onclick=()=>void action("cancel",async()=>{await bridge.cancelInstall();});toggle.onclick=()=>void action("toggle",()=>bridge.enable(!status?.enabled));
 async function refresh(){if(disposed)return;loading=true;loadFailed=false;displayError=undefined;render();const current=revision;try{const next=await bridge.status();if(!disposed&&current===revision)status=next;}catch{if(!disposed){loadFailed=true;displayError=t("local.statusError");}}finally{loading=false;if(!disposed)render();}}
 retryStatus.onclick=()=>void refresh();
 if(native&&!options.preview){void refresh();void onLocalRuntimeProgress(next=>{if(!disposed){revision++;status=next;displayError=undefined;loadFailed=false;render();}}).then(unlisten=>{if(disposed)unlisten();else stop=unlisten;});}
 render();return{el,dispose(){disposed=true;stop?.();}};
}
