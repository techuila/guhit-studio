import { describe, expect, it } from "vitest";
import {
  angleDeg,
  closestOnSegment,
  dirDeg,
  lineLineIntersection,
  orientedRect,
  pointInPolygon,
  polygonArea,
  pt,
  readableDeg,
  rectFromPoints,
  rotate,
  segSegIntersection,
  shapeInsideRect,
  shapeIntersectsRect,
  thickSegment,
} from "./geom";

describe("geom", () => {
  it("rotates counter-clockwise", () => {
    const p = rotate(pt(1, 0), 90);
    expect(p.x).toBeCloseTo(0);
    expect(p.y).toBeCloseTo(1);
    expect(angleDeg(pt(0, -1))).toBeCloseTo(270);
  });

  it("gives exact axis directions", () => {
    expect(dirDeg(180)).toEqual({ x: -1, y: 0 });
    expect(dirDeg(270)).toEqual({ x: 0, y: -1 });
    expect(dirDeg(45).x).toBeCloseTo(Math.SQRT1_2);
  });

  it("finds the closest point on a segment, clamped", () => {
    const c = closestOnSegment(pt(5, 3), pt(0, 0), pt(10, 0));
    expect(c.point).toEqual(pt(5, 0));
    expect(c.dist).toBeCloseTo(3);
    expect(closestOnSegment(pt(-4, 3), pt(0, 0), pt(10, 0)).point).toEqual(pt(0, 0));
  });

  it("intersects segments and lines", () => {
    expect(segSegIntersection(pt(0, 0), pt(10, 10), pt(0, 10), pt(10, 0))).toEqual(pt(5, 5));
    expect(segSegIntersection(pt(0, 0), pt(1, 1), pt(0, 10), pt(10, 0))).toBeNull();
    expect(lineLineIntersection(pt(0, 0), pt(1, 1), pt(0, 10), pt(10, 0))).toEqual(pt(5, 5));
    expect(lineLineIntersection(pt(0, 0), pt(1, 0), pt(0, 1), pt(1, 1))).toBeNull();
  });

  it("tests polygons", () => {
    const sq = [pt(0, 0), pt(10, 0), pt(10, 10), pt(0, 10)];
    expect(pointInPolygon(pt(5, 5), sq)).toBe(true);
    expect(pointInPolygon(pt(15, 5), sq)).toBe(false);
    expect(polygonArea(sq)).toBeCloseTo(100);
  });

  it("builds a wall rectangle counter-clockwise", () => {
    const r = thickSegment(pt(0, 0), pt(1000, 0), 150);
    expect(r[0]).toEqual(pt(0, -75));
    expect(r[2]).toEqual(pt(1000, 75));
    expect(polygonArea(r)).toBeGreaterThan(0);
  });

  it("window and crossing tests", () => {
    const shape = orientedRect(pt(50, 50), 40, 20, 0);
    expect(shapeInsideRect(shape, rectFromPoints(pt(0, 0), pt(100, 100)))).toBe(true);
    expect(shapeInsideRect(shape, rectFromPoints(pt(0, 0), pt(60, 100)))).toBe(false);
    expect(shapeIntersectsRect(shape, rectFromPoints(pt(0, 0), pt(60, 100)), true)).toBe(true);
    expect(shapeIntersectsRect(shape, rectFromPoints(pt(200, 200), pt(300, 300)), true)).toBe(false);
    // rectangle fully inside the polygon still counts as touching
    expect(shapeIntersectsRect(shape, rectFromPoints(pt(45, 45), pt(55, 55)), true)).toBe(true);
    // a segment crossing the rectangle with both ends outside
    expect(shapeIntersectsRect([pt(-10, 50), pt(200, 50)], rectFromPoints(pt(0, 0), pt(100, 100)), false)).toBe(true);
  });

  it("keeps text readable", () => {
    expect(readableDeg(180)).toBeCloseTo(0);
    expect(readableDeg(90)).toBeCloseTo(90);
    expect(readableDeg(135)).toBeCloseTo(-45);
    expect(readableDeg(270)).toBeCloseTo(90);
    expect(readableDeg(300)).toBeCloseTo(-60);
  });
});
