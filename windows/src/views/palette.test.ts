import { describe, expect, it } from "vitest";
import { filterPalette, normalizeQuery, opensMenuFromChat } from "./palette";
import { plannerEn } from "../core/locales/planner-en";
import { plannerFa } from "../core/locales/planner-fa";

describe("palette", () => {
  it("lists everything for an empty filter", () => {
    expect(filterPalette("")).toHaveLength(10);
    expect(filterPalette("کدنویسی")).toEqual(["activity"]);
    expect(filterPalette("فعالیت")).toEqual(["activity"]);
  });

  it("matches labels in either language and folds Arabic letters", () => {
    expect(filterPalette("یاد")).toEqual(["notes", "reminders"]);
    expect(filterPalette("rem")).toEqual(["reminders"]);
    // Arabic yeh/kaf as typed on some keyboards.
    expect(filterPalette("كارها")).toEqual(["tasks"]);
    expect(filterPalette("تنظيمات")).toEqual([]);
    expect(filterPalette("feedback")).toEqual([]);
    expect(filterPalette("zzz")).toEqual([]);
  });

  it("ignores zero-width joiners", () => {
    expect(normalizeQuery("یادداشت‌ها")).toBe("یادداشتها");
    expect(filterPalette("یادداشتها")).toEqual(["notes"]);
  });

  it("opens from the chat only for a «/» typed into an empty field", () => {
    expect(opensMenuFromChat("", "/")).toBe(true);
    // A «/» in front of a draft keeps the draft.
    expect(opensMenuFromChat("salam", "/salam")).toBe(false);
    // A pasted path stays text, in an empty field or not.
    expect(opensMenuFromChat("", "/etc/hosts")).toBe(false);
    expect(opensMenuFromChat("see ", "see /etc/hosts")).toBe(false);
    // Deleting back down to «/» doesn't reopen it.
    expect(opensMenuFromChat("/a", "/")).toBe(false);
  });

  it("has every planner string in both languages", () => {
    expect(Object.keys(plannerFa).sort()).toEqual(Object.keys(plannerEn).sort());
  });
});
