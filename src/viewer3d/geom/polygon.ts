// Small 2D polygon helpers. Pure math, no three.js scene objects.

import { ShapeUtils, Vector2 } from "three";
import type { Pt } from "./coords";

export const EPS = 1e-6;

export function signedArea(poly: Pt[]): number {
  let a = 0;
  for (let i = 0; i < poly.length; i++) {
    const p = poly[i];
    const q = poly[(i + 1) % poly.length];
    a += p.x * q.y - q.x * p.y;
  }
  return a / 2;
}

/** Drops repeated points (also a closing point equal to the first). */
export function cleanPolygon(poly: Pt[], tol = 1e-4): Pt[] {
  const out: Pt[] = [];
  for (const p of poly) {
    if (!Number.isFinite(p.x) || !Number.isFinite(p.y)) continue;
    const last = out[out.length - 1];
    if (last && Math.abs(last.x - p.x) < tol && Math.abs(last.y - p.y) < tol) continue;
    out.push({ x: p.x, y: p.y });
  }
  while (out.length > 1) {
    const a = out[0];
    const b = out[out.length - 1];
    if (Math.abs(a.x - b.x) < tol && Math.abs(a.y - b.y) < tol) out.pop();
    else break;
  }
  return out;
}

/** Cleaned, counter-clockwise copy. Empty when the input is not a polygon. */
export function ensureCCW(poly: Pt[]): Pt[] {
  const p = cleanPolygon(poly);
  if (p.length < 3) return [];
  const a = signedArea(p);
  if (Math.abs(a) < EPS) return [];
  return a < 0 ? p.reverse() : p;
}

export interface Bounds2 {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export function boundsOf(points: Pt[]): Bounds2 | null {
  if (points.length === 0) return null;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const p of points) {
    if (p.x < minX) minX = p.x;
    if (p.y < minY) minY = p.y;
    if (p.x > maxX) maxX = p.x;
    if (p.y > maxY) maxY = p.y;
  }
  return { minX, minY, maxX, maxY };
}

export function pointInPolygon(p: Pt, poly: Pt[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const a = poly[i];
    const b = poly[j];
    if (a.y > p.y !== b.y > p.y && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x) {
      inside = !inside;
    }
  }
  return inside;
}

/**
 * Sutherland-Hodgman clip against one half plane: keeps the part where
 * nx * x + ny * y <= c.
 */
export function clipHalfPlane(poly: Pt[], nx: number, ny: number, c: number): Pt[] {
  const out: Pt[] = [];
  const n = poly.length;
  for (let i = 0; i < n; i++) {
    const a = poly[i];
    const b = poly[(i + 1) % n];
    const da = nx * a.x + ny * a.y - c;
    const db = nx * b.x + ny * b.y - c;
    const ina = da <= EPS;
    const inb = db <= EPS;
    if (ina) out.push(a);
    if (ina !== inb) {
      const t = da / (da - db);
      out.push({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t });
    }
  }
  return cleanPolygon(out);
}

/** Keeps the part of the polygon with lo <= x <= hi. Infinite bounds are allowed. */
export function clipToXRange(poly: Pt[], lo: number, hi: number): Pt[] {
  let p = poly;
  if (Number.isFinite(lo)) p = clipHalfPlane(p, -1, 0, -lo);
  if (Number.isFinite(hi)) p = clipHalfPlane(p, 1, 0, hi);
  return p;
}

/**
 * Mitred outward offset of a counter-clockwise polygon. Good for building
 * footprints (mostly right angles). Very sharp corners are limited so a spike
 * cannot run away.
 */
export function offsetPolygon(polyIn: Pt[], dist: number): Pt[] {
  const poly = ensureCCW(polyIn);
  if (poly.length < 3 || Math.abs(dist) < EPS) return poly;
  const n = poly.length;
  const out: Pt[] = [];
  for (let i = 0; i < n; i++) {
    const prev = poly[(i + n - 1) % n];
    const cur = poly[i];
    const next = poly[(i + 1) % n];
    const d1 = normalize({ x: cur.x - prev.x, y: cur.y - prev.y });
    const d2 = normalize({ x: next.x - cur.x, y: next.y - cur.y });
    // Outward normal of a CCW edge is its right side.
    const n1 = { x: d1.y, y: -d1.x };
    const n2 = { x: d2.y, y: -d2.x };
    const bis = normalize({ x: n1.x + n2.x, y: n1.y + n2.y });
    const cos = bis.x * n1.x + bis.y * n1.y;
    const scale = dist / Math.max(cos, 0.35);
    out.push({ x: cur.x + bis.x * scale, y: cur.y + bis.y * scale });
  }
  return out;
}

export function normalize(v: Pt): Pt {
  const len = Math.hypot(v.x, v.y);
  if (len < EPS) return { x: 0, y: 0 };
  return { x: v.x / len, y: v.y / len };
}

/** Triangle index triples for a simple polygon (any winding). */
export function triangulate(poly: Pt[]): [number, number, number][] {
  if (poly.length < 3) return [];
  if (poly.length === 3) return [[0, 1, 2]];
  const contour = poly.map((p) => new Vector2(p.x, p.y));
  return ShapeUtils.triangulateShape(contour, []) as [number, number, number][];
}

export interface LongAxis {
  /** Unit direction of the longest extent. */
  dir: Pt;
  /** Extent along `dir` and across it. */
  length: number;
  width: number;
}

/**
 * Long axis of a room polygon. Tests every edge direction and keeps the one
 * with the largest extent, so rotated rectangular rooms work.
 */
export function longAxis(poly: Pt[]): LongAxis {
  let best: LongAxis = { dir: { x: 1, y: 0 }, length: 0, width: 0 };
  for (let i = 0; i < poly.length; i++) {
    const a = poly[i];
    const b = poly[(i + 1) % poly.length];
    const d = normalize({ x: b.x - a.x, y: b.y - a.y });
    if (d.x === 0 && d.y === 0) continue;
    let lo = Infinity;
    let hi = -Infinity;
    let lo2 = Infinity;
    let hi2 = -Infinity;
    for (const p of poly) {
      const s = p.x * d.x + p.y * d.y;
      const t = -p.x * d.y + p.y * d.x;
      lo = Math.min(lo, s);
      hi = Math.max(hi, s);
      lo2 = Math.min(lo2, t);
      hi2 = Math.max(hi2, t);
    }
    if (hi - lo > best.length + 1e-3) best = { dir: d, length: hi - lo, width: hi2 - lo2 };
  }
  return best;
}

/**
 * Plan footprint of a `w` by `d` rectangle centered on `center`, turned `deg`
 * degrees counter-clockwise. Counter-clockwise corners.
 */
export function orientedRect(center: Pt, w: number, d: number, deg: number): Pt[] {
  const a = (deg * Math.PI) / 180;
  const c = Math.cos(a);
  const s = Math.sin(a);
  return [
    [-w / 2, -d / 2],
    [w / 2, -d / 2],
    [w / 2, d / 2],
    [-w / 2, d / 2],
  ].map(([x, y]) => ({ x: center.x + x * c - y * s, y: center.y + x * s + y * c }));
}

/** Closest point to `p` on the segment a-b. */
export function closestOnSegment(p: Pt, a: Pt, b: Pt): Pt {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const l2 = dx * dx + dy * dy;
  if (l2 < EPS) return { x: a.x, y: a.y };
  const t = Math.min(Math.max(((p.x - a.x) * dx + (p.y - a.y) * dy) / l2, 0), 1);
  return { x: a.x + dx * t, y: a.y + dy * t };
}

/** True when the segments p1-p2 and q1-q2 cross or touch. */
export function segmentsIntersect(p1: Pt, p2: Pt, q1: Pt, q2: Pt): boolean {
  const cross = (o: Pt, a: Pt, b: Pt) => (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
  const d1 = cross(q1, q2, p1);
  const d2 = cross(q1, q2, p2);
  const d3 = cross(p1, p2, q1);
  const d4 = cross(p1, p2, q2);
  if (((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0))) return true;
  const on = (o: Pt, a: Pt, b: Pt, d: number) =>
    Math.abs(d) < EPS && Math.min(o.x, a.x) - EPS <= b.x && b.x <= Math.max(o.x, a.x) + EPS && Math.min(o.y, a.y) - EPS <= b.y && b.y <= Math.max(o.y, a.y) + EPS;
  return on(q1, q2, p1, d1) || on(q1, q2, p2, d2) || on(p1, p2, q1, d3) || on(p1, p2, q2, d4);
}

export function centroid(poly: Pt[]): Pt {
  const a = signedArea(poly);
  if (Math.abs(a) < EPS) {
    const b = boundsOf(poly);
    return b ? { x: (b.minX + b.maxX) / 2, y: (b.minY + b.maxY) / 2 } : { x: 0, y: 0 };
  }
  let cx = 0;
  let cy = 0;
  for (let i = 0; i < poly.length; i++) {
    const p = poly[i];
    const q = poly[(i + 1) % poly.length];
    const w = p.x * q.y - q.x * p.y;
    cx += (p.x + q.x) * w;
    cy += (p.y + q.y) * w;
  }
  return { x: cx / (6 * a), y: cy / (6 * a) };
}
