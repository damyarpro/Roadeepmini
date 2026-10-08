import {describe,it,expect} from "vitest";
import {inputMeter} from "./input-meter";
import {setLanguage} from "../core/i18n";
describe("measured microphone feedback",()=>{
 it("shows waiting, silence and measured input without time-based animation",()=>{
  setLanguage("en");const meter=inputMeter();const progress=meter.el.querySelector("meter")!;expect(progress.value).toBe(0);expect(progress.min).toBe(0);expect(progress.max).toBe(1);expect(progress.getAttribute("aria-label")).toBe("Microphone input level");expect(meter.el.textContent).toContain("Waiting");
  meter.update({rms:0,peak:0});expect(meter.el.textContent).toContain("quiet");expect(progress.value).toBe(0);
  meter.update({rms:.05,peak:.2});expect(progress.value).toBeCloseTo(.4);expect(meter.el.textContent).toContain("received");expect(meter.el.querySelector(".input-meter-peak")).toBeNull();
  expect(progress.getAttribute("aria-valuetext")).toBe("Input level: 40%");expect(progress.getAttribute("aria-valuetext")).not.toContain("RMS");meter.update();expect(progress.value).toBe(0);
 });
 it("keeps compact feedback accessible without exposing recognition text",()=>{
  setLanguage("fa");const meter=inputMeter(true);meter.update({rms:.25,peak:1});expect(meter.el.classList.contains("compact")).toBe(true);expect(meter.el.querySelector("meter")!.value).toBe(1);expect(meter.el.querySelector("meter")!.getAttribute("aria-valuetext")).toContain("۱۰۰");
 });
});
