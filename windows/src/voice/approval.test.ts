import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApprovalBox, VOICE_SETTLE_MS, containsWords, matchApproval, normalizeText, type ApprovalOutcome } from "./approval";

describe("matchApproval", () => {
  const table: [string, "yes" | "no" | null][] = [
    ["بله", "yes"], ["بله.", "yes"], ["آره", "yes"], ["اره", "yes"], ["باشه", "yes"], ["تایید", "yes"], ["تأیید می‌کنم", "yes"],
    ["انجام بده", "yes"], ["انجامش بده لطفا", "yes"], ["اوکی", "yes"], ["OK", "yes"], ["okay!", "yes"], ["Yes please", "yes"],
    ["sure", "yes"], ["do it", "yes"], ["باشي", null], ["بلي", "yes"], ["بَله", "yes"], ["باشــه", "yes"],
    ["نه", "no"], ["نه!", "no"], ["نخیر", "no"], ["لغو کن", "no"], ["کنسل", "no"], ["انجام نده", "no"], ["انجام‌نده", "no"],
    ["انجامش نده", "no"], ["no", "no"], ["Cancel it", "no"], ["stop", "no"], ["don't do it", "no"], ["do not do it", "no"],
    ["تایید نمی‌کنم", "no"], ["تأیيد نمیکنم", "no"],
    // "نه" must not match inside other words; unrelated words stay null.
    ["نهایتا ببینیم", null], ["نهار خوردم", null], ["بنه", null], ["noted", null], ["okayish", null], ["yesterday", null], ["", null],
    ["سلام چطوری", null], ["anyone", null],
    // Both answers → keep waiting.
    ["بله نه", null], ["نه، انجام بده", null], ["ok no", null], ["باشه ولی نه", null],
    // Formal and colloquial affirmatives.
    ["تأیید", "yes"], ["تائید", "yes"], ["تاییده", "yes"], ["تأیید میکنم", "yes"], ["بلی", "yes"], ["آرع", "yes"], ["آری", "yes"],
    ["باشع", "yes"], ["اکی", "yes"], ["اوکِی", "yes"], ["اوکیه", "yes"], ["حتماً", "yes"], ["حتما حتما", "yes"], ["البته", "yes"],
    ["قبول", "yes"], ["قبوله", "yes"], ["موافقم", "yes"], ["موافقَم", "yes"], ["درسته", "yes"], ["صحیحه", "yes"], ["بفرما", "yes"],
    ["بفرمایید", "yes"], ["بزن بریم", "yes"], ["بریم", "yes"], ["انجام بدید", "yes"], ["انجام بدین", "yes"], ["بکن", "yes"], ["بکنش", "yes"],
    ["بکنید", "yes"], ["ثبت کن", "yes"], ["ثبتش کن", "yes"], ["ثبت بشه", "yes"], ["بنویس", "yes"], ["بنویسش", "yes"], ["اضافه کن", "yes"],
    ["اضافش کن", "yes"], ["اضافه‌اش کن", "yes"], ["ذخیره کن", "yes"], ["برو", "yes"], ["برو جلو", "yes"], ["ادامه بده", "yes"],
    ["چرا که نه", "yes"], ["چرا نه", "yes"], ["دقیقاً", "yes"], ["همینه", "yes"], ["عالیه", "yes"], ["خوبه", "yes"],
    ["آره، انجامش بده", "yes"], ["بله لطفاً همین کار رو بکن", "yes"], ["yeah", "yes"], ["yep", "yes"], ["confirm", "yes"], ["approve", "yes"],
    ["go ahead", "yes"], ["Okay, go ahead!", "yes"],
    // Stretched, typed or transcribed oddly.
    ["آرررره", "yes"], ["باشههههه", "yes"], ["بلهههه", "yes"], ["تاییییید", "yes"], ["نههههه", "no"], ["اوکـــی", "yes"], ["آره‌‌", "yes"],
    ["تأييد", "yes"], ["تایيد", "yes"], ["  بله  ", "yes"], ["بله!!!", "yes"], ["«تأیید»", "yes"],
    // «اوهوم»/«آهان» only alone.
    ["اوهوم", "yes"], ["آهان", "yes"], ["اوهوم اوهوم", "yes"], ["آهان فهمیدم", null], ["اوهوم ولی صبر کن", "no"],
    // Formal and colloquial negatives.
    ["رد", "no"], ["رد کن", "no"], ["ردش کن", "no"], ["نَه", "no"], ["نچ", "no"], ["اصلاً", "no"], ["اصلا نه", "no"], ["نمی‌خوام", "no"],
    ["نمیخوام", "no"], ["نمی‌خواهم", "no"], ["نکن", "no"], ["نکنید", "no"], ["نکنش", "no"], ["انجام ندید", "no"], ["ثبت نکن", "no"],
    ["پاک نکن", "no"], ["پاکش نکن", "no"], ["لغو", "no"], ["لغوش کن", "no"], ["کنسل", "no"], ["کنسلش کن", "no"], ["بیخیال", "no"],
    ["بی‌خیال", "no"], ["ولش کن", "no"], ["دست نگه دار", "no"], ["فعلاً نه", "no"], ["بعداً", "no"], ["اشتباهه", "no"], ["غلطه", "no"],
    ["موافق نیستم", "no"], ["قبول ندارم", "no"], ["تاییدش نکن", "no"], ["ننویس", "no"], ["اضافه نکن", "no"], ["ادامه نده", "no"],
    ["درست نیست", "no"], ["nope", "no"], ["reject", "no"], ["don't", "no"], ["not now", "no"], ["Cancel", "no"], ["نه ولش کن", "no"],
    ["نه، بیخیال", "no"],
    // «صبر کن» rejects only when nothing else answered.
    ["صبر کن", "no"], ["صبر کن، آره بکن", null], ["صبر کن ببینم", "no"],
    // Delete-only words.
    ["پاکش کن", null], ["حذفش کن", null],
    // Tricky.
    ["نه تنها این", null], ["نه فقط این", null], ["انجام بده یا نه؟", null], ["آره یا نه", null], ["رد نکن", null],
    ["تأیید یا رد؟", null], ["تأیید رد", null], ["باشه اصلا", "yes"], ["اصلا یادم نبود", null], ["بریم بعداً", null],
    ["نهال", null], ["رده", null], ["ردیف", null], ["بلیت", null], ["آرامش", null], ["باشگاه", null], ["تاییدیه گرفتم؟", null],
    // Questions are never answers (checked before punctuation is stripped).
    ["درسته؟", null], ["خوبه؟", null], ["آره؟", null], ["ok?", null], ["باشه ?", null], ["تأیید؟»", null], ["بله؟ ", null],
    // A yes root negated.
    ["اوکی نیست", "no"], ["خوب نیست", "no"], ["درست نیست", "no"], ["قبول نیست", "no"], ["not sure", "no"], ["not really", "no"],
    ["I'm not sure", "no"], ["sure not", "no"], ["not ok", "no"], ["not okay", "no"], ["تایید نیست", "no"], ["باشه نیست", "no"],
    // Bare «برو» only alone.
    ["برو", "yes"], ["برو بیرون", null], ["برو گمشو", null], ["برو جلو", "yes"],
    // Hesitation next to an answer → unclear; alone → no.
    ["یه لحظه", "no"], ["یک لحظه", "no"], ["وایسا", "no"], ["wait", "no"], ["hold on", "no"], ["وایسا، آره", null], ["یک لحظه، باشه", null],
    ["hold on, yes", null], ["wait no", null],
    // Unsure, reassurance or double negatives.
    ["no problem", null], ["no worries", null], ["مشکلی نیست", null], ["no worries, do it", "yes"], ["I don't know", null],
    ["نمی‌دونم", null], ["نمیدونم والا", null], ["don't stop", null], ["مطمئن نیستم", null],
    ["۱۲۳", null], ["go", null], ["okey dokey", null], ["خوب", null], ["خیلی خب ولی نه", null], ["خیلی خب", "yes"],
  ];
  it.each(table)("%s → %s", (text, expected) => { expect(matchApproval(text)).toBe(expected); });

  it("has a large table", () => { expect(table.length).toBeGreaterThanOrEqual(120); });

  it("accepts delete-only words for a pending delete", () => {
    for (const text of ["پاکش کن", "حذفش کن", "آره پاکش کن"]) expect(matchApproval(text, { action: "delete" })).toBe("yes");
    expect(matchApproval("پاکش نکن", { action: "delete" })).toBe("no");
    expect(matchApproval("پاکش کن", { action: "create" })).toBeNull();
    expect(matchApproval("پاکش کن؟", { action: "delete" })).toBeNull();
  });

  it("normalises Arabic letters, diacritics, ZWNJ, tatweel and digits", () => {
    expect(normalizeText("كيك‌ها ۱۲٣ ـتـ")).toBe("کیک ها 123 ت");
    expect(normalizeText("تأييد")).toBe(normalizeText("تایید"));
    expect(normalizeText("آرررره")).toBe("اره"); expect(normalizeText("تاییییید")).toBe("تایید"); expect(normalizeText("خانۀ")).toBe("خانه");
  });
});

describe("ApprovalBox", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });
  const box = () => {
    const changed = vi.fn(); let n = 0;
    const b = new ApprovalBox({ now: () => Date.now(), setTimeout: (fn, ms) => setTimeout(fn, ms), clearTimeout: h => clearTimeout(h as number), uuid: () => `a${++n}` }, changed);
    return { b, changed };
  };
  const outcome = (p: Promise<ApprovalOutcome> | null) => { const r: { v?: ApprovalOutcome } = {}; void p?.then(v => { r.v = v; }); return r; };

  it("holds one pending card and refuses a second", async () => {
    const { b, changed } = box();
    const first = outcome(b.request("add_task", "افزودن"));
    expect(b.pending).toMatchObject({ id: "a1", tool: "add_task", summary: "افزودن", expiresAt: Date.now() + 60_000 });
    expect(b.request("delete_task", "حذف")).toBeNull();
    expect(b.decide(true)).toBe(true); await Promise.resolve();
    expect(first.v).toBe("approved"); expect(b.pending).toBeUndefined(); expect(changed).toHaveBeenCalledTimes(2);
    expect(b.decide(true)).toBe(false);
  });

  it("decides by voice after the transcript settles, across split deltas", async () => {
    const { b } = box(); const r = outcome(b.request("t", "s")); b.setVisible(true);
    b.hear("ن"); await vi.advanceTimersByTimeAsync(100); b.hear("هایتا"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS);
    expect(r.v).toBeUndefined(); // «نهایتا» is not «نه»
    b.hear(" نه"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(r.v).toBe("rejected");
  });

  it("expires after 60 s and ignores later answers", async () => {
    const { b } = box(); const r = outcome(b.request("t", "s")); b.setVisible(true);
    await vi.advanceTimersByTimeAsync(59_999); expect(r.v).toBeUndefined();
    await vi.advanceTimersByTimeAsync(1); expect(r.v).toBe("expired");
    b.hear("بله"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(b.pending).toBeUndefined();
  });

  it("counts spoken answers only while the card is visible; clicks always count", async () => {
    const { b } = box(); const r = outcome(b.request("t", "s"));
    b.hear("بله"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(r.v).toBeUndefined();
    b.setVisible(true); b.hear("بل"); b.setVisible(false); b.hear("ه"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(r.v).toBeUndefined();
    b.decide(true); await Promise.resolve(); expect(r.v).toBe("approved");
    const r2 = outcome(b.request("t", "s")); b.setVisible(true); b.hear("بله"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS);
    expect(r2.v).toBe("approved");
  });

  it("ignores an utterance the echo check recognises", async () => {
    const changed = vi.fn(); let n = 0;
    const b = new ApprovalBox({ now: () => Date.now(), setTimeout: (fn, ms) => setTimeout(fn, ms), clearTimeout: h => clearTimeout(h as number), uuid: () => `a${++n}` },
      changed, undefined, u => u.includes("باشه"));
    const r = outcome(b.request("t", "s")); b.setVisible(true);
    b.hear("باشه"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(r.v).toBeUndefined();
    b.hear("بله"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(r.v).toBe("approved");
  });

  it("finds whole-word echoes", () => {
    expect(containsWords("خب، باشه انجامش می‌دم", "باشه")).toBe(true);
    expect(containsWords("نهایتا", "نه")).toBe(false); expect(containsWords("x", "")).toBe(false);
  });

  it("treats delete-like tool names as a delete context", async () => {
    for (const tool of ["app__fs__rm_file", "app__db__purge", "app__db__drop_table", "app__s3__erase", "app__mail__trash", "app__fs__unlink", "delete_note", "forget_about_user"]) {
      const { b } = box(); const r = outcome(b.request(tool, "s")); b.setVisible(true);
      b.hear("پاکش کن"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(r.v, tool).toBe("approved");
    }
    for (const tool of ["app__gh__create_issue", "app__x__dropbox_list", "add_note"]) {
      const { b } = box(); const r = outcome(b.request(tool, "s")); b.setVisible(true);
      b.hear("پاکش کن"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(r.v, tool).toBeUndefined();
    }
  });

  it("ignores a click naming another card", async () => {
    const { b } = box(); const r = outcome(b.request("t", "s")); b.setVisible(true);
    expect(b.decide(true, "other")).toBe(false); expect(b.decide(false, "a1")).toBe(true); await Promise.resolve();
    expect(r.v).toBe("rejected");
  });

  it("ignores speech that started before the card's audio clock", async () => {
    const { b } = box(); const r = outcome(b.request("t", "s", 5000)); b.setVisible(true);
    b.hear("بله", 4900); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(r.v).toBeUndefined();
    b.hear("بله", 5100); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(r.v).toBe("approved");
  });

  it("forgets an unclear utterance so a later clear one decides", async () => {
    const { b } = box(); const r = outcome(b.request("t", "s")); b.setVisible(true);
    b.hear("نه بله"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(r.v).toBeUndefined();
    b.hear("سلام"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(r.v).toBeUndefined();
    b.hear("بله"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS); expect(r.v).toBe("approved");
  });

  it("cancels on session end and ignores speech with nothing pending", async () => {
    const { b } = box(); b.hear("بله"); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS);
    const r = outcome(b.request("t", "s")); b.setVisible(true); await vi.advanceTimersByTimeAsync(VOICE_SETTLE_MS);
    expect(r.v).toBeUndefined(); // words said before the card don't count
    b.cancel(); await Promise.resolve(); expect(r.v).toBe("cancelled");
  });
});
