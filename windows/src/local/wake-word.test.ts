import {describe,it,expect} from "vitest";
import {parseWakeWord} from "./wake-word";
describe("explicit voice wake prefix",()=>{
 it.each(["رودیپ","رو دیپ","رو‌دیپ","رُودِيپ","رودیپَ","رودیپـ","رودیب","رو دیب","ROADEEP","سلام، رودیپ!"," hello Roadeep "])("accepts %s",text=>expect(parseWakeWord(text)).toEqual({addressed:true,command:""}));
 it("preserves the original request after the normalized prefix",()=>expect(parseWakeWord("سلام، رُودِيپ! یک تسک بذار که نان بخرم")).toEqual({addressed:true,command:"یک تسک بذار که نان بخرم"}));
 it("accepts the pinned recognizer's exact P/B variant while preserving its command",()=>expect(parseWakeWord("رو دیب، برای من یک یادداشت ثبت کن")).toEqual({addressed:true,command:"برای من یک یادداشت ثبت کن"}));
 it.each(["رودیپک","my roadeep task","درباره رودیپ حرف زدم","رو دیپیک","رودیبک","رو دیبیک","درباره رو دیب حرف زدم","باری","باییی","hello world"])("does not guess a wake from %s",text=>expect(parseWakeWord(text)).toEqual({addressed:false,command:text}));
});
