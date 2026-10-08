import {Bridge,IS_TAURI} from "../core/bridge";
import {State,type AdminVoiceMode,type Settings} from "../core/state";
import {h} from "../views/dom";
import {t} from "../core/i18n";
import {helpDisclosure} from "../settings/help";
import "./messages";
export function voiceModeSettings(options:{native?:boolean;settings?():Settings;save?(mode:AdminVoiceMode):Promise<void>}={}){
 const native=options.native??IS_TAURI;const settings=options.settings??(()=>State.settings);let busy=false;
 const notice=h("p",{class:"notice err",role:"status",hidden:true});const mode=()=>settings().adminVoiceMode==="always"?"always":"manual";
 const inputs:HTMLInputElement[]=[];const choices=h("div",{class:"voice-mode-choices"});
 for(const value of ["manual","always"] as const){const input=h("input",{type:"radio",name:`admin-voice-mode-${crypto.randomUUID()}`,value}) as HTMLInputElement;inputs.push(input);const label=h("label",{class:"voice-mode-choice"},input,h("span",{text:t(`adminVoice.mode.${value}`)}));choices.append(label);input.onchange=()=>{if(busy||!input.checked)return;busy=true;notice.hidden=true;render();const save=options.save??(async(next:AdminVoiceMode)=>{const updated={...settings(),adminVoiceMode:next};await Bridge.saveSettingsChecked(updated);State.settings=updated;State.notify();});void save(value).catch(()=>{notice.textContent=t("adminVoice.modeError");notice.hidden=false;}).finally(()=>{busy=false;render();});};}
 const hint=h("p",{class:"hint voice-mode-hint"});const el=h("fieldset",{class:"voice-mode-settings"},h("legend",{},h("span",{text:t("adminVoice.modeTitle")}),helpDisclosure(t("adminVoice.modeHelp"),`${t("settings.help")}: ${t("adminVoice.modeTitle")}`)),choices,hint,notice);
 function render(){for(const input of inputs){input.checked=input.value===mode();input.disabled=busy||!native;input.closest("label")!.classList.toggle("is-selected",input.checked);input.closest("label")!.classList.toggle("is-disabled",input.disabled);}hint.textContent=t(busy?"adminVoice.modeSaving":mode()==="always"?"adminVoice.modeAlwaysHint":"adminVoice.modeManualHint");el.setAttribute("aria-busy",String(busy));}
 // A single group name provides native arrow-key navigation.
 const name=inputs[0].name;for(const input of inputs)input.name=name;render();return el;
}
