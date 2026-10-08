import "./appearance.css";
import { registerMessages, t } from "../core/i18n";
import { h } from "../views/dom";
import { helpDisclosure } from "./help";
import { BotEngine } from "../character/engine";
import { CHARACTER_OPTIONS, DEFAULT_APPEARANCE, colorHex, normalizeAppearance, type CharacterAppearance } from "../character/appearance";
import { stillFrame } from "../character/fixed";
import { POSES, type StateId } from "../character/bloub/states";
import { CATALOGUE } from "../character/motion";
import { makeBlock } from "../character/bloub/cycles";
import { paintFrame } from "../character/render";
import type { Settings } from "../core/state";
import { Bridge } from "../core/bridge";

registerMessages({
  "appearance.title":"Character appearance", "appearance.hint":"Choose the body shape, colour and resting expression. Every state keeps its own animation; tap one below to watch it.",
  "appearance.shape":"Shape", "appearance.color":"Colour", "appearance.expression":"Resting expression", "appearance.states":"States",
  "appearance.playAll":"Play all", "appearance.stop":"Stop",
  "appearance.reset":"Restore original", "appearance.preview":"Live character preview", "appearance.saved":"Appearance saved", "appearance.failed":"Appearance could not be saved. Previous choice restored.",
  "appearance.saving":"Saving appearance…",
}, {
  "appearance.title":"ظاهر کاراکتر", "appearance.hint":"فرم بدن، رنگ و حالت چهرهٔ آرام را انتخاب کنید. هر وضعیت انیمیشن خودش را دارد؛ برای دیدنش روی آن بزنید.",
  "appearance.shape":"فرم", "appearance.color":"رنگ", "appearance.expression":"حالت چهرهٔ آرام", "appearance.states":"وضعیت‌ها",
  "appearance.playAll":"پخش همه", "appearance.stop":"توقف",
  "appearance.reset":"بازگشت به ظاهر اصلی", "appearance.preview":"پیش‌نمایش زندهٔ کاراکتر", "appearance.saved":"ظاهر ذخیره شد", "appearance.failed":"ذخیرهٔ ظاهر انجام نشد؛ انتخاب قبلی بازگردانده شد.",
  "appearance.saving":"در حال ذخیرهٔ ظاهر…",
});

/** The island's backdrop: the preview shows the character where it really lives. */
const STAGE = "#0b0b0d";
const THUMB = 44;
const PREVIEW = 168;

let refresh: (() => void) | undefined;
export function refreshAppearance() { refresh?.(); }

/** One still frame of bloub, drawn once — the tiles have no animation loop. */
function drawStill(canvas: HTMLCanvasElement, look: CharacterAppearance, state: StateId = "idle") {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  const size = THUMB;
  if (canvas.width !== Math.round(size * dpr)) {
    canvas.width = Math.round(size * dpr);
    canvas.height = Math.round(size * dpr);
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, size, size);
  paintFrame(ctx, stillFrame(look, state, POSES[state]), size / 2, size / 2, size * 0.3, { ink: colorHex(look.color), paper: STAGE, backdrop: STAGE });
}

interface Choice<T extends string> { id: T; text: string; swatch?: string }

/**
 * An accessible radio group of tiles: one tab stop, arrows move and select,
 * Home/End jump. Each tile can carry a still preview canvas or a colour swatch.
 */
function radioGroup<T extends string>(key: string, title: string, choices: Choice<T>[], onPick: (id: T) => void) {
  const heading = h("h4", { id:`appearance-${key}-title`, class:"appearance-group-title", text:title });
  const list = h("div", { class:`appearance-tiles appearance-tiles-${key}`, role:"radiogroup", "aria-labelledby":heading.id });
  const tiles = new Map<T, { button: HTMLButtonElement; canvas: HTMLCanvasElement | null }>();
  const ids = choices.map((c) => c.id);
  for (const choice of choices) {
    const canvas = choice.swatch ? null : h("canvas", { class:"appearance-thumb", width:String(THUMB), height:String(THUMB), "aria-hidden":"true" });
    const button = h("button", { type:"button", role:"radio", class:"appearance-tile", "data-id":choice.id, "aria-checked":"false", tabindex:"-1", title:choice.text,
      onclick:() => onPick(choice.id) },
      canvas ?? h("span", { class:"appearance-swatch", style:`background:${choice.swatch}`, "aria-hidden":"true" }),
      h("span", { class:"appearance-tile-label", text:choice.text }));
    tiles.set(choice.id, { button, canvas });
    list.append(button);
  }
  list.addEventListener("keydown", (e) => {
    const current = ids.findIndex((id) => tiles.get(id)?.button === document.activeElement);
    if (current < 0) return;
    const rtl = getComputedStyle(list).direction === "rtl";
    const step = { ArrowRight: rtl ? -1 : 1, ArrowLeft: rtl ? 1 : -1, ArrowDown: 1, ArrowUp: -1 }[e.key];
    let next = step === undefined ? -1 : (current + step + ids.length) % ids.length;
    if (e.key === "Home") next = 0;
    if (e.key === "End") next = ids.length - 1;
    if (next < 0) return;
    e.preventDefault();
    tiles.get(ids[next])?.button.focus();
    onPick(ids[next]);
  });
  return {
    el: h("div", { class:"appearance-group" }, heading, list),
    sync(selected: T, disabled: boolean, paint?: (canvas: HTMLCanvasElement, id: T) => void) {
      for (const [id, tile] of tiles) {
        const on = id === selected;
        tile.button.setAttribute("aria-checked", String(on));
        tile.button.tabIndex = on ? 0 : -1;
        tile.button.disabled = disabled;
        if (tile.canvas && paint) paint(tile.canvas, id);
      }
    },
  };
}

export function appearanceRows(host: { settings(): Settings; save(): Promise<void> }): HTMLElement {
  const engine = new BotEngine();
  const canvas = h("canvas", { role:"img", "aria-label":t("appearance.preview"), class:"appearance-live", width:String(PREVIEW), height:String(PREVIEW) });
  const notice = h("span", { class:"appearance-note", role:"status", "aria-live":"polite" });
  const reset = h("button", { type:"button", class:"link appearance-reset", text:t("appearance.reset"), onclick:() => void change({ ...DEFAULT_APPEARANCE }) });
  let busy = false;

  const pick = (patch: Partial<CharacterAppearance>) => {
    const value = normalizeAppearance({ ...normalizeAppearance(host.settings().characterAppearance), ...patch });
    void change(value);
  };
  const shapes = radioGroup("shape", t("appearance.shape"),
    CHARACTER_OPTIONS.shapes.map((o) => ({ id:o.id, text:t(`character.shape.${o.id}`) })), (shape) => pick({ shape }));
  const colors = radioGroup("color", t("appearance.color"),
    CHARACTER_OPTIONS.colors.map((o) => ({ id:o.id, text:t(`character.color.${o.id}`), swatch:o.hex })), (color) => pick({ color }));
  const expressions = radioGroup("expression", t("appearance.expression"),
    CHARACTER_OPTIONS.expressions.map((o) => ({ id:o.id, text:t(`character.expression.${o.id}`) })), (expression) => pick({ expression }));

  // ── State strip: every bloub state, frozen at its most readable pose; a tap plays it.
  const queue: StateId[] = [];
  let playing: StateId | null = null;
  const stateButtons = new Map<StateId, { button: HTMLButtonElement; canvas: HTMLCanvasElement }>();
  const strip = h("div", { class:"appearance-states", role:"group", "aria-labelledby":"appearance-states-title" });
  for (const id of CATALOGUE) {
    const thumb = h("canvas", { class:"appearance-thumb", width:String(THUMB), height:String(THUMB), "aria-hidden":"true" });
    const button = h("button", { type:"button", class:"appearance-state", "aria-pressed":"false", "data-state":id,
      onclick:() => { queue.length = 0; play(id); } },
      thumb, h("span", { class:"appearance-tile-label", text:t(`character.state.${id}`) }));
    stateButtons.set(id, { button, canvas:thumb });
    strip.append(button);
  }
  const playAll = h("button", { type:"button", class:"link appearance-play-all", text:t("appearance.playAll"), onclick:() => {
    if (queue.length || playing) { queue.length = 0; playing = null; engine.playState("idle", 0); syncStrip(); wake(); return; }
    queue.push(...CATALOGUE);
    play(queue.shift()!);
  } });
  function play(id: StateId) {
    playing = id;
    const hold = makeBlock(id).duration + 0.3;
    engine.playState(id, hold);
    playingUntil = engine.time + hold;
    syncStrip();
    wake();
  }
  let playingUntil = 0;
  function syncStrip() {
    for (const [id, { button }] of stateButtons) button.setAttribute("aria-pressed", String(id === playing));
    playAll.textContent = queue.length || playing ? t("appearance.stop") : t("appearance.playAll");
  }

  // ── Live preview: a frame loop only while something moves and the page is visible.
  let raf = 0;
  let last = 0;
  let hovering = false;
  const stage = h("div", { class:"appearance-stage" }, canvas);
  stage.addEventListener("pointermove", (e) => {
    const r = canvas.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) return;
    engine.lookX = Math.tanh((e.clientX - (r.left + r.width / 2)) / 120);
    engine.lookY = -Math.tanh((e.clientY - (r.top + r.height / 2)) / 120);
    hovering = true;
    wake();
  });
  stage.addEventListener("pointerleave", () => { hovering = false; engine.lookX = 0; engine.lookY = 0; wake(); });
  function frame(now: number) {
    raf = 0;
    if (!canvas.isConnected || document.hidden) { last = 0; return; }
    const dt = last ? Math.min(0.05, (now - last) / 1000) : 0;
    last = now;
    engine.update(dt);
    if (playing && engine.time >= playingUntil) {
      playing = null;
      const next = queue.shift();
      if (next) play(next); else syncStrip();
    }
    paint();
    if (engine.busy || hovering || playing) raf = requestAnimationFrame(frame);
    else last = 0;
  }
  function wake() { if (!raf && canvas.isConnected) raf = requestAnimationFrame(frame); }
  function paint() {
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    if (canvas.width !== Math.round(PREVIEW * dpr)) { canvas.width = Math.round(PREVIEW * dpr); canvas.height = Math.round(PREVIEW * dpr); }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, PREVIEW, PREVIEW);
    engine.draw(ctx, PREVIEW, PREVIEW);
  }
  document.addEventListener("visibilitychange", () => { if (!document.hidden) wake(); });

  function draw() {
    const value = normalizeAppearance(host.settings().characterAppearance);
    BotEngine.setAppearance(value);
    shapes.sync(value.shape, busy, (c, shape) => drawStill(c, { ...value, shape }));
    colors.sync(value.color, busy);
    expressions.sync(value.expression, busy, (c, expression) => drawStill(c, { ...value, expression }));
    for (const [id, { canvas: thumb }] of stateButtons) drawStill(thumb, value, id);
    reset.disabled = busy;
    engine.update(0);
    paint();
    wake();
  }
  async function change(value: CharacterAppearance) {
    if (busy) return;
    busy = true;
    notice.textContent = t("appearance.saving");
    const previous = host.settings().characterAppearance;
    host.settings().characterAppearance = value;
    draw();
    try { await host.save(); notice.textContent = t("appearance.saved"); }
    catch { host.settings().characterAppearance = previous; notice.textContent = t("appearance.failed"); void Bridge.log("character appearance save failed"); }
    finally { busy = false; draw(); }
  }
  refresh = draw;
  draw();
  return h("div", { class:"appearance-editor" },
    h("div", { class:"appearance-heading" }, h("div", {class:"set-label-line"}, h("h3", { text:t("appearance.title") }), helpDisclosure(t("appearance.hint"), `${t("settings.help")}: ${t("appearance.title")}`)), reset),
    h("div", { class:"appearance-content" },
      h("div", { class:"appearance-preview" }, stage),
      h("div", { class:"appearance-groups" }, shapes.el, colors.el, expressions.el)),
    h("div", { class:"appearance-group" },
      h("div", { class:"appearance-states-head" }, h("h4", { id:"appearance-states-title", class:"appearance-group-title", text:t("appearance.states") }), playAll),
      strip),
    notice,
  );
}
