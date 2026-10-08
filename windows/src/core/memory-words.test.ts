import { describe, expect, it } from "vitest";
import { memoryTriggerWords } from "./memory-words";

describe("memoryTriggerWords", () => {
  it("finds every Persian trigger phrase", () => {
    const cases: [string, string][] = [
      ["این قواعد را حفظ کن.", "حفظ کن"],
      ["لطفاً این قواعد را حفظ کنید.", "حفظ کنید"],
      ["باید این را حفظ کنی", "حفظ کنی"],
      ["این نکته را به خاطر بسپار.", "به خاطر بسپار"],
      ["این نکته را به‌خاطر بسپار.", "به خاطر بسپار"],
      ["یادت باشه که کوتاه جواب بدهی.", "یادت باش"],
      ["یادتان باشد که مؤدب باشید.", "یادتان باش"],
      ["تنظیمات را ذخیره کن", "ذخیره کن"],
      ["فراموش نکن که منبع بدهی.", "فراموش نکن"],
      ["این را به حافظه بسپار", "به حافظه"],
      ["در حافظه نگه دار", "در حافظه"],
    ];
    for (const [text, phrase] of cases) expect(memoryTriggerWords(text), text).toContain(phrase);
  });

  it("tolerates half-spaces, several spaces and Arabic letter forms", () => {
    expect(memoryTriggerWords("حفظ‌کنید")).toEqual(["حفظ کنید"]);
    expect(memoryTriggerWords("به   خاطر\n بسپار")).toEqual(["به خاطر بسپار"]);
    expect(memoryTriggerWords("ذخيره كن")).toEqual(["ذخيره كن"]);
    expect(memoryTriggerWords("یادت‌باشه")).toEqual(["یادت باش"]);
  });

  it("finds the English phrases on word boundaries, any case", () => {
    expect(memoryTriggerWords("Always REMEMBER the user's name.")).toEqual(["REMEMBER"]);
    expect(memoryTriggerWords("Memorize these rules. memorise them.")).toEqual(["Memorize", "memorise"]);
    expect(memoryTriggerWords("Save  this, store this, note this.")).toEqual(["Save this", "store this", "note this"]);
    expect(memoryTriggerWords("Keep this for later and add this to memory.")).toEqual([
      "Keep this for later",
      "add this to memory",
    ]);
  });

  it("lists each phrase once, in order of appearance", () => {
    expect(memoryTriggerWords("Remember this. فراموش نکن. remember that. حفظ کن")).toEqual([
      "Remember",
      "فراموش نکن",
      "حفظ کن",
    ]);
  });

  it("ignores ordinary instructions", () => {
    for (const text of [
      "",
      "You are a concise editor. Answer in short bullet points.",
      "تو ویراستاری دقیق هستی. پاسخ‌ها را کوتاه و فهرست‌وار بنویس.",
      "این قواعد را همیشه نگه دار.",
      "حفظ کنترل گفتگو با توست.",
      "اطلاعات را در ذخیره‌کننده‌ها نگه ندار.",
      "Use the savings account rules. Don't restore this file. A remembered tune; unremembered.",
      "حافظهٔ رم این لپ‌تاپ کم است.",
      "مادر حافظه‌ای قوی دارد.",
    ]) {
      expect(memoryTriggerWords(text), text).toEqual([]);
    }
  });
});
