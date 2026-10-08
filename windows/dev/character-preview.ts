// Browser-only review harness for the bloub character. Vite's production entry points
// exclude this page.
import { BotEngine, effectiveEyeMotion, type EyeMotion } from "../src/character/engine";
import { CHARACTER_OPTIONS, normalizeAppearance } from "../src/character/appearance";
import { stillFrame } from "../src/character/fixed";
import { makeBlock } from "../src/character/bloub/cycles";
import { STATE_BY_ID, type StateId } from "../src/character/bloub/states";
import { CATALOGUE, STATE_CHOREOGRAPHY } from "../src/character/motion";
import { STATE_LABELS } from "../src/character/labels";
import { paintFrame } from "../src/character/render";
import { colorHex } from "../src/character/appearance";
import type { BotEmoteName, BotStateName } from "../src/core/layout";
import { Sound } from "../src/core/sound";

if (!import.meta.env.DEV) throw new Error("Character preview requires the Vite development server.");

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const params = new URLSearchParams(location.search);
Sound.enabled = false;

let look = normalizeAppearance({ shape: params.get("shape"), color: params.get("color"), expression: params.get("expression") });
const fill = (select: HTMLSelectElement, options: readonly { id: string; label: { fa: string } }[], value: string) => {
  for (const o of options) select.append(new Option(o.label.fa, o.id, false, o.id === value));
};
fill($("shape"), CHARACTER_OPTIONS.shapes, look.shape);
fill($("color"), CHARACTER_OPTIONS.colors, look.color);
fill($("expression"), CHARACTER_OPTIONS.expressions, look.expression);
const contextSelect = $<HTMLSelectElement>("context");
for (const id of Object.keys(STATE_CHOREOGRAPHY)) contextSelect.append(new Option(id, id));
const initialState = params.get("state");
if (initialState && initialState in STATE_CHOREOGRAPHY) contextSelect.value = initialState;
const reducedInput = $<HTMLInputElement>("reduced");
reducedInput.checked = params.get("reduced") === "1";

function applyPrefs() {
  BotEngine.setAppearance(look);
  BotEngine.setEyeMotion(effectiveEyeMotion($<HTMLSelectElement>("eyes").value as EyeMotion, reducedInput.checked));
  BotEngine.setCelebrations(true, reducedInput.checked);
}
applyPrefs();

// ── Island character: hero + native diameters ─────────────────────────────────

const OVERHANG = 40; // island.ts BOT_OVERHANG
interface Live { canvas: HTMLCanvasElement; engine: BotEngine; w: number; h: number }
const lives: Live[] = [];
function liveCanvas(host: HTMLElement, w: number, h: number, caption?: string): Live {
  const canvas = document.createElement("canvas");
  const dpr = Math.min(2, devicePixelRatio || 1);
  canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr);
  canvas.style.width = `${w}px`; canvas.style.height = `${h}px`;
  canvas.setAttribute("role", "img");
  canvas.setAttribute("aria-label", caption ?? "پیش‌نمایش کاراکتر");
  const engine = new BotEngine();
  engine.particleOverhang = h - w;
  if (caption) {
    const figure = document.createElement("figure");
    const cap = document.createElement("figcaption");
    cap.textContent = caption;
    figure.append(canvas, cap);
    host.append(figure);
  } else host.append(canvas);
  const live = { canvas, engine, w, h };
  lives.push(live);
  return live;
}
const hero = liveCanvas($("hero"), 240, 240);
for (const d of [20, 44, 62]) {
  const w = Math.round(d / 0.6);
  liveCanvas($("native"), w, w + OVERHANG, `${d} px`);
}
const each = (fn: (e: BotEngine) => void) => lives.forEach((l) => fn(l.engine));
const setContext = () => each((e) => e.setState(contextSelect.value as BotStateName, true));
setContext();

$("hero").addEventListener("pointermove", (ev) => {
  const r = hero.canvas.getBoundingClientRect();
  each((e) => {
    e.lookX = Math.tanh((ev.clientX - (r.left + r.width / 2)) / 260);
    e.lookY = -Math.tanh((ev.clientY - (r.top + r.height / 2)) / 200);
  });
});
for (const id of ["shape", "color", "expression"] as const) {
  $<HTMLSelectElement>(id).addEventListener("change", (ev) => {
    look = normalizeAppearance({ ...look, [id]: (ev.target as HTMLSelectElement).value });
    applyPrefs();
  });
}
$("eyes").addEventListener("change", applyPrefs);
reducedInput.addEventListener("change", () => { applyPrefs(); setContext(); });
contextSelect.addEventListener("change", setContext);
$<HTMLSelectElement>("emote").addEventListener("change", (ev) => {
  const v = (ev.target as HTMLSelectElement).value as BotEmoteName | "";
  if (v) each((e) => e.triggerEmote(v));
});
$("arrive").addEventListener("click", () => each((e) => e.arrive()));
$("celebrate").addEventListener("click", () => each((e) => e.celebrate("task")));
$("slap").addEventListener("click", () => each((e) => e.slap()));
const mailbox = $<HTMLButtonElement>("mailbox");
mailbox.addEventListener("click", () => {
  const on = mailbox.getAttribute("aria-pressed") !== "true";
  mailbox.setAttribute("aria-pressed", String(on));
  each((e) => { e.animateMorph(on ? 1 : 0); e.slotHTarget = on ? 0.2 : 0; });
});

// ── Board: every catalogue state, replayed on its own measured duration ──────

const CARD = 96;
interface Tile { canvas: HTMLCanvasElement; state: StateId }
const tiles: Tile[] = [];
for (const id of [...CATALOGUE, "swirl" as const]) {
  const card = document.createElement("figure");
  card.className = "state-card";
  const canvas = document.createElement("canvas");
  const dpr = Math.min(2, devicePixelRatio || 1);
  canvas.width = CARD * dpr; canvas.height = CARD * dpr;
  canvas.style.width = canvas.style.height = `${CARD}px`;
  canvas.setAttribute("role", "img");
  canvas.setAttribute("aria-label", STATE_LABELS[id].fa);
  const name = document.createElement("strong");
  name.textContent = STATE_LABELS[id].fa;
  const code = document.createElement("span");
  code.textContent = id;
  card.append(canvas, name, code);
  $("board").append(card);
  tiles.push({ canvas, state: id });
}
/** Each tile replays its state on the fixed body (fixed.ts): board time is absolute. */
function paintBoard(t: number) {
  const dpr = Math.min(2, devicePixelRatio || 1);
  for (const tile of tiles) {
    const period = Math.max(makeBlock(tile.state).duration, STATE_BY_ID.get(tile.state)!.duration) + 0.6;
    const local = t % period;
    const ctx = tile.canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, CARD, CARD);
    paintFrame(ctx, stillFrame(look, tile.state, local), CARD / 2, CARD / 2, CARD * 0.3, { ink: colorHex(look.color), paper: null, backdrop: "#000000" });
  }
}

// ── Loop (paused in hidden tabs) and automation hook ──────────────────────────

let last = 0;
let boardTime = 0;
let paused = false;
function draw() {
  const dpr = Math.min(2, devicePixelRatio || 1);
  for (const l of lives) {
    const ctx = l.canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, l.w, l.h);
    l.engine.draw(ctx, l.w, l.h);
  }
  paintBoard(boardTime);
  $("clock").textContent = `${hero.engine.time.toFixed(2)} s · ${hero.engine.bloubState}`;
}
function frame(now: number) {
  const dt = last ? Math.min(0.05, (now - last) / 1000) : 0;
  last = now;
  if (!paused) {
    boardTime += dt;
    each((e) => e.update(dt));
    draw();
  }
  if (!document.hidden) requestAnimationFrame(frame);
}
document.addEventListener("visibilitychange", () => { last = 0; if (!document.hidden) requestAnimationFrame(frame); });
requestAnimationFrame(frame);

declare global {
  interface Window { characterPreview: unknown }
}
window.characterPreview = {
  setState: (s: BotStateName) => { contextSelect.value = s; setContext(); },
  /** Pauses and renders the board and the island character `seconds` after a fresh start. */
  renderAt(seconds: number) {
    paused = true;
    const t = Math.max(0, Math.min(60, Number(seconds) || 0));
    for (const l of lives) {
      l.engine = new BotEngine();
      l.engine.particleOverhang = l.h - l.w;
      l.engine.setState(contextSelect.value as BotStateName, true);
      for (let x = 0; x < t; x += 1 / 60) l.engine.update(1 / 60);
    }
    boardTime = t;
    draw();
    return { state: hero.engine.bloubState, expression: hero.engine.currentExpression, busy: hero.engine.busy };
  },
  play() { paused = false; },
};

window.addEventListener("error", (e) => { const el = $("error"); el.hidden = false; el.textContent = e.message; });
