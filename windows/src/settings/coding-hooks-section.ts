import { Bridge, IS_TAURI, type CodingHookStatus } from "../core/bridge";
import { getLanguage } from "../core/i18n";
import { CODING_PROVIDERS } from "../activity/providers";
import { h, clear } from "../views/dom";
import { helpDisclosure, sectionHead, statusBadge } from "./ui";
import "./coding-hooks.css";

const copy = (fa: string, en: string) => getLanguage() === "fa" ? fa : en;
const reasonText = (status: CodingHookStatus) => status.provider === "codex"
  ? copy("بعد از نصب، در Codex بخش /hooks را باز کن و اتصال رودیپ را تأیید کن. اجازهٔ اجرای کارها فقط با انتخاب خودت صادر می‌شود.", "After installation, open /hooks in Codex and trust the Roadeep hooks. Tool permissions still require your choice.")
  : !status.supported ? copy("این ابزار فعلاً اتصال هوک سازگار با ویندوز در رودیپ ندارد.", "This tool currently has no compatible Windows hook adapter in Roadeep.")
  : status.provider === "kiro" ? copy("این اتصال برای Kiro IDE نسخهٔ ۱ یا Kiro CLI نسخهٔ ۳ و جدیدتر است و فقط در پروژهٔ انتخاب‌شده نصب می‌شود.", "Requires Kiro IDE 1.x or CLI 3.x or later. Hooks are installed only in the selected project.")
  : status.provider === "opencode" ? copy("این اتصال برای OpenCode نسخهٔ ۲ است. افزونه فقط در پروژهٔ انتخاب‌شده نصب می‌شود؛ اجازه‌ها در خود OpenCode می‌مانند.", "Requires OpenCode 2. The plugin applies only to the selected project; permissions remain in OpenCode.")
  : status.provider === "vscode" ? copy("اتصال Copilot در VS Code به قابلیت هوک نسخهٔ نصب‌شده وابسته است و فقط در پروژهٔ انتخاب‌شده نصب می‌شود.", "VS Code Copilot hooks require support in your installed version and apply only to the selected project.")
  : status.scope === "project" ? copy("اتصال فقط برای پوشهٔ پروژه‌ای نصب می‌شود که انتخاب می‌کنی. مسیر کامل پوشه را وارد کن.", "Hooks are installed only in the project you choose. Enter its absolute folder path.")
  : status.permission ? copy("رویدادها و درخواست اجازه در اپ نمایش داده می‌شوند. پاسخ به اجازه فقط با کلیک تو ارسال می‌شود.", "Events and permission requests appear in the app. Permission answers require your click.")
  : copy("شروع، ابزارها و پایان کار در اپ نمایش داده می‌شوند. تأیید اجازه در خود ابزار انجام می‌شود.", "Session, tool and completion events appear in the app. Permissions remain in the coding tool.");

const LEGACY_TEXT = () => copy("نیاز به به‌روزرسانی — نسخهٔ قبلی برنامه نصب شده است", "Needs update — installed by the previous version");

export function hookPreviewFixtures(): CodingHookStatus[] {
  return Object.entries(CODING_PROVIDERS).map(([provider,label]) => ({provider,label,supported:!["cline","hermes"].includes(provider),scope:["vscode","kiro","opencode"].includes(provider)?"project":"user",installed:provider==="claude",detected:["claude","codex","vscode"].includes(provider),hookReady:true,settingsPath:provider==="opencode"?"D:/Example/.opencode/plugins/roadeep/index.ts":`C:/Users/Example/.${provider}/hooks.json`,hookPath:"C:/Roadeep/roadeep-hook.exe",events:["SessionStart","PreToolUse","Stop"],permission:["claude","codex"].includes(provider)}));
}

/** One provider per disclosure: connection status stays visible; setup appears on demand. */
export function codingHooksSection(initial?: CodingHookStatus[]): HTMLElement {
  const list = h("div", {class:"card coding-hook-list"});
  const section = h("section", {class:"sec", "aria-labelledby":"sec-coding-hooks-title"},
    sectionHead({id:"sec-coding-hooks-title",icon:"integrations",title:copy("اتصال ابزارهای کدنویسی", "Coding tool connections")}), list);
  const error = h("div", {class:"notice err",role:"alert",hidden:true});
  const retry = h("button",{type:"button",text:copy("تلاش دوباره","Retry"),hidden:true,onclick:()=>void load()});
  section.append(error,retry);
  const draw = (statuses: CodingHookStatus[]) => {
    clear(list);
    for (const status of statuses) list.append(providerRow(status));
  };
  const load = async () => {
    retry.hidden = true; error.hidden = true;
    list.setAttribute("aria-busy","true");
    const loading=h("p",{class:"coding-hook-loading",role:"status",text:copy("خواندن وضعیت اتصال‌ها…","Reading connection status…")});
    if(!list.childElementCount)list.append(loading);
    try { draw(await Bridge.codingHooksStatus()); }
    catch { error.textContent = copy("وضعیت اتصال‌ها خوانده نشد. دوباره تلاش کن.","Connection status could not be read. Try again."); error.hidden=false; retry.hidden=false; }
    finally{loading.remove();list.removeAttribute("aria-busy");}
  };
  if (initial) draw(initial); else if (IS_TAURI) { list.setAttribute("aria-busy","true"); void load().finally(()=>list.removeAttribute("aria-busy")); }
  else draw(hookPreviewFixtures());
  return section;
}

function providerRow(status: CodingHookStatus): HTMLElement {
  const details = h("details", {class:"coding-hook-provider"});
  const name=h("span",{class:"coding-hook-name",dir:"auto",text:status.label});
  const badge=h("span");
  const setBadge=()=>{clear(badge);badge.append(status.legacyRelay&&!status.installed ? statusBadge("warn",LEGACY_TEXT()) : statusBadge(status.installed ? "ok" : status.supported ? "off" : "warn", status.installed ? copy("نصب شده","Installed") : status.supported ? copy("نصب نشده","Not installed") : copy("پشتیبانی نشده","Unsupported")));};setBadge();
  const summary = h("summary", {class:"coding-hook-summary"}, name, badge,h("span",{class:"coding-hook-chevron","aria-hidden":"true",text:"⌄"}));
  const content=h("div",{class:"coding-hook-setup"});
  details.append(summary,content);
  const project=h("input",{id:`hook-project-${status.provider}`,type:"text",dir:"ltr",placeholder:"D:\\Projects\\MyApp",autocomplete:"off"}) as HTMLInputElement;
  const feedback=h("div",{id:`hook-feedback-${status.provider}`,class:"notice",role:"status",hidden:true});
  project.setAttribute("aria-describedby",feedback.id);
  project.addEventListener("input",()=>{project.removeAttribute("aria-invalid");feedback.hidden=true;});
  const notify=(text:string,failed=false)=>{feedback.textContent=text;feedback.className=`notice ${failed?"err":"ok"}`;feedback.hidden=false;};
  const draw=()=>{
    clear(content);
    content.append(h("div",{class:"coding-hook-meta"},h("span",{text:status.scope==="project"?copy("همین پروژه","This project"):copy("حساب ویندوز","Windows account")}),helpDisclosure(reasonText(status),copy(`راهنمای اتصال ${status.label}`,`Help connecting ${status.label}`))));
    if(status.scope!=="project")content.append(h("p",{class:"hint coding-hook-detection",text:status.detected?copy("پیکربندی ابزار پیدا شد","Tool configuration found"):copy("پیکربندی ابزار هنوز پیدا نشده","Tool configuration not found yet")}));
    if(!status.supported){content.append(h("p",{class:"hint",text:reasonText(status)}));return;}
    if(status.scope==="project")content.append(h("label",{for:project.id,text:copy("پوشهٔ پروژه","Project folder")}),project);
    if(status.settingsPath)content.append(h("div",{class:"path",dir:"ltr",text:status.settingsPath}));
    if(status.provider==="codex")content.append(h("p",{class:"hint",text:copy("پس از نصب، اتصال را در /hooks کدکس تأیید کن.","After installation, trust this connection in Codex /hooks.")}));
    if(status.legacyRelay&&!status.installed)content.append(h("div",{class:"notice warn",text:LEGACY_TEXT()}));
    if(!status.hookReady)content.append(h("div",{class:"notice warn",text:copy("فایل اتصال آماده نیست. نسخهٔ کامل اپ را نصب کن.","The hook relay is missing. Install the complete app.")}));
    const install=h("button",{type:"button",class:"primary",text:status.installed||status.legacyRelay?copy("به‌روزرسانی اتصال","Update hooks"):copy("نصب اتصال","Install hooks"),onclick:()=>void preview(false)}) as HTMLButtonElement;
    install.disabled=!status.hookReady;
    const actions=h("div",{class:"actions"},install);
    if(status.installed)actions.append(h("button",{type:"button",class:"danger",text:copy("حذف اتصال","Remove hooks"),onclick:()=>void preview(true)}));
    content.append(actions,feedback);
  };
  let busy=false;
  async function preview(remove:boolean){
    if(busy)return;
    const projectPath=status.scope==="project"?project.value.trim():null;
    if(status.scope==="project"&&!projectPath){notify(copy("مسیر کامل پوشهٔ پروژه را وارد کن.","Enter the absolute project folder path."),true);project.setAttribute("aria-invalid","true");project.focus();return;}
    if(!IS_TAURI){notify(copy("نصب اتصال فقط داخل اپ نصب‌شده انجام می‌شود.","Install hooks from the installed app."),true);return;}
    busy=true;content.setAttribute("aria-busy","true");
    const buttons=[...content.querySelectorAll<HTMLButtonElement>(".actions button")];
    for(const button of buttons)button.disabled=true;
    const progress=h("p",{class:"hint",role:"status",text:copy("آماده‌سازی پیش‌نمایش…","Preparing preview…")});content.append(progress);
    try{
      const result=await Bridge.codingHooksPreview(status.provider,projectPath,remove);
      clear(content);
      const diff=h("pre",{class:"diff coding-hook-diff",dir:"ltr",tabindex:"0",text:result.diff});
      const confirm=h("button",{type:"button",class:remove?"danger":"primary",text:remove?copy("تأیید حذف","Confirm removal"):copy("تأیید نصب","Confirm installation")}) as HTMLButtonElement;
      content.append(h("p",{class:"hint",text:copy("تغییرات زیر را بررسی کن؛ فقط بعد از تأیید نوشته می‌شوند.","Review these changes; they are written only after confirmation.")}),
        h("div",{},h("strong",{text:copy("فایل مقصد","Destination file")}),h("div",{class:"path",dir:"ltr",text:result.settingsPath})),diff,
        h("div",{},h("strong",{text:copy("نسخهٔ پشتیبان","Backup file")}),h("div",{class:"path",dir:"ltr",text:result.backup})));
      const back=h("button",{type:"button",text:copy("برگشت","Back"),onclick:()=>{if(!busy){draw();content.querySelector<HTMLButtonElement>(".actions button")?.focus();}}}) as HTMLButtonElement;
      const saving=h("p",{class:"hint",role:"status",hidden:true,text:copy("ثبت تغییرات اتصال…","Saving hook changes…")});
      confirm.onclick=async()=>{
        if(busy)return;busy=true;confirm.disabled=back.disabled=true;saving.hidden=false;content.setAttribute("aria-busy","true");
        try{status=await Bridge.codingHooksApply(status.provider,result.fingerprint,projectPath,remove);draw();notify(copy("اتصال به‌روز شد. برای بارگذاری هوک‌ها جلسهٔ تازه‌ای در ابزار باز کن.","Hooks updated. Start a new coding session to load them."));setBadge();summary.focus();}
        catch{content.append(h("div",{class:"notice err",role:"alert",text:copy("تغییرات ثبت نشد؛ ممکن است فایل پس از پیش‌نمایش تغییر کرده باشد. برگرد و دوباره بررسی کن.","Changes were not saved; the configuration may have changed. Go back and review it again.")}));confirm.disabled=false;}
        finally{busy=false;back.disabled=false;saving.hidden=true;content.removeAttribute("aria-busy");}
      };
      content.append(saving,h("div",{class:"actions"},confirm,back));confirm.focus();
    }catch{notify(copy("پیش‌نمایش آماده نشد. مسیر پروژه و معتبر بودن فایل تنظیمات را بررسی کن.","Preview could not be prepared. Check the project path and configuration file."),true);}
    finally{busy=false;progress.remove();for(const button of buttons)button.disabled=false;content.removeAttribute("aria-busy");}
  }
  draw();return details;
}
