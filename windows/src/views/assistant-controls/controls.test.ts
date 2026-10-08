import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createAssistantControls, type AssistantSnapshot } from "./index";
import { BridgeAssistant, type ComputerStatus, type ComputerResult } from "../../core/bridge-assistant";
import { setLanguage } from "../../core/i18n";
import { assistantError, voiceError } from "./messages";

const instances:Array<ReturnType<typeof createAssistantControls>>=[];
function setup(status:Partial<ComputerStatus>={}){
 const snapshot:AssistantSnapshot={visible:true,signedIn:true,busy:false,agentId:"default",identity:"default"};
 const computerStatus=vi.fn(async()=>({available:true,state:"running",agentId:snapshot.agentId,takeover:false,...status} as ComputerStatus));
 const computerOperate=vi.fn(async():Promise<ComputerResult>=>({ok:true,data:{text:"safe"}}));
 const controls=createAssistantControls({snapshot:()=>snapshot,setup:vi.fn(),changed:()=>{},bridge:{...BridgeAssistant,computerStatus,computerOperate}});
 instances.push(controls);document.body.append(controls.el);controls.sync();return{controls,snapshot,computerStatus,computerOperate};
}
const flush=async()=>{await Promise.resolve();await Promise.resolve();await Promise.resolve();};
beforeEach(()=>{setLanguage("en");vi.useFakeTimers();});
afterEach(()=>{for(const control of instances.splice(0))control.dispose();document.body.replaceChildren();vi.useRealTimers();});
describe("assistant control lifecycle",()=>{
 it("offers only the computer workspace; voice belongs to the header microphone",()=>{
  const {controls}=setup();
  const chips=controls.el.querySelectorAll<HTMLButtonElement>(".assistant-chip");
  expect(chips).toHaveLength(1);expect(chips[0].textContent).toBe("Computer");
  expect(controls.el.textContent).not.toContain("Start call");
 });
 it("polls computer status only while its panel and chat are visible, preserving unchanged input",async()=>{
  const {controls,snapshot,computerStatus}=setup({takeover:true});
  controls.el.querySelectorAll<HTMLButtonElement>(".assistant-chip")[0].click();await flush();
  const field=controls.el.querySelector<HTMLInputElement>('input[aria-label="Website URL"]')!;field.value="https://example.org";field.dispatchEvent(new Event("input"));field.focus();
  await vi.advanceTimersByTimeAsync(5000);expect(computerStatus).toHaveBeenCalledTimes(2);expect(document.activeElement).toBe(field);
  snapshot.visible=false;controls.sync();await vi.advanceTimersByTimeAsync(15000);expect(computerStatus).toHaveBeenCalledTimes(2);
 });
 it("never dispatches human browser operations without takeover and clears old-agent drafts",async()=>{
  const {controls,snapshot,computerOperate}=setup();controls.el.querySelectorAll<HTMLButtonElement>(".assistant-chip")[0].click();await flush();
  const field=controls.el.querySelector<HTMLInputElement>('input[aria-label="Website URL"]')!;field.value="https://old-agent.example";field.dispatchEvent(new Event("input"));
  const open=[...controls.el.querySelectorAll<HTMLButtonElement>("button")].find(b=>b.textContent==="Open website")!;expect(open.disabled).toBe(true);open.click();expect(computerOperate).not.toHaveBeenCalled();
  snapshot.identity="new";snapshot.agentId="new";controls.sync();await flush();
  expect(controls.el.querySelector<HTMLInputElement>('input[aria-label="Website URL"]')?.value).toBe("");
 });
 it("does not claim native or provider success and localizes actionable failures",()=>{
  expect(assistantError("computer-docker-unavailable")).toContain("Docker Desktop");
  expect(voiceError("NotAllowedError")).toContain("Microphone access");
  expect(voiceError("voice-not-configured")).toContain("OpenAI API key");
  setLanguage("fa");expect(assistantError("computer-image-missing")).toContain("Docker Desktop");expect(voiceError("HTTP 401")).toContain("کلید API");
 });
 it("links tabs to a panel, supports keyboard navigation, and returns focus on Escape",async()=>{
  const {controls}=setup({takeover:true});
  const chip=controls.el.querySelectorAll<HTMLButtonElement>(".assistant-chip")[0];chip.click();await flush();
  const browser=controls.el.querySelector<HTMLButtonElement>('[role="tab"][aria-selected="true"]')!;browser.focus();browser.dispatchEvent(new KeyboardEvent("keydown",{key:"ArrowRight",bubbles:true}));await flush();
  const selected=controls.el.querySelector<HTMLButtonElement>('[role="tab"][aria-selected="true"]')!;expect(selected.textContent).toBe("Files");expect(selected.tabIndex).toBe(0);expect(document.activeElement).toBe(selected);
  expect(controls.el.querySelector('[role="tabpanel"]')?.getAttribute("aria-labelledby")).toBe(selected.id);
  selected.dispatchEvent(new KeyboardEvent("keydown",{key:"Escape",bubbles:true}));expect(document.activeElement).toBe(chip);expect(controls.el.querySelector<HTMLElement>(".assistant-panel")?.hidden).toBe(true);
 });
 it("validates technical inputs before IPC and keeps keyboard coordinate controls available",async()=>{
  const {controls,computerOperate}=setup({takeover:true});controls.el.querySelectorAll<HTMLButtonElement>(".assistant-chip")[0].click();await flush();
  const url=controls.el.querySelector<HTMLInputElement>('input[aria-label="Website URL"]')!;expect(url.dir).toBe("ltr");url.value="javascript:alert(1)";url.dispatchEvent(new Event("input"));
  [...controls.el.querySelectorAll<HTMLButtonElement>("button")].find(b=>b.textContent==="Open website")!.click();await flush();expect(computerOperate).not.toHaveBeenCalled();expect(controls.el.querySelector(".assistant-notice")?.textContent).toContain("Check the website");
  const x=controls.el.querySelector<HTMLInputElement>('input[aria-label="Horizontal position (0–1023)"]')!;x.value="42";x.dispatchEvent(new Event("input"));
  const y=controls.el.querySelector<HTMLInputElement>('input[aria-label="Vertical position (0–639)"]')!;y.value="55";y.dispatchEvent(new Event("input"));
  [...controls.el.querySelectorAll<HTMLButtonElement>("button")].find(b=>b.textContent==="Click position")!.click();await flush();expect(computerOperate).toHaveBeenCalledWith("default",{action:"click",x:42,y:55});
 });
 it("owns its text direction and makes screenshot keyboard activation focus coordinates without dispatch",async()=>{
  setLanguage("fa");const {controls,computerOperate}=setup({takeover:true});setLanguage("fa");controls.sync();expect(controls.el.dir).toBe("rtl");setLanguage("en");controls.sync();expect(controls.el.dir).toBe("ltr");
  controls.el.querySelectorAll<HTMLButtonElement>(".assistant-chip")[0].click();await flush();
  computerOperate.mockResolvedValueOnce({ok:true,data:{width:1024,height:640},image:"data:image/png;base64,AAAA"});
  [...controls.el.querySelectorAll<HTMLButtonElement>("button")].find(b=>b.textContent==="Refresh screen")!.click();await flush();
  const screen=controls.el.querySelector<HTMLButtonElement>(".assistant-screen-button")!;const calls=computerOperate.mock.calls.length;screen.click();
  expect(computerOperate.mock.calls).toHaveLength(calls);expect(document.activeElement).toBe(controls.el.querySelector('input[data-field="assistant.coordinateX"]'));expect(controls.el.querySelector<HTMLDetailsElement>('[role="tabpanel"] .assistant-advanced')?.open).toBe(true);
 });
 it("preserves expanded keyboard controls and visible focus across pending operations, results and tab switches",async()=>{
  const {controls,computerOperate,snapshot}=setup({takeover:true});controls.el.querySelectorAll<HTMLButtonElement>(".assistant-chip")[0].click();await flush();
  const detail=controls.el.querySelector<HTMLDetailsElement>('[role="tabpanel"] details')!;detail.open=true;
  const x=controls.el.querySelector<HTMLInputElement>('input[data-field="assistant.coordinateX"]')!;x.value="42";x.dispatchEvent(new Event("input"));x.focus();
  let finish:(result:ComputerResult)=>void=()=>{};computerOperate.mockImplementationOnce(()=>new Promise(resolve=>finish=resolve));
  [...controls.el.querySelectorAll<HTMLButtonElement>("button")].find(b=>b.textContent==="Click position")!.click();await flush();
  expect(controls.el.querySelector<HTMLDetailsElement>('[role="tabpanel"] details')?.open).toBe(true);expect(document.activeElement?.getAttribute("data-field")).toBe("assistant.coordinateX");
  finish({ok:true,data:"done"});await flush();expect(controls.el.querySelector<HTMLDetailsElement>('[role="tabpanel"] details')?.open).toBe(true);expect(document.activeElement?.getAttribute("data-field")).toBe("assistant.coordinateX");
  controls.el.querySelector<HTMLButtonElement>('[role="tab"][data-action="assistant.files"]')!.click();await flush();controls.el.querySelector<HTMLButtonElement>('[role="tab"][data-action="assistant.browser"]')!.click();await flush();expect(controls.el.querySelector<HTMLDetailsElement>('[role="tabpanel"] details')?.open).toBe(true);
  snapshot.identity="other";snapshot.agentId="other";controls.sync();await flush();expect(controls.el.querySelector<HTMLDetailsElement>('[role="tabpanel"] details')?.open).toBe(false);
 });
});
