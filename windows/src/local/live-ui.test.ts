import {beforeEach,describe,expect,it,vi} from "vitest";
import {voiceApprovalCard,secondsLeft,liveLevel,heardSince,IDLE_SNAPSHOT} from "./live-ui";
import {setLanguage} from "../core/i18n";
vi.mock("../core/bridge",()=>({IS_TAURI:false,Bridge:{log:async()=>{}}}));
beforeEach(()=>setLanguage("fa"));
describe("voice approval card",()=>{
 const approval={id:"ap1",tool:"add_task",summary:"افزودن کار «خرید نان» برای فردا",expiresAt:100_000};
 it("shows the Persian summary, a countdown, the voice hint and the user's latest words",()=>{
  const card=voiceApprovalCard(()=>{});
  card.update({...IDLE_SNAPSHOT,phase:"listening",approval,transcripts:[{id:"u0",speaker:"user",text:"یه کار اضافه کن"}]},58_000);
  expect(card.el.querySelector<HTMLElement>(".voice-approval-heard")?.hidden).toBe(true);
  card.update({...IDLE_SNAPSHOT,phase:"listening",approval,transcripts:[{id:"u0",speaker:"user",text:"یه کار اضافه کن"},{id:"u1",speaker:"user",text:"آره انجامش بده"},{id:"a1",speaker:"assistant",text:"تأیید می‌کنید؟"}]},58_200);
  expect(card.el.textContent).toContain("افزودن کار «خرید نان» برای فردا");
  expect(card.el.querySelector(".voice-approval-countdown")?.textContent).toBe("۴۲ ثانیه");
  expect(card.el.textContent).toContain("بگویید «تأیید» یا «رد»");
  expect(card.el.querySelector(".voice-approval-heard")?.textContent).toBe("شنیدم: آره انجامش بده");
  expect([...card.el.querySelectorAll("button")].map(b=>b.textContent)).toEqual(["رد","تأیید"]);
 });
 it("shows the full arguments in a scrollable, focusable LTR block when given",()=>{
  const card=voiceApprovalCard(()=>{});const block=card.el.querySelector<HTMLElement>(".voice-approval-details")!;
  card.update({...IDLE_SNAPSHOT,approval},0);expect(block.hidden).toBe(true);
  const details=JSON.stringify({server:"github",title:"x".repeat(500)},null,2);
  card.update({...IDLE_SNAPSHOT,approval:{...approval,details}},0);
  expect(block.hidden).toBe(false);expect(block.textContent).toBe(details);expect(block.dir).toBe("ltr");expect(block.tabIndex).toBe(0);
 });
 it("forwards clicks only while an approval is pending",()=>{
  const decide=vi.fn();const card=voiceApprovalCard(decide);const [reject,approve]=[...card.el.querySelectorAll<HTMLButtonElement>("button")];
  card.update({...IDLE_SNAPSHOT,approval},0);approve.click();reject.click();expect(decide.mock.calls).toEqual([[true,"ap1"],[false,"ap1"]]);
  card.update({...IDLE_SNAPSHOT},0);expect(approve.disabled).toBe(true);approve.click();expect(decide).toHaveBeenCalledTimes(2);
 });
 it("counts only speech heard after the card appeared, including the new part of a growing line",()=>{
  const before=new Map([["u1","یه کار"],["a1","باشه"]]);
  expect(heardSince({...IDLE_SNAPSHOT,transcripts:[{id:"u1",speaker:"user",text:"یه کار"}]},before)).toBe("");
  expect(heardSince({...IDLE_SNAPSHOT,transcripts:[{id:"u1",speaker:"user",text:"یه کار اضافه کن. بله"}]},before)).toBe("اضافه کن. بله");
  expect(heardSince({...IDLE_SNAPSHOT,transcripts:[{id:"u1",speaker:"user",text:"یه کار"},{id:"u2",speaker:"user",text:"نه"},{id:"a2",speaker:"assistant",text:"باشه"}]},before)).toBe("نه");
 });
 it("never counts below zero and clamps levels",()=>{
  expect(secondsLeft(1000,5000)).toBe(0);expect(secondsLeft(5001,1000)).toBe(5);
  expect(liveLevel({...IDLE_SNAPSHOT,phase:"listening",inputLevel:3})).toBe(1);expect(liveLevel({...IDLE_SNAPSHOT,phase:"thinking",inputLevel:.5})).toBe(0);expect(liveLevel({...IDLE_SNAPSHOT,phase:"speaking",outputLevel:Number.NaN})).toBe(0);
 });
});
