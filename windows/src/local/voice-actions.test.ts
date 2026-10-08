import {describe,it,expect,vi,afterEach} from "vitest";
import {voiceDesktopAction,voiceTaskError} from "./voice-actions";
import {LocalRouteCancelled} from "./routing";
import {setLanguage,t} from "../core/i18n";
const options={fromVoice:true,query:"مای کامپیوتر باز کن",language:"fa",requestId:"voice-test",cancelled:()=>false};
afterEach(()=>setLanguage("en"));
describe("voice desktop routing",()=>{
 it("returns the native handled result without requiring login or a local model",async()=>{
  const reply={handled:true,text:"باز شد",action:"this_pc"};const desktopVoiceTry=vi.fn(async()=>reply);
  expect(await voiceDesktopAction({...options,bridge:{desktopVoiceTry}})).toEqual(reply);
  expect(desktopVoiceTry).toHaveBeenCalledExactlyOnceWith(options.query,"fa","voice-test");
 });
 it("preserves unknown requests for the existing router and never parses frontend keywords",async()=>{
  const desktopVoiceTry=vi.fn(async()=>({handled:false,text:""}));
  expect(await voiceDesktopAction({...options,query:"open This PC and delete files",bridge:{desktopVoiceTry}})).toBeNull();
  expect(desktopVoiceTry).toHaveBeenCalledOnce();
 });
 it("never sends a typed request to the voice executor",async()=>{
  const desktopVoiceTry=vi.fn();expect(await voiceDesktopAction({...options,fromVoice:false,bridge:{desktopVoiceTry}})).toBeNull();expect(desktopVoiceTry).not.toHaveBeenCalled();
 });
 it("does not execute after cancellation or retry a late result",async()=>{
  const desktopVoiceTry=vi.fn();await expect(voiceDesktopAction({...options,cancelled:()=>true,bridge:{desktopVoiceTry}})).rejects.toBeInstanceOf(LocalRouteCancelled);expect(desktopVoiceTry).not.toHaveBeenCalled();
  let cancelled=false;let finish:(value:{handled:boolean;text:string})=>void=()=>{};
  const deferred=vi.fn(()=>new Promise<{handled:boolean;text:string}>(resolve=>finish=resolve));
  const pending=voiceDesktopAction({...options,cancelled:()=>cancelled,bridge:{desktopVoiceTry:deferred}});cancelled=true;finish({handled:true,text:"opened"});await expect(pending).rejects.toBeInstanceOf(LocalRouteCancelled);expect(deferred).toHaveBeenCalledOnce();
 });
 it("preserves launcher errors and reports failures without execution claims or diagnostics",async()=>{
  const error=new Error("desktop-launch-failed");await expect(voiceDesktopAction({...options,bridge:{desktopVoiceTry:async()=>{throw error;}}})).rejects.toBe(error);
  for(const language of ["en","fa"] as const){setLanguage(language);expect(voiceTaskError(error)).toBe(t("local.desktopFailed"));expect(voiceTaskError("voice-sensitive-action-blocked")).toBe(t("local.voiceSensitiveBlocked"));expect(voiceTaskError("private raw stderr")).toBe(t("local.voiceTaskFailed"));}
 });
});
