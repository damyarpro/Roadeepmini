import {BridgeAssistant,type MemoryFact,type MemoryView} from "../core/bridge-assistant";
import {Bridge} from "../core/bridge";
import {formatNumber,getLanguage,t} from "../core/i18n";
import {h} from "../views/dom";
import {assistantError} from "../views/assistant-controls/messages";
import "./messages";
import "./style.css";

type MemoryBridge=Pick<typeof BridgeAssistant,"memoryGet"|"memoryDelete"|"memoryClear"|"memorySetEnabled">;
const KNOWN_TOOLS=new Set(["add_task","list_tasks","complete_task","add_note","list_notes","update_note","add_reminder","control_focus","ask_roadeep","log_habit","show_view"]);
const CATEGORIES=["preference","habit","style","fact"] as const;

/** Readable tool names for the usage summary; app tools show their own title part. */
function toolName(name:string){
 if(KNOWN_TOOLS.has(name))return t(`memory.tool.${name}`);
 const app=/^app__[^_]+(?:_[^_]+)*__(.+)$/.exec(name);
 return (app?.[1]??name).replace(/_/g," ");
}
/** The 1–3 busiest hours as ranges («۹ تا ۱۰»), most used first. */
export function usualHours(histogram:readonly number[]):number[]{
 const total=histogram.reduce((sum,n)=>sum+(Number.isFinite(n)?n:0),0);
 if(total<3)return [];
 return histogram.map((count,hour)=>({count,hour})).filter(x=>x.count>0).sort((a,b)=>b.count-a.count||a.hour-b.hour).slice(0,3).map(x=>x.hour);
}
export function topTools(counts:Readonly<Record<string,number>>,limit=3){
 return Object.entries(counts).filter(([,n])=>n>0).sort((a,b)=>b[1]-a[1]||a[0].localeCompare(b[0])).slice(0,limit);
}
const day=(ms:number)=>new Intl.DateTimeFormat(getLanguage()==="fa"?"fa-IR":"en-GB",{dateStyle:"medium"}).format(ms);

/**
 * «حافظهٔ رودیپ»: what the live assistant learned about the user (a local file), with a switch,
 * per-fact delete, clear-all and a usage summary. Never shows anything it was not given by native.
 */
export function memorySettings(options:{native:boolean;bridge?:MemoryBridge;confirm?(text:string):boolean}){
 const bridge=options.bridge??BridgeAssistant;const ask=options.confirm??(text=>window.confirm(text));
 let view:MemoryView|undefined;let busy=false;let disposed=false;
 const toggle=h("input",{type:"checkbox",role:"switch",class:"memory-toggle"});
 const toggleLabel=h("label",{class:"memory-switch"},toggle,h("span",{text:t("memory.enabled")}));
 const status=h("p",{class:"hint",role:"status"});
 const notice=h("p",{class:"notice",role:"alert",hidden:true});
 const list=h("ul",{class:"memory-facts","aria-label":t("memory.factsTitle")});
 const usage=h("div",{class:"memory-usage"});
 const clear=h("button",{type:"button",class:"danger",text:t("memory.clearAll")});
 const el=h("div",{class:"memory-settings",dir:getLanguage()==="fa"?"rtl":"ltr"},
  h("h3",{text:t("memory.title")}),h("p",{class:"hint",text:t("memory.description")}),toggleLabel,status,
  h("h4",{text:t("memory.factsTitle")}),list,h("h4",{text:t("memory.usageTitle")}),usage,
  h("div",{class:"actions"},clear),h("p",{class:"hint memory-privacy",text:t("memory.privacy")}),notice);
 const fail=(error:unknown)=>{if(disposed)return;void Bridge.log("memory: settings action failed");notice.textContent=assistantError(error);notice.hidden=false;};
 function fact(item:MemoryFact){
  const remove=h("button",{type:"button",class:"memory-delete","aria-label":t("memory.delete",{text:item.text}),title:t("memory.delete",{text:item.text}),text:"×"});
  remove.onclick=()=>void act(()=>bridge.memoryDelete(item.id));
  const category=CATEGORIES.includes(item.category as typeof CATEGORIES[number])?item.category:"fact";
  return h("li",{class:"memory-fact"},h("span",{class:`memory-chip is-${category}`,text:t(`memory.category.${category}`)}),
   h("span",{class:"memory-text",dir:"auto",text:item.text}),h("span",{class:"memory-date",text:day(item.updatedAt||item.createdAt)}),remove);
 }
 function render(){
  toggle.disabled=clear.disabled=busy||!options.native||!view;
  for(const button of list.querySelectorAll("button"))button.disabled=busy;
  if(!view){status.textContent=options.native?t("assistant.loading"):t("assistant.native");return;}
  toggle.checked=view.enabled;
  status.textContent=t(view.enabled?"memory.on":"memory.off",{count:formatNumber(view.facts.length)});
  list.replaceChildren(...(view.facts.length?[...view.facts].sort((a,b)=>(b.updatedAt||b.createdAt)-(a.updatedAt||a.createdAt)).map(fact):[h("li",{class:"memory-empty",text:t("memory.empty")})]));
  clear.disabled=clear.disabled||(!view.facts.length&&!view.stats.recent.length);
  const tools=topTools(view.stats.toolCounts);const hours=usualHours(view.stats.hourHistogram);
  usage.replaceChildren(
   h("p",{text:tools.length?t("memory.topTools",{list:tools.map(([name,n])=>`${toolName(name)} (${formatNumber(n)})`).join(getLanguage()==="fa"?"، ":", ")}):t("memory.noUsage")}),
   ...(hours.length?[h("p",{text:t("memory.hours",{list:hours.map(hour=>t("memory.hourRange",{from:formatNumber(hour),to:formatNumber((hour+1)%24)})).join(getLanguage()==="fa"?"، ":", ")})})]:[]),
  );
 }
 async function refresh(){if(!options.native){render();return;}try{const next=await bridge.memoryGet();if(!disposed){view=next;render();}}catch(error){fail(error);}}
 async function act(action:()=>Promise<unknown>){
  if(busy||!options.native)return;busy=true;notice.hidden=true;render();
  try{await action();await refresh();}catch(error){fail(error);}finally{busy=false;if(!disposed)render();}
 }
 toggle.addEventListener("change",()=>{const on=toggle.checked;void act(()=>bridge.memorySetEnabled(on));});
 clear.addEventListener("click",()=>{if(ask(t("memory.clearConfirm")))void act(()=>bridge.memoryClear());});
 render();void refresh();
 return{el,refresh,dispose(){disposed=true;}};
}
