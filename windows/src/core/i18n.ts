// UI language: string lookup, direction, number display and the fonts.
//
// Both windows (island and settings) import this module separately, so each
// holds its own copy of the current language; the "settings-changed" event is
// what keeps them in step.

import { Bridge } from "./bridge";
import { en } from "./locales/en";
import { fa } from "./locales/fa";

export type Language = "fa" | "en";

export const DEFAULT_LANGUAGE: Language = "fa";

let current: Language = DEFAULT_LANGUAGE;
const reportedMissing = new Set<string>();

/** Strings a module brings along (see registerMessages); the main dictionaries win. */
const extraEn: Record<string, string> = {};
const extraFa: Record<string, string> = {};

/** Adds a module's own strings to both languages. */
export function registerMessages(enMessages: Record<string, string>, faMessages: Record<string, string>) {
  Object.assign(extraEn, enMessages);
  Object.assign(extraFa, faMessages);
}

/** Anything that isn't exactly "en" is Persian — the default the Rust side uses too. */
export function normalizeLanguage(value: unknown): Language {
  return value === "en" ? "en" : "fa";
}

/** Switches the language and tags the document so the CSS can follow (`:root[lang]`). */
export function setLanguage(lang: Language | string) {
  current = normalizeLanguage(lang);
  if (typeof document !== "undefined") document.documentElement.lang = current;
}

export function getLanguage(): Language {
  return current;
}

export function isRtl(): boolean {
  return current === "fa";
}

/** BCP 47 locale for Intl. fa-IR gives Persian digits (and the Persian calendar for dates). */
export function locale(): string {
  return current === "fa" ? "fa-IR" : "en-US";
}

/** Locale for dates: English keeps the system's own date order, as before. */
export function dateLocale(): string | undefined {
  return current === "fa" ? "fa-IR" : undefined;
}

/**
 * Looks a string up in the current language, falling back to English. `{name}`
 * placeholders are filled from `vars`; numbers are formatted for the language,
 * so they show Persian digits in fa.
 */
export function t(key: string, vars?: Record<string, string | number>): string {
  let s = (current === "fa" ? fa[key] ?? extraFa[key] : undefined) ?? en[key] ?? extraEn[key];
  if (s == null) {
    if (!reportedMissing.has(key)) {
      reportedMissing.add(key);
      console.warn(`[roadeep] i18n: missing key "${key}"`);
      void Bridge.log(`i18n missing key ${key}`);
    }
    return key;
  }
  if (vars) {
    s = s.replace(/\{(\w+)\}/g, (match, name: string) => {
      const v = vars[name];
      if (v == null) return match;
      return typeof v === "number" ? formatNumber(v) : v;
    });
  }
  return s;
}

const numberFormats = new Map<string, Intl.NumberFormat>();

/**
 * Display-only number formatting. No grouping by default, so English output
 * matches the plain `String(n)` the views used before.
 */
export function formatNumber(n: number, opts: Intl.NumberFormatOptions = {}): string {
  const key = `${current}|${JSON.stringify(opts)}`;
  let fmt = numberFormats.get(key);
  if (!fmt) {
    fmt = new Intl.NumberFormat(locale(), { useGrouping: false, ...opts });
    numberFormats.set(key, fmt);
  }
  return fmt.format(n);
}

/**
 * Wraps text in first-strong isolates so a Latin path or error message dropped
 * into a Persian sentence (or the reverse) can't reorder its neighbours.
 */
export function isolate(text: string): string {
  return `⁨${text}⁩`;
}

const RTL_CHAR = /[֐-ࣿיִ-﷿ﹰ-﻿]/;
const STRONG_CHAR = /[\p{L}]/u;

/** Direction of the first strong character, or null when there is none (empty, digits only…). */
export function textDirection(text: string): "rtl" | "ltr" | null {
  for (const ch of text) {
    if (RTL_CHAR.test(ch)) return "rtl";
    if (STRONG_CHAR.test(ch)) return "ltr";
  }
  return null;
}

// ── Fonts ─────────────────────────────────────────────────────────────────────

let fontsLoading: Promise<void> | null = null;

/**
 * Loads every Vazirmatn weight. Canvases have no font-display: swap, so they
 * wait on this before drawing text; the DOM awaits it once at boot. Resolves
 * even when loading fails — the system stack then takes over, which is logged.
 */
export function loadFonts(): Promise<void> {
  if (fontsLoading) return fontsLoading;
  if (typeof document === "undefined" || !document.fonts) {
    fontsLoading = Promise.resolve();
    return fontsLoading;
  }
  const loads: Promise<FontFace[]>[] = [];
  for (const weight of [400, 500, 600, 700]) {
    for (const family of ['"Vazirmatn"', '"Vazirmatn Persian"']) {
      loads.push(document.fonts.load(`${weight} 16px ${family}`, "سلام Aa"));
    }
  }
  fontsLoading = Promise.all(loads).then(
    () => undefined,
    (err) => {
      console.warn("[roadeep] Vazirmatn failed to load", err);
      void Bridge.log(`fonts Vazirmatn failed to load: ${String(err)}`);
    },
  );
  return fontsLoading;
}

let canvasFamily: { lang: Language; family: string } | null = null;

/**
 * The font-family list for canvas text. Read from the CSS `--font` variable so
 * the stylesheet stays the one place the stack is declared.
 */
export function canvasFontFamily(): string {
  if (canvasFamily?.lang === current) return canvasFamily.family;
  const fromCss =
    typeof document !== "undefined"
      ? getComputedStyle(document.documentElement).getPropertyValue("--font").trim()
      : "";
  const family = fromCss || '"Vazirmatn", system-ui, "Segoe UI", sans-serif';
  canvasFamily = { lang: current, family };
  return family;
}
