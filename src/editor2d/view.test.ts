import { describe, expect, it } from "vitest";
import { pt } from "./geom";
import { MAX_SCALE, MIN_SCALE, boundsVisible, fitRect, gridSteps, snapStep, toScreen, toWorld, zoomAt } from "./view";

describe("view", () => {
  it("flips y and round trips", () => {
    const v = { scale: 0.1, ox: 100, oy: 500 };
    const s = toScreen(v, pt(1000, 2000));
    expect(s).toEqual(pt(200, 300));
    const w = toWorld(v, s);
    expect(w.x).toBeCloseTo(1000);
    expect(w.y).toBeCloseTo(2000);
  });

  it("zooms about the cursor", () => {
    const v = { scale: 0.1, ox: 100, oy: 500 };
    const cursor = pt(340, 220);
    const before = toWorld(v, cursor);
    const z = zoomAt(v, cursor, 1.7);
    const after = toWorld(z, cursor);
    expect(after.x).toBeCloseTo(before.x);
    expect(after.y).toBeCloseTo(before.y);
    expect(z.scale).toBeCloseTo(0.17);
  });

  it("clamps zoom", () => {
    const v = { scale: 1, ox: 0, oy: 0 };
    expect(zoomAt(v, pt(0, 0), 1000).scale).toBe(MAX_SCALE);
    expect(zoomAt(v, pt(0, 0), 1e-9).scale).toBe(MIN_SCALE);
  });

  it("fits bounds centered", () => {
    const v = fitRect({ minX: 0, minY: 0, maxX: 8000, maxY: 6000 }, 1000, 800, 50);
    const c = toScreen(v, pt(4000, 3000));
    expect(c.x).toBeCloseTo(500);
    expect(c.y).toBeCloseTo(400);
    expect(toScreen(v, pt(0, 0)).x).toBeGreaterThanOrEqual(50 - 1e-6);
    expect(toScreen(v, pt(0, 6000)).y).toBeGreaterThanOrEqual(50 - 1e-6);
  });

  it("checks whether bounds still fit the viewport", () => {
    const v = fitRect({ minX: 0, minY: 0, maxX: 8000, maxY: 6000 }, 1000, 800, 50);
    expect(boundsVisible(v, { minX: 0, minY: 0, maxX: 8000, maxY: 6000 }, 1000, 800)).toBe(true);
    // Shrinking the viewport a lot without rescaling crops the same bounds.
    expect(boundsVisible(v, { minX: 0, minY: 0, maxX: 8000, maxY: 6000 }, 400, 800)).toBe(false);
    expect(boundsVisible(v, { minX: 0, minY: 0, maxX: 0, maxY: 0 }, 1000, 800)).toBe(true);
  });

  it("adapts the grid", () => {
    const far = gridSteps(100, 0.01);
    expect(far.minor * 0.01).toBeGreaterThanOrEqual(9);
    expect(far.major).toBeGreaterThan(far.minor);
    const normal = gridSteps(100, 0.12);
    expect(normal.minor).toBe(100);
    expect(normal.major).toBe(1000);
    const near = gridSteps(100, 3);
    expect(near.minor).toBe(10);
    expect(snapStep(100, 3)).toBe(10);
    expect(snapStep(100, 0.01)).toBe(100);
  });
});
