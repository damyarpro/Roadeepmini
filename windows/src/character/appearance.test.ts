import { describe, expect, it } from "vitest";
import { CHARACTER_OPTIONS, DEFAULT_APPEARANCE, applyCharacterPatch, normalizeAppearance } from "./appearance";
import { characterMessages } from "./labels";
import { COLORS, SHAPES } from "./bloub/skins";
import { EXPRESSIONS } from "./bloub/expressions";
import { SEQUENCE } from "./bloub/states";

describe("character appearance", () => {
  it("keeps the default for absent or malformed settings", () => {
    for (const value of [undefined, null, [], 3, "cercle", { shape: "script", color: {}, expression: false }]) {
      expect(normalizeAppearance(value)).toEqual(DEFAULT_APPEARANCE);
    }
    expect(DEFAULT_APPEARANCE).toEqual({ shape: "cercle", color: "creme", expression: "neutre" });
  });

  it("accepts every bloub id", () => {
    for (const s of SHAPES) expect(normalizeAppearance({ shape: s.id }).shape).toBe(s.id);
    for (const c of COLORS) expect(normalizeAppearance({ color: c.id }).color).toBe(c.id);
    for (const e of EXPRESSIONS) expect(normalizeAppearance({ expression: e.id }).expression).toBe(e.id);
  });

  it("migrates the pre-bloub {body, eyes, color, accessory}", () => {
    expect(normalizeAppearance({ body: "round", eyes: "pill", color: "neutral", accessory: "none" })).toEqual(DEFAULT_APPEARANCE);
    expect(normalizeAppearance({ body: "softSquare", eyes: "wide", color: "mint", accessory: "star" }))
      .toEqual({ shape: "squircle", color: "turquoise", expression: "surpris" });
    expect(normalizeAppearance({ eyes: "dot", color: "sky" })).toEqual({ shape: "cercle", color: "bleu", expression: "attentif" });
    // A current value wins over a stale legacy one.
    expect(normalizeAppearance({ shape: "nuage", body: "softSquare" }).shape).toBe("nuage");
  });

  it("lists every option with Persian and English names", () => {
    expect(CHARACTER_OPTIONS.shapes).toHaveLength(8);
    expect(CHARACTER_OPTIONS.colors).toHaveLength(12);
    expect(CHARACTER_OPTIONS.expressions).toHaveLength(16);
    for (const o of [...CHARACTER_OPTIONS.shapes, ...CHARACTER_OPTIONS.colors, ...CHARACTER_OPTIONS.expressions]) {
      expect(o.label.fa.trim()).not.toBe("");
      expect(o.label.en.trim()).not.toBe("");
      expect(o.label.fa).toMatch(/[؀-ۿ]/);
    }
    for (const c of CHARACTER_OPTIONS.colors) expect(c.hex).toMatch(/^#[0-9a-f]{6}$/);
    expect(Object.isFrozen(CHARACTER_OPTIONS.shapes)).toBe(true);
  });

  it("has an i18n key for every shape, colour, expression and state in both languages", () => {
    const fa = characterMessages("fa");
    const en = characterMessages("en");
    expect(Object.keys(fa).sort()).toEqual(Object.keys(en).sort());
    const keys = [
      ...SHAPES.map((s) => `character.shape.${s.id}`), ...COLORS.map((c) => `character.color.${c.id}`),
      ...EXPRESSIONS.map((e) => `character.expression.${e.id}`), ...SEQUENCE.map((s) => `character.state.${s}`),
    ];
    for (const key of keys) {
      expect(fa[key], key).toBeTruthy();
      expect(en[key], key).toBeTruthy();
    }
  });
});

describe("applyCharacterPatch", () => {
  it("applies known ids and keeps the rest", () => {
    const settings = { characterAppearance: { ...DEFAULT_APPEARANCE } as unknown };
    expect(applyCharacterPatch(settings, { color: "violet" })).toEqual({ ...DEFAULT_APPEARANCE, color: "violet" });
    applyCharacterPatch(settings, { shape: "goutte", expression: "fier", extra: "ignored" });
    expect(settings.characterAppearance).toEqual({ shape: "goutte", color: "violet", expression: "fier" });
  });

  it("migrates a legacy stored value while patching", () => {
    const settings = { characterAppearance: { body: "softSquare", eyes: "dot", color: "sky", accessory: "none" } as unknown };
    expect(applyCharacterPatch(settings, { expression: "timide" })).toEqual({ shape: "squircle", color: "bleu", expression: "timide" });
  });

  it("rejects unknown ids, empty and malformed patches without touching the settings", () => {
    const original = { shape: "nuage", color: "rose", expression: "triste" };
    const settings = { characterAppearance: { ...original } as unknown };
    for (const bad of [null, "cercle", [], {}, { shape: "star" }, { color: "#fff" }, { shape: "cercle", expression: 4 }]) {
      expect(() => applyCharacterPatch(settings, bad)).toThrow();
      expect(settings.characterAppearance).toEqual(original);
    }
  });
});
