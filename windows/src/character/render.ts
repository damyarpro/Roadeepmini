// Draws one bloub frame (bloub/engine.ts `sample()`) on a Canvas 2D context.
//
// bloub renders SVG: a body path, eye capsules punched out of it through a <mask>, dots,
// a notification pastille and 3D rings split into a back half (behind the body) and a
// front half. Canvas does the same with Path2D: the eyes are holes, either erased
// (`destination-out`, so whatever is behind the canvas shows through — the island's
// black or its glow) or filled with a known paper colour when the canvas is shared.

import type { BotFrame } from "./bloub/engine";
import { NOTIF_BLUE } from "./bloub/decor";
import { mixHex } from "./bloub/skins";
import type { Glyph } from "./fixed";

export interface PaintOptions {
  /** Body colour, `#rrggbb`. */
  ink: string;
  /**
   * What the eye holes show. `null` erases them (the canvas must hold only this
   * character); a colour fills them with it (a canvas shared with other drawings).
   */
  paper: string | null;
  /** Background the particles fade into (and the hole colour's fallback). */
  backdrop: string;
  /** Eye size multiplier around each eye's centre (hover, tiny mini characters). */
  eyeScale?: number;
  /** Body squash around the centre. */
  sx?: number;
  sy?: number;
  /** A mailbox slot cut near the top: 0…1 open × 0…1 morph. */
  slot?: { open: number; morph: number };
}

/** Engine units per ball radius (bloub `RAYON`). */
export const UNIT = 100;

const pathCache = new Map<string, Path2D>();
/** Eye capsules repeat between frames (only their transform moves): cache them. */
function cachedPath(d: string): Path2D {
  let p = pathCache.get(d);
  if (!p) {
    if (pathCache.size > 256) pathCache.clear();
    p = new Path2D(d);
    pathCache.set(d, p);
  }
  return p;
}

function luminance(hex: string): number {
  const v = parseInt(hex.slice(1), 16);
  return (0.2126 * ((v >> 16) & 255) + 0.7152 * ((v >> 8) & 255) + 0.0722 * (v & 255)) / 255;
}

function strokeArcs(x: CanvasRenderingContext2D, frame: BotFrame, half: "back" | "front") {
  for (const arc of frame.arcs) {
    const d = arc[half];
    if (!d) continue;
    const g = x.createLinearGradient(arc.grad.x1, arc.grad.y1, arc.grad.x2, arc.grad.y2);
    arc.grad.stops.forEach((c, i) => g.addColorStop(i / (arc.grad.stops.length - 1), c));
    x.globalAlpha = arc.opacity;
    x.strokeStyle = g;
    x.lineWidth = arc.width;
    x.stroke(new Path2D(d));
  }
  x.globalAlpha = 1;
}

function fillDots(x: CanvasRenderingContext2D, frame: Pick<BotFrame, "dots">, o: PaintOptions, alpha = 1) {
  for (const dot of frame.dots) {
    x.globalAlpha = dot.opacity * alpha;
    x.fillStyle = dot.color ?? (dot.depth === undefined ? o.ink : mixHex(o.backdrop, o.ink, dot.depth));
    if (dot.d) {
      x.save();
      x.translate(dot.x, dot.y);
      x.rotate(((dot.rot ?? 0) * Math.PI) / 180);
      x.scale(UNIT, UNIT);
      x.fill(cachedPath(dot.d));
      x.restore();
    } else {
      x.beginPath();
      x.arc(dot.x, dot.y, dot.r, 0, Math.PI * 2);
      x.fill();
    }
  }
  x.globalAlpha = 1;
}

/**
 * Paints `frame` centred on (cx, cy), one ball radius = `radius` CSS pixels.
 * The caller has applied its DPR transform; the context state is restored on return.
 */
export function paintFrame(x: CanvasRenderingContext2D, frame: BotFrame & { glyph?: Glyph | null }, cx: number, cy: number, radius: number, o: PaintOptions) {
  const k = radius / UNIT;
  if (!(k > 0) || !Number.isFinite(cx + cy)) return;
  x.save();
  x.translate(cx, cy);
  x.scale(k * (o.sx ?? 1), k * (o.sy ?? 1));
  x.lineCap = "round";
  x.lineJoin = "round";

  // Back half of the rings, then the burst's particles: the body occludes both.
  strokeArcs(x, frame, "back");
  if (frame.dotsBehind) fillDots(x, frame, o);

  const body = new Path2D(frame.bodyPath);
  x.globalAlpha = frame.bodyAlpha;
  x.fillStyle = o.ink;
  x.fill(body);
  // An ink body on the black island would vanish: a faint rim keeps its outline.
  if (o.paper === null && luminance(o.ink) < 0.12) {
    x.strokeStyle = "rgba(255,255,255,0.22)";
    x.lineWidth = 1.2 / k;
    x.stroke(body);
  }

  // Holes: the eyes, the pastille's notch and the mailbox slot, clipped to the body.
  x.save();
  x.clip(body);
  if (o.paper === null) x.globalCompositeOperation = "destination-out";
  else x.fillStyle = o.paper;
  const es = o.eyeScale ?? 1;
  for (const eye of frame.eyes) {
    const [a, b, c, d, e, f] = eye.m;
    x.save();
    x.globalAlpha = frame.bodyAlpha * eye.alpha;
    x.transform(a * es, b * es, c * es, d * es, e, f);
    x.fill(cachedPath(eye.d));
    x.restore();
  }
  if (frame.notch) {
    x.globalAlpha = 1;
    x.beginPath();
    x.arc(frame.notch.x, frame.notch.y, frame.notch.r, 0, Math.PI * 2);
    x.fill();
  }
  const slot = o.slot;
  if (slot && slot.morph > 0.05 && slot.open > 0.01) {
    const w = UNIT * 1.3 * slot.morph;
    const h = Math.min(UNIT * 0.5, UNIT * slot.open * slot.morph);
    const top = -UNIT * 0.62;
    x.globalAlpha = 1;
    x.beginPath();
    x.roundRect(-w / 2, top, w, h, Math.min(w, h) / 2);
    x.fill();
  }
  x.restore();
  x.globalAlpha = 1;

  if (!frame.dotsBehind) fillDots(x, frame, o);
  if (frame.notif) {
    x.fillStyle = NOTIF_BLUE;
    x.beginPath();
    x.arc(frame.notif.x, frame.notif.y, frame.notif.r, 0, Math.PI * 2);
    x.fill();
  }
  strokeArcs(x, frame, "front");

  // A glyph state's own silhouette ("…", "!", the sleep dot), miniature beside the head.
  const g = frame.glyph;
  if (g && g.alpha > 0.01) {
    x.save();
    x.translate(g.x, g.y);
    x.scale(g.scale, g.scale);
    x.globalAlpha = g.alpha;
    x.fillStyle = o.ink;
    const glyphPath = new Path2D(g.bodyPath);
    x.fill(glyphPath);
    fillDots(x, g, o, g.alpha);
    // Same faint rim as the body for an ink colour on the black island.
    if (o.paper === null && luminance(o.ink) < 0.12) {
      x.globalAlpha = g.alpha;
      x.strokeStyle = "rgba(255,255,255,0.22)";
      x.lineWidth = 1.2 / (k * g.scale);
      x.stroke(glyphPath);
      for (const dot of g.dots) {
        if (dot.d) continue;
        x.beginPath();
        x.arc(dot.x, dot.y, dot.r, 0, Math.PI * 2);
        x.stroke();
      }
    }
    x.restore();
  }
  x.restore();
}
