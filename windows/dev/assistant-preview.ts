import "../src/style.css";
import { h } from "../src/views/dom";
import { setLanguage, t } from "../src/core/i18n";
import { createAssistantControls } from "../src/views/assistant-controls";
import { BridgeAssistant, type ComputerStatus } from "../src/core/bridge-assistant";

const query=new URLSearchParams(location.search);
const language=query.get("lang")==="fa"?"fa":"en";setLanguage(language);document.documentElement.lang=language;document.documentElement.dir=language==="fa"?"rtl":"ltr";
const scene=query.get("scene")??"setup";
const style=h("style",{text:"html{height:auto;min-height:100%;overflow:auto}body{height:auto;overflow:auto;margin:0;background:#202126;font-family:var(--font);color:var(--ink);min-height:100vh;padding:24px;box-sizing:border-box}#preview{width:100%;max-width:640px;margin:40px auto;background:#000;border-radius:20px;padding:20px;box-sizing:border-box;min-width:0}.preview-label{font-size:12px;color:var(--dim);line-height:1.7;margin-bottom:18px}.preview-title{font-size:16px;margin:0 0 8px}.preview-chat{font-size:13px;line-height:1.8;border-bottom:1px solid var(--hairline);padding:12px 0;margin-bottom:16px}@media(max-width:450px){body{padding:12px}#preview{margin:12px auto;padding:16px}}"});document.head.append(style);
const root=document.querySelector<HTMLElement>("#preview")!;
const status:ComputerStatus={available:scene!=="setup",agentId:"default",state:scene==="setup"?"unavailable":scene==="stopped"?"stopped":"running",takeover:scene==="control"};
const controls=createAssistantControls({snapshot:()=>({visible:true,signedIn:true,busy:false,agentId:"default",identity:"preview"}),setup:()=>{document.querySelector(".preview-label")!.textContent=t("assistant.native");},changed:()=>{},preview:true,bridge:{...BridgeAssistant,computerStatus:async()=>status}});
root.append(h("p",{class:"preview-label",text:t("assistant.preview")}),h("h1",{class:"preview-title",text:language==="fa"?"گفت‌وگو با ایجنت":"Chat with your agent"}),h("p",{class:"preview-chat",text:language==="fa"?"محیط کار جدای ایجنت را باز کنید.":"Open your agent’s isolated workspace."}),controls.el);
controls.sync();controls.el.querySelector<HTMLButtonElement>(".assistant-chip")!.click();
window.addEventListener("pagehide",()=>controls.dispose(),{once:true});
