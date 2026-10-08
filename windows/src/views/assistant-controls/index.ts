import { h, clear, svg } from "../dom";
import { t, isRtl } from "../../core/i18n";
import { BridgeAssistant, onComputerSnapshot, type ComputerOperation, type ComputerStatus } from "../../core/bridge-assistant";
import { assistantError } from "./messages";
import "./style.css";

type Bridge = typeof BridgeAssistant;
let nextPanelId=0;
export interface AssistantSnapshot { visible: boolean; signedIn: boolean; busy: boolean; agentId: string; identity: string }
export interface AssistantOptions {
 /** Chat hides legacy controls; settings and other explicit surfaces retain them. */
 hideChips?:boolean;
 snapshot(): AssistantSnapshot;
 setup(): void;
 changed(): void;
 bridge?: Bridge;
 preview?: boolean;
}
/** The isolated computer workspace. Voice lives in the header microphone (src/local/global-voice.ts): one live session app-wide. */
export function createAssistantControls(options: AssistantOptions) {
 const panelId=`assistant-${++nextPanelId}`;
 const bridge = options.bridge ?? BridgeAssistant;
 const computerButton = h("button", {type:"button", class:"assistant-chip", "aria-expanded":"false"}, svg("M3 4h18v13H3z M12 17v4 M8 21h8",14,{stroke:1.7}), h("span", {text:t("assistant.computer")}));
 const chips = h("div", {class:"assistant-chips",hidden:options.hideChips===true}, computerButton);
 const heading = h("strong");
 const close = h("button", {type:"button", class:"assistant-close", "aria-label":t("assistant.close"), text:"×"});
 const notice = h("p", {class:"assistant-notice",role:"alert",hidden:true});
 const content = h("div", {class:"assistant-content"});
 const panel = h("section", {class:"assistant-panel",hidden:true,"aria-label":t("assistant.title")}, h("div",{class:"assistant-heading"},heading,close), notice,content);
 const el = h("div", {class:"assistant-controls"}, chips,panel);
 el.dir=isRtl()?"rtl":"ltr";
 let opened: "computer" | null = null;
 let visible = false;
 let identity = "";
 let agentId = "default";
 let generation = 0;
 let disposed = false;
 let pending = false;
 let timer: ReturnType<typeof setTimeout> | undefined;
 let pendingLabel="";
 let computer: ComputerStatus | null = null;
 let activeTab: "browser" | "files" | "terminal" = "browser";
 let screen: string | null = null;
 let screenWidth = 1024;
 let screenHeight = 640;
 let output = "";
 const detailState=new Map<string,boolean>();
 const drafts = {url:"",text:"",key:"Enter",path:"",contents:"",command:"",x:"0",y:"0"};
 let unlisten: (()=>void) | undefined;
 if(!options.preview)void onComputerSnapshot(snapshot=>{
  if(disposed||snapshot.agentId!==agentId||!snapshot.image.startsWith("data:image/png;base64,")||snapshot.image.length>2*1024*1024)return;
  screen=snapshot.image;
  const data=snapshot.data as {width?:number;height?:number}|null;
  screenWidth=data?.width||1024;screenHeight=data?.height||640;
  if(visible&&opened==="computer"){renderComputer();options.changed();}
 }).then(stop=>{if(disposed)stop();else unlisten=stop;});
 const error = (text: string) => {notice.textContent=text;notice.hidden=!text;options.changed();};
 const button = (key:string, action:()=>void, disabled=false, primary=false) => h("button",{type:"button",class:primary?"assistant-primary":"assistant-action",text:t(key),"data-action":key,disabled,onclick:action});
 function field(label:string,value:string,change:(value:string)=>void,multiline=false) {
  const input = multiline ? h("textarea",{rows:3}) : h("input",{type:"text"});
  input.value=value; input.setAttribute("aria-label",t(label)); input.dir=["assistant.url","assistant.path","assistant.command","assistant.key","assistant.coordinateX","assistant.coordinateY"].includes(label)?"ltr":"auto";
  input.dataset.field=label;
  if(input instanceof HTMLInputElement){
   if(label==="assistant.url"){input.type="url";input.maxLength=4096;}
   if(label==="assistant.path")input.maxLength=500;
   if(label==="assistant.coordinateX"||label==="assistant.coordinateY"){input.type="number";input.min="0";input.max=String(label==="assistant.coordinateX"?screenWidth-1:screenHeight-1);}
  }else input.maxLength=label==="assistant.command"?4000:60000;
  input.addEventListener("input",()=>change(input.value));
  input.addEventListener("keydown",event=>event.stopPropagation());
  return h("label",{class:"assistant-field"},h("span",{text:t(label)}),input);
 }
 function stopRefresh() { clearTimeout(timer);timer=undefined; }
 function scheduleRefresh() {
  stopRefresh();
  if (visible && opened === "computer" && !disposed && !options.preview) timer=setTimeout(()=>void refreshComputer(),5000);
 }
 async function refreshComputer() {
  if (!visible || opened !== "computer" || disposed || pending) return;
  stopRefresh();const revision=generation;
  try {
   const next=await bridge.computerStatus(agentId);
   if(revision!==generation||disposed||!visible)return;
   const changed=JSON.stringify(computer)!==JSON.stringify(next);
   computer=next; if(changed){renderComputer();options.changed();}
  } catch(cause){if(revision===generation&&!disposed)error(assistantError(cause));}
  finally { if(revision===generation) scheduleRefresh(); }
 }
 async function mutate(action:()=>Promise<unknown>,label="assistant.working") {
  if(pending||!visible||disposed||options.preview)return;
  stopRefresh();pending=true;pendingLabel=label;error("");renderComputer();const revision=generation;
  try { await action(); }
  catch(cause){if(revision===generation&&!disposed)error(assistantError(cause));}
  finally {if(revision===generation&&!disposed){pending=false;renderComputer();void refreshComputer();}}
 }
 async function operate(operation:ComputerOperation) {
  if(operation.action==="navigate"){
   try{const url=new URL(operation.url);if(!["https:","http:"].includes(url.protocol)||url.username||url.password)throw new Error();}
   catch{error(t("assistant.invalidInput"));return;}
  }
  if(["list","read","write"].includes(operation.action)){
   const path=(operation as {path?:string}).path??"";
   if((!path&&operation.action!=="list")||path.startsWith("/")||/[\\:\u0000-\u001f]/.test(path)||path.split("/").some(part=>part===".."||(!part&&path!==""))){error(t("assistant.invalidInput"));return;}
  }
  if(operation.action==="terminal"&&!operation.command.trim()){error(t("assistant.invalidInput"));return;}
  if(operation.action==="click"&&(!Number.isInteger(operation.x)||!Number.isInteger(operation.y)||operation.x<0||operation.x>=screenWidth||operation.y<0||operation.y>=screenHeight)){error(t("assistant.invalidInput"));return;}
  await mutate(async()=>{
   const revision=generation;
   const result=await bridge.computerOperate(agentId,operation);
   if(revision!==generation||disposed)return;
   if(!result.ok)throw new Error("COMPUTER_OPERATION_FAILED");
   output=typeof result.data==="string"?result.data:JSON.stringify(result.data,null,2);
   if(result.image?.startsWith("data:image/png;base64,")) {
    screen=result.image;
    const data=result.data as {width?:number;height?:number}|null;
    screenWidth=data?.width||1024;screenHeight=data?.height||640;
   }
   if(operation.action==="read"){
    const data=result.data as {text?:string}|null;
    drafts.contents=typeof data?.text==="string"?data.text:output;
   }
  });
 }
 function renderComputer() {
  if(opened!=="computer")return;
  for(const detail of content.querySelectorAll<HTMLDetailsElement>("details[data-detail]")){
   if(detail.dataset.agent===agentId)detailState.set(detail.dataset.detail!,detail.open);
  }
  const focused=document.activeElement;
  const label=focused?.getAttribute("role")==="tab"?`assistant.${activeTab}`:focused?.getAttribute("data-action")||focused?.getAttribute("aria-label")||focused?.textContent;
  const selection=focused instanceof HTMLInputElement||focused instanceof HTMLTextAreaElement?[focused.selectionStart,focused.selectionEnd]:null;
  queueMicrotask(()=>{if(label)for(const node of content.querySelectorAll<HTMLElement>("button,input,textarea")){if((node.getAttribute("data-action")||node.getAttribute("aria-label")||node.textContent)===label){
   const closed=[...content.querySelectorAll<HTMLDetailsElement>("details")].find(detail=>!detail.open&&detail.contains(node)&&!detail.querySelector(":scope > summary")?.contains(node));
   if(closed){closed.querySelector<HTMLElement>(":scope > summary")?.focus();break;}
   if(node.hidden||getComputedStyle(node).display==="none"||(node instanceof HTMLButtonElement&&node.disabled))break;
   node.focus();if(selection&&(node instanceof HTMLTextAreaElement||(node instanceof HTMLInputElement&&node.type==="text")))node.setSelectionRange(selection[0],selection[1]);break;
  }}});
  clear(content);
  if(!computer?.available)content.append(h("p",{class:"assistant-summary",text:t("assistant.computerHint")}));
  const state=computer?.state??"unavailable";
  content.append(h("div",{class:"assistant-status",role:"status",text:t(`assistant.${state}`)}));
  if(pending)content.append(h("p",{class:"assistant-pending",role:"status",text:t(pendingLabel)}));
  if(!computer?.available){
   content.append(h("p",{class:"assistant-summary",text:t("assistant.dockerMissing")}),h("div",{class:"assistant-actions"},button("assistant.setup",options.setup,false,true),button("assistant.refresh",()=>void refreshComputer(),pending)));
   return;
  }
  const running=state==="running";
  content.append(h("div",{class:"assistant-actions"},
   ...(state==="stopped"?[button("assistant.start",()=>void mutate(()=>bridge.computerLifecycle(agentId,"start"),"assistant.starting"),pending,true)]:[]),
   ...(running?[button("assistant.pause",()=>void mutate(()=>bridge.computerLifecycle(agentId,"pause"),"assistant.pausing"),pending)]:[]),
   ...(state==="paused"?[button("assistant.resume",()=>void mutate(()=>bridge.computerLifecycle(agentId,"resume"),"assistant.resuming"),pending,true)]:[]),
   ...(state!=="stopped"?[button("assistant.stop",()=>{if(window.confirm(t("assistant.stopConfirm")))void mutate(()=>bridge.computerLifecycle(agentId,"stop"),"assistant.stopping");},pending)]:[]),
   button("assistant.refresh",()=>void refreshComputer(),pending),
  ));
  const advanced=h("details",{class:"assistant-advanced"},h("summary",{text:t("assistant.reset")}));
  rememberDetail(advanced,"reset");
  advanced.append(button("assistant.reset",()=>{if(window.confirm(t("assistant.resetConfirm")))void mutate(()=>bridge.computerLifecycle(agentId,"reset"),"assistant.resetting");},pending));
  if(!running){content.append(advanced);return;}
  const takeover=!!computer.takeover;
  content.append(h("div",{class:"assistant-control-state"},h("span",{text:t(takeover?"assistant.owner":"assistant.agent")}),button(takeover?"assistant.release":"assistant.take",()=>void mutate(()=>bridge.computerTakeover(agentId,!takeover),takeover?"assistant.releasing":"assistant.taking"),pending,true)));
  const tabs=h("div",{class:"assistant-tabs",role:"tablist","aria-label":t("assistant.computer")});
  for(const tab of ["browser","files","terminal"] as const){
   const b=button(`assistant.${tab}`,()=>{activeTab=tab;renderComputer();options.changed();});
   b.id=`${panelId}-${tab}-tab`;b.setAttribute("role","tab");b.setAttribute("aria-selected",String(tab===activeTab));b.setAttribute("aria-controls",`${panelId}-workspace`);b.tabIndex=tab===activeTab?0:-1;
   b.addEventListener("keydown",event=>{
    const names=["browser","files","terminal"] as const;
    if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;
    event.preventDefault();event.stopPropagation();
    const delta=(event.key==="ArrowRight"?1:-1)*(isRtl()?-1:1);
    activeTab=event.key==="Home"?names[0]:event.key==="End"?names[2]:names[(names.indexOf(activeTab)+delta+3)%3];renderComputer();content.querySelector<HTMLButtonElement>(`#${panelId}-${activeTab}-tab`)?.focus();options.changed();
   });tabs.append(b);
  }
  content.append(tabs);
  const workspace=h("div",{id:`${panelId}-workspace`,role:"tabpanel","aria-labelledby":`${panelId}-${activeTab}-tab`,tabindex:0});
  content.append(workspace);
  const disabled=pending||!takeover;
  if(activeTab==="browser"){
   workspace.append(field("assistant.url",drafts.url,v=>drafts.url=v),h("div",{class:"assistant-actions"},button("assistant.go",()=>void operate({action:"navigate",url:drafts.url}),disabled),button("assistant.screenshot",()=>void operate({action:"screenshot"}),disabled)));
   if(screen){
    const image=h("img",{class:"assistant-screen",src:screen,alt:t("assistant.browser")});
    const screenButton=h("button",{type:"button",class:"assistant-screen-button",disabled,"aria-label":t("assistant.controlHint")},image);
    screenButton.addEventListener("click",event=>{
     if(event.detail===0){
      const controls=workspace.querySelector<HTMLDetailsElement>(".assistant-advanced");
      if(controls)controls.open=true;
      workspace.querySelector<HTMLInputElement>('input[data-field="assistant.coordinateX"]')?.focus();
      options.changed();return;
     }
     const rect=image.getBoundingClientRect();
     const x=Math.round((event.clientX-rect.left)/rect.width*screenWidth);
     const y=Math.round((event.clientY-rect.top)/rect.height*screenHeight);
     if(rect.width&&rect.height)void operate({action:"click",x:Math.max(0,Math.min(screenWidth-1,x)),y:Math.max(0,Math.min(screenHeight-1,y))});
    });workspace.append(screenButton);
   }
   const browserControls=h("details",{class:"assistant-advanced"},h("summary",{text:t("assistant.browserControls")}));
   rememberDetail(browserControls,"browser-controls");
   browserControls.append(field("assistant.type",drafts.text,v=>drafts.text=v),button("assistant.typeAction",()=>void operate({action:"type",text:drafts.text}),disabled),field("assistant.key",drafts.key,v=>drafts.key=v),h("div",{class:"assistant-actions"},button("assistant.sendKey",()=>void operate({action:"key",key:drafts.key}),disabled),button("assistant.scrollUp",()=>void operate({action:"scroll",deltaY:-480}),disabled),button("assistant.scrollDown",()=>void operate({action:"scroll",deltaY:480}),disabled)),h("div",{class:"assistant-coordinate-fields"},field("assistant.coordinateX",drafts.x,v=>drafts.x=v),field("assistant.coordinateY",drafts.y,v=>drafts.y=v)),button("assistant.clickPosition",()=>void operate({action:"click",x:Number(drafts.x),y:Number(drafts.y)}),disabled));
   workspace.append(h("p",{class:"assistant-summary",text:t("assistant.controlHint")}),browserControls);
  }else if(activeTab==="files"){
   workspace.append(field("assistant.path",drafts.path,v=>drafts.path=v),h("div",{class:"assistant-actions"},button("assistant.list",()=>void operate({action:"list",path:drafts.path}),disabled),button("assistant.read",()=>void operate({action:"read",path:drafts.path}),disabled)),field("assistant.contents",drafts.contents,v=>drafts.contents=v,true),button("assistant.write",()=>void operate({action:"write",path:drafts.path,text:drafts.contents}),disabled,true));
  }else{
   workspace.append(field("assistant.command",drafts.command,v=>drafts.command=v,true),button("assistant.run",()=>void operate({action:"terminal",command:drafts.command}),disabled,true));
  }
  if(output){
   const result=h("details",{class:"assistant-output"},h("summary",{text:t("assistant.output")}),h("pre",{dir:"auto",text:output.slice(0,12000)}));
   rememberDetail(result,"output",true);workspace.append(result);
  }
  content.append(advanced);
 }
 function rememberDetail(detail:HTMLDetailsElement,kind:string,defaultOpen=false){
  const key=`${activeTab}:${kind}`;
  detail.dataset.detail=key;detail.dataset.agent=agentId;detail.open=detailState.get(key)??defaultOpen;
  detail.addEventListener("toggle",()=>{if(detail.isConnected&&detail.dataset.agent===agentId)detailState.set(key,detail.open);});
 }
 function toggle(next:typeof opened) {
  opened=opened===next?null:next;
  panel.hidden=!opened;computerButton.setAttribute("aria-expanded",String(opened==="computer"));
  heading.textContent=t("assistant.computer");error("");stopRefresh();
  if(opened==="computer"){renderComputer();void refreshComputer();}
  else computerButton.focus();
  options.changed();
 }
 computerButton.addEventListener("click",()=>toggle("computer"));close.addEventListener("click",()=>toggle(opened));
 panel.addEventListener("keydown",event=>{if(event.key==="Escape"){event.stopPropagation();toggle(opened);}});
 const visibility=()=>{if(document.hidden)stopRefresh();};
 document.addEventListener("visibilitychange",visibility);
 return {
  el,
  sync(){
   if(disposed)return;
   el.dir=isRtl()?"rtl":"ltr";
   const next=options.snapshot();
   const changed=identity!==next.identity;
   if(changed){generation++;pending=false;computer=null;screen=null;output="";detailState.clear();for(const key of Object.keys(drafts) as (keyof typeof drafts)[])drafts[key]=key==="key"?"Enter":["x","y"].includes(key)?"0":"";identity=next.identity;agentId=next.agentId;stopRefresh();}
   const becameVisible=!visible&&next.visible;
   visible=next.visible&&!document.hidden;
   if(!visible)stopRefresh();
   computerButton.disabled=!next.signedIn;
   if(visible&&(becameVisible||changed)&&opened==="computer")void refreshComputer();
  },
  dispose(){disposed=true;generation++;stopRefresh();unlisten?.();document.removeEventListener("visibilitychange",visibility);},
 };
}
