import { isRtl, registerMessages, t } from "../core/i18n";
import { h, svg } from "../views/dom";
import "./help.css";
registerMessages({"settings.help":"More information"},{"settings.help":"توضیح بیشتر"});
let nextId = 0;
/** Inline disclosure keeps explanations in reading order without global listeners. */
export function helpDisclosure(text: string, label?: string): HTMLElement {
  const id = `settings-help-${++nextId}`;
  const button = h("button", {type:"button",class:"settings-help-button","aria-label":label ?? t("settings.help"),"aria-expanded":"false","aria-controls":id},svg("M12 2a10 10 0 1 0 0 20a10 10 0 1 0 0-20z M12 7v6 M12 17h.01",18,{stroke:1.75}));
  const content = h("span", {id,class:"settings-help-content",hidden:true,dir:isRtl()?"rtl":"ltr",text});
  const root = h("span", {class:"settings-help"},button,content);
  const setOpen = (open: boolean) => {button.setAttribute("aria-expanded",String(open));content.hidden=!open;root.classList.toggle("is-open",open);};
  button.addEventListener("click",()=>setOpen(content.hidden));
  root.addEventListener("keydown",event=>{if(event.key==="Escape"&&!content.hidden){event.preventDefault();event.stopPropagation();setOpen(false);button.focus();}});
  return root;
}
