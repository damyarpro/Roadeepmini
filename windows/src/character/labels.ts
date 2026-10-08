// Persian and English names for everything the character can be or do. The ids are
// bloub's own (src/character/bloub), so a stored setting maps 1:1 onto the engine.
// The compiler checks every id has both labels: adding a shape without its name fails.

import type { ColorId, ShapeId } from "./bloub/skins";
import type { ExpressionId } from "./bloub/expressions";
import type { StateId } from "./bloub/states";

export interface Label { fa: string; en: string }

export const SHAPE_LABELS: Record<ShapeId, Label> = {
  cercle: { fa: "دایره", en: "Circle" },
  galet: { fa: "ریگ", en: "Pebble" },
  squircle: { fa: "مربع گرد", en: "Squircle" },
  capsule: { fa: "کپسول", en: "Capsule" },
  triangle: { fa: "مثلث", en: "Triangle" },
  hexagone: { fa: "شش‌ضلعی", en: "Hexagon" },
  nuage: { fa: "ابر", en: "Cloud" },
  goutte: { fa: "قطره", en: "Droplet" },
};

export const COLOR_LABELS: Record<ColorId, Label> = {
  encre: { fa: "جوهری", en: "Ink" },
  creme: { fa: "کرم", en: "Cream" },
  brun: { fa: "قهوه‌ای", en: "Brown" },
  rouge: { fa: "قرمز", en: "Red" },
  orange: { fa: "نارنجی", en: "Orange" },
  ambre: { fa: "کهربایی", en: "Amber" },
  vert: { fa: "سبز", en: "Green" },
  turquoise: { fa: "فیروزه‌ای", en: "Turquoise" },
  bleu: { fa: "آبی", en: "Blue" },
  violet: { fa: "بنفش", en: "Purple" },
  rose: { fa: "صورتی", en: "Pink" },
  gris: { fa: "خاکستری", en: "Grey" },
};

export const EXPRESSION_LABELS: Record<ExpressionId, Label> = {
  neutre: { fa: "خنثی", en: "Neutral" },
  attentif: { fa: "حواس‌جمع", en: "Attentive" },
  surpris: { fa: "متعجب", en: "Surprised" },
  excite: { fa: "هیجان‌زده", en: "Excited" },
  heureux: { fa: "خوشحال", en: "Happy" },
  hilare: { fa: "خندان", en: "Laughing" },
  colere: { fa: "عصبانی", en: "Angry" },
  triste: { fa: "غمگین", en: "Sad" },
  effraye: { fa: "ترسیده", en: "Scared" },
  mefiant: { fa: "بدگمان", en: "Suspicious" },
  confus: { fa: "گیج", en: "Confused" },
  curieux: { fa: "کنجکاو", en: "Curious" },
  fier: { fa: "مفتخر", en: "Proud" },
  timide: { fa: "خجالتی", en: "Shy" },
  blase: { fa: "بی‌حوصله", en: "Unimpressed" },
  somnolent: { fa: "خواب‌آلود", en: "Sleepy" },
};

export const STATE_LABELS: Record<StateId, Label> = {
  idle: { fa: "آرام", en: "Idle" },
  thinking: { fa: "سه نقطه", en: "Thinking" },
  wink: { fa: "چشمک", en: "Wink" },
  wide: { fa: "چشم‌های گشاد", en: "Wide eyes" },
  alert: { fa: "هشدار", en: "Alert" },
  notify: { fa: "اعلان", en: "Notification" },
  exclaim: { fa: "علامت تعجب", en: "Exclamation" },
  sleep: { fa: "خواب", en: "Sleep" },
  egg: { fa: "تخم‌مرغی", en: "Egg" },
  hexagon: { fa: "شش‌ضلعی", en: "Hexagon" },
  play: { fa: "پخش", en: "Play" },
  orbit: { fa: "مدار", en: "Orbit" },
  burst: { fa: "ترکیدن", en: "Burst" },
  comet: { fa: "دنباله‌دار", en: "Comet" },
  swirl: { fa: "چرخش", en: "Swirl" },
};

/** i18n keys (`character.shape.cercle`, …) for the locale tables. */
export function characterMessages(lang: keyof Label): Record<string, string> {
  const out: Record<string, string> = {};
  const add = (group: string, table: Record<string, Label>) => {
    for (const [id, label] of Object.entries(table)) out[`character.${group}.${id}`] = label[lang];
  };
  add("shape", SHAPE_LABELS);
  add("color", COLOR_LABELS);
  add("expression", EXPRESSION_LABELS);
  add("state", STATE_LABELS);
  return out;
}
