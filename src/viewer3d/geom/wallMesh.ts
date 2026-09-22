// Wall solids with real openings, without a CSG library.
//
// The wall is handled in its own frame: u along the centerline from the start
// point, v to the left of it, h up. The mitred plan outline is cut into strips
// at every opening edge (u), and heights are cut into intervals at every sill
// and head (h). Each (strip, interval) cell is solid or void. Solid cells get
// their outer side faces and caps, and every solid/void boundary between two
// neighbouring strips gets a jamb face. The mitred ends come straight from the
// outline, so corners still close, and reveals exist on all four sides.

import type { Pt, Triple } from "./coords";
import { MeshData, planUv, sideUv, type P3 } from "./meshData";
import { boundsOf, clipToXRange, ensureCCW, triangulate } from "./polygon";

export interface WallOpeningInput {
  /** Distance from the wall start to the opening center, mm. */
  offset: number;
  width: number;
  height: number;
  sill: number;
}

export interface WallInput {
  start: Pt;
  end: Pt;
  thickness: number;
  /** Full wall height, mm. */
  height: number;
  /** Level elevation, mm. */
  elevation: number;
  /** Derived plan outline. Missing or broken outlines fall back to a rectangle. */
  outline?: Pt[] | null;
  openings: WallOpeningInput[];
  /** Cutaway: nothing is built above this height above the level floor. */
  maxHeight?: number | null;
}

export interface WallFrame {
  origin: Pt;
  dir: Pt;
  left: Pt;
  length: number;
}

export function wallFrame(start: Pt, end: Pt): WallFrame | null {
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  const length = Math.hypot(dx, dy);
  if (!(length > 1e-3)) return null;
  const dir = { x: dx / length, y: dy / length };
  return { origin: start, dir, left: { x: -dir.y, y: dir.x }, length };
}

export function toLocal(f: WallFrame, p: Pt): Pt {
  const rx = p.x - f.origin.x;
  const ry = p.y - f.origin.y;
  return { x: rx * f.dir.x + ry * f.dir.y, y: rx * f.left.x + ry * f.left.y };
}

export function toPlan(f: WallFrame, uv: Pt): Pt {
  return {
    x: f.origin.x + f.dir.x * uv.x + f.left.x * uv.y,
    y: f.origin.y + f.dir.y * uv.x + f.left.y * uv.y,
  };
}

export interface ClampedOpening {
  a: number;
  b: number;
  bottom: number;
  top: number;
}

/** Openings clamped to the wall body and to the built height. */
export function clampOpenings(
  openings: WallOpeningInput[],
  uMin: number,
  uMax: number,
  height: number,
): ClampedOpening[] {
  const out: ClampedOpening[] = [];
  const margin = 1;
  for (const o of openings) {
    if (![o.offset, o.width, o.height, o.sill].every(Number.isFinite)) continue;
    const a = Math.max(o.offset - o.width / 2, uMin + margin);
    const b = Math.min(o.offset + o.width / 2, uMax - margin);
    const bottom = Math.min(Math.max(o.sill, 0), height);
    const top = Math.min(Math.max(o.sill + o.height, 0), height);
    if (b - a < 1 || top - bottom < 1) continue;
    out.push({ a, b, bottom, top });
  }
  return out;
}

function uniqueSorted(values: number[], tol = 0.01): number[] {
  const s = [...values].sort((x, y) => x - y);
  const out: number[] = [];
  for (const v of s) if (out.length === 0 || v - out[out.length - 1] > tol) out.push(v);
  return out;
}

/** Local outline (u, v), counter-clockwise. Falls back to a plain rectangle. */
export function localOutline(input: WallInput, f: WallFrame): Pt[] {
  if (input.outline && input.outline.length >= 3) {
    const local = ensureCCW(input.outline.map((p) => toLocal(f, p)));
    if (local.length >= 3) return local;
  }
  const t = Math.max(input.thickness, 1) / 2;
  return [
    { x: 0, y: -t },
    { x: f.length, y: -t },
    { x: f.length, y: t },
    { x: 0, y: t },
  ];
}

/** Builds the wall into `md`. Returns false when there was nothing to build. */
export function buildWallMesh(input: WallInput, md: MeshData): boolean {
  const f = wallFrame(input.start, input.end);
  if (!f) return false;
  let H = input.height;
  if (input.maxHeight != null) H = Math.min(H, input.maxHeight);
  if (!(H > 1)) return false;

  const outline = localOutline(input, f);
  const ob = boundsOf(outline);
  if (!ob) return false;
  const openings = clampOpenings(input.openings, ob.minX, ob.maxX, H);

  const uCuts = uniqueSorted(openings.flatMap((o) => [o.a, o.b]));
  const hLevels = uniqueSorted([0, H, ...openings.flatMap((o) => [o.bottom, o.top])]);
  const stripEdges = [-Infinity, ...uCuts, Infinity];
  const stripCount = stripEdges.length - 1;
  const intervalCount = hLevels.length - 1;

  // solid[strip][interval]
  const solid: boolean[][] = [];
  const polys: Pt[][] = [];
  for (let s = 0; s < stripCount; s++) {
    const lo = stripEdges[s];
    const hi = stripEdges[s + 1];
    const mid = Number.isFinite(lo) && Number.isFinite(hi) ? (lo + hi) / 2 : Number.isFinite(lo) ? lo + 1 : hi - 1;
    const row: boolean[] = [];
    for (let k = 0; k < intervalCount; k++) {
      const hm = (hLevels[k] + hLevels[k + 1]) / 2;
      const isVoid = openings.some((o) => mid > o.a && mid < o.b && hm > o.bottom && hm < o.top);
      row.push(!isVoid);
    }
    solid.push(row);
    polys.push(clipToXRange(outline, lo, hi));
  }

  const E = input.elevation;
  const lift = (uv: Pt, h: number): P3 => {
    const p = toPlan(f, uv);
    return { x: p.x, y: p.y, h: E + h };
  };
  const toWorldDir = (du: number, dv: number): Triple => {
    const x = f.dir.x * du + f.left.x * dv;
    const y = f.dir.y * du + f.left.y * dv;
    return [x, 0, -y];
  };
  const alongUv = sideUv(f.dir);
  const acrossUv = sideUv(f.left);
  const onCut = (u: number, cut: number) => Number.isFinite(cut) && Math.abs(u - cut) < 0.01;

  for (let s = 0; s < stripCount; s++) {
    const poly = polys[s];
    if (poly.length < 3) continue;
    const lo = stripEdges[s];
    const hi = stripEdges[s + 1];
    const tris = triangulate(poly);

    for (let k = 0; k < intervalCount; k++) {
      if (!solid[s][k]) continue;
      const h0 = hLevels[k];
      const h1 = hLevels[k + 1];

      // Outer side faces. Edges that lie on a cut line are handled as jambs.
      for (let i = 0; i < poly.length; i++) {
        const a = poly[i];
        const b = poly[(i + 1) % poly.length];
        if ((onCut(a.x, lo) && onCut(b.x, lo)) || (onCut(a.x, hi) && onCut(b.x, hi))) continue;
        const du = b.x - a.x;
        const dv = b.y - a.y;
        const len = Math.hypot(du, dv);
        if (len < 1e-6) continue;
        const outward = toWorldDir(dv / len, -du / len);
        const uv = Math.abs(du) >= Math.abs(dv) ? alongUv : acrossUv;
        md.pushQuad(lift(a, h0), lift(b, h0), lift(b, h1), lift(a, h1), outward, uv);
      }

      // Top cap: wall top, or the sill under a window.
      if (k === intervalCount - 1 || !solid[s][k + 1]) {
        for (const [i, j, l] of tris) {
          const a = lift(poly[i], h1);
          const b = lift(poly[j], h1);
          const c = lift(poly[l], h1);
          md.pushTri(a, b, c, [0, 1, 0], planUv(a), planUv(b), planUv(c));
        }
      }
      // Bottom cap: the underside of a lintel, and the wall base so the solid is closed.
      if (k === 0 || !solid[s][k - 1]) {
        for (const [i, j, l] of tris) {
          const a = lift(poly[i], h0);
          const b = lift(poly[j], h0);
          const c = lift(poly[l], h0);
          md.pushTri(a, b, c, [0, -1, 0], planUv(a), planUv(b), planUv(c));
        }
      }
    }
  }

  // Jambs: solid on one side of a cut, void on the other.
  for (let s = 0; s < stripCount - 1; s++) {
    const cut = stripEdges[s + 1];
    const section = [...polys[s], ...polys[s + 1]].filter((p) => onCut(p.x, cut));
    if (section.length < 2) continue;
    let vMin = Infinity;
    let vMax = -Infinity;
    for (const p of section) {
      vMin = Math.min(vMin, p.y);
      vMax = Math.max(vMax, p.y);
    }
    if (vMax - vMin < 1e-6) continue;
    for (let k = 0; k < intervalCount; k++) {
      const left = solid[s][k];
      const right = solid[s + 1][k];
      if (left === right) continue;
      // The face looks into the void.
      const outward = toWorldDir(left ? 1 : -1, 0);
      const a = { x: cut, y: vMin };
      const b = { x: cut, y: vMax };
      md.pushQuad(
        lift(a, hLevels[k]),
        lift(b, hLevels[k]),
        lift(b, hLevels[k + 1]),
        lift(a, hLevels[k + 1]),
        outward,
        acrossUv,
      );
    }
  }

  return md.positions.length > 0;
}
