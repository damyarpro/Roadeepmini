import {h} from "../views/dom";
import {t} from "../core/i18n";
import type {AudioInputLevel} from "./voice";
import "./messages";
/** Values come only from PCM callbacks; the visual scale saturates at RMS 0.125. */
export function inputMeter(compact=false){
 const fill=h("meter",{min:0,max:1,value:0,"aria-label":t("local.inputLevel")}) as HTMLMeterElement;
 const label=h("span",{class:"input-meter-label",text:t("local.inputLevel")});
 const feedback=h("span",{class:"input-meter-feedback",text:t("local.inputWaiting")});
 const el=h("div",{class:`input-meter${compact?" compact":""}`},label,h("div",{class:"input-meter-track"},fill),feedback);
 function update(level?:AudioInputLevel){
  const rms=Math.max(0,Math.min(1,level?.rms??0));
  fill.value=Math.min(1,rms*8);
  const message=t(!level?"local.inputWaiting":rms<.003?"local.inputQuiet":"local.inputDetected");
  feedback.textContent=message;el.title=message;fill.setAttribute("aria-valuetext",t("local.inputPercent",{value:Math.round(fill.value*100)}));
 }
 update();return{el,update};
}
