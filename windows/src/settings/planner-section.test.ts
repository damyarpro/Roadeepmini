import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clampCount, eyeMotionRow, plannerSection, refreshPlannerSection } from "./planner-section";
import { DEFAULT_SETTINGS, type Settings } from "../core/state";
import { setLanguage, t } from "../core/i18n";

let settings: Settings;
let save: ReturnType<typeof vi.fn>;
const host = () => ({ settings: () => settings, save: save as unknown as () => Promise<void> });

beforeEach(() => {
  settings = { ...DEFAULT_SETTINGS };
  save = vi.fn(async () => {});
  setLanguage("en");
});

afterEach(() => {
  document.body.innerHTML = "";
  document.documentElement.dir = "ltr";
});

function changeNumber(input: HTMLInputElement, value: string) {
  input.value = value;
  input.dispatchEvent(new Event("change"));
}

describe("clampCount", () => {
  it("rounds and holds values in range, keeping the fallback for junk", () => {
    expect(clampCount("focusMinutes", 49.6)).toBe(50);
    expect(clampCount("focusMinutes", 1)).toBe(5);
    expect(clampCount("focusMinutes", 999)).toBe(120);
    expect(clampCount("breakMinutes", Number.NaN, 7)).toBe(7);
    expect(clampCount("roundsBeforeLongBreak", Number.POSITIVE_INFINITY)).toBe(4);
    expect(clampCount("longBreakMinutes", 15)).toBe(15);
  });
});

describe("planner section", () => {
  it("shows every planner setting in both languages", () => {
    for (const lang of ["en", "fa"] as const) {
      setLanguage(lang);
      document.body.replaceChildren(plannerSection(host()));
      const text = document.body.textContent ?? "";
      expect(text).not.toMatch(/plannerSettings\./);
      expect(document.querySelectorAll("input.num")).toHaveLength(4);
      expect(document.querySelectorAll("button.switch")).toHaveLength(4);
    }
  });

  it("saves a duration, and pulls an out-of-range one back with a note", () => {
    document.body.replaceChildren(plannerSection(host()));
    const focus = document.getElementById("pl-focusMinutes") as HTMLInputElement;
    expect(focus.value).toBe("25");

    changeNumber(focus, "40");
    expect(settings.focusMinutes).toBe(40);
    expect(save).toHaveBeenCalledTimes(1);
    expect(document.getElementById("pl-focusMinutes-note")!.textContent).toBe("");

    changeNumber(focus, "500");
    expect(settings.focusMinutes).toBe(120);
    expect(focus.value).toBe("120");
    expect(document.getElementById("pl-focusMinutes-note")!.textContent).toBe(t("plannerSettings.clamped", { min: 5, max: 120 }));

    // Cleared field: the stored value comes back, nothing is saved.
    save.mockClear();
    changeNumber(focus, "");
    expect(focus.value).toBe("120");
    expect(save).not.toHaveBeenCalled();
  });

  it("flips the switches and notes when Roadeep's sound is off", () => {
    document.body.replaceChildren(plannerSection(host()));
    const switches = [...document.querySelectorAll<HTMLButtonElement>("button.switch")];
    switches[3].click(); // celebrations
    expect(settings.celebrations).toBe(false);
    switches[0].click(); // focus sound
    expect(settings.focusSound).toBe(false);
    expect(save).toHaveBeenCalledTimes(2);

    expect(document.body.textContent).not.toContain(t("plannerSettings.soundOff"));
    settings = { ...settings, soundEnabled: false };
    refreshPlannerSection();
    expect(document.body.textContent).toContain(t("plannerSettings.soundOff"));
  });
});

describe("eye motion row", () => {
  it("is a radio group that saves the choice", () => {
    document.body.replaceChildren(eyeMotionRow(host()));
    const radios = [...document.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
    expect(radios.map((r) => r.getAttribute("aria-checked"))).toEqual(["true", "false", "false"]);
    expect(radios.map((r) => r.tabIndex)).toEqual([0, -1, -1]);

    radios[2].click();
    expect(settings.eyeMotion).toBe("still");
    expect(radios[2].getAttribute("aria-checked")).toBe("true");
    expect(document.getElementById("gen-eye-hint")!.textContent).toBe(t("plannerSettings.eye.stillHint"));
    expect(save).toHaveBeenCalledTimes(1);
    const explanation = document.getElementById("gen-eye-hint")!;
    expect(explanation.hidden).toBe(true);
    document.querySelector<HTMLButtonElement>(".settings-help-button")!.click();
    expect(explanation.hidden).toBe(false);

    radios[2].click();
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("follows the reading direction with the arrow keys", () => {
    setLanguage("fa");
    document.body.replaceChildren(eyeMotionRow(host()));
    const normal = document.getElementById("gen-eye-normal")!;
    normal.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }));
    expect(settings.eyeMotion).toBe("calm");
    document.getElementById("gen-eye-calm")!.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));
    expect(settings.eyeMotion).toBe("still");
  });
});
