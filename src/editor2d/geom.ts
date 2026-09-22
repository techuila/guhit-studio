// Plain 2D geometry helpers. All values are model millimeters, +y north.
// Pure functions, no DOM. Tested in geom.test.ts.

export interface P {
  x: number;
  y: number;
}

export interface Seg {
  a: P;
  b: P;
}

export interface Rect {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export const EPS = 1e-6;

export const pt = (x: number, y: number): P => ({ x, y });
export const add = (a: P, b: P): P => ({ x: a.x + b.x, y: a.y + b.y });
export const sub = (a: P, b: P): P => ({ x: a.x - b.x, y: a.y - b.y });
export const mul = (a: P, k: number): P => ({ x: a.x * k, y: a.y * k });
export const dot = (a: P, b: P): number => a.x * b.x + a.y * b.y;
export const cross = (a: P, b: P): number => a.x * b.y - a.y * b.x;
export const len = (a: P): number => Math.hypot(a.x, a.y);
export const dist = (a: P, b: P): number => Math.hypot(a.x - b.x, a.y - b.y);
export const lerp = (a: P, b: P, t: number): P => ({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t });
export const mid = (a: P, b: P): P => lerp(a, b, 0.5);
/** Left hand normal (rotated 90 degrees counter-clockwise). */
export const left = (a: P): P => ({ x: -a.y, y: a.x });

export function unit(a: P): P {
  const l = len(a);
  return l < EPS ? { x: 1, y: 0 } : { x: a.x / l, y: a.y / l };
}

export const deg2rad = (d: number): number => (d * Math.PI) / 180;
export const rad2deg = (r: number): number => (r * 180) / Math.PI;

/** Unit vector at `deg` degrees counter-clockwise from +x. */
export function dirDeg(deg: number): P {
  const r = deg2rad(deg);
  // Exact on the axes, so a wall typed at 180 degrees stays on its y.
  const clean = (v: number): number => (Math.abs(v) < 1e-12 ? 0 : Math.abs(Math.abs(v) - 1) < 1e-12 ? Math.sign(v) : v);
  return { x: clean(Math.cos(r)), y: clean(Math.sin(r)) };
}

/** Angle of a vector in degrees, 0 to 360, counter-clockwise from +x. */
export function angleDeg(a: P): number {
  return normDeg(rad2deg(Math.atan2(a.y, a.x)));
}

export function normDeg(a: number): number {
  const r = a % 360;
  return r < 0 ? r + 360 : r;
}

/** Smallest absolute difference between two angles, degrees. */
export function angleDiffDeg(a: number, b: number): number {
  const d = Math.abs(normDeg(a) - normDeg(b));
  return d > 180 ? 360 - d : d;
}

export function rotate(p: P, deg: number, pivot: P = { x: 0, y: 0 }): P {
  const r = deg2rad(deg);
  const c = Math.cos(r);
  const s = Math.sin(r);
  const dx = p.x - pivot.x;
  const dy = p.y - pivot.y;
  return { x: pivot.x + dx * c - dy * s, y: pivot.y + dx * s + dy * c };
}

/** Parameter of the projection of p on the line a-b (0 at a, 1 at b). */
export function projectParam(p: P, a: P, b: P): number {
  const ab = sub(b, a);
  const l2 = dot(ab, ab);
  if (l2 < EPS) return 0;
  return dot(sub(p, a), ab) / l2;
}

export function closestOnSegment(p: P, a: P, b: P): { point: P; t: number; dist: number } {
  const t = Math.max(0, Math.min(1, projectParam(p, a, b)));
  const point = lerp(a, b, t);
  return { point, t, dist: dist(p, point) };
}

export function closestOnLine(p: P, a: P, b: P): P {
  return lerp(a, b, projectParam(p, a, b));
}

export function distToSegment(p: P, a: P, b: P): number {
  return closestOnSegment(p, a, b).dist;
}

/** Intersection of segments a-b and c-d, or null. */
export function segSegIntersection(a: P, b: P, c: P, d: P): P | null {
  const r = sub(b, a);
  const s = sub(d, c);
  const den = cross(r, s);
  if (Math.abs(den) < EPS) return null;
  const t = cross(sub(c, a), s) / den;
  const u = cross(sub(c, a), r) / den;
  if (t < -EPS || t > 1 + EPS || u < -EPS || u > 1 + EPS) return null;
  return add(a, mul(r, t));
}

/** Intersection of the infinite lines through a-b and c-d, or null when parallel. */
export function lineLineIntersection(a: P, b: P, c: P, d: P): P | null {
  const r = sub(b, a);
  const s = sub(d, c);
  const den = cross(r, s);
  if (Math.abs(den) < EPS) return null;
  const t = cross(sub(c, a), s) / den;
  return add(a, mul(r, t));
}

export function pointInPolygon(p: P, poly: readonly P[]): boolean {
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

export function distToPolygonEdge(p: P, poly: readonly P[]): number {
  let best = Infinity;
  for (let i = 0; i < poly.length; i++) {
    best = Math.min(best, distToSegment(p, poly[i], poly[(i + 1) % poly.length]));
  }
  return best;
}

export function polygonArea(poly: readonly P[]): number {
  let s = 0;
  for (let i = 0; i < poly.length; i++) s += cross(poly[i], poly[(i + 1) % poly.length]);
  return s / 2;
}

export function rectFromPoints(a: P, b: P): Rect {
  return {
    minX: Math.min(a.x, b.x),
    minY: Math.min(a.y, b.y),
    maxX: Math.max(a.x, b.x),
    maxY: Math.max(a.y, b.y),
  };
}

export function emptyRect(): Rect {
  return { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity };
}

export function rectIsEmpty(r: Rect): boolean {
  return !(r.maxX >= r.minX && r.maxY >= r.minY);
}

export function expandRect(r: Rect, p: P): void {
  if (p.x < r.minX) r.minX = p.x;
  if (p.y < r.minY) r.minY = p.y;
  if (p.x > r.maxX) r.maxX = p.x;
  if (p.y > r.maxY) r.maxY = p.y;
}

export function pointInRect(p: P, r: Rect): boolean {
  return p.x >= r.minX && p.x <= r.maxX && p.y >= r.minY && p.y <= r.maxY;
}

export function rectCorners(r: Rect): P[] {
  return [pt(r.minX, r.minY), pt(r.maxX, r.minY), pt(r.maxX, r.maxY), pt(r.minX, r.maxY)];
}

export function segIntersectsRect(a: P, b: P, r: Rect): boolean {
  if (pointInRect(a, r) || pointInRect(b, r)) return true;
  const c = rectCorners(r);
  for (let i = 0; i < 4; i++) {
    if (segSegIntersection(a, b, c[i], c[(i + 1) % 4])) return true;
  }
  return false;
}

/** True when every point of the shape is inside the rectangle. */
export function shapeInsideRect(points: readonly P[], r: Rect): boolean {
  return points.length > 0 && points.every((p) => pointInRect(p, r));
}

/** True when a closed polygon (or an open polyline) touches the rectangle. */
export function shapeIntersectsRect(points: readonly P[], r: Rect, closed: boolean): boolean {
  if (points.length === 0) return false;
  if (points.length === 1) return pointInRect(points[0], r);
  const n = closed ? points.length : points.length - 1;
  for (let i = 0; i < n; i++) {
    if (segIntersectsRect(points[i], points[(i + 1) % points.length], r)) return true;
  }
  if (closed && points.length >= 3) return pointInPolygon(pt(r.minX, r.minY), points);
  return false;
}

/** Corners of a w x d rectangle centered on c, rotated by deg. Counter-clockwise from the local (-x, -y) corner. */
export function orientedRect(c: P, w: number, d: number, deg: number): P[] {
  const hw = w / 2;
  const hd = d / 2;
  return [pt(-hw, -hd), pt(hw, -hd), pt(hw, hd), pt(-hw, hd)].map((p) => add(c, rotate(p, deg)));
}

/** Rectangle around the centerline a-b with the given thickness. Counter-clockwise. */
export function thickSegment(a: P, b: P, thickness: number): P[] {
  const n = mul(left(unit(sub(b, a))), thickness / 2);
  return [sub(a, n), sub(b, n), add(b, n), add(a, n)];
}

export function roundTo(v: number, step: number): number {
  if (step <= 0) return v;
  return Math.round(v / step) * step;
}

export function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/** Rotation in degrees that keeps text readable (never upside down). */
export function readableDeg(deg: number): number {
  let a = normDeg(deg);
  if (a > 90 && a <= 270) a -= 180;
  if (a > 270) a -= 360;
  return a;
}
