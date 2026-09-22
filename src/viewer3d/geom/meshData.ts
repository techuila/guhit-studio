// Triangle soup accumulator with flat normals and world-scaled UVs.
// Input points are plan mm + height mm. Output arrays are three.js world
// meters, ready for a BufferGeometry. Pure data, no GPU objects.

import { planToWorld, type Pt, type Triple } from "./coords";
import { ensureCCW, triangulate } from "./polygon";

/** Plan point with a height, all mm. */
export interface P3 {
  x: number;
  y: number;
  h: number;
}

export type Uv = [number, number];

export class MeshData {
  positions: number[] = [];
  normals: number[] = [];
  uvs: number[] = [];

  get triangleCount(): number {
    return this.positions.length / 9;
  }

  /**
   * Adds one triangle. `outward` is a rough outward direction in world space;
   * the winding is flipped when needed so the face looks that way.
   */
  pushTri(a: P3, b: P3, c: P3, outward: Triple, uva: Uv, uvb: Uv, uvc: Uv): void {
    const A = planToWorld(a.x, a.y, a.h);
    let B = planToWorld(b.x, b.y, b.h);
    let C = planToWorld(c.x, c.y, c.h);
    let nx = (B[1] - A[1]) * (C[2] - A[2]) - (B[2] - A[2]) * (C[1] - A[1]);
    let ny = (B[2] - A[2]) * (C[0] - A[0]) - (B[0] - A[0]) * (C[2] - A[2]);
    let nz = (B[0] - A[0]) * (C[1] - A[1]) - (B[1] - A[1]) * (C[0] - A[0]);
    const len = Math.hypot(nx, ny, nz);
    if (len < 1e-12) return; // degenerate
    nx /= len;
    ny /= len;
    nz /= len;
    if (nx * outward[0] + ny * outward[1] + nz * outward[2] < 0) {
      [B, C] = [C, B];
      [uvb, uvc] = [uvc, uvb];
      nx = -nx;
      ny = -ny;
      nz = -nz;
    }
    this.positions.push(A[0], A[1], A[2], B[0], B[1], B[2], C[0], C[1], C[2]);
    this.normals.push(nx, ny, nz, nx, ny, nz, nx, ny, nz);
    this.uvs.push(uva[0], uva[1], uvb[0], uvb[1], uvc[0], uvc[1]);
  }

  pushQuad(a: P3, b: P3, c: P3, d: P3, outward: Triple, uv: (p: P3) => Uv): void {
    this.pushTri(a, b, c, outward, uv(a), uv(b), uv(c));
    this.pushTri(a, c, d, outward, uv(a), uv(c), uv(d));
  }

  /** Signed volume in cubic meters. Positive for a closed, outward-facing mesh. */
  signedVolume(): number {
    const p = this.positions;
    let v = 0;
    for (let i = 0; i < p.length; i += 9) {
      v +=
        p[i] * (p[i + 4] * p[i + 8] - p[i + 5] * p[i + 7]) -
        p[i + 1] * (p[i + 3] * p[i + 8] - p[i + 5] * p[i + 6]) +
        p[i + 2] * (p[i + 3] * p[i + 7] - p[i + 4] * p[i + 6]);
    }
    return v / 6;
  }

  bounds(): { min: Triple; max: Triple } | null {
    const p = this.positions;
    if (p.length === 0) return null;
    const min: Triple = [Infinity, Infinity, Infinity];
    const max: Triple = [-Infinity, -Infinity, -Infinity];
    for (let i = 0; i < p.length; i += 3) {
      for (let k = 0; k < 3; k++) {
        if (p[i + k] < min[k]) min[k] = p[i + k];
        if (p[i + k] > max[k]) max[k] = p[i + k];
      }
    }
    return { min, max };
  }
}

/** UV in meters for horizontal faces: plan x, plan y. */
export const planUv = (p: P3): Uv => [p.x * 0.001, p.y * 0.001];

/** UV in meters for a vertical face running along `dir`: distance along, height. */
export function sideUv(dir: Pt): (p: P3) => Uv {
  return (p) => [(p.x * dir.x + p.y * dir.y) * 0.001, p.h * 0.001];
}

export interface PrismOptions {
  /** Height of the underside at a plan point, mm. */
  bottom: (p: Pt) => number;
  /** Height of the top side at a plan point, mm. */
  top: (p: Pt) => number;
  /** Return true to leave out the side face of edge i (from point i to i + 1). */
  skipEdge?: (i: number, a: Pt, b: Pt) => boolean;
  skipTop?: boolean;
  skipBottom?: boolean;
  skipSides?: boolean;
  topUv?: (p: P3) => Uv;
  bottomUv?: (p: P3) => Uv;
  sideUvFor?: (dir: Pt) => (p: P3) => Uv;
}

/**
 * A solid over a plan polygon between two height functions (both must be
 * planar over the polygon, for example constants or one roof slope).
 */
export function pushPrism(md: MeshData, polyIn: Pt[], opts: PrismOptions): void {
  const poly = ensureCCW(polyIn);
  if (poly.length < 3) return;
  const tris = triangulate(poly);
  const topUv = opts.topUv ?? planUv;
  const bottomUv = opts.bottomUv ?? topUv;
  const top = (p: Pt): P3 => ({ x: p.x, y: p.y, h: opts.top(p) });
  const bot = (p: Pt): P3 => ({ x: p.x, y: p.y, h: opts.bottom(p) });

  for (const [i, j, k] of tris) {
    if (!opts.skipTop) {
      const a = top(poly[i]);
      const b = top(poly[j]);
      const c = top(poly[k]);
      md.pushTri(a, b, c, [0, 1, 0], topUv(a), topUv(b), topUv(c));
    }
    if (!opts.skipBottom) {
      const a = bot(poly[i]);
      const b = bot(poly[j]);
      const c = bot(poly[k]);
      md.pushTri(a, b, c, [0, -1, 0], bottomUv(a), bottomUv(b), bottomUv(c));
    }
  }

  for (let i = 0; i < poly.length && !opts.skipSides; i++) {
    const a = poly[i];
    const b = poly[(i + 1) % poly.length];
    if (opts.skipEdge?.(i, a, b)) continue;
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const len = Math.hypot(dx, dy);
    if (len < 1e-6) continue;
    const dir = { x: dx / len, y: dy / len };
    // Right side of a CCW edge points out. Plan (nx, ny) -> world (nx, 0, -ny).
    const outward: Triple = [dir.y, 0, dir.x];
    const uv = (opts.sideUvFor ?? sideUv)(dir);
    md.pushQuad(bot(a), bot(b), top(b), top(a), outward, uv);
  }
}
