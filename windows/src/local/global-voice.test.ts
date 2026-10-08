import {afterEach,beforeEach,describe,expect,it,vi} from "vitest";
import type {LiveSnapshot} from "../voice/live";
import type {ToolHost} from "../voice/tools";
const mocks=vi.hoisted(()=>{
 const idle={phase:"idle",muted:false,transcripts:[],inputLevel:0,outputLevel:0};
 const voice={
  listeners:new Set<(s:any)=>void>(),current:idle as any,host:undefined as any,
  start:vi.fn(async()=>{}),end:vi.fn(async()=>{}),decide:vi.fn(),setApprovalVisible:vi.fn(),
  snapshot(){return voice.current;},
  subscribe(fn:(s:any)=>void){voice.listeners.add(fn);return()=>voice.listeners.delete(fn);},
  emit(next:any){voice.current=next;for(const fn of voice.listeners)fn(next);},
 };
 return{idle,voice,
  state:{settings:{language:"en"},paused:false,mode:"expanded",view:"overview",isPinned:false,pendingApproval:null as unknown,notify:vi.fn(),subscribe:vi.fn(),defaultView:()=>"overview",chatHistory:[]},
  settings:vi.fn(async()=>{}),log:vi.fn(async()=>{}),play:vi.fn(),
 };
});
vi.mock("../core/bridge",()=>({IS_TAURI:true,Bridge:{openSettingsWindow:mocks.settings,log:mocks.log,saveSettingsChecked:vi.fn(async()=>{})}}));
vi.mock("../core/state",()=>({State:mocks.state}));
vi.mock("../core/sound",()=>({Sound:{play:mocks.play}}));
vi.mock("../voice/live",()=>({liveVoice:()=>mocks.voice,setLiveVoiceHost:(host:ToolHost)=>{mocks.voice.host=host;}}));
import {globalVoiceRemembered,REMEMBERED_MS,bindGlobalVoiceChat,bindGlobalVoiceIsland,globalVoiceActive,globalVoiceButton,globalVoiceIndicator,initializeGlobalVoice,decideGlobalVoice} from "./global-voice";
import {setLanguage} from "../core/i18n";
const flush=async()=>{for(let i=0;i<6;i++)await Promise.resolve();};
const snap=(patch:Partial<LiveSnapshot>):LiveSnapshot=>({...mocks.idle,...patch} as LiveSnapshot);
const island={alert:vi.fn(),setView:vi.fn(),dropPin:vi.fn(),reveal:vi.fn()};
beforeEach(()=>{setLanguage("en");bindGlobalVoiceIsland(island);initializeGlobalVoice();mocks.voice.emit(snap({}));vi.clearAllMocks();Object.assign(mocks.state,{paused:false,mode:"expanded",view:"overview",isPinned:false,pendingApproval:null});});
afterEach(()=>{mocks.voice.emit(snap({}));document.body.replaceChildren();vi.useRealTimers();});
describe("header microphone · live conversation",()=>{
 it("starts and ends the single live session from the button",async()=>{
  const button=globalVoiceButton();document.body.append(button);
  expect(button.getAttribute("aria-pressed")).toBe("false");expect(button.getAttribute("aria-label")).toBe("Start live conversation");
  button.click();await flush();expect(mocks.voice.start).toHaveBeenCalledOnce();
  mocks.voice.emit(snap({phase:"listening"}));
  expect(globalVoiceActive()).toBe(true);expect(button.getAttribute("aria-pressed")).toBe("true");expect(button.dataset.phase).toBe("listening");expect(button.title).toContain("Listening");
  button.click();await flush();expect(mocks.voice.end).toHaveBeenCalledOnce();expect(mocks.voice.start).toHaveBeenCalledOnce();
 });
 it("does not start while the app is paused",async()=>{
  mocks.state.paused=true;const button=globalVoiceButton();button.click();await flush();expect(mocks.voice.start).not.toHaveBeenCalled();
 });
 it("drives the ring from the active speaker's level without redrawing the island",()=>{
  const button=globalVoiceButton();
  mocks.voice.emit(snap({phase:"listening",inputLevel:.5,outputLevel:.9}));expect(button.style.getPropertyValue("--voice-level")).toBe("0.500");
  const notifications=mocks.state.notify.mock.calls.length;
  mocks.voice.emit(snap({phase:"listening",inputLevel:.2,outputLevel:.9}));expect(button.style.getPropertyValue("--voice-level")).toBe("0.200");
  expect(mocks.state.notify.mock.calls.length).toBe(notifications);
  mocks.voice.emit(snap({phase:"speaking",inputLevel:.2,outputLevel:.7}));expect(button.style.getPropertyValue("--voice-level")).toBe("0.700");
  mocks.voice.emit(snap({phase:"listening",muted:true,inputLevel:.8}));expect(button.style.getPropertyValue("--voice-level")).toBe("0.000");expect(button.classList.contains("is-muted")).toBe(true);
 });
 it("shows the latest transcript line while talking and the phase otherwise",()=>{
  const indicator=globalVoiceIndicator();const label=indicator.querySelector<HTMLElement>(".global-voice-phase")!;
  expect(label.hidden).toBe(true);
  mocks.voice.emit(snap({phase:"thinking",transcripts:[{id:"u1",speaker:"user",text:"Add a task"}]}));expect(label.hidden).toBe(false);expect(label.textContent).toBe("Thinking…");
  mocks.voice.emit(snap({phase:"speaking",transcripts:[{id:"u1",speaker:"user",text:"Add a task"},{id:"a1",speaker:"assistant",text:"Sure, which day?"}]}));expect(label.textContent).toBe("Sure, which day?");expect(label.dataset.speaker).toBe("assistant");
 });
 it("surfaces a live error and opens settings when no key is configured",()=>{
  const indicator=globalVoiceIndicator();
  mocks.voice.emit(snap({phase:"error",error:"گفتگو شروع نشد (Configure the voice API key in Settings)"}));
  expect(indicator.querySelector(".global-voice-phase")?.textContent).toBe("Live conversation stopped");expect(indicator.title).toContain("OpenAI API key");
  expect(mocks.settings).toHaveBeenCalledWith("assistant");expect(globalVoiceActive()).toBe(false);
 });
 it("never shows live.ts Persian errors untranslated in English",()=>{
  const button=globalVoiceButton();
  mocks.voice.emit(snap({phase:"error",error:"اجازهٔ دسترسی به میکروفون داده نشد."}));expect(button.title).toBe("The live conversation could not connect. Check your microphone, API key and internet connection.");
  setLanguage("fa");mocks.voice.emit(snap({phase:"error",error:"میکروفونی پیدا نشد."}));expect(button.title).toBe("میکروفونی پیدا نشد.");
 });
 it("pins the island on the approval card and lets go when the approval clears",()=>{
  vi.useFakeTimers();
  const approval={id:"ap1",tool:"add_task",summary:"افزودن کار «خرید»",expiresAt:Date.now()+60000};
  mocks.voice.emit(snap({phase:"listening",approval}));
  expect(mocks.state.isPinned).toBe(true);expect(island.alert).toHaveBeenCalledWith("voiceApproval");expect(mocks.play).toHaveBeenCalledWith("approval");
  const before=mocks.state.notify.mock.calls.length;vi.advanceTimersByTime(2000);expect(mocks.state.notify.mock.calls.length).toBeGreaterThan(before);
  mocks.voice.emit(snap({phase:"listening",approval}));expect(island.alert).toHaveBeenCalledOnce();
  mocks.state.view="voiceApproval";mocks.voice.emit(snap({phase:"listening"}));
  expect(mocks.state.isPinned).toBe(false);expect(island.dropPin).toHaveBeenCalledOnce();expect(island.setView).toHaveBeenCalledWith("overview");
  const notifications=mocks.state.notify.mock.calls.length;vi.advanceTimersByTime(3000);expect(mocks.state.notify.mock.calls.length).toBe(notifications);
 });
 it("keeps a Claude Code permission pin when the voice approval clears",()=>{
  mocks.voice.emit(snap({phase:"listening",approval:{id:"ap2",tool:"add_note",summary:"x",expiresAt:Date.now()+60000}}));
  mocks.state.pendingApproval={requestId:"r"};mocks.voice.emit(snap({phase:"listening"}));
  expect(mocks.state.isPinned).toBe(true);expect(island.dropPin).not.toHaveBeenCalled();expect(island.alert).toHaveBeenLastCalledWith("approval");
 });
 it("opens a planner preview on its own page, and queues behind an on-screen coding-agent card",()=>{
  const preview={view:"tasks",kind:"task",fields:[{label:"عنوان",value:"خرید نان"}]};
  mocks.voice.emit(snap({phase:"listening",approval:{id:"pv1",tool:"add_task",summary:"x",expiresAt:Date.now()+60000,preview} as LiveSnapshot["approval"]}));
  expect(island.alert).toHaveBeenCalledWith("tasks");mocks.voice.emit(snap({phase:"listening"}));island.alert.mockClear();
  const indicator=globalVoiceIndicator();
  Object.assign(mocks.state,{pendingApproval:{requestId:"r"},view:"approval"});
  mocks.voice.emit(snap({phase:"listening",approval:{id:"pv2",tool:"add_task",summary:"x",expiresAt:Date.now()+60000,preview} as LiveSnapshot["approval"]}));
  expect(island.alert).not.toHaveBeenCalled();expect(mocks.state.isPinned).toBe(true);
  expect(indicator.querySelector(".global-voice-phase")?.textContent).toBe("Waiting for your approval");
 });
 it("cancels the chat turn when the voice tool's signal aborts",async()=>{
  const host=mocks.voice.host as ToolHost;let finish:(v:string)=>void=()=>{};const stop=vi.fn();
  bindGlobalVoiceChat(()=>new Promise<string>(resolve=>finish=resolve),stop);
  const controller=new AbortController();const pending=host.ask("long question",controller.signal);await flush();
  controller.abort();expect(stop).toHaveBeenCalledOnce();finish("late");await pending;
  await expect(host.ask("q",controller.signal)).rejects.toThrow("voice-ask-cancelled");
  await host.showView("tasks");expect(island.setView).toHaveBeenCalledWith("tasks");await host.showView("week");expect(island.setView).toHaveBeenCalledWith("week");
 });
 it("tells the controller when the approval is really on screen",()=>{
  const card={id:"vis",tool:"update_settings",summary:"x",expiresAt:Date.now()+60000};
  mocks.state.view="overview";mocks.voice.emit(snap({phase:"listening",approval:card}));expect(mocks.voice.setApprovalVisible).not.toHaveBeenCalled();
  mocks.state.view="voiceApproval";mocks.voice.emit(snap({phase:"listening",approval:card,inputLevel:.3}));expect(mocks.voice.setApprovalVisible).toHaveBeenLastCalledWith(true);
  mocks.state.mode="compact";mocks.voice.emit(snap({phase:"listening",approval:card,inputLevel:.4}));expect(mocks.voice.setApprovalVisible).toHaveBeenLastCalledWith(false);
  mocks.state.mode="expanded";mocks.voice.emit(snap({phase:"listening",approval:card,inputLevel:.5}));expect(mocks.voice.setApprovalVisible).toHaveBeenLastCalledWith(true);
  mocks.voice.setApprovalVisible.mockClear();mocks.voice.emit(snap({phase:"listening",approval:{...card,id:"vis2"}}));expect(mocks.voice.setApprovalVisible).toHaveBeenCalledWith(true);
  mocks.voice.emit(snap({phase:"listening"}));expect(mocks.voice.setApprovalVisible).toHaveBeenLastCalledWith(false);
 });
 it("refuses to navigate away while an approval waits, with a sentence the voice can relay",async()=>{
  const host=mocks.voice.host as ToolHost;mocks.state.pendingApproval={requestId:"r"};
  await expect(Promise.resolve().then(()=>host.showView("tasks"))).rejects.toThrow("ابتدا به درخواست تأیید باز پاسخ دهید.");expect(island.setView).not.toHaveBeenCalled();
 });
 it("shows «به خاطر سپردم: …» briefly when the voice remembers something, once per memory",()=>{
  vi.useFakeTimers();setLanguage("fa");
  const note=globalVoiceRemembered();document.body.append(note);expect(note.hidden).toBe(true);
  mocks.voice.emit(snap({phase:"listening",remembered:{text:"جواب‌های کوتاه را ترجیح می‌دهد",at:1}}));
  expect(note.hidden).toBe(false);expect(note.textContent).toBe("به خاطر سپردم: جواب‌های کوتاه را ترجیح می‌دهد");expect(note.getAttribute("role")).toBe("status");
  vi.advanceTimersByTime(REMEMBERED_MS+300);expect(note.hidden).toBe(true);
  mocks.voice.emit(snap({phase:"speaking",remembered:{text:"جواب‌های کوتاه را ترجیح می‌دهد",at:1}}));expect(note.hidden).toBe(true);
  mocks.voice.emit(snap({phase:"speaking",remembered:{text:"صبح‌ها کار می‌کند",at:2}}));expect(note.hidden).toBe(false);
 });
 it("does not pull the island to the chat while an approval card waits",async()=>{
  const host=mocks.voice.host as ToolHost;bindGlobalVoiceChat(vi.fn(async()=>"ok"));
  mocks.state.pendingApproval={requestId:"r"};await host.ask("q");expect(island.setView).not.toHaveBeenCalledWith("prompt");
  mocks.state.pendingApproval=null;mocks.voice.emit(snap({phase:"listening",approval:{id:"ap3",tool:"add_note",summary:"x",expiresAt:Date.now()+60000}}));island.setView.mockClear();
  await host.ask("q");expect(island.setView).not.toHaveBeenCalledWith("prompt");
 });
 it("stops rendering a header button once it leaves the document",async()=>{
  const tick=()=>new Promise(resolve=>setTimeout(resolve,0));
  const button=globalVoiceButton();document.body.append(button);await tick();
  button.remove();await tick();
  mocks.voice.emit(snap({phase:"listening",inputLevel:.5}));
  expect(button.dataset.phase).toBe("idle");expect(button.style.getPropertyValue("--voice-level")).toBe("0.000");
 });
 it("forwards decisions and wires the tool host to the island and chat",async()=>{
  decideGlobalVoice(true,"ap9");expect(mocks.voice.decide).toHaveBeenCalledWith(true,"ap9");
  const host=mocks.voice.host as ToolHost;
  await host.showView("planner");expect(island.setView).toHaveBeenCalledWith("today");
  const send=vi.fn(async()=>"answer");bindGlobalVoiceChat(send);
  await expect(host.ask("what's the weather?")).resolves.toBe("answer");expect(send).toHaveBeenCalledWith("what's the weather?");expect(island.setView).toHaveBeenCalledWith("prompt");
  expect(host.characterOptions()).toEqual({shapes:[],colors:[],expressions:[]});
  await expect(Promise.resolve().then(()=>host.setCharacter({color:"x"}))).rejects.toThrow("voice-character-unavailable");
 });
});
