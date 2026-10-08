import { describe, expect, it } from "vitest";
import {
  addDays, dayKeyOf, daysBetween, formatJalaliNumeric, isDayKey, isJalaliLeap, isWorkday, satWeekday,
  toGregorian, toJalali, weekStart,
} from "./jalali";

// The same known pairs as src-tauri/src/planner/jalali.rs.
const PAIRS: [[number, number, number], [number, number, number]][] = [
  [[2026, 3, 21], [1405, 1, 1]],
  [[2025, 3, 20], [1403, 12, 30]],
  [[2024, 2, 29], [1402, 12, 10]],
  [[2027, 3, 21], [1406, 1, 1]],
  [[2025, 3, 21], [1404, 1, 1]],
  [[2026, 10, 3], [1405, 7, 11]],
];

describe("jalali", () => {
  it("converts the known pairs both ways", () => {
    for (const [[gy, gm, gd], [jy, jm, jd]] of PAIRS) {
      expect(toJalali(gy, gm, gd)).toEqual({ jy, jm, jd });
      expect(toGregorian(jy, jm, jd)).toEqual({ gy, gm, gd });
    }
  });

  it("round-trips every day over several years", () => {
    let key = "2023-01-01";
    for (let i = 0; i < 365 * 6; i++) {
      const [gy, gm, gd] = key.split("-").map(Number);
      const j = toJalali(gy, gm, gd);
      expect(toGregorian(j.jy, j.jm, j.jd)).toEqual({ gy, gm, gd });
      // Consecutive days never skip or repeat a Jalali day.
      const next = addDays(key, 1);
      const [ny, nm, nd] = next.split("-").map(Number);
      const nj = toJalali(ny, nm, nd);
      const sameMonth = nj.jy === j.jy && nj.jm === j.jm;
      if (sameMonth) expect(nj.jd).toBe(j.jd + 1);
      else expect(nj.jd).toBe(1);
      key = next;
    }
  });

  it("knows the leap years", () => {
    expect(isJalaliLeap(1403)).toBe(true);
    expect(isJalaliLeap(1404)).toBe(false);
    expect(isJalaliLeap(1399)).toBe(true);
  });

  it("formats day keys as Jalali dates", () => {
    expect(formatJalaliNumeric("2026-04-04")).toBe("1405/01/15");
  });

  it("builds and checks local day keys", () => {
    expect(dayKeyOf(new Date(2026, 0, 5, 23, 59))).toBe("2026-01-05");
    expect(isDayKey("2026-02-30")).toBe(false);
    expect(isDayKey("2026-02-28")).toBe(true);
    expect(isDayKey("tomorrow")).toBe(false);
    expect(addDays("2026-02-28", 1)).toBe("2026-03-01");
    expect(daysBetween("2026-03-01", "2026-02-27")).toBe(-2);
  });

  it("starts the week on Saturday and works Saturday to Wednesday", () => {
    // 2026-03-21 is a Saturday.
    expect(satWeekday("2026-03-21")).toBe(0);
    expect(satWeekday("2026-03-27")).toBe(6);
    expect(weekStart("2026-03-25")).toBe("2026-03-21");
    expect(isWorkday("2026-03-25")).toBe(true); // Wednesday
    expect(isWorkday("2026-03-26")).toBe(false); // Thursday
    expect(isWorkday("2026-03-27")).toBe(false); // Friday
  });
});
