// Roof meshes (lane L2). Every roof the view draws comes from the engine's
// `Derived.roofs` (DECISIONS D54): `buildDerivedRoof` turns each plane into a
// sloped prism (covering on top, soffit and edges in a trim mesh),
// `buildDerivedInfill` gives the gable ends a wall's thickness, and
// `buildRoofTrim` hangs the fascia along eaves and rakes and the gutter
// along the eaves. Nothing here works out where a roof goes.

import type { RoofGeometry, RoofLine, Vec3 } from "../../contract/bindings";
import type { Pt, Triple } from "./coords";
import { MeshData, pushPrism, sideUv, type P3, type Uv } from "./meshData";
import { ensureCCW, pointInPolygon, triangulate } from "./polygon";

// ------------------------------------------------------------ derived roofs

/** A roof plane's top as heights over plan: z = z0 + gx * x + gy * y, mm. */
export interface PlaneFit {
  gx: number;
  gy: number;
  z0: number;
}

/** The plane through a planar polygon (Newell normal). Null when it stands upright or has under three corners. */
export function fitPlane(pts: readonly Vec3[]): PlaneFit | null {
  if (pts.length < 3) return null;
  let nx = 0;
  let ny = 0;
  let nz = 0;
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i];
    const b = pts[(i + 1) % pts.length];
    nx += (a.y - b.y) * (a.z + b.z);
    ny += (a.z - b.z) * (a.x + b.x);
    nz += (a.x - b.x) * (a.y + b.y);
  }
  if (Math.abs(nz) < 1e-9) return null;
  const gx = -nx / nz;
  const gy = -ny / nz;
  const o = pts[0];
  return { gx, gy, z0: o.z - gx * o.x - gy * o.y };
}

const planeAt = (f: PlaneFit, p: Pt) => f.z0 + f.gx * p.x + f.gy * p.y;

/** True when a plane edge a-b runs along a ridge, hip or valley: an inner edge with no side face. */
function innerEdge(lines: readonly RoofLine[], a: Pt, b: Pt): boolean {
  const near = (p: Pt, l: RoofLine) => {
    const dx = l.b.x - l.a.x;
    const dy = l.b.y - l.a.y;
    const len2 = dx * dx + dy * dy;
    const t = len2 > 0 ? Math.min(Math.max(((p.x - l.a.x) * dx + (p.y - l.a.y) * dy) / len2, 0), 1) : 0;
    return Math.hypot(l.a.x + dx * t - p.x, l.a.y + dy * t - p.y) < 1;
  };
  return lines.some((l) => (l.kind === "ridge" || l.kind === "hip" || l.kind === "valley") && near(a, l) && near(b, l));
}

export interface DerivedRoofMesh {
  /** Highest point of the top side, mm above the project zero. */
  peak: number;
  /** Lowest point of the underside, mm above the project zero. */
  low: number;
  /** Every plane's plan polygon. */
  planes: Pt[][];
}

/**
 * A derived roof's covering into `md` (the sloped tops, sheets running down
 * each slope) and its soffit and outer edges into `trim`, together one
 * closed solid per plane with no faces inside along ridges, hips and
 * valleys. `floorMm` is the floor of the level it sits on.
 */
export function buildDerivedRoof(g: RoofGeometry, floorMm: number, md: MeshData, trim: MeshData = md): DerivedRoofMesh {
  let peak = -Infinity;
  let low = Infinity;
  const planes: Pt[][] = [];
  for (const plane of g.planes) {
    const fit = fitPlane(plane);
    if (!fit) continue;
    const poly = plane.map((p) => ({ x: p.x, y: p.y }));
    planes.push(poly);
    const top = (p: Pt) => floorMm + planeAt(fit, p);
    const under = (p: Pt) => top(p) - g.vertical_mm;
    // Sheets and tiles run down the slope: u along the eave, v down the slope.
    const tan = Math.hypot(fit.gx, fit.gy);
    const up = tan > 1e-9 ? { x: fit.gx / tan, y: fit.gy / tan } : { x: 0, y: 1 };
    const cos = 1 / Math.sqrt(1 + tan * tan);
    const slopeUv = (p: P3): Uv => [(p.x * up.y - p.y * up.x) * 0.001, ((p.x * up.x + p.y * up.y) / cos) * 0.001];
    pushPrism(md, poly, { bottom: under, top, topUv: slopeUv, skipBottom: true, skipSides: true });
    pushPrism(trim, poly, { bottom: under, top, skipTop: true, skipEdge: (_i, a, b) => innerEdge(g.lines, a, b) });
    for (const p of poly) {
      peak = Math.max(peak, top(p));
      low = Math.min(low, under(p));
    }
  }
  return { peak, low, planes };
}

/**
 * The gable-end infill (`RoofGeometry.infill`, vertical polygons on the wall
 * line) as solids `thickness` deep, inward of the wall line. Each polygon
 * runs along its first edge, the wall top, the outline's way round, so
 * inward is its left.
 */
export function buildDerivedInfill(g: RoofGeometry, floorMm: number, thickness: number, md: MeshData): void {
  const t = Math.max(thickness, 10);
  for (const poly of g.infill) {
    if (poly.length < 3) continue;
    const a = poly[0];
    const b = poly[1];
    const len = Math.hypot(b.x - a.x, b.y - a.y);
    if (len < 1) continue;
    const dir = { x: (b.x - a.x) / len, y: (b.y - a.y) / len };
    const inward = { x: -dir.y, y: dir.x };
    // The polygon in its own plane: u along the wall, z up.
    const flat: Pt[] = poly.map((p) => ({ x: (p.x - a.x) * dir.x + (p.y - a.y) * dir.y, y: p.z }));
    const ring = ensureCCW(flat);
    if (ring.length < 3) continue;
    const at = (q: Pt, depth: number): P3 => ({ x: a.x + dir.x * q.x + inward.x * depth, y: a.y + dir.y * q.x + inward.y * depth, h: floorMm + q.y });
    const uv = sideUv(dir);
    // Plan outward (the right of the wall line) maps to world (x, 0, -y).
    const outward: Triple = [-inward.x, 0, inward.y];
    const inside: Triple = [inward.x, 0, -inward.y];
    for (const [i, j, k] of triangulate(ring)) {
      for (const [depth, face] of [
        [0, outward],
        [t, inside],
      ] as const) {
        const p = at(ring[i], depth);
        const q = at(ring[j], depth);
        const r = at(ring[k], depth);
        md.pushTri(p, q, r, face, uv(p), uv(q), uv(r));
      }
    }
    const base = Math.min(...ring.map((q) => q.y));
    for (let i = 0; i < ring.length; i++) {
      const p = ring[i];
      const q = ring[(i + 1) % ring.length];
      // The edge on the wall top needs no face.
      if (Math.abs(p.y - base) < 1e-6 && Math.abs(q.y - base) < 1e-6) continue;
      const du = q.x - p.x;
      const dz = q.y - p.y;
      const l = Math.hypot(du, dz);
      if (l < 1e-6) continue;
      const nu = dz / l;
      const nz = -du / l;
      const out: Triple = [dir.x * nu, nz, -dir.y * nu];
      md.pushQuad(at(p, 0), at(q, 0), at(q, t), at(p, t), out, uv);
    }
  }
}

/** Fascia board: thickness, and how far its face reaches under the roof edge for each mm of its size. */
export const FASCIA_THICK_MM = 25;
const FASCIA_DROP_PER_MM = 0.9;

/**
 * Fascia along every eave run and rake (`size` is its face, mm; null for
 * none) into `fascia`, and a box gutter along the eave runs (`gutterSize`,
 * null for none) into `gutter`. Each board sits just outside the roof's
 * edge, from the top side down past the underside.
 */
export function buildRoofTrim(
  g: RoofGeometry,
  floorMm: number,
  size: number | null,
  gutterSize: number | null,
  fascia: MeshData,
  gutter: MeshData,
): void {
  const planes = g.planes.map((pl) => pl.map((p) => ({ x: p.x, y: p.y })));
  const inside = (p: Pt) => planes.some((poly) => pointInPolygon(p, poly));
  const segments: { a: Vec3; b: Vec3; eave: boolean }[] = [];
  for (const run of g.eaves) for (let i = 0; i + 1 < run.length; i++) segments.push({ a: run[i], b: run[i + 1], eave: true });
  for (const l of g.lines) if (l.kind === "rake") segments.push({ a: l.a, b: l.b, eave: false });
  for (const { a, b, eave } of segments) {
    const len = Math.hypot(b.x - a.x, b.y - a.y);
    if (len < 1) continue;
    const dir = { x: (b.x - a.x) / len, y: (b.y - a.y) / len };
    // Outward: the side of the edge away from the roof.
    const mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
    let n = { x: dir.y, y: -dir.x };
    if (inside({ x: mid.x + n.x * 10, y: mid.y + n.y * 10 })) n = { x: -n.x, y: -n.y };
    const along = (p: Pt) => Math.min(Math.max(((p.x - a.x) * dir.x + (p.y - a.y) * dir.y) / len, 0), 1);
    const topAt = (p: Pt) => floorMm + a.z + (b.z - a.z) * along(p);
    if (size !== null) {
      const drop = g.vertical_mm + size * FASCIA_DROP_PER_MM;
      const quad = [a, { x: a.x + n.x * FASCIA_THICK_MM, y: a.y + n.y * FASCIA_THICK_MM }, { x: b.x + n.x * FASCIA_THICK_MM, y: b.y + n.y * FASCIA_THICK_MM }, b];
      pushPrism(fascia, quad, { bottom: (p) => topAt(p) - drop, top: topAt });
    }
    if (gutterSize !== null && eave && Math.abs(b.z - a.z) < 1) {
      // A box gutter outside the fascia: its floor and its outer lip.
      const w = gutterSize;
      const off = size !== null ? FASCIA_THICK_MM : 0;
      const z = floorMm + a.z - g.vertical_mm - gutterSize * 0.5;
      const strip = (o0: number, o1: number) => [
        { x: a.x + n.x * o0, y: a.y + n.y * o0 },
        { x: a.x + n.x * o1, y: a.y + n.y * o1 },
        { x: b.x + n.x * o1, y: b.y + n.y * o1 },
        { x: b.x + n.x * o0, y: b.y + n.y * o0 },
      ];
      pushPrism(gutter, strip(off, off + w), { bottom: () => z, top: () => z + 12 });
      pushPrism(gutter, strip(off + w - 12, off + w), { bottom: () => z, top: () => z + gutterSize * 0.6 });
    }
  }
}
