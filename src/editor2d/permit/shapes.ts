// Plan geometry of the permit annotations (lane L1): grid bubbles, section
// heads and elevation targets, dimension strings and the lot's sides. Shared
// by drawing (draw.ts), picking (hitPermit.ts) and the plan shapes in
// model.ts. Pure, tested in shapes.test.ts.

import type { Dimension, GridLine, Lot, ViewMark } from "../../contract/bindings";
import type { P } from "../geom";
import { add, dist, dot, left, lerp, mul, polygonArea, sub, unit } from "../geom";
import { lookDir } from "./labels";

/** Screen sizes in CSS pixels at uiScale 1. Symbols keep their size on screen at any zoom. */
export const GRID_BUBBLE_PX = 11;
export const SECTION_HEAD_PX = 10;
export const ELEVATION_TARGET_PX = 12;

/**
 * Plan mm per CSS pixel from the room label height the plan passes to
 * shapes and hits (render.ts `labelHeightMm`, 12 px).
 */
export function mmPerPx(labelHeightMm: number): number {
  return labelHeightMm / 12;
}

function dirOf(a: P, b: P): P {
  return dist(a, b) < 1e-9 ? { x: 1, y: 0 } : unit(sub(b, a));
}

/** Points around a circle, starting at angle `fromRad` and closing back on it. */
export function circlePoints(c: P, r: number, fromRad = 0, n = 20): P[] {
  const out: P[] = [];
  for (let i = 0; i <= n; i++) {
    const a = fromRad + (i / n) * Math.PI * 2;
    out.push({ x: c.x + r * Math.cos(a), y: c.y + r * Math.sin(a) });
  }
  return out;
}

// ------------------------------------------------------------------ grids

export interface GridGeometry {
  dir: P;
  /** Bubble centers, one past each end so the line meets the circle. */
  bubbleA: P;
  bubbleB: P;
}

export function gridGeometry(g: Pick<GridLine, "start" | "end">, rMm: number): GridGeometry {
  const dir = dirOf(g.start, g.end);
  return { dir, bubbleA: sub(g.start, mul(dir, rMm)), bubbleB: add(g.end, mul(dir, rMm)) };
}

/** Bubble, line, bubble as one open outline: what the selection traces and the marquee tests. */
export function gridOutline(g: Pick<GridLine, "start" | "end">, rMm: number): P[] {
  const geo = gridGeometry(g, rMm);
  const towardLine = Math.atan2(geo.dir.y, geo.dir.x);
  return [...circlePoints(geo.bubbleA, rMm, towardLine), ...circlePoints(geo.bubbleB, rMm, towardLine + Math.PI)];
}

// ------------------------------------------------------------------ view marks

export interface MarkGeometry {
  dir: P;
  /** Unit vector of where it looks. */
  look: P;
  mid: P;
  /** Section heads, one past each end of the cut. */
  headA: P;
  headB: P;
}

export function markGeometry(m: Pick<ViewMark, "start" | "end" | "flip">, rMm: number): MarkGeometry {
  const dir = dirOf(m.start, m.end);
  return {
    dir,
    look: lookDir(m.start, m.end, m.flip),
    mid: lerp(m.start, m.end, 0.5),
    headA: sub(m.start, mul(dir, rMm)),
    headB: add(m.end, mul(dir, rMm)),
  };
}

/** The radius of a mark's symbol in plan mm. */
export function markRadiusMm(m: Pick<ViewMark, "mark_kind">, pxMm: number): number {
  return (m.mark_kind === "section" ? SECTION_HEAD_PX : ELEVATION_TARGET_PX) * pxMm;
}

/** The mark's outline: head, cut, head for a section; the plane through the target for an elevation. */
export function markOutline(m: Pick<ViewMark, "start" | "end" | "flip" | "mark_kind">, rMm: number): P[] {
  const g = markGeometry(m, rMm);
  const towardLine = Math.atan2(g.dir.y, g.dir.x);
  if (m.mark_kind === "section") {
    return [...circlePoints(g.headA, rMm, towardLine), ...circlePoints(g.headB, rMm, towardLine + Math.PI)];
  }
  const back = Math.atan2(-g.dir.y, -g.dir.x);
  return [m.start, ...circlePoints(g.mid, rMm, back), m.end];
}

// ------------------------------------------------------------------ dimension strings

/** A dimension string: witness points between a and b. render.ts leaves these to `drawDimensionStrings`. */
export function isDimensionString(d: Pick<Dimension, "through">): boolean {
  return (d.through?.length ?? 0) > 0;
}

export interface StringGeometry {
  dir: P;
  normal: P;
  /** Each witness point (a, through..., b) on the dimension line. */
  onLine: P[];
  /** Position of each point along `dir` from a, mm. */
  ts: number[];
  /** Ends of the dimension line: the first and last point along it. */
  p1: P;
  p2: P;
  segments: { from: P; to: P; length: number; mid: P }[];
}

export function stringPoints(d: Pick<Dimension, "a" | "b" | "through">): P[] {
  return [d.a, ...(d.through ?? []), d.b];
}

/** Every witness point projected onto the line at `offset_mm`, along a to b. */
export function stringGeometry(d: Pick<Dimension, "a" | "b" | "through" | "offset_mm">): StringGeometry {
  const pts = stringPoints(d);
  const dir = dirOf(d.a, d.b);
  const normal = left(dir);
  const base = add(d.a, mul(normal, d.offset_mm));
  const ts = pts.map((p) => dot(sub(p, d.a), dir));
  const onLine = ts.map((t) => add(base, mul(dir, t)));
  let lo = 0;
  let hi = 0;
  ts.forEach((t, i) => {
    if (t < ts[lo]) lo = i;
    if (t > ts[hi]) hi = i;
  });
  const segments = [];
  for (let i = 0; i + 1 < onLine.length; i++) {
    segments.push({ from: onLine[i], to: onLine[i + 1], length: Math.abs(ts[i + 1] - ts[i]), mid: lerp(onLine[i], onLine[i + 1], 0.5) });
  }
  return { dir, normal, onLine, ts, p1: onLine[lo], p2: onLine[hi], segments };
}

// ------------------------------------------------------------------ the lot

/** +1 when the corners run counter-clockwise, -1 clockwise. */
export function windingOf(corners: readonly P[]): 1 | -1 {
  return polygonArea(corners) >= 0 ? 1 : -1;
}

/** The corners counter-clockwise, as the contract stores them. */
export function counterClockwise(corners: readonly P[]): P[] {
  return windingOf(corners) === 1 ? [...corners] : [...corners].reverse();
}

export interface LotSideGeo {
  from: P;
  to: P;
  mid: P;
  dir: P;
  /** Unit normal pointing out of the lot. */
  out: P;
  length: number;
}

/** Side i runs from corner i to i + 1 (the last back to the first). */
export function lotSides(lot: Pick<Lot, "corners">): LotSideGeo[] {
  const c = lot.corners;
  const w = windingOf(c);
  return c.map((from, i) => {
    const to = c[(i + 1) % c.length];
    const dir = dirOf(from, to);
    // Counter-clockwise, the outside is on the right of each side.
    const out = mul(left(dir), -w);
    return { from, to, mid: lerp(from, to, 0.5), dir, out, length: dist(from, to) };
  });
}

/** The outward direction at corner i: between its two sides' outward normals. */
export function cornerOut(lot: Pick<Lot, "corners">, i: number): P {
  const sides = lotSides(lot);
  const n = sides.length;
  if (n === 0) return { x: 0, y: 1 };
  const a = sides[(i - 1 + n) % n].out;
  const b = sides[i].out;
  const s = add(a, b);
  return Math.hypot(s.x, s.y) < 1e-9 ? b : unit(s);
}

/** The lot outline closed back on its first corner: an open shape that traces every side. */
export function lotOutline(lot: Pick<Lot, "corners">): P[] {
  return lot.corners.length > 0 ? [...lot.corners, lot.corners[0]] : [];
}
