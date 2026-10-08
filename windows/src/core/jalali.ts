// Solar Hijri (Jalali) calendar and the planner's day keys.
//
// Stored dates are Gregorian local day keys ("YYYY-MM-DD") so they sort and
// compare as plain strings; the Persian UI shows them as Jalali dates. The
// conversion is the arithmetic one (the 33-year break table, valid for Jalali
// years 1–3177), the same as src-tauri/src/planner/jalali.rs — no Intl calendar,
// whose availability depends on the WebView's ICU data.

export interface JalaliDate {
  jy: number;
  jm: number;
  jd: number;
}

export interface GregorianDate {
  gy: number;
  gm: number;
  gd: number;
}

/** Years at which the 33-year leap cycle shifts. */
const BREAKS = [-61, 9, 38, 199, 426, 686, 756, 818, 1111, 1181, 1210, 1635, 2060, 2097, 2192, 2262, 2324, 2394, 2456, 3178];

const div = (a: number, b: number) => Math.trunc(a / b);
const mod = (a: number, b: number) => a - Math.trunc(a / b) * b;

/** Leap offset and the Gregorian March day on which Jalali year `jy` starts. */
function jalCal(jy: number): { leap: number; gy: number; march: number } {
  const gy = jy + 621;
  let leapJ = -14;
  let jp = BREAKS[0];
  let jump = 0;
  for (let i = 1; i < BREAKS.length; i++) {
    const jm = BREAKS[i];
    jump = jm - jp;
    if (jy < jm) break;
    leapJ += div(jump, 33) * 8 + div(mod(jump, 33), 4);
    jp = jm;
  }
  let n = jy - jp;
  leapJ += div(n, 33) * 8 + div(mod(n, 33) + 3, 4);
  if (mod(jump, 33) === 4 && jump - n === 4) leapJ += 1;
  const leapG = div(gy, 4) - div((div(gy, 100) + 1) * 3, 4) - 150;
  const march = 20 + leapJ - leapG;
  if (jump - n < 6) n = n - jump + div(jump + 4, 33) * 33;
  let leap = mod(mod(n + 1, 33) - 1, 4);
  if (leap === -1) leap = 4;
  return { leap, gy, march };
}

/** Gregorian date → Julian day number. */
function g2d(gy: number, gm: number, gd: number): number {
  let d = div((gy + div(gm - 8, 6) + 100100) * 1461, 4) + div(153 * mod(gm + 9, 12) + 2, 5) + gd - 34840408;
  d = d - div(div(gy + 100100 + div(gm - 8, 6), 100) * 3, 4) + 752;
  return d;
}

/** Julian day number → Gregorian date. */
function d2g(jdn: number): GregorianDate {
  let j = 4 * jdn + 139361631;
  j = j + div(div(4 * jdn + 183187720, 146097) * 3, 4) * 4 - 3908;
  const i = div(mod(j, 1461), 4) * 5 + 308;
  const gd = div(mod(i, 153), 5) + 1;
  const gm = mod(div(i, 153), 12) + 1;
  const gy = div(j, 1461) - 100100 + div(8 - gm, 6);
  return { gy, gm, gd };
}

export function toJalali(gy: number, gm: number, gd: number): JalaliDate {
  const jdn = g2d(gy, gm, gd);
  const gYear = d2g(jdn).gy;
  let jy = gYear - 621;
  const r = jalCal(jy);
  let k = jdn - g2d(gYear, 3, r.march);
  if (k >= 0) {
    if (k <= 185) return { jy, jm: 1 + div(k, 31), jd: mod(k, 31) + 1 };
    k -= 186;
  } else {
    jy -= 1;
    k += 179;
    if (r.leap === 1) k += 1;
  }
  return { jy, jm: 7 + div(k, 30), jd: mod(k, 30) + 1 };
}

export function toGregorian(jy: number, jm: number, jd: number): GregorianDate {
  const r = jalCal(jy);
  const jdn = g2d(r.gy, 3, r.march) + (jm - 1) * 31 - div(jm, 7) * (jm - 7) + jd - 1;
  return d2g(jdn);
}

export function isJalaliLeap(jy: number): boolean {
  return jalCal(jy).leap === 0;
}

// ── Day keys ──────────────────────────────────────────────────────────────────

const pad2 = (n: number) => String(n).padStart(2, "0");

/** The LOCAL calendar day of `d` as "YYYY-MM-DD". */
export function dayKeyOf(d: Date): string {
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}

const DAY_KEY = /^(\d{4})-(\d{2})-(\d{2})$/;

export function isDayKey(value: unknown): value is string {
  if (typeof value !== "string") return false;
  const m = DAY_KEY.exec(value);
  if (!m) return false;
  const d = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  return dayKeyOf(d) === value;
}

/** Local midnight of a day key (noon would dodge DST edges, but Iran has had none since 2022). */
export function dateOfDayKey(key: string): Date {
  const m = DAY_KEY.exec(key);
  if (!m) return new Date(NaN);
  return new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
}

export function addDays(key: string, n: number): string {
  const d = dateOfDayKey(key);
  d.setDate(d.getDate() + n);
  return dayKeyOf(d);
}

/** Whole days from `a` to `b` (positive when b is later). */
export function daysBetween(a: string, b: string): number {
  const da = dateOfDayKey(a);
  const db = dateOfDayKey(b);
  // Rounded: a DST change makes the gap 23 or 25 hours.
  return Math.round((db.getTime() - da.getTime()) / 86_400_000);
}

/** 0 = Saturday … 6 = Friday: the Iranian week. */
export function satWeekday(key: string): number {
  return (dateOfDayKey(key).getDay() + 1) % 7;
}

/** The Saturday that starts the week holding `key`. */
export function weekStart(key: string): string {
  return addDays(key, -satWeekday(key));
}

/**
 * Saturday to Wednesday: the Iranian work week, which is what a "weekdays"
 * reminder or habit means (same rule as the Rust scheduler).
 */
export function isWorkday(key: string): boolean {
  return satWeekday(key) <= 4;
}

export function jalaliOfDayKey(key: string): JalaliDate {
  const m = DAY_KEY.exec(key);
  if (!m) return { jy: 0, jm: 0, jd: 0 };
  return toJalali(Number(m[1]), Number(m[2]), Number(m[3]));
}

/** "1405/01/15" — ASCII digits; the UI converts them for display. */
export function formatJalaliNumeric(key: string): string {
  const j = jalaliOfDayKey(key);
  return `${j.jy}/${pad2(j.jm)}/${pad2(j.jd)}`;
}
