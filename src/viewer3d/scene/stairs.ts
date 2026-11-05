// Stairs in the 3D view (lane L2): straight, L and U, from the engine's
// `Derived.stairs` (DECISIONS D54), never from the stair's own numbers. Each
// flight is a concrete sawtooth with a sloped soffit, extruded across its
// width; each landing a 200 mm slab under its top. All of one stair is one
// mesh, cut at the cutaway height.

import * as THREE from "three";
import type { Level, Stair, StairGeometry } from "../../contract/bindings";
import type { Pt } from "../geom/coords";
import { MeshData, pushPrism, type P3 } from "../geom/meshData";
import { clipHalfPlane, ensureCCW, orientedRect, signedArea, triangulate } from "../geom/polygon";
import type { SceneHost } from "./buildScene";
import { tagElement, type Kit } from "./kit";
import type { MaterialLibrary } from "./materials";

/** The concrete under a flight, measured across its nosing line. */
export const STAIR_WAIST_MM = 200;
/** A landing's slab. */
export const LANDING_SLAB_MM = 200;

/** The engine's geometry of a stair, if the document has it. */
export function stairGeometryOf(derived: { stairs?: StairGeometry[] } | null | undefined, id: string): StairGeometry | null {
  return derived?.stairs?.find((g) => g.stair_id === id) ?? null;
}

/** Every stair on a shown level, while the Stairs layer is on, into `host.root`. */
export function buildStairs(host: SceneHost): void {
  const { doc, lib, kit, root, levels, lowest, levelShown, levelCut, visible, locked, addBounds } = host;
  lib.category = "stair";
  if (!visible("stairs")) return;
  for (const e of doc.project.elements) {
    if (e.kind !== "stair") continue;
    const level = levels.get(e.level_id) ?? lowest;
    if (!level || !levelShown(level)) continue;
    const cut = levelCut(level);
    const g = stairGeometryOf(doc.derived, e.id);
    const mesh = buildStair(kit, lib, e, level, cut === null ? null : cut / 1000, g);
    if (!mesh) continue;
    tagElement(mesh, e.id, locked("stairs"));
    root.add(mesh);
    const top = level.elevation_mm + (cut ?? level.height_mm);
    for (const piece of g ? stairPieces(g) : [fallbackOutline(e)]) addBounds(piece, level.elevation_mm, top);
  }
}

/** Plan pieces of a derived stair: its flights and landings. */
export function stairPieces(g: StairGeometry): Pt[][] {
  return [...g.flights.map((f) => f.outline), ...g.landings.map((l) => l.polygon)];
}

/** One straight flight from its fields, for a stair the engine has not answered for yet. */
function fallbackOutline(s: Stair): Pt[] {
  const a = (s.rotation_deg * Math.PI) / 180;
  const mid = { x: s.origin.x - Math.sin(a) * (s.run_mm / 2), y: s.origin.y + Math.cos(a) * (s.run_mm / 2) };
  return orientedRect(mid, s.width_mm, s.run_mm, s.rotation_deg);
}

/**
 * A flight's side profile, (u up the flight, z above its own base), mm: the
 * sawtooth of its treads and risers, the top riser to the next floor or
 * landing, and a soffit `STAIR_WAIST_MM` under the nosing line back down to
 * the base.
 */
export function flightProfile(treads: number, going: number, rise: number): Pt[] {
  const run = treads * going;
  const pts: Pt[] = [{ x: 0, y: 0 }];
  for (let i = 0; i < treads; i++) {
    pts.push({ x: i * going, y: (i + 1) * rise });
    pts.push({ x: (i + 1) * going, y: (i + 1) * rise });
  }
  const topH = treads * rise;
  const under = Math.max(topH - rise - STAIR_WAIST_MM, 0);
  pts.push({ x: run, y: under });
  if (under > 0) {
    const slope = rise / going;
    pts.push({ x: Math.max(run - under / slope, going * 0.5), y: 0 });
  }
  return pts;
}

/**
 * A vertical profile `prof` (x along `up` from `origin`, y height above
 * `base`), extruded `half` to each side of the `up` line, into `md`.
 */
export function pushProfile(md: MeshData, prof: Pt[], origin: Pt, up: Pt, half: number, base: number): void {
  const poly = ensureCCW(prof);
  if (poly.length < 3) return;
  const right = { x: up.y, y: -up.x };
  const at = (p: Pt, side: number): P3 => ({ x: origin.x + up.x * p.x + right.x * side, y: origin.y + up.y * p.x + right.y * side, h: base + p.y });
  const uvSide = (p: P3): [number, number] => [((p.x - origin.x) * up.x + (p.y - origin.y) * up.y) * 0.001, p.h * 0.001];
  for (const side of [-half, half]) {
    // World outward of the side face: plan right maps to world (x, -y).
    const out: [number, number, number] = [right.x * Math.sign(side), 0, -right.y * Math.sign(side)];
    for (const [i, j, k] of triangulate(poly)) {
      const a = at(poly[i], side);
      const b = at(poly[j], side);
      const c = at(poly[k], side);
      md.pushTri(a, b, c, out, uvSide(a), uvSide(b), uvSide(c));
    }
  }
  for (let i = 0; i < poly.length; i++) {
    const p = poly[i];
    const q = poly[(i + 1) % poly.length];
    const du = q.x - p.x;
    const dz = q.y - p.y;
    const len = Math.hypot(du, dz);
    if (len < 1e-6) continue;
    // Outward of a counter-clockwise profile edge: its right, (dz, -du) in (u, z).
    const nu = dz / len;
    const nz = -du / len;
    const out: [number, number, number] = [up.x * nu, nz, -up.y * nu];
    const uv = (pt: P3): [number, number] => [((pt.x - origin.x) * right.x + (pt.y - origin.y) * right.y) * 0.001, (((pt.x - origin.x) * up.x + (pt.y - origin.y) * up.y) + pt.h) * 0.001];
    md.pushQuad(at(p, -half), at(q, -half), at(q, half), at(p, half), out, uv);
  }
}

/** Plan frame of a derived flight from its outline: first riser middle, up, width, run. */
export function flightFrame(f: StairGeometry["flights"][number]): { origin: Pt; up: Pt; width: number; run: number } | null {
  if (f.outline.length !== 4) return null;
  const [b0, b1, , t0] = f.outline;
  const width = Math.hypot(b1.x - b0.x, b1.y - b0.y);
  const run = (t0.x - b0.x) * f.up.x + (t0.y - b0.y) * f.up.y;
  if (!(width > 1 && run > 1)) return null;
  return { origin: { x: (b0.x + b1.x) / 2, y: (b0.y + b1.y) / 2 }, up: f.up, width, run };
}

/**
 * The solids of a derived stair into `md`, heights in mm above the project
 * zero: every flight's profile and every landing's slab, clipped to `cutMm`
 * above its level's floor when given.
 */
export function pushStairGeometry(md: MeshData, g: StairGeometry, floorMm: number, cutMm: number | null): void {
  const rise = g.riser_mm;
  for (const f of g.flights) {
    const fr = flightFrame(f);
    if (!fr || f.nosings.length === 0) continue;
    const below = Math.max(Math.round(f.nosings[0].z / rise) - 1, 0);
    const baseZ = below * rise;
    let prof = flightProfile(f.nosings.length, fr.run / f.nosings.length, rise);
    if (cutMm !== null) {
      if (cutMm - baseZ < 20) continue;
      prof = clipHalfPlane(prof, 0, 1, cutMm - baseZ);
    }
    if (prof.length < 3) continue;
    pushProfile(md, prof, fr.origin, fr.up, fr.width / 2, floorMm + baseZ);
  }
  for (const l of g.landings) {
    const top = cutMm === null ? l.z : Math.min(l.z, cutMm);
    const bottom = l.z - LANDING_SLAB_MM;
    if (top - bottom < 20 || Math.abs(signedArea(l.polygon)) < 1) continue;
    pushPrism(md, l.polygon, { bottom: () => floorMm + bottom, top: () => floorMm + top });
  }
}

/**
 * Concrete stair as one mesh. With the engine's geometry `g`, every flight
 * and landing; without it (a ghost the engine has not answered for), the
 * straight flight its fields give, as Wave 1 drew it. `maxHeight` (meters
 * above the level floor) is the cutaway height.
 */
export function buildStair(kit: Kit, lib: MaterialLibrary, s: Stair, level: Level, maxHeight?: number | null, g?: StairGeometry | null): THREE.Mesh | null {
  const cutMm = maxHeight == null ? null : maxHeight * 1000;
  if (cutMm !== null && cutMm < 20) return null;
  const md = new MeshData();
  if (g) {
    pushStairGeometry(md, g, level.elevation_mm, cutMm);
    if (md.triangleCount === 0) return null;
    return kit.mesh(kit.fromMeshData(md), lib.get("mat-floor-concrete", undefined, false));
  }
  const risers = Math.min(Math.max(Math.round(s.riser_count), 1), 200);
  const treads = Math.max(risers - 1, 1);
  const rise = level.height_mm / risers;
  if (!(s.run_mm > 10 && s.width_mm > 10 && level.height_mm > 10)) return null;
  let prof = flightProfile(treads, s.run_mm / treads, rise);
  if (cutMm !== null) prof = clipHalfPlane(prof, 0, 1, cutMm);
  if (prof.length < 3) return null;
  const a = (s.rotation_deg * Math.PI) / 180;
  pushProfile(md, prof, s.origin, { x: -Math.sin(a), y: Math.cos(a) }, s.width_mm / 2, level.elevation_mm);
  if (md.triangleCount === 0) return null;
  return kit.mesh(kit.fromMeshData(md), lib.get("mat-floor-concrete", undefined, false));
}
