import { beforeEach, describe, expect, it, vi } from "vitest";
import { assistantSettingsSection } from "./assistant-settings";
import { BridgeAssistant, type VoiceStatus } from "../core/bridge-assistant";
import { setLanguage } from "../core/i18n";
vi.mock("../core/bridge",()=>({IS_TAURI:true,onEvent:async()=>()=>{},onRoadeepSession:async()=>()=>{},Bridge:{log:async()=>{},roadeepSession:async()=>({signedIn:false,user:null})}}));
const flush=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
// Native may send either the bare name or the «صدای …» description; both read «صدای X».
const voices=[{id:"marin",label:"رها"},{id:"quartz",label:"صدای آوا"},{id:"cinder",label:"مهراد"}];
const models=["gpt-6-luna","gpt-6-sol"];
const status=(configured:boolean,model="gpt-6-luna",voice="marin"):VoiceStatus=>({configured,model,voice,voices,models});
const setup={computerSetup:async()=>({runtimePath:"C:/Installed/runtime",buildCommand:"docker build -t roadeep-computer:1 'C:/Installed/runtime'"}),memoryGet:async()=>({enabled:true,facts:[],stats:{toolCounts:{},hourHistogram:Array(24).fill(0),weekdayHistogram:Array(7).fill(0),recent:[]}})};
beforeEach(()=>{document.body.replaceChildren();setLanguage("en");});
describe("live conversation settings",()=>{
 it("never prefills credentials, clears the input before dispatch, and shows the bundled build command",async()=>{
  const voiceConfigure=vi.fn(async()=>status(true));
  const section=assistantSettingsSection({...BridgeAssistant,...setup,voiceStatus:async()=>status(true),voiceConfigure});document.body.append(section);await flush();
  const secret=section.querySelector<HTMLInputElement>('input[type="password"]')!;
  expect(secret.value).toBe("");expect(secret.autocomplete).toBe("new-password");
  secret.value="example-private-key";section.querySelector("form")!.dispatchEvent(new Event("submit",{cancelable:true}));
  expect(secret.value).toBe("");expect(voiceConfigure).toHaveBeenCalledWith("gpt-6-luna","marin","example-private-key");await flush();
  expect(section.textContent).not.toContain("example-private-key");expect(section.querySelector("pre")?.textContent).toContain("C:/Installed/runtime");
 });
 it("lists native voices with their Persian names and the allowed backend models",async()=>{
  setLanguage("fa");
  const voiceConfigure=vi.fn(async()=>status(true,"gpt-6-sol","quartz"));
  const section=assistantSettingsSection({...BridgeAssistant,...setup,voiceStatus:async()=>status(true),voiceConfigure});document.body.append(section);await flush();
  const [voice,model]=[...section.querySelectorAll<HTMLSelectElement>("select")];
  expect([...voice.options].map(o=>o.textContent)).toEqual(["صدای رها","صدای آوا","صدای مهراد"]);expect(section.textContent).toContain("نام دستیار همیشه رودیپ است");expect(voice.value).toBe("marin");
  expect([...model.options].map(o=>o.value)).toEqual(models);expect(model.value).toBe("gpt-6-luna");
  voice.value="quartz";model.value="gpt-6-sol";section.querySelector("form")!.dispatchEvent(new Event("submit",{cancelable:true}));
  expect(voiceConfigure).toHaveBeenCalledWith("gpt-6-sol","quartz",undefined);
  expect(section.textContent).toContain("گفتگوی زنده");expect(section.textContent).toContain("۰٫۰۵ دلار");
 });
 it("drops the admin voice enrollment and voice-mode controls",async()=>{
  const section=assistantSettingsSection({...BridgeAssistant,...setup,voiceStatus:async()=>status(false)});document.body.append(section);await flush();
  expect(section.querySelector(".admin-voice-settings,.voice-mode-settings,.voice-proposal-card")).toBeNull();
  expect(section.querySelector(".memory-settings")?.textContent).toContain("Roadeep's memory");
  expect([...section.querySelectorAll("option")].map(o=>o.textContent)).toContain("Voice رها · marin");
  expect(section.textContent).toContain("Live conversation");expect(section.textContent).toContain("$0.05 per minute");expect(section.textContent).toContain("Not set up");
 });
 it("keeps existing key on a blank save and refreshes status after a real key removal",async()=>{
  let configured=true;
  const voiceConfigure=vi.fn(async()=>status(configured));
  const voiceClearKey=vi.fn(async()=>{configured=false;});
  const section=assistantSettingsSection({...BridgeAssistant,...setup,voiceStatus:async()=>status(configured),voiceConfigure,voiceClearKey});document.body.append(section);await flush();
  section.querySelector("form")!.dispatchEvent(new Event("submit",{cancelable:true}));await flush();expect(voiceConfigure).toHaveBeenCalledWith("gpt-6-luna","marin",undefined);
  [...section.querySelectorAll<HTMLButtonElement>("button")].find(b=>b.textContent==="Remove key")!.click();await flush();expect(voiceClearKey).toHaveBeenCalledOnce();expect(section.textContent).toContain("Not set up");
 });
 it("names the pending save and asks for key reentry on failure without exposing it",async()=>{
  let reject:(cause:unknown)=>void=()=>{};
  const section=assistantSettingsSection({...BridgeAssistant,...setup,voiceStatus:async()=>status(false),voiceConfigure:()=>new Promise((_,fail)=>reject=fail)});document.body.append(section);await flush();
  const secret=section.querySelector<HTMLInputElement>('input[type="password"]')!;secret.value="example-secret-private";section.querySelector("form")!.dispatchEvent(new Event("submit",{cancelable:true}));expect(section.textContent).toContain("Saving…");reject(new Error("credential failure"));await flush();
  expect(section.textContent).toContain("Enter it again to retry");expect(section.textContent).not.toContain("example-secret-private");expect(secret.value).toBe("");
 });
 it("explains that settings cannot change during a live conversation",async()=>{
  const section=assistantSettingsSection({...BridgeAssistant,...setup,voiceStatus:async()=>status(true),voiceConfigure:async()=>{throw new Error("End voice before changing settings");}});document.body.append(section);await flush();
  section.querySelector("form")!.dispatchEvent(new Event("submit",{cancelable:true}));await flush();
  expect(section.textContent).toContain("End the live conversation");
 });
});
