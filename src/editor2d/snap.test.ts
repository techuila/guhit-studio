import { describe, expect, it } from "vitest";
import { pt } from "./geom";
import { computeIntersections, snap, type SnapOptions, type SnapScene } from "./snap";

// An 8000 x 6000 box with a partition at x = 5000.
function scene(): SnapScene {
  const c = [
    { a: pt(0, 0), b: pt(8000, 0) },
    { a: pt(8000, 0), b: pt(8000, 6000) },
    { a: pt(8000, 6000), b: pt(0, 6000) },
    { a: pt(0, 6000), b: pt(0, 0) },
    { a: pt(5000, 0), b: pt(5000, 6000) },
  ];
  const s: SnapScene = {
    centerlines: c,
    faces: [{ a: pt(75, 75), b: pt(4925, 75) }],
    points: c.flatMap((x) => [x.a, x.b]),
    facePoints: [pt(75, 75)],
    intersections: [],
  };
  computeIntersections(s);
  return s;
}

const base: SnapOptions = { enabled: true, ortho: false, tolerance: 120, gridStep: 100, anchor: null };

describe("snap", () => {
  it("prefers endpoints", () => {
    const r = snap(pt(60, -50), scene(), base);
    expect(r.type).toBe("endpoint");
    expect(r.point).toEqual(pt(0, 0));
  });

  it("finds centerline intersections", () => {
    const s = scene();
    s.points = [];
    const r = snap(pt(5040, 30), s, base);
    expect(r.type).toBe("intersection");
    expect(r.point).toEqual(pt(5000, 0));
  });

  it("finds midpoints", () => {
    const r = snap(pt(8050, 3040), scene(), base);
    expect(r.type).toBe("midpoint");
    expect(r.point).toEqual(pt(8000, 3000));
  });

  it("snaps to the nearest point on a wall at a round distance", () => {
    const r = snap(pt(1234, 40), scene(), base);
    expect(r.type).toBe("nearest");
    expect(r.point).toEqual(pt(1200, 0));
  });

  it("uses faces only when asked", () => {
    const s = scene();
    const off = snap(pt(2010, 600), s, { ...base, tolerance: 80 });
    expect(off.type).not.toBe("face");
    const s2 = scene();
    s2.centerlines = [];
    s2.points = [];
    const on = snap(pt(2010, 100), s2, { ...base, tolerance: 80, useFaces: true });
    expect(on.type).toBe("face");
    expect(on.point.y).toBeCloseTo(75);
  });

  it("falls back to the grid", () => {
    const r = snap(pt(2449, 3051), { ...scene(), points: [] }, base);
    expect(r.type).toBe("grid");
    expect(r.point).toEqual(pt(2400, 3100));
  });

  it("locks to 0/45/90 near those angles and rounds the length", () => {
    const r = snap(pt(12040, 2030), { ...scene(), points: [] }, { ...base, anchor: pt(10000, 2000) });
    expect(r.type).toBe("angle");
    expect(r.angleLocked).toBe(true);
    expect(r.point.x).toBeCloseTo(12000);
    expect(r.point.y).toBeCloseTo(2000);
    const d = snap(pt(11010, 3000), { ...scene(), points: [] }, { ...base, anchor: pt(10000, 2000) });
    expect(d.angleLocked).toBe(true);
    expect(d.point.x - 10000).toBeCloseTo(d.point.y - 2000);
  });

  it("does not lock far from a lock angle", () => {
    const r = snap(pt(12000, 2700), { ...scene(), points: [] }, { ...base, anchor: pt(10000, 2000) });
    expect(r.angleLocked).toBe(false);
    expect(r.type).toBe("grid");
  });

  it("ortho forces 0/90 even with snapping off", () => {
    const r = snap(pt(12000, 2700), scene(), { ...base, enabled: false, ortho: true, anchor: pt(10000, 2000) });
    expect(r.angleLocked).toBe(true);
    expect(r.point).toEqual(pt(12000, 2000));
    expect(r.guides[0].kind).toBe("angle");
  });

  it("aligns with other endpoints along a locked ray", () => {
    // Drawing east from (1000, 3000): the ray meets the vertical through x = 5000.
    const r = snap(pt(5030, 3020), scene(), { ...base, anchor: pt(1000, 3000) });
    expect(r.angleLocked).toBe(true);
    expect(r.point.x).toBeCloseTo(5000);
    expect(r.point.y).toBeCloseTo(3000);
    expect(["intersection", "extension", "midpoint"]).toContain(r.type);
  });

  it("gives extension guides for a free cursor", () => {
    const s = scene();
    s.centerlines = [];
    s.intersections = [];
    const r = snap(pt(8020, 9040), s, base);
    expect(r.type).toBe("extension");
    expect(r.point.x).toBe(8000);
    expect(r.guides.length).toBeGreaterThan(0);
    expect(r.guides[0].kind).toBe("extension");
  });

  it("finds the perpendicular foot from the anchor", () => {
    const s = scene();
    s.points = [];
    s.intersections = [];
    // Anchor inside the room, cursor near the east wall but not at a lock angle.
    const r = snap(pt(7950, 2260), s, { ...base, anchor: pt(6000, 2250), angleToleranceDeg: 0.01 });
    expect(r.type).toBe("perpendicular");
    expect(r.point).toEqual(pt(8000, 2250));
  });

  it("returns the raw cursor when disabled", () => {
    const r = snap(pt(61, -49), scene(), { ...base, enabled: false });
    expect(r.type).toBe("none");
    expect(r.point).toEqual(pt(61, -49));
  });
});
