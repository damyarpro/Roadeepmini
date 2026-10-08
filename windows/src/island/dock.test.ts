import { afterEach, describe, expect, it } from "vitest";
import {
  EDGE_MARGIN, SHOULDER, canDragFrom, cornerRadii, dockedSize, dragEffect, dragTransform,
  islandRectFor, layoutShift, parseEdge, previewCenter, previewLayout, previewMaxCenter,
  shoulderSize, sideBotPlacement, wakeStripRect,
} from "./dock";
import { devDockFromUrl, devWindowOrigin } from "./dev-preview";
import { CHAT_MAX_H, chatPromptHeight, setChatMaxH } from "../core/layout";

const panel = { panelW: 1040, panelH: 900 };

describe("dock edges", () => {
  it("parses only the three dock edges", () => {
    expect(parseEdge("top")).toBe("top");
    expect(parseEdge("left")).toBe("left");
    expect(parseEdge("right")).toBe("right");
    expect(parseEdge("bottom")).toBeNull();
    expect(parseEdge(undefined)).toBeNull();
  });

  it("stands the compact and hidden island up on a side, never the expanded one", () => {
    expect(dockedSize("top", "compact", { w: 288, h: 32 })).toEqual({ w: 288, h: 32 });
    expect(dockedSize("left", "compact", { w: 288, h: 32 })).toEqual({ w: 32, h: 288 });
    expect(dockedSize("right", "hidden", { w: 184, h: 0 })).toEqual({ w: 0, h: 184 });
    expect(dockedSize("left", "expanded", { w: 640, h: 160 })).toEqual({ w: 640, h: 160 });
  });
});

describe("island rect in its window", () => {
  it("hangs from the top edge, centred", () => {
    expect(islandRectFor("top", { panelW: 720, panelH: 480 }, 288, 32)).toEqual({ x: 216, y: EDGE_MARGIN, w: 288, h: 32 });
  });

  it("hugs the left or right edge, centred vertically", () => {
    expect(islandRectFor("left", panel, 32, 288)).toEqual({ x: EDGE_MARGIN, y: 306, w: 32, h: 288 });
    expect(islandRectFor("right", panel, 640, 160)).toEqual({ x: 1040 - EDGE_MARGIN - 640, y: 370, w: 640, h: 160 });
  });

  it("keeps its centre along the edge whatever its size, so growth needs no window move", () => {
    for (const [w, h] of [[32, 288], [640, 160], [640, 832], [1000, 832]]) {
      const r = islandRectFor("left", panel, w, h);
      expect(r.y + r.h / 2).toBe(450);
      expect(r.x).toBe(EDGE_MARGIN);
    }
  });

  it("measures the shift that keeps the island in place across a layout change", () => {
    const top = { edge: "top" as const, panelW: 1168, panelH: 972 };
    const left = { edge: "left" as const, panelW: 1040, panelH: 900 };
    // A horizontal compact pill: centre (584, 32) at the top, (160, 450) on the left.
    expect(layoutShift(top, left, 288, 32)).toEqual({ dx: 584 - 160, dy: 32 - 450 });
    expect(layoutShift(left, left, 32, 288)).toEqual({ dx: 0, dy: 0 });
  });
});

describe("shape", () => {
  it("squares the edge-side corners while docked and rounds them while floating", () => {
    expect(cornerRadii("top", 14, 1)).toBe("0px 0px 14px 14px");
    expect(cornerRadii("left", 22, 1)).toBe("0px 22px 22px 0px");
    expect(cornerRadii("right", 22, 1)).toBe("22px 0px 0px 22px");
    expect(cornerRadii("left", 14, 0)).toBe("14px 14px 14px 14px");
    expect(cornerRadii("top", 14, 0.5)).toBe("7px 7px 14px 14px");
  });

  it("sizes the shoulders by the island's depth from the edge and its attachment", () => {
    expect(shoulderSize("top", 288, 32, 1)).toBe(SHOULDER);
    expect(shoulderSize("top", 184, 6, 1)).toBe(6);
    expect(shoulderSize("left", 10, 288, 1)).toBe(10);
    expect(shoulderSize("right", 640, 160, 0)).toBe(0);
  });

  it("lays the wake strip along the docked edge", () => {
    expect(wakeStripRect("top", { panelW: 720, panelH: 480 })).toEqual({ x: 208, y: EDGE_MARGIN, w: 304, h: 8 });
    expect(wakeStripRect("left", panel)).toEqual({ x: EDGE_MARGIN, y: 298, w: 8, h: 304 });
    expect(wakeStripRect("right", panel)).toEqual({ x: 1040 - EDGE_MARGIN - 8, y: 298, w: 8, h: 304 });
  });

  it("moves the compact character to the top end when the island stands up", () => {
    expect(sideBotPlacement("left", "compact", { cx: 40, cy: 16, diameter: 20 })).toEqual({ cx: 16, cy: 40, diameter: 20 });
    expect(sideBotPlacement("top", "compact", { cx: 40, cy: 16 })).toEqual({ cx: 40, cy: 16 });
    expect(sideBotPlacement("right", "expanded", { cx: 68, cy: 100 })).toEqual({ cx: 68, cy: 100 });
  });
});

describe("drag effect", () => {
  it("stretches along the motion, capped, and leans into it", () => {
    const slow = dragEffect(260, 0);
    expect(slow.sx).toBeCloseTo(0.1);
    expect(slow.sy).toBe(0);
    expect(slow.tilt).toBeGreaterThan(0);
    const fast = dragEffect(-20000, 20000);
    expect(Math.hypot(fast.sx, fast.sy)).toBeCloseTo(0.22);
    expect(fast.tilt).toBe(-5);
  });

  it("leaves a resting island untransformed", () => {
    expect(dragTransform(0, 0, 0)).toBe("");
    const t = dragTransform(0.2, 0, 2);
    expect(t).toContain("rotate(2deg)");
    expect(t).toContain("scale(1.2, 0.88)");
  });
});

describe("where a drag may start", () => {
  afterEach(() => document.body.replaceChildren());

  function island() {
    document.body.innerHTML = `
      <div id="island">
        <div id="content">
          <div id="header"><div class="tabs"><button class="tab">h</button></div><div class="spacer"></div></div>
          <div id="views">
            <div class="view on"><div class="card"><div class="stack"><div class="title">Hello</div></div>
              <input class="chat-input"><a href="#">link</a></div>
              <div class="chat-log"><div class="chat-row"><div class="box"></div></div></div>
            </div>
          </div>
        </div>
      </div>`;
    return document.getElementById("island")!;
  }

  it("starts from the header bar and the card's empty margins", () => {
    const el = island();
    expect(canDragFrom(el.querySelector(".spacer"), el)).toBe(true);
    expect(canDragFrom(el.querySelector("#header"), el)).toBe(true);
    expect(canDragFrom(el.querySelector(".card"), el)).toBe(true);
    expect(canDragFrom(el.querySelector(".stack"), el)).toBe(true);
  });

  it("never from controls, text, the chat log or outside the island", () => {
    const el = island();
    expect(canDragFrom(el.querySelector(".tab"), el)).toBe(false);
    expect(canDragFrom(el.querySelector(".title"), el)).toBe(false);
    expect(canDragFrom(el.querySelector("input"), el)).toBe(false);
    expect(canDragFrom(el.querySelector("a"), el)).toBe(false);
    expect(canDragFrom(el.querySelector(".chat-log .box"), el)).toBe(false);
    expect(canDragFrom(document.body, el)).toBe(false);
    expect(canDragFrom(null, el)).toBe(false);
  });
});

describe("browser preview layout (mirrors dock.rs)", () => {
  const work = { w: 1920, h: 1032 };

  it("matches the Rust sizes for a top dock", () => {
    expect(previewLayout("top", 0.5, work)).toEqual({
      edge: "top", panelW: 1168, panelH: 972, chatMaxH: 932, maxW: 1100, maxH: 932,
    });
  });

  it("matches the Rust sizes for a side dock, shorter chat near an end", () => {
    expect(previewLayout("left", 0.5, work)).toEqual({
      edge: "left", panelW: 1040, panelH: 900, chatMaxH: 832, maxW: 1000, maxH: 832,
    });
    expect(previewLayout("right", 0, work).chatMaxH).toBe(280);
    expect(previewLayout("right", 0, work).panelH).toBe(900);
  });

  it("clamps the centre and pulls the maximised island in", () => {
    expect(previewCenter("top", 0, work)).toBe(340);
    expect(previewCenter("left", 1, work)).toBe(1032 - 240);
    expect(previewMaxCenter("top", 0, work)).toBe(570);
    expect(previewMaxCenter("left", 0, work)).toBe(516);
  });

  it("reads the dock and work area from the query string", () => {
    const d = devDockFromUrl("?dock=right&pos=0.25&wa=1920x1040", { w: 800, h: 600 });
    expect(d).toEqual({ edge: "right", pos: 0.25, work: { w: 1920, h: 1040 } });
    expect(devDockFromUrl("?dock=bottom&pos=x", { w: 800, h: 600 })).toEqual({ edge: "top", pos: 0.5, work: { w: 800, h: 600 } });
    const o = devWindowOrigin(d, previewLayout("right", 0.25, d.work), false);
    expect(o.x).toBe(1920 - 1040 + EDGE_MARGIN);
  });
});

describe("chat height limit", () => {
  afterEach(() => setChatMaxH(440));

  it("follows the display", () => {
    expect(CHAT_MAX_H).toBe(440);
    setChatMaxH(932);
    expect(CHAT_MAX_H).toBe(932);
    expect(chatPromptHeight(3, 900)).toBe(900);
    expect(chatPromptHeight(3, 2000)).toBe(932);
    setChatMaxH(100);
    expect(CHAT_MAX_H).toBe(240);
    setChatMaxH(Number.NaN);
    expect(CHAT_MAX_H).toBe(240);
  });
});
