// Phrases that make Roadeep's server treat a message as a "save to memory"
// request. A local agent's instructions ride in the first message of a thread,
// so instructions containing one get the server's canned memory reply instead
// of an answer. The settings window warns about them while the user types.

/** Space, tab, newline, ZWNJ (half-space) or none between Persian words. */
const GAP = "[\\s\\u200c]*";
/** Persian and Arabic forms of the letters keyboards mix up. */
const YE = "[یي]";
const KAF = "[کك]";
/** Not followed by another Persian letter: «حفظ کن» but not «حفظ کنترل». */
const END = "(?![\\u0600-\\u06ff])";
/** Not preceded by one either: «در حافظه» but not «مادر حافظه». */
const START = "(?<![\\u0600-\\u06ff])";

const PERSIAN: RegExp[] = [
  // حفظ کن / حفظ کنید / حفظ کنی
  new RegExp(`حفظ${GAP}${KAF}ن(?:${YE}د|${YE})?${END}`, "g"),
  new RegExp(`${START}به${GAP}خاطر${GAP}بسپار(?:${YE}د)?`, "g"),
  new RegExp(`${YE}ادت${GAP}باش`, "g"),
  new RegExp(`${YE}ادتان${GAP}باش`, "g"),
  new RegExp(`ذخ${YE}ره${GAP}${KAF}ن(?:${YE}د|${YE})?${END}`, "g"),
  new RegExp(`فراموش${GAP}ن${KAF}ن(?:${YE}د|${YE})?${END}`, "g"),
  new RegExp(`${START}به${GAP}حافظه`, "g"),
  new RegExp(`${START}در${GAP}حافظه`, "g"),
];

const ENGLISH: RegExp[] = [
  /\bremember\b/gi,
  /\bmemori[sz]e\b/gi,
  /\bsave\s+this\b/gi,
  /\bstore\s+this\b/gi,
  /\bnote\s+this\b/gi,
  /\bkeep\s+this\s+for\s+later\b/gi,
  /\badd\s+this\s+to\s+memory\b/gi,
];

/** One space for any run of spaces or half-spaces, so a phrase reads cleanly in a warning. */
function tidy(match: string): string {
  return match.replace(/[\s‌]+/g, " ").trim();
}

/**
 * The trigger phrases found in `text`, as written there (spacing tidied), each
 * once and in the order they appear.
 */
export function memoryTriggerWords(text: string): string[] {
  const hits: { at: number; phrase: string }[] = [];
  for (const re of [...PERSIAN, ...ENGLISH]) {
    re.lastIndex = 0;
    for (const m of text.matchAll(re)) hits.push({ at: m.index ?? 0, phrase: tidy(m[0]) });
  }
  hits.sort((a, b) => a.at - b.at);
  const seen = new Set<string>();
  const out: string[] = [];
  for (const { phrase } of hits) {
    const key = phrase.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(phrase);
  }
  return out;
}
