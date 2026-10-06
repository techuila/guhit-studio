import { describe, expect, it } from "vitest";
import { LABEL_OFFSET, bubbleAt, centeredOn, labelFlip, onCanvas, planToCanvas, sameView } from "./placement";

describe("planToCanvas", () => {
  it("uses the plan transform: +y north is up on screen", () => {
    const view = { scale: 0.1, ox: 100, oy: 500 };
    expect(planToCanvas(view, { x: 1000, y: 2000 })).toEqual({ x: 200, y: 300 });
    expect(planToCanvas(view, { x: 0, y: 0 })).toEqual({ x: 100, y: 500 });
  });

  it("centers a point at the same zoom", () => {
    const view = { scale: 0.08, ox: 12, oy: 700 };
    const next = centeredOn(view, { x: 4000, y: 3000 }, 800, 600);
    expect(next.scale).toBe(0.08);
    expect(planToCanvas(next, { x: 4000, y: 3000 })).toEqual({ x: 400, y: 300 });
  });
});

describe("onCanvas", () => {
  it("knows when a cursor is on the canvas", () => {
    expect(onCanvas({ x: 10, y: 10 }, 800, 600)).toBe(true);
    expect(onCanvas({ x: -1, y: 10 }, 800, 600)).toBe(false);
    expect(onCanvas({ x: -1, y: 10 }, 800, 600, 4)).toBe(true);
    expect(onCanvas({ x: 801, y: 601 }, 800, 600)).toBe(false);
  });
});

describe("labelFlip", () => {
  it("opens right and down in the open", () => {
    expect(labelFlip({ x: 100, y: 100 }, 1000, 700)).toEqual({ x: false, y: false });
  });

  it("opens left near the right edge and up near the bottom", () => {
    expect(labelFlip({ x: 950, y: 100 }, 1000, 700)).toEqual({ x: true, y: false });
    expect(labelFlip({ x: 100, y: 690 }, 1000, 700)).toEqual({ x: false, y: true });
  });

  it("never flips in a pane too small to have another side", () => {
    expect(labelFlip({ x: 250, y: 110 }, 260, 120)).toEqual({ x: false, y: false });
  });
});

describe("bubbleAt", () => {
  const size = { w: 180, h: 40 };
  it("sits beside the pointer", () => {
    expect(bubbleAt({ x: 100, y: 100 }, size, 800, 600)).toEqual({ x: 100 + LABEL_OFFSET.x, y: 100 + LABEL_OFFSET.y });
  });

  it("moves to the other side at an edge and stays on the canvas", () => {
    const p = bubbleAt({ x: 790, y: 590 }, size, 800, 600);
    expect(p.x + size.w).toBeLessThanOrEqual(800);
    expect(p.y + size.h).toBeLessThanOrEqual(600);
    expect(p.x).toBeLessThan(790);
    const tiny = bubbleAt({ x: 50, y: 20 }, size, 120, 50);
    expect(tiny.x).toBeGreaterThanOrEqual(6);
    expect(tiny.y).toBeGreaterThanOrEqual(6);
  });
});

describe("sameView", () => {
  it("ignores sub-pixel noise but sees a real change", () => {
    const v = { scale: 0.1, ox: 10, oy: 20 };
    expect(sameView(null, v)).toBe(false);
    expect(sameView(v, { ...v, ox: 10.001 })).toBe(true);
    expect(sameView(v, { ...v, ox: 10.5 })).toBe(false);
    expect(sameView(v, { ...v, scale: 0.11 })).toBe(false);
  });
});
