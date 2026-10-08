import type { RoadeepModel } from "../core/bridge";
import { isRtl, registerMessages, t } from "../core/i18n";
import { h, svg } from "./dom";
import "./chat-models.css";

registerMessages({
  "chat.modelLabel": "Model for new chats",
  "chat.modelCurrent": "This chat: {name}",
  "chat.modelUnknown": "Unknown model",
  "chat.modelDefault": "Automatic",
  "chat.modelDefaultNamed": "Automatic · {name}",
  "chat.modelLoading": "Loading models…",
  "chat.modelEmpty": "No text models available",
  "chat.modelFailed": "Models unavailable",
  "chat.modelRetry": "Retry loading models",
  "chat.modelUnavailable": "{name} · unavailable",
  "chat.modelAgent": "Model set by agent",
  "chat.modelNewChat": "Changing the model starts a new chat. Your previous chat stays in history.",
  "chat.modelSearch": "Search models",
  "chat.modelNoResults": "No matching models",
  "chat.modelHelp": "Help with choosing a model",
  "chat.modelSaved": "Model changed. Ready for a new chat.",
  "chat.modelSaving": "Changing model…",
  "chat.modelSaveFailed": "Could not change the model. Try again.",
}, {
  "chat.modelLabel": "مدل گفت‌وگوی تازه",
  "chat.modelCurrent": "این گفت‌وگو: {name}",
  "chat.modelUnknown": "مدل نامشخص",
  "chat.modelDefault": "خودکار",
  "chat.modelDefaultNamed": "خودکار · {name}",
  "chat.modelLoading": "در حال دریافت مدل‌ها…",
  "chat.modelEmpty": "مدل متنی در دسترس نیست",
  "chat.modelFailed": "مدل‌ها در دسترس نیستند",
  "chat.modelRetry": "دریافت دوباره مدل‌ها",
  "chat.modelUnavailable": "{name} · در دسترس نیست",
  "chat.modelAgent": "مدل توسط دستیار تعیین شده",
  "chat.modelNewChat": "تغییر مدل، گفت‌وگوی تازه‌ای آغاز می‌کند. گفت‌وگوی قبلی در تاریخچه می‌ماند.",
  "chat.modelSearch": "جست‌وجوی مدل‌ها",
  "chat.modelNoResults": "مدلی پیدا نشد",
  "chat.modelHelp": "راهنمای انتخاب مدل",
  "chat.modelSaved": "مدل تغییر کرد. آمادهٔ گفت‌وگوی تازه.",
  "chat.modelSaving": "در حال تغییر مدل…",
  "chat.modelSaveFailed": "تغییر مدل انجام نشد. دوباره تلاش کنید.",
});

export interface ChatModelSnapshot {
  signedIn: boolean;
  busy: boolean;
  selected: string;
  pinnedModel: string | null;
  currentModel?: string | null;
  hasThread?: boolean;
}

export interface ChatModelHost {
  snapshot(): ChatModelSnapshot;
  load(): Promise<RoadeepModel[]>;
  change(model: string): Promise<void>;
  log(message: string): void;
  busyChanged?(): void;
}

let nextPickerId=0;

/** The backend supplies the account's selectable text catalogue; names are display text only. */
export function createChatModelPicker(host: ChatModelHost) {
  const id=`chat-model-popover-${++nextPickerId}`;
  const label = h("span");
  const select = h("button", { type: "button", class: "chat-model-pick", "aria-haspopup": "dialog", "aria-expanded": "false", "aria-controls":id }, label, svg("M6 9l6 6 6-6", 12, {stroke:1.7}));
  const retry = h("button", { type: "button", class: "chat-model-retry", text: "↻" });
  const status = h("span", { class: "chat-model-status", role: "status", "aria-live": "polite" });
  const currentLabel = h("span", { class: "chat-model-current" });
  const hint = h("div", { class: "chat-model-hint" }, currentLabel, status);
  const search = h("input", { type:"search", class:"chat-model-search", autocomplete:"off" });
  const list = h("div", { class:"chat-model-options", role:"listbox" });
  const help = h("button", { type:"button", class:"chat-model-help", "aria-expanded":"false", "aria-controls":`${id}-help` });
  help.append(svg("M12 8v5 M12 16h.01 M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0",18,{stroke:1.7}));
  const explanation = h("p", { id:`${id}-help`, class:"chat-model-explanation", hidden:true });
  const popover = h("div", { class:"chat-model-popover", id, role:"dialog", hidden:true }, h("div",{class:"chat-model-search-row"},search,help), explanation,list);
  const el = h("div", { class: "chat-model-control" }, select, retry);
  let open = false;
  let disposed = false;
  const options = () => [...list.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
  function close(focus = false) {
    open = false; popover.hidden = true; popover.remove(); select.setAttribute("aria-expanded","false");
    document.removeEventListener("pointerdown", outside);
    window.removeEventListener("resize", resized);
    if(focus)select.focus();
  }
  function outside(event:Event) { if(!el.contains(event.target as Node)&&!popover.contains(event.target as Node))close(); }
  function resized(){close();}
  function show() {
    if(select.disabled||disposed)return;
    open=true;search.value="";renderOptions();popover.hidden=false;document.body.append(popover);
    const rect=select.getBoundingClientRect();
    const width=Math.min(300,Math.max(220,window.innerWidth-24));
    popover.style.width=`${width}px`;
    popover.style.left=`${Math.max(12,Math.min(isRtl()?rect.right-width:rect.left,window.innerWidth-width-12))}px`;
    popover.style.top=`${rect.bottom+6}px`;
    popover.style.maxHeight=`${Math.max(140,window.innerHeight-rect.bottom-18)}px`;
    select.setAttribute("aria-expanded","true"); search.focus();
    document.addEventListener("pointerdown",outside);window.addEventListener("resize",resized);
  }
  function renderOptions() {
    const s=host.snapshot(); const chosen=s.pinnedModel??(s.hasThread&&s.currentModel?s.currentModel:s.selected);
    const fallback=models?.find(m=>m.isDefault);
    const entries=[{id:"",displayName:fallback?t("chat.modelDefaultNamed",{name:fallback.displayName||fallback.id}):t("chat.modelDefault"),provider:null},...(models??[])];
    const needle=search.value.trim().toLocaleLowerCase();
    list.replaceChildren();
    for(const model of entries){
      const name=model.displayName||model.id;
      if(needle&&!`${name} ${model.id} ${model.provider??""}`.toLocaleLowerCase().includes(needle))continue;
      const option=h("button",{type:"button",class:"chat-model-option",role:"option","aria-selected":String(model.id===chosen),"data-model-id":model.id},h("span",{text:name}),h("span",{class:"chat-model-check",text:model.id===chosen?"✓":""}));
      option.addEventListener("click",event=>commit(model.id,event.detail===0));list.append(option);
    }
    if(!list.childElementCount)list.append(h("p",{class:"chat-model-empty",role:"status",text:t("chat.modelNoResults")}));
  }
  search.addEventListener("input",renderOptions);
  popover.addEventListener("keydown",event=>{
    event.stopPropagation();
    if(event.key==="Escape"){event.preventDefault();close(true);return;}
    const rows=options();const index=rows.indexOf(document.activeElement as HTMLButtonElement);
    if(event.key==="ArrowDown"||event.key==="ArrowUp"){
      event.preventDefault();
      const next=index<0?(event.key==="ArrowDown"?0:rows.length-1):(index+(event.key==="ArrowDown"?1:-1)+rows.length)%rows.length;
      rows[next]?.focus();
    }
    if(event.key==="Enter"&&document.activeElement===search){event.preventDefault();rows[0]?.click();}
    if(event.key==="Tab"){
      const focusable=[search,help,...rows];const at=focusable.indexOf(document.activeElement as HTMLInputElement);
      if(event.shiftKey&&at===0){event.preventDefault();focusable.at(-1)?.focus();}
      else if(!event.shiftKey&&at===focusable.length-1){event.preventDefault();search.focus();}
    }
  });
  help.addEventListener("click",()=>{explanation.hidden=!explanation.hidden;help.setAttribute("aria-expanded",String(!explanation.hidden));});
  select.addEventListener("click",()=>open?close():show());
  select.addEventListener("keydown",event=>{event.stopPropagation();if(event.key==="ArrowDown"){event.preventDefault();show();}});
  let models: RoadeepModel[] | null = null;
  let loading = false;
  let failed = false;
  let saving = false;
  let authenticated = false;
  let epoch = 0;
  let renderKey = "";
  let feedback: "catalog" | "save" | null = null;

  function render() {
    const s = host.snapshot();
    el.hidden = !s.signedIn;
    hint.hidden = !s.signedIn;
    const selected = s.pinnedModel ?? (s.hasThread && s.currentModel ? s.currentModel : s.selected);
    const key = JSON.stringify([isRtl(), selected, s.pinnedModel, s.currentModel, s.hasThread, s.busy, loading, failed, saving, models]);
    if (key === renderKey) return;
    renderKey = key;
    currentLabel.hidden = !s.hasThread;
    const current = models?.find((model) => model.id === s.currentModel);
    currentLabel.textContent = s.hasThread ? t("chat.modelCurrent", { name: current?.displayName || s.currentModel || t("chat.modelUnknown") }) : "";
    const unavailable = loading ? "chat.modelLoading" : failed ? "chat.modelFailed" : !models?.length ? "chat.modelEmpty" : null;
    const currentModel=models?.find(m=>m.id===selected);
    const fallback=models?.find(m=>m.isDefault);
    label.textContent=s.hasThread&&!s.currentModel&&!s.pinnedModel?t("chat.modelUnknown"):
      selected?(currentModel?.displayName||selected):fallback?t("chat.modelDefaultNamed",{name:fallback.displayName||fallback.id}):t("chat.modelDefault");
    if(unavailable&&!selected)label.textContent=t(unavailable);
    select.dataset.modelId=selected;
    select.dir = isRtl() ? "rtl" : "ltr";popover.dir=select.dir;
    select.disabled = s.busy || saving || !!s.pinnedModel || !!unavailable;
    select.title = t(s.pinnedModel ? "chat.modelAgent" : "chat.modelLabel");
    select.setAttribute("aria-label", `${t("chat.modelLabel")} · ${label.textContent}`);
    search.placeholder=t("chat.modelSearch");search.setAttribute("aria-label",search.placeholder);
    popover.setAttribute("aria-label",t("chat.modelLabel"));list.setAttribute("aria-label",t("chat.modelLabel"));
    help.setAttribute("aria-label",t("chat.modelHelp"));explanation.textContent=t("chat.modelNewChat");
    if(select.disabled||!s.signedIn)close();else if(open)renderOptions();
    retry.hidden = !failed;
    retry.disabled = s.busy || saving || loading;
    retry.title = t("chat.modelRetry");
    retry.setAttribute("aria-label", retry.title);
    el.setAttribute("aria-busy", String(loading || saving));
  }

  async function load() {
    if (loading || !host.snapshot().signedIn) return;
    const generation = ++epoch;
    loading = true;
    failed = false;
    if (feedback === "catalog") { status.textContent = ""; feedback = null; }
    render();
    try {
      const result = await host.load();
      if (generation !== epoch || !host.snapshot().signedIn) return;
      models = result;
    } catch {
      if (generation !== epoch || !host.snapshot().signedIn) return;
      failed = true;
      host.log("chat: model catalogue unavailable");
      feedback = "catalog";
      status.textContent = t("chat.modelFailed");
    } finally {
      if (generation === epoch) {
        loading = false;
        render();
      }
    }
  }

  function commit(next:string,restoreKeyboardFocus=false) {
    const s = host.snapshot();
    if(!s.hasThread&&next===s.selected){close(true);return;}
    if (!s.signedIn || s.busy || saving || s.pinnedModel || !models?.length || failed || loading ||
      (!s.hasThread && next === s.selected) || (next && !models.some((m) => m.id === next))) {
      renderKey = "";
      render();
      return;
    }
    close(true);
    saving = true;
    host.busyChanged?.();
    status.textContent = t("chat.modelSaving");
    feedback = "save";
    render();
    void host.change(next).then(() => {
      status.textContent = t("chat.modelSaved");
    }, () => {
      host.log("chat: model change failed");
      status.textContent = t("chat.modelSaveFailed");
    }).finally(() => {
      saving = false;
      host.busyChanged?.();
      renderKey = "";
      render();
      if(restoreKeyboardFocus&&!disposed&&!select.disabled&&(document.activeElement===document.body||document.activeElement===select))select.focus();
    });
  }
  retry.addEventListener("click", () => void load());

  return {
    el,
    hint,
    get busy() { return saving; },
    dispose(){disposed=true;epoch++;close();},
    sync() {
      const s = host.snapshot();
      if (!s.signedIn && authenticated) {
        epoch++;
        models = null;
        failed = loading = false;
        status.textContent = "";
        feedback = null;
      }
      const first = s.signedIn && !authenticated;
      authenticated = s.signedIn;
      if (first) void load();
      render();
    },
  };
}
