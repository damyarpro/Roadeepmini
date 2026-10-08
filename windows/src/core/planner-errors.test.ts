import { afterEach, describe, expect, it } from "vitest";
import { errorCode, localizeError } from "./error-text";
import { setLanguage } from "./i18n";

// The planner's codes (src-tauri/src/errors.rs, PLANNER_*).
const CODES = [
  "E_PLANNER_STORE|Access is denied. (os error 5)",
  "E_PLANNER_INVALID|title",
  "E_PLANNER_NOT_FOUND",
  "E_PLANNER_LIMIT|2000",
];

afterEach(() => setLanguage("fa"));

describe("planner errors", () => {
  it("are known codes with text in both languages", () => {
    for (const raw of CODES) {
      expect(errorCode(raw)).toBe(raw.split("|")[0]);
      for (const lang of ["en", "fa"] as const) {
        setLanguage(lang);
        const text = localizeError(raw);
        expect(text).not.toContain("E_PLANNER");
        expect(text).not.toContain("err.planner");
        expect(text).not.toMatch(/\{\w+\}/);
      }
    }
  });

  it("shows the limit in the UI language's digits", () => {
    setLanguage("fa");
    expect(localizeError("E_PLANNER_LIMIT|2000")).toContain("۲");
    setLanguage("en");
    expect(localizeError("E_PLANNER_LIMIT|2000")).toContain("2");
  });
});
