import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS } from "../core/state";
import { appearanceRows } from "./appearance";
import { setLanguage } from "../core/i18n";

vi.mock("../core/sound", () => ({ Sound: { play: vi.fn() } }));

function context(): CanvasRenderingContext2D {
  const noop = () => {};
  const gradient = { addColorStop: noop };
  return new Proxy({}, { get: (_t, p) => String(p).startsWith("create") ? () => gradient : noop, set: () => true }) as CanvasRenderingContext2D;
}

const radios = (view: HTMLElement, group: string) => [...view.querySelectorAll<HTMLButtonElement>(`.appearance-tiles-${group} [role=radio]`)];
const checked = (view: HTMLElement, group: string) => radios(view, group).find((b) => b.getAttribute("aria-checked") === "true")?.dataset.id;

describe("appearance preferences", () => {
  beforeEach(() => {
    setLanguage("en");
    vi.stubGlobal("Path2D", class {});
    vi.stubGlobal("requestAnimationFrame", () => 0);
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(context());
  });

  it("offers every bloub shape, colour and expression as labelled radio groups", () => {
    const settings = structuredClone(DEFAULT_SETTINGS);
    const view = appearanceRows({ settings: () => settings, save: async () => {} });
    expect(radios(view, "shape")).toHaveLength(8);
    expect(radios(view, "color")).toHaveLength(12);
    expect(radios(view, "expression")).toHaveLength(16);
    expect([checked(view, "shape"), checked(view, "color"), checked(view, "expression")]).toEqual(["cercle", "creme", "neutre"]);
    for (const group of view.querySelectorAll("[role=radiogroup]")) {
      const id = group.getAttribute("aria-labelledby")!;
      expect(view.querySelector(`#${id}`)?.textContent).toBeTruthy();
      // One tab stop per group.
      expect([...group.querySelectorAll("[role=radio]")].filter((b) => (b as HTMLElement).tabIndex === 0)).toHaveLength(1);
    }
    expect(radios(view, "shape").map((b) => b.textContent)).toContain("Cloud");
    expect(view.querySelector("canvas.appearance-live")?.getAttribute("aria-label")).toBe("Live character preview");
  });

  it("shows a strip of the 14 states that play in the preview", () => {
    const settings = structuredClone(DEFAULT_SETTINGS);
    const view = appearanceRows({ settings: () => settings, save: async () => {} });
    const states = [...view.querySelectorAll<HTMLButtonElement>(".appearance-state")];
    expect(states).toHaveLength(14);
    expect(states.map((b) => b.dataset.state)).toContain("orbit");
    states.find((b) => b.dataset.state === "burst")!.click();
    expect(states.find((b) => b.dataset.state === "burst")!.getAttribute("aria-pressed")).toBe("true");
    const all = view.querySelector<HTMLButtonElement>(".appearance-play-all")!;
    expect(all.textContent).toBe("Stop");
    all.click();
    expect(states.every((b) => b.getAttribute("aria-pressed") === "false")).toBe(true);
  });

  it("persists a choice, moves with the arrow keys, and resets to the original", async () => {
    const settings = structuredClone(DEFAULT_SETTINGS); const save = vi.fn().mockResolvedValue(undefined);
    const view = appearanceRows({ settings: () => settings, save });
    document.body.append(view);
    radios(view, "color").find((b) => b.dataset.id === "violet")!.click();
    await vi.waitFor(() => expect(view.textContent).toContain("Appearance saved"));
    expect(settings.characterAppearance.color).toBe("violet");
    expect(checked(view, "color")).toBe("violet");

    const shapes = radios(view, "shape");
    shapes[0].focus();
    shapes[0].dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));
    await vi.waitFor(() => expect(settings.characterAppearance.shape).toBe("goutte"));

    await vi.waitFor(() => expect(save).toHaveBeenCalledTimes(2));
    view.querySelector<HTMLButtonElement>(".appearance-reset")!.click();
    await vi.waitFor(() => expect(save).toHaveBeenCalledTimes(3));
    expect(settings.characterAppearance).toEqual({ shape: "cercle", color: "creme", expression: "neutre" });
    view.remove();
  });

  it("restores the previous choice and shows a save failure", async () => {
    const settings = structuredClone(DEFAULT_SETTINGS);
    const view = appearanceRows({ settings: () => settings, save: async () => { throw new Error("private backend error"); } });
    radios(view, "expression").find((b) => b.dataset.id === "triste")!.click();
    await vi.waitFor(() => expect(view.textContent).toContain("Appearance could not be saved"));
    expect(settings.characterAppearance.expression).toBe("neutre");
    expect(checked(view, "expression")).toBe("neutre");
    expect(view.textContent).not.toContain("private backend error");
  });
});
