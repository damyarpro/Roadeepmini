// Voice approval of the live assistant's changes. The model never decides: the
// app reads the user's own input transcript (said after the approval card
// appeared) and the card's buttons. One pending approval at a time; it expires.

export type ApprovalAnswer = "yes" | "no";
export type ApprovalOutcome = "approved" | "rejected" | "expired" | "cancelled";

const DIACRITICS = /[ً-ٰٟۖ-ۭ]/g;
const DIGITS: Record<string, string> = {};
for (let i = 0; i < 10; i++) { DIGITS[String.fromCharCode(0x06F0 + i)] = String(i); DIGITS[String.fromCharCode(0x0660 + i)] = String(i); }

/** Lower-case, one Persian alphabet (ی ک ا ه), no diacritics/tatweel, stretched letters collapsed, ZWNJ and punctuation as spaces, ASCII digits. */
export function normalizeText(text: string): string {
  return text.normalize("NFC").toLowerCase()
    .replace(/[يى]/g, "ی").replace(/ك/g, "ک").replace(/[أإٱآ]/g, "ا").replace(/ؤ/g, "و").replace(/[ئ]/g, "ی").replace(/[ةۀ]/g, "ه")
    .replace(DIACRITICS, "").replace(/ـ/g, "")
    // Stretched speech («آرررره», «نههه»): runs of 3+ letters collapse; ی keeps its legitimate double («تایید»).
    .replace(/(\p{L})\1{2,}/gu, (_, c: string) => c === "ی" ? "یی" : c)
    .replace(/[۰-۹٠-٩]/g, d => DIGITS[d] ?? d)
    .replace(/['’‘`]/g, "")
    .replace(/[​-‏‪-‮⁦-⁩﻿]/g, " ")
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();
}

export const tokens = (text: string): string[] => { const n = normalizeText(text); return n ? n.split(" ") : []; };

type Flag = "alone" | "delete" | "weak" | "neutral" | "question";
interface Phrase { words: string[]; answer: ApprovalAnswer | null; flag?: Flag }
const list = (answer: ApprovalAnswer | null, items: string[], flag?: Flag): Phrase[] =>
  items.map(text => ({ words: tokens(text), answer, ...(flag ? { flag } : {}) }));

// Negations are listed whole, so «انجام نده» wins over «انجام» + «بده» and «چرا که نه» over «نه».
const PHRASES: Phrase[] = [
  ...list("no", [
    "رد", "رد کن", "ردش کن", "رد کنید", "نه", "نخیر", "نچ", "نمیخوام", "نمی خوام", "نمیخواهم", "نمی خواهم", "نمیخواد", "نمی خواد",
    "نکن", "نکنید", "نکنش", "نکنین", "انجام نده", "انجامش نده", "انجام ندید", "انجام ندین", "ثبت نکن", "ثبتش نکن", "پاک نکن", "پاکش نکن",
    "ننویس", "ننویسش", "اضافه نکن", "اضافش نکن", "ذخیره نکن", "ادامه نده", "نرو", "لغو", "لغو کن", "لغوش کن", "کنسل", "کنسل کن", "کنسلش کن",
    "بیخیال", "بی خیال", "ولش کن", "ول کن", "دست نگه دار", "دست نگهدار", "فعلا نه", "بعدا", "اشتباهه", "غلطه", "اشتباه",
    "موافق نیستم", "مخالفم", "قبول ندارم", "قبول نیست", "قبول نمیکنم", "قبول نمی کنم", "تایید نمیکنم", "تایید نمی کنم", "تایید نکن", "تاییدش نکن",
    "درست نیست", "خوب نیست", "نمیشه", "نمی شه", "اصلا نه", "به هیچ وجه", "هرگز",
    "no", "nope", "nah", "cancel", "reject", "stop", "dont", "do not", "dont do it", "do not do it", "not ok", "not okay", "never mind",
    "nevermind", "not now", "no way",
  ]),
  ...list("yes", [
    "تایید", "تاییده", "تایید میکنم", "تایید می کنم", "تاییدش کن", "بله", "بلی", "آره", "اره", "آرع", "ارع", "آری", "باشه", "باشع",
    "اوکی", "اکی", "اوکیه", "اوکی هست", "حتما", "البته", "قبول", "قبوله", "قبول دارم", "موافقم", "موافق هستم", "درسته", "صحیحه", "بفرما", "بفرمایید",
    "بزن بریم", "بریم", "انجام بده", "انجامش بده", "انجام بدید", "انجام بدین", "انجامش بدید", "بکن", "بکنش", "بکنید", "بکنین",
    "ثبت کن", "ثبتش کن", "ثبت بشه", "ثبت کنید", "بنویس", "بنویسش", "اضافه کن", "اضافش کن", "اضافه اش کن", "ذخیره کن", "ذخیرش کن", "ذخیره اش کن",
    "برو جلو", "ادامه بده", "چرا که نه", "چرا نه", "دقیقا", "همینه", "عالیه", "خوبه", "خیلی خوبه", "خیلی خب", "حله", "ردیفه", "اوکی بفرما",
    "yes", "yeah", "yep", "yup", "ok", "okay", "sure", "confirm", "confirmed", "approve", "approved", "do it", "go ahead", "of course",
  ]),
  ...list("yes", ["پاک کن", "پاکش کن", "پاک کنید", "حذف کن", "حذفش کن", "delete it"], "delete"),
  ...list("yes", ["اوهوم", "اهوم", "آهان", "اهان", "اها", "mhm", "uh huh", "برو"], "alone"),
  ...list("no", ["اصلا"], "alone"),
  // Hesitation: a no on its own, but next to a real answer it makes the utterance unclear.
  ...list("no", ["صبر کن", "صبر کنید", "یه لحظه", "یک لحظه", "وایسا", "وایستا", "wait", "hold on", "wait a second", "one moment"], "weak"),
  // A yes root negated right after it.
  ...list("no", ["اوکی نیست", "خوب نیست", "درست نیست", "قبول نیست", "not sure", "not really", "im not sure", "i am not sure", "sure not",
    "not ok", "not okay", "not good", "not right", "not yet"]),
  // Words that look like an answer but are not one.
  ...list(null, ["نه تنها", "نه فقط", "نه اینکه", "رد نکن", "ردش نکن", "تا حالا نه", "no problem", "no worries", "مشکلی نیست"], "neutral"),
  // Unsure or asking back: the whole utterance is no answer.
  ...list(null, ["یا نه", "آره یا نه", "یا خیر", "or not", "نمیدونم", "نمی دونم", "نمیدانم", "نمی دانم", "مطمئن نیستم",
    "dont know", "do not know", "dont stop", "do not stop"], "question"),
].sort((x, y) => y.words.length - x.words.length);

export interface ApprovalContext { action?: "create" | "update" | "delete" }

/**
 * "yes"/"no" when the text clearly answers; null when it has neither, both, or
 * asks back («انجام بده یا نه؟»). Whole words only; negations beat their roots;
 * «پاکش کن» counts only for a delete; «اوهوم» only alone; «صبر کن» only when
 * nothing else was said (with another answer it makes the utterance unclear);
 * a question («درسته؟») is never an answer. The card's button labels «تأیید» and «رد» always work.
 */
const NEGATORS = new Set(["نیست", "نیستم", "نیستش", "not"]);
/** Ends with a question mark (before punctuation is stripped): «درسته؟», "ok?". */
const QUESTION_END = /[؟?][\s"'»”’)\]]*$/u;

export function matchApproval(text: string, context: ApprovalContext = {}): ApprovalAnswer | null {
  if (QUESTION_END.test(text)) return null;
  const words = tokens(text);
  const strong = new Set<ApprovalAnswer>(); const weak = new Set<ApprovalAnswer>();
  for (let i = 0; i < words.length;) {
    const hit = PHRASES.find(({ words: p }) => p.length && p.every((w, k) => words[i + k] === w));
    if (!hit) { i++; continue; }
    const start = i;
    i += hit.words.length;
    // «اوکی نیست», "not sure": a negator right after (or "not" right before) a yes root makes it a no.
    if (hit.answer === "yes" && (NEGATORS.has(words[i]) || words[start - 1] === "not")) {
      if (NEGATORS.has(words[i])) i++;
      strong.add("no"); continue;
    }
    if (hit.flag === "question") return null;
    if (hit.flag === "neutral" || !hit.answer) continue;
    if (hit.flag === "delete" && context.action !== "delete") continue;
    if (hit.flag === "alone") { if (words.every(w => hit.words.includes(w))) strong.add(hit.answer); continue; }
    (hit.flag === "weak" ? weak : strong).add(hit.answer);
  }
  if (weak.size && strong.size) return null;
  const found = strong.size ? strong : weak;
  return found.size === 1 ? [...found][0] : null;
}

export type LivePreviewView = "tasks" | "notes" | "reminders" | "habits" | "focus" | "today";
export type LivePreviewKind = "task" | "note" | "reminder" | "habit" | "focus";
/**
 * What a planner change will look like, for the in-place draft on its page.
 * `action`: create → a draft row; update/delete → highlight the row `targetId`.
 */
export interface LivePreview {
  view: LivePreviewView;
  kind: LivePreviewKind;
  action: "create" | "update" | "delete";
  targetId?: string;
  fields: { label: string; value: string }[];
}
/**
 * `summary`: one short Persian line. `details`: the full pretty-printed arguments
 * (never truncated, ≤16 KB) for tools without a planner `preview`.
 */
export interface LiveApproval { id: string; tool: string; summary: string; expiresAt: number; preview?: LivePreview; details?: string }

export interface ApprovalClock {
  now(): number;
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
  uuid(): string;
}

export const APPROVAL_TTL_MS = 60_000;
/** Voice is decided once the transcript has been quiet this long, so a word split across deltas is whole. */
export const VOICE_SETTLE_MS = 600;
const HEARD_MAX = 600;

interface Pending { card: LiveApproval; resolve(outcome: ApprovalOutcome): void; expiry: unknown; heard: string; settle?: unknown; since: number; visible: boolean; context: ApprovalContext }

/** Contiguous whole-word containment of `needle` in `haystack` (both normalised). */
export function containsWords(haystack: string, needle: string): boolean {
  const n = tokens(needle); const h = tokens(haystack);
  if (!n.length) return false;
  for (let i = 0; i + n.length <= h.length; i++) if (n.every((w, k) => h[i + k] === w)) return true;
  return false;
}

const clonePreview = (p: LivePreview): LivePreview => ({ ...p, fields: p.fields.map(f => ({ ...f })) });
export const cloneCard = (c: LiveApproval): LiveApproval => ({ ...c, ...(c.preview ? { preview: clonePreview(c.preview) } : {}) });

/** At most one pending approval; decided by click, by the user's voice, or by its expiry. */
export class ApprovalBox {
  private current?: Pending;
  /** `echo(utterance)` true → the settled utterance repeats the assistant and is ignored. */
  constructor(private readonly clock: ApprovalClock, private readonly changed: () => void, private readonly ttl = APPROVAL_TTL_MS,
    private readonly echo: (utterance: string) => boolean = () => false) {}

  /** Spoken answers count only while the card is on screen; each new card starts hidden. */
  setVisible(visible: boolean): void {
    const pending = this.current; if (!pending) return;
    pending.visible = visible;
    if (!visible) { pending.heard = ""; if (pending.settle !== undefined) { this.clock.clearTimeout(pending.settle); pending.settle = undefined; } }
  }

  get pending(): LiveApproval | undefined { return this.current ? cloneCard(this.current.card) : undefined; }

  /**
   * Null when another approval is already pending. `since` is the session audio
   * clock (ms) when the card appeared: speech that started earlier never counts.
   */
  request(tool: string, summary: string, since = -Infinity, preview?: LivePreview, details?: string): Promise<ApprovalOutcome> | null {
    if (this.current) return null;
    return new Promise(resolve => {
      const card: LiveApproval = { id: this.clock.uuid(), tool, summary, expiresAt: this.clock.now() + this.ttl, ...(preview ? { preview: clonePreview(preview) } : {}), ...(details !== undefined ? { details } : {}) };
      // «پاکش کن» answers only a delete: the preview says so, or the tool's name does.
      const action = preview?.action ?? (/(^|_)(delete|remove|forget|rm|purge|drop|erase|trash|unlink)(_|$)/i.test(tool) ? "delete" : undefined);
      const pending: Pending = { card, resolve, heard: "", expiry: undefined, since, visible: false, context: action ? { action } : {} };
      pending.expiry = this.clock.setTimeout(() => this.finish(pending, "expired"), this.ttl);
      this.current = pending;
      this.changed();
    });
  }

  /** A click on the card. False when nothing is pending or `id` names another (stale) card. */
  decide(approve: boolean, id?: string): boolean {
    if (!this.current || (id !== undefined && id !== this.current.card.id)) return false;
    this.finish(this.current, approve ? "approved" : "rejected");
    return true;
  }

  /**
   * User transcript said after the card appeared; decided once an utterance
   * settles on a clear answer. Deltas carry their own spacing and are joined as
   * is (a word may be split across them). An unclear utterance is forgotten, so a
   * later clear answer still decides.
   */
  hear(delta: string, startMs?: number): void {
    const pending = this.current;
    if (!pending || !delta || !pending.visible) return;
    if (startMs !== undefined && startMs < pending.since) return;
    pending.heard = (pending.heard + delta).slice(-HEARD_MAX);
    if (pending.settle !== undefined) this.clock.clearTimeout(pending.settle);
    pending.settle = this.clock.setTimeout(() => {
      pending.settle = undefined;
      if (this.current !== pending) return;
      const utterance = pending.heard;
      pending.heard = "";
      const answer = this.echo(utterance) ? null : matchApproval(utterance, pending.context);
      if (answer) this.finish(pending, answer === "yes" ? "approved" : "rejected");
    }, VOICE_SETTLE_MS);
  }

  /** Session ended: the pending card resolves as cancelled. */
  cancel(): void { if (this.current) this.finish(this.current, "cancelled"); }

  private finish(pending: Pending, outcome: ApprovalOutcome): void {
    if (this.current !== pending) return;
    this.current = undefined;
    this.clock.clearTimeout(pending.expiry);
    if (pending.settle !== undefined) this.clock.clearTimeout(pending.settle);
    pending.resolve(outcome);
    this.changed();
  }
}
