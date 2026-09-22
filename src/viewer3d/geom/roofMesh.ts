// Roof presets over a footprint polygon: flat, shed, gable.
//
// The underside of a pitched roof passes through the top outer edge of the
// walls, so the eaves dip below the wall top the way real eaves do. The space
// between the flat wall tops and the sloped underside is closed with infill
// panels along the footprint edges (the triangular gable ends).

import type { Axis, RoofKind } from "../../contract/bindings";
import type { Pt, Triple } from "./coords";
import { MeshData, pushPrism, sideUv, type P3, type Uv } from "./meshData";
import { boundsOf, clipHalfPlane, ensureCCW, offsetPolygon } from "./polygon";

export interface RoofInput {
  kind: RoofKind;
  pitchDeg: number;
  overhang: number;
  thickness: number;
  ridgeAxis: Axis;
  /** Outer wall faces of the top level, plan mm. */
  footprint: Pt[];
  /** Height of the wall tops, mm above project zero. */
  baseHeight: number;
  /** Thickness of the gable infill panels, mm. */
  infillThickness?: number;
}

export interface RoofProfile {
  /** Height of the roof underside at a plan point, mm. */
  under: (p: Pt) => number;
  /** Vertical roof thickness, mm. */
  vertical: number;
  /** Coordinate across the slope (the one the height depends on). */
  across: (p: Pt) => number;
  /** Coordinate along the ridge. */
  along: (p: Pt) => number;
  /** Ridge position in the across coordinate. Null when there is no ridge. */
  ridge: number | null;
  /** Highest point of the roof top side, mm. */
  peak: number;
  tan: number;
  cos: number;
}

export function clampPitch(deg: number): number {
  if (!Number.isFinite(deg)) return 0;
  return Math.min(Math.max(deg, 0), 60);
}

/** Height functions for a roof. Null for kind "none" or a missing footprint. */
export function roofProfile(input: RoofInput): RoofProfile | null {
  const fp = ensureCCW(input.footprint);
  const b = boundsOf(fp);
  if (input.kind === "none" || !b) return null;
  const thickness = Math.max(input.thickness, 10);
  const base = input.baseHeight;

  if (input.kind === "flat") {
    return {
      under: () => base,
      vertical: thickness,
      across: (p) => p.y,
      along: (p) => p.x,
      ridge: null,
      peak: base + thickness,
      tan: 0,
      cos: 1,
    };
  }

  const pitch = (clampPitch(input.pitchDeg) * Math.PI) / 180;
  const tan = Math.tan(pitch);
  const cos = Math.cos(pitch);
  const vertical = thickness / cos;

  if (input.kind === "shed") {
    // ridge_axis is the slope direction for a shed roof: it rises along +axis.
    const across = input.ridgeAxis === "x" ? (p: Pt) => p.x : (p: Pt) => p.y;
    const along = input.ridgeAxis === "x" ? (p: Pt) => p.y : (p: Pt) => p.x;
    const lo = input.ridgeAxis === "x" ? b.minX : b.minY;
    const hi = input.ridgeAxis === "x" ? b.maxX : b.maxY;
    const overhang = Math.max(input.overhang, 0);
    return {
      under: (p) => base + (across(p) - lo) * tan,
      vertical,
      across,
      along,
      ridge: null,
      peak: base + (hi + overhang - lo) * tan + vertical,
      tan,
      cos,
    };
  }

  // Gable: the ridge runs along ridge_axis, the slopes fall across it.
  const across = input.ridgeAxis === "x" ? (p: Pt) => p.y : (p: Pt) => p.x;
  const along = input.ridgeAxis === "x" ? (p: Pt) => p.x : (p: Pt) => p.y;
  const lo = input.ridgeAxis === "x" ? b.minY : b.minX;
  const hi = input.ridgeAxis === "x" ? b.maxY : b.maxX;
  const mid = (lo + hi) / 2;
  const half = (hi - lo) / 2;
  return {
    under: (p) => base + (half - Math.abs(across(p) - mid)) * tan,
    vertical,
    across,
    along,
    ridge: mid,
    peak: base + half * tan + vertical,
    tan,
    cos,
  };
}

/**
 * Roof covering: one prism for flat and shed, two for a gable. With `trim`,
 * the top surface goes into `md` and the soffit and fascia go into `trim`, so
 * they can carry a plain material. Together they are one closed solid.
 */
export function buildRoofMesh(input: RoofInput, md: MeshData, trim: MeshData = md): RoofProfile | null {
  const prof = roofProfile(input);
  if (!prof) return null;
  const outline = offsetPolygon(input.footprint, Math.max(input.overhang, 0));
  if (outline.length < 3) return null;

  // Sheets and tiles run down the slope: u along the ridge, v down the slope.
  const slopeUv = (p: P3): Uv => [prof.along(p) * 0.001, (prof.across(p) / prof.cos) * 0.001];
  const top = (p: Pt) => prof.under(p) + prof.vertical;

  if (prof.ridge === null) {
    pushPrism(md, outline, { bottom: prof.under, top, topUv: slopeUv, skipBottom: true, skipSides: true });
    pushPrism(trim, outline, { bottom: prof.under, top, skipTop: true, sideUvFor: sideUv });
    return prof;
  }

  const ridge = prof.ridge;
  const axisX = input.ridgeAxis === "x";
  // across <= ridge, then across >= ridge
  const halves = [
    clipHalfPlane(outline, axisX ? 0 : 1, axisX ? 1 : 0, ridge),
    clipHalfPlane(outline, axisX ? 0 : -1, axisX ? -1 : 0, -ridge),
  ];
  for (const half of halves) {
    pushPrism(md, half, { bottom: prof.under, top, topUv: slopeUv, skipBottom: true, skipSides: true });
    pushPrism(trim, half, {
      bottom: prof.under,
      top,
      skipTop: true,
      // The two halves meet on the ridge line. No internal face there.
      skipEdge: (_i, a, b) =>
        Math.abs(prof.across(a) - ridge) < 0.01 && Math.abs(prof.across(b) - ridge) < 0.01,
    });
  }
  return prof;
}

/**
 * Infill between the flat wall tops and the sloped roof underside, along every
 * footprint edge that needs it. These are the gable end triangles (and the
 * tall side and sloped ends of a shed roof).
 */
export function buildRoofInfill(input: RoofInput, md: MeshData): void {
  const prof = roofProfile(input);
  if (!prof || prof.tan === 0) return;
  const fp = ensureCCW(input.footprint);
  const t = Math.max(input.infillThickness ?? 150, 10);
  const base = input.baseHeight;

  for (let i = 0; i < fp.length; i++) {
    const a = fp[i];
    const b = fp[(i + 1) % fp.length];
    const len = Math.hypot(b.x - a.x, b.y - a.y);
    if (len < 1) continue;
    const dir = { x: (b.x - a.x) / len, y: (b.y - a.y) / len };
    const inward = { x: -dir.y, y: dir.x };

    // Split where the edge crosses the ridge so every piece is planar.
    const stops: Pt[] = [a];
    if (prof.ridge !== null) {
      const da = prof.across(a) - prof.ridge;
      const db = prof.across(b) - prof.ridge;
      if (da * db < 0) {
        const k = da / (da - db);
        stops.push({ x: a.x + (b.x - a.x) * k, y: a.y + (b.y - a.y) * k });
      }
    }
    stops.push(b);

    for (let s = 0; s < stops.length - 1; s++) {
      const p = stops[s];
      const q = stops[s + 1];
      const hp = prof.under(p);
      const hq = prof.under(q);
      if (Math.max(hp, hq) - base < 1) continue;
      const uv = sideUv(dir);
      const outward: Triple = [dir.y, 0, dir.x];
      const inwardW: Triple = [-dir.y, 0, -dir.x];
      const shift = (pt: Pt): Pt => ({ x: pt.x + inward.x * t, y: pt.y + inward.y * t });
      const face = (pa: Pt, pb: Pt, ha: number, hb: number, out: Triple) => {
        const a0: P3 = { x: pa.x, y: pa.y, h: base };
        const b0: P3 = { x: pb.x, y: pb.y, h: base };
        const b1: P3 = { x: pb.x, y: pb.y, h: Math.max(hb, base) };
        const a1: P3 = { x: pa.x, y: pa.y, h: Math.max(ha, base) };
        md.pushQuad(a0, b0, b1, a1, out, uv);
      };
      face(p, q, hp, hq, outward);
      const pi = shift(p);
      const qi = shift(q);
      face(pi, qi, prof.under(pi), prof.under(qi), inwardW);
    }
  }
}
