import {describe,it,expect,afterEach} from "vitest";
import {localError} from "./messages";
import {setLanguage,t} from "../core/i18n";
afterEach(()=>setLanguage("en"));
describe("speaker registration feedback",()=>{
 it.each(["en","fa"] as const)("separates registration rejection reasons in %s",language=>{
  setLanguage(language);
  const inconsistent=localError(new Error("speaker-enrollment-inconsistent"));
  const insufficient=localError("speaker-insufficient");
  const short=localError("speaker-enrollment-too-short");
  expect(inconsistent).toBe(t("adminVoice.enrollmentInconsistent"));
  expect(insufficient).toBe(t("adminVoice.enrollmentInsufficient"));
  expect(localError("speaker-no-speech")).toBe(insufficient);
  expect(localError("speaker-enrollment-insufficient")).toBe(insufficient);
  expect(short).toBe(t("adminVoice.enrollmentTooShort"));
  expect(localError("speaker-too-short")).toBe(short);
  expect(new Set([inconsistent,insufficient,short]).size).toBe(3);
  for(const message of [inconsistent,insufficient,short])expect(message).not.toMatch(/speaker-|cosine|embedding/i);
 });
 it("preserves capture, profile and unknown-error feedback",()=>{
  setLanguage("en");
  expect(localError("local-mic-no-data")).toBe(t("local.inputUnavailable"));
  expect(localError(new DOMException("Blocked","NotAllowedError"))).toBe(t("local.micDenied"));
  expect(localError("speaker-profile-missing")).toBe(t("adminVoice.missing"));
  expect(localError("speaker-enrollment-failed")).toBe(t("adminVoice.enrollmentError"));
  expect(localError("untrusted private diagnostic")).toBe(t("local.voiceError"));
 });
});
