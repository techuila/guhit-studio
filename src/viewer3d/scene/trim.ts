// Trim and the D50 slab parts of the 3D model: skirting boards along room
// walls (the roof fascia and gutters are in geom/roofMesh.ts), railings along the runs the
// engine works out (`Derived.railings`), and the pieces a void cuts out of a
// floor. Pure geometry into MeshData, plan mm, no three.js objects, so the
// shapes are tested without a GPU (trim.test.ts).
//
// The engine decides where rails go (DECISIONS D50: "the plan, the 3D view
// and the exports draw these and never work out open edges themselves");
// this file only gives them a form.

import type { RailingRun, RailingStyle } from "../../contract/bindings";
import type { Pt } from "../geom/coords";
import { MeshData, pushPrism, type P3 } from "../geom/meshData";
import { ensureCCW, signedArea, triangulate } from "../geom/polygon";

/** Skirting: 100 mm tall, 12 mm proud of the wall, the usual PH tile skirting. */
export const SKIRTING_HEIGHT_MM = 100;
export const SKIRTING_THICK_MM = 12;
/** Runs shorter than this are left out (a sliver between a door and a corner). */
const MIN_RUN_MM = 20;

/** A door on its wall's centerline, for the gaps in the skirting. */
export interface DoorSpan {
  a: Pt;
  b: Pt;
  /** Half the host wall's thickness. */
  halfThick: number;
}

/**
 * Where the skirting runs along a room: every edge of its polygon (inner
 * wall faces, counter-clockwise) less the doors in the walls it follows. A
 * door counts for an edge when its wall runs parallel to the edge and the
 * edge lies on that wall's face.
 */
export function skirtingRuns(polyIn: Pt[], doors: DoorSpan[]): [Pt, Pt][] {
  const poly = ensureCCW(polyIn);
  const out: [Pt, Pt][] = [];
  for (let i = 0; i < poly.length; i++) {
    const p = poly[i];
    const q = poly[(i + 1) % poly.length];
    const len = Math.hypot(q.x - p.x, q.y - p.y);
    if (len < MIN_RUN_MM) continue;
    const ux = (q.x - p.x) / len;
    const uy = (q.y - p.y) / len;
    let keep: [number, number][] = [[0, len]];
    for (const d of doors) {
      const dl = Math.hypot(d.b.x - d.a.x, d.b.y - d.a.y);
      if (dl < 1) continue;
      const dx = (d.b.x - d.a.x) / dl;
      const dy = (d.b.y - d.a.y) / dl;
      if (Math.abs(ux * dy - uy * dx) > 0.02) continue;
      const off = Math.abs(ux * (d.a.y - p.y) - uy * (d.a.x - p.x));
      if (off > d.halfThick + 30) continue;
      const t1 = (d.a.x - p.x) * ux + (d.a.y - p.y) * uy;
      const t2 = (d.b.x - p.x) * ux + (d.b.y - p.y) * uy;
      const lo = Math.min(t1, t2);
      const hi = Math.max(t1, t2);
      const next: [number, number][] = [];
      for (const [a, b] of keep) {
        if (hi <= a || lo >= b) next.push([a, b]);
        else {
          if (lo > a) next.push([a, lo]);
          if (hi < b) next.push([hi, b]);
        }
      }
      keep = next;
    }
    for (const [a, b] of keep) {
      if (b - a < MIN_RUN_MM) continue;
      out.push([
        { x: p.x + ux * a, y: p.y + uy * a },
        { x: p.x + ux * b, y: p.y + uy * b },
      ]);
    }
  }
  return out;
}

/** Skirting along a room's walls, standing on `floorMm` (absolute). */
export function pushSkirting(md: MeshData, poly: Pt[], doors: DoorSpan[], floorMm: number, maxTopMm: number | null = null): void {
  const top = Math.min(floorMm + SKIRTING_HEIGHT_MM, maxTopMm ?? Infinity);
  if (!(top > floorMm + 1)) return;
  for (const [a, b] of skirtingRuns(poly, doors)) {
    const len = Math.hypot(b.x - a.x, b.y - a.y);
    // Inward: the left of a counter-clockwise edge.
    const nx = (-(b.y - a.y) / len) * SKIRTING_THICK_MM;
    const ny = ((b.x - a.x) / len) * SKIRTING_THICK_MM;
    const quad = [a, b, { x: b.x + nx, y: b.y + ny }, { x: a.x + nx, y: a.y + ny }];
    pushPrism(md, quad, { bottom: () => floorMm, top: () => top, skipBottom: true });
  }
}

// --------------------------------------------------------------- railings

/** Railing parts, mm. */
export const RAIL = {
  post: 40,
  postEvery: 1500,
  topW: 50,
  topD: 40,
  bar: 14,
  barEvery: 110,
  bottomAt: 100,
  glass: 12,
  glassFoot: 80,
  solid: 120,
  cap: 160,
  capD: 40,
} as const;

/** Where a railing's parts go: steel (posts, rails, bars), glass panels, and solid parapets. */
export interface RailingMeshes {
  steel: MeshData;
  glass: MeshData;
  solid: MeshData;
}

/**
 * A beam from `a` to `b` (its axis, plan mm and heights) with a horizontal
 * width `w` across it and a vertical depth `d`: a handrail, flat or along a
 * flight. The section stays vertical on a slope, as a rail's sides are.
 */
export function pushBeam(md: MeshData, a: P3, b: P3, w: number, d: number): void {
  const len = Math.hypot(b.x - a.x, b.y - a.y);
  if (len < 1) return;
  const sx = (-(b.y - a.y) / len) * (w / 2);
  const sy = ((b.x - a.x) / len) * (w / 2);
  const c = (p: P3, s: number, h: number): P3 => ({ x: p.x + sx * s, y: p.y + sy * s, h: p.h + h * (d / 2) });
  const a00 = c(a, -1, -1);
  const a10 = c(a, 1, -1);
  const a11 = c(a, 1, 1);
  const a01 = c(a, -1, 1);
  const b00 = c(b, -1, -1);
  const b10 = c(b, 1, -1);
  const b11 = c(b, 1, 1);
  const b01 = c(b, -1, 1);
  // World directions: plan (x, y, h) to world (x, h, -y).
  const along: [number, number, number] = [(b.x - a.x) / len, 0, -(b.y - a.y) / len];
  const side: [number, number, number] = [sx, 0, -sy];
  const uv = (p: P3): [number, number] => [(p.x + p.y) * 0.001, p.h * 0.001];
  md.pushQuad(a01, a11, b11, b01, [0, 1, 0], uv);
  md.pushQuad(a00, b00, b10, a10, [0, -1, 0], uv);
  md.pushQuad(a10, b10, b11, a11, side, uv);
  md.pushQuad(a00, a01, b01, b00, [-side[0], 0, -side[2]], uv);
  md.pushQuad(a00, a10, a11, a01, [-along[0], 0, -along[2]], uv);
  md.pushQuad(b00, b01, b11, b10, along, uv);
}

/** An upright square bar centered on `p`, turned to the run's direction `dir`, from `h0` to `h1`. */
function pushPost(md: MeshData, p: Pt, dir: Pt, size: number, h0: number, h1: number): void {
  if (!(h1 > h0 + 0.5)) return;
  const hx = (dir.x * size) / 2;
  const hy = (dir.y * size) / 2;
  const sx = (-dir.y * size) / 2;
  const sy = (dir.x * size) / 2;
  const sq = [
    { x: p.x - hx - sx, y: p.y - hy - sy },
    { x: p.x + hx - sx, y: p.y + hy - sy },
    { x: p.x + hx + sx, y: p.y + hy + sy },
    { x: p.x - hx + sx, y: p.y - hy + sy },
  ];
  pushPrism(md, sq, { bottom: () => h0, top: () => h1 });
}

/**
 * One railing run in the style it asks for. `baseMm` is its level's floor
 * (the run's point heights are above it); `maxTopMm`, absolute, cuts it at
 * the cutaway height: uprights stop there and rails above it are left out.
 */
export function pushRailing(out: RailingMeshes, run: Pick<RailingRun, "points" | "height_mm" | "style">, baseMm: number, maxTopMm: number | null = null): void {
  const pts = run.points;
  const h = Math.max(run.height_mm, 100);
  const cap = maxTopMm ?? Infinity;
  const style: RailingStyle = run.style;
  for (let i = 0; i + 1 < pts.length; i++) {
    const A = pts[i];
    const B = pts[i + 1];
    const len = Math.hypot(B.x - A.x, B.y - A.y);
    if (len < 1) continue;
    const dir = { x: (B.x - A.x) / len, y: (B.y - A.y) / len };
    const footAt = (t: number) => baseMm + A.z + (B.z - A.z) * t;
    const at = (t: number): Pt => ({ x: A.x + (B.x - A.x) * t, y: A.y + (B.y - A.y) * t });
    // A beam at `offset` above the foot, cut where it meets the cutaway.
    const beam = (md: MeshData, offset: number, w: number, d: number) => {
      let t0 = 0;
      let t1 = 1;
      const top0 = footAt(0) + offset + d / 2;
      const top1 = footAt(1) + offset + d / 2;
      if (top0 > cap && top1 > cap) return;
      if (top0 > cap) t0 = (cap - top0) / (top1 - top0);
      if (top1 > cap) t1 = (cap - top0) / (top1 - top0);
      if (t1 - t0 < 1e-3) return;
      const p0 = at(t0);
      const p1 = at(t1);
      pushBeam(md, { ...p0, h: footAt(t0) + offset }, { ...p1, h: footAt(t1) + offset }, w, d);
    };
    const upright = (md: MeshData, t: number, size: number, from: number, to: number) => {
      const f = footAt(t);
      pushPost(md, at(t), dir, size, f + from, Math.min(f + to, cap));
    };
    if (style === "solid") {
      // A parapet: a wall along the edge with a wider cap.
      const ws = RAIL.solid / 2;
      const nx = -dir.y * ws;
      const ny = dir.x * ws;
      const quad = [
        { x: A.x - nx, y: A.y - ny },
        { x: B.x - nx, y: B.y - ny },
        { x: B.x + nx, y: B.y + ny },
        { x: A.x + nx, y: A.y + ny },
      ];
      const foot = (p: Pt) => footAt(((p.x - A.x) * dir.x + (p.y - A.y) * dir.y) / len);
      const wallTop = (p: Pt) => Math.min(foot(p) + h - RAIL.capD, cap);
      if (Math.min(wallTop(A), wallTop(B)) > Math.min(foot(A), foot(B)) + 1) {
        pushPrism(out.solid, quad, { bottom: foot, top: wallTop });
      }
      beam(out.solid, h - RAIL.capD / 2, RAIL.cap, RAIL.capD);
      continue;
    }
    // Posts at the start of every segment and along it; the last one closes the run.
    const posts = Math.max(1, Math.ceil(len / RAIL.postEvery));
    for (let k = 0; k < posts; k++) upright(out.steel, k / posts, RAIL.post, 0, h - RAIL.topD);
    if (i + 2 === pts.length) upright(out.steel, 1, RAIL.post, 0, h - RAIL.topD);
    beam(out.steel, h - RAIL.topD / 2, RAIL.topW, RAIL.topD);
    if (style === "glass") {
      // One panel per segment between the foot and the handrail.
      const g0 = RAIL.glassFoot;
      const g1 = h - RAIL.topD;
      if (g1 > g0) {
        const ws = RAIL.glass / 2;
        const nx = -dir.y * ws;
        const ny = dir.x * ws;
        const inset = Math.min(RAIL.post, len / 4);
        const a = { x: A.x + dir.x * inset, y: A.y + dir.y * inset };
        const b = { x: B.x - dir.x * inset, y: B.y - dir.y * inset };
        const quad = [
          { x: a.x - nx, y: a.y - ny },
          { x: b.x - nx, y: b.y - ny },
          { x: b.x + nx, y: b.y + ny },
          { x: a.x + nx, y: a.y + ny },
        ];
        const foot = (p: Pt) => footAt(((p.x - A.x) * dir.x + (p.y - A.y) * dir.y) / len);
        pushPrism(out.glass, quad, { bottom: (p) => Math.min(foot(p) + g0, cap), top: (p) => Math.min(foot(p) + g1, cap) });
      }
      continue;
    }
    // Bars: a bottom rail and upright bars between it and the top rail.
    beam(out.steel, RAIL.bottomAt, RAIL.topD, RAIL.topD);
    const bars = Math.max(1, Math.floor(len / RAIL.barEvery));
    for (let k = 1; k < bars; k++) {
      const t = k / bars;
      // Leave room for the posts.
      const fromPost = (t * len) % (len / posts);
      if (fromPost < RAIL.post || len / posts - fromPost < RAIL.post) continue;
      upright(out.steel, t, RAIL.bar, RAIL.bottomAt + RAIL.topD / 2, h - RAIL.topD);
    }
  }
}

// ------------------------------------------------------------------- voids

/**
 * A polygon as convex pieces (itself when it is convex, else its
 * triangles), for `subtractWells`, which cuts convex holes.
 */
export function convexPieces(polyIn: Pt[]): Pt[][] {
  const poly = ensureCCW(polyIn);
  if (poly.length < 3 || Math.abs(signedArea(poly)) < 1) return [];
  let convex = true;
  for (let i = 0; i < poly.length && convex; i++) {
    const a = poly[i];
    const b = poly[(i + 1) % poly.length];
    const c = poly[(i + 2) % poly.length];
    if ((b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x) < -1e-6) convex = false;
  }
  if (convex) return [poly];
  return triangulate(poly).map(([i, j, k]) => ensureCCW([poly[i], poly[j], poly[k]]));
}

/** True when `p` is inside a convex, counter-clockwise polygon. */
export function insideConvex(p: Pt, poly: Pt[]): boolean {
  for (let i = 0; i < poly.length; i++) {
    const a = poly[i];
    const b = poly[(i + 1) % poly.length];
    if ((b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x) < 0) return false;
  }
  return true;
}
