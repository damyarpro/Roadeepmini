// The character's look: bloub's customiser — body shape, colour and rest expression.
// Stored in settings as bloub ids. Anything unknown falls back to the default, and a
// settings file from before bloub ({body, eyes, color, accessory}) is migrated.

import { COLORS, COLOR_BY_ID, SHAPES, SHAPE_BY_ID, type ColorId, type ShapeId } from "./bloub/skins";
import { EXPRESSIONS, EXPRESSION_BY_ID, type ExpressionId } from "./bloub/expressions";
import { COLOR_LABELS, EXPRESSION_LABELS, SHAPE_LABELS, type Label } from "./labels";

export interface CharacterAppearance {
  shape: ShapeId;
  color: ColorId;
  expression: ExpressionId;
}

/**
 * Cream, not bloub's ink: the island is black, and an ink body would vanish on it.
 * Cream keeps the light body with dark eyes the character always had.
 */
export const DEFAULT_APPEARANCE: Readonly<CharacterAppearance> = Object.freeze({
  shape: "cercle", color: "creme", expression: "neutre",
});

// Pre-bloub values → the closest bloub choice.
const LEGACY_SHAPE: Record<string, ShapeId> = { round: "cercle", softSquare: "squircle" };
const LEGACY_COLOR: Record<string, ColorId> = { neutral: "creme", sky: "bleu", mint: "turquoise" };
const LEGACY_EXPRESSION: Record<string, ExpressionId> = { pill: "neutre", dot: "attentif", wide: "surpris" };

const pick = <T extends string>(value: unknown, known: ReadonlyMap<string, unknown>): T | undefined =>
  typeof value === "string" && known.has(value) ? value as T : undefined;
const legacy = <T extends string>(value: unknown, table: Record<string, T>): T | undefined =>
  typeof value === "string" && Object.prototype.hasOwnProperty.call(table, value) ? table[value] : undefined;

export function normalizeAppearance(value: unknown): CharacterAppearance {
  const raw = value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
  return {
    shape: pick<ShapeId>(raw.shape, SHAPE_BY_ID) ?? legacy(raw.body, LEGACY_SHAPE) ?? DEFAULT_APPEARANCE.shape,
    color: pick<ColorId>(raw.color, COLOR_BY_ID) ?? legacy(raw.color, LEGACY_COLOR) ?? DEFAULT_APPEARANCE.color,
    expression: pick<ExpressionId>(raw.expression, EXPRESSION_BY_ID) ?? legacy(raw.eyes, LEGACY_EXPRESSION) ?? DEFAULT_APPEARANCE.expression,
  };
}

export interface CharacterOption<T extends string> { id: T; label: Label }

/** Everything the voice assistant and the settings page may choose from. */
export const CHARACTER_OPTIONS: {
  readonly shapes: readonly CharacterOption<ShapeId>[];
  readonly colors: readonly (CharacterOption<ColorId> & { hex: string })[];
  readonly expressions: readonly CharacterOption<ExpressionId>[];
} = Object.freeze({
  shapes: Object.freeze(SHAPES.map((s) => Object.freeze({ id: s.id, label: SHAPE_LABELS[s.id] }))),
  colors: Object.freeze(COLORS.map((c) => Object.freeze({ id: c.id, label: COLOR_LABELS[c.id], hex: c.hex }))),
  expressions: Object.freeze(EXPRESSIONS.map((e) => Object.freeze({ id: e.id, label: EXPRESSION_LABELS[e.id] }))),
});

export type CharacterPatch = Partial<Record<keyof CharacterAppearance, unknown>>;

/**
 * Applies an untrusted patch (voice tool, URL) to `settings.characterAppearance`.
 * Each field must be a known id; unknown fields are ignored. Returns the new value, or
 * throws a short reason when nothing valid was asked for. Mutates only on success.
 */
export function applyCharacterPatch<S extends { characterAppearance: unknown }>(settings: S, patch: unknown): CharacterAppearance {
  if (!patch || typeof patch !== "object" || Array.isArray(patch)) throw new Error("invalid character patch");
  const p = patch as Record<string, unknown>;
  const next = normalizeAppearance(settings.characterAppearance);
  const invalid: string[] = [];
  let changed = 0;
  const field = <K extends keyof CharacterAppearance>(key: K, known: ReadonlyMap<string, unknown>) => {
    if (p[key] === undefined) return;
    const id = pick<CharacterAppearance[K]>(p[key], known);
    if (id === undefined) { invalid.push(key); return; }
    next[key] = id;
    changed++;
  };
  field("shape", SHAPE_BY_ID);
  field("color", COLOR_BY_ID);
  field("expression", EXPRESSION_BY_ID);
  if (invalid.length) throw new Error(`unknown character ${invalid.join(", ")}`);
  if (!changed) throw new Error("empty character patch");
  settings.characterAppearance = next;
  return next;
}

/** Hex of a colour id (the island uses it as the body ink). */
export const colorHex = (id: ColorId): string => COLOR_BY_ID.get(id)?.hex ?? "#f1efe9";
