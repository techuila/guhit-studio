// Placement math for mounted objects (`CatalogItem::mount`): wall devices on
// a wall face, the latch-side switch guide at a door, ceiling heights and the
// window aircon in a window opening. Pure, tested in mount.test.ts.
// Contract: docs/CONTRACT.md, "Devices, fixtures and links".

import type { Asset, CatalogItem, DisplayUnit, Mount, Opening, Wall } from "../contract/bindings";
import type { P, Seg } from "./geom";
import { formatHeight } from "./pipe";
import {
  add,
  angleDeg,
  clamp,
  closestOnLine,
  closestOnSegment,
  cross,
  dist,
  dot,
  len,
  lerp,
  mul,
  normDeg,
  pointInPolygon,
  polygonArea,
  roundTo,
  sub,
  unit,
} from "./geom";

const EPS = 1e-6;

/** Switch centers go this far from the latch jamb of the nearest door (BP 344 IRR 2024). */
export const LATCH_OFFSET_MM = 200;

/** The level height the catalog's ceiling elevations were written for (`DEFAULT_LEVEL_HEIGHT_MM`). */
export const CATALOG_LEVEL_HEIGHT_MM = 3000;

/** A door or window on the host wall, as a span along a face, mm from the face start. */
export interface FaceGap {
  id: string;
  from: number;
  to: number;
}

/** One long face of a wall, where wall objects mount with their back. */
export interface WallFace {
  wallId: string;
  /** The face edge of the resolved wall outline. */
  a: P;
  b: P;
  /** Unit normal pointing away from the wall body. */
  out: P;
  /** Which side of the wall direction (start to end) the face is on: left is +1. */
  side: 1 | -1;
  /** Openings of the host wall along this face. */
  gaps: FaceGap[];
}

/**
 * The two long faces of a wall, from its resolved outline (mitred corners),
 * with its openings as gaps. End caps and short bevels are not faces.
 */
export function facesOfWall(
  wall: Pick<Wall, "id" | "start" | "end" | "thickness_mm">,
  outline: readonly P[],
  openings: readonly Pick<Opening, "id" | "offset_mm" | "width_mm">[],
): WallFace[] {
  const dir = unit(sub(wall.end, wall.start));
  const faces: WallFace[] = [];
  for (let i = 0; i < outline.length; i++) {
    const a = outline[i];
    const b = outline[(i + 1) % outline.length];
    const L = dist(a, b);
    if (L <= wall.thickness_mm * 1.5 + 1) continue;
    const along = unit(sub(b, a));
    // Faces run along the wall. Anything else is a cap or a bevel.
    if (Math.abs(cross(along, dir)) > 0.05) continue;
    const m = lerp(a, b, 0.5);
    const off = sub(m, closestOnLine(m, wall.start, wall.end));
    if (len(off) < wall.thickness_mm * 0.25) continue;
    const out = unit(off);
    const gaps: FaceGap[] = [];
    for (const o of openings) {
      const ja = add(wall.start, mul(dir, o.offset_mm - o.width_mm / 2));
      const jb = add(wall.start, mul(dir, o.offset_mm + o.width_mm / 2));
      const ta = dot(sub(ja, a), along);
      const tb = dot(sub(jb, a), along);
      const g = { id: o.id, from: Math.min(ta, tb), to: Math.max(ta, tb) };
      if (g.to > 0 && g.from < L) gaps.push(g);
    }
    faces.push({ wallId: wall.id, a, b, out, side: cross(dir, out) >= 0 ? 1 : -1, gaps });
  }
  return faces;
}

export interface WallMount {
  face: WallFace;
  /** Center of the object along the face, mm from `face.a`. */
  t: number;
  /** Footprint center: the back sits on the face. */
  position: P;
  rotationDeg: number;
  /** False when the object cannot go here. See `reason`. */
  valid: boolean;
  /** "short": the face is narrower than the object. "opening": only a door or window is left there. */
  reason: "short" | "opening" | null;
}

/** Rotation that puts an object's back (local +y) against a face whose normal is `out`. */
export function rotationFacing(out: P): number {
  return normDeg(angleDeg(mul(out, -1)) - 90);
}

/**
 * `t` rounded to a grid `step`: on a face that runs along x or y the object
 * lands on the plan grid (a face often spans several rooms, so its start is
 * no useful origin); on any other face the distance from the start rounds.
 */
export function roundAlongFace(face: Pick<WallFace, "a" | "b">, t: number, step: number): number {
  if (step <= 0) return t;
  const along = unit(sub(face.b, face.a));
  if (Math.abs(along.x) > 0.9999 || Math.abs(along.y) > 0.9999) {
    const axis = Math.abs(along.x) > 0.9999 ? "x" : "y";
    const p = face.a[axis] + along[axis] * t;
    return (roundTo(p, step) - face.a[axis]) / along[axis];
  }
  return roundTo(t, step);
}

/**
 * Puts a w x d object with its back on `face`, centered at `tRaw` mm from the
 * face start, rounded to the grid `step` (`roundAlongFace`). The object stays
 * on the face and never covers a door or window: it moves to the nearer free
 * side of the opening instead.
 */
export function mountOnFace(face: WallFace, tRaw: number, width: number, depth: number, step: number): WallMount {
  const along = unit(sub(face.b, face.a));
  const L = dist(face.a, face.b);
  const lo = width / 2;
  const hi = L - width / 2;
  let valid = true;
  let reason: WallMount["reason"] = null;
  let t: number;
  if (hi < lo - EPS) {
    t = L / 2;
    valid = false;
    reason = "short";
  } else {
    t = clamp(roundAlongFace(face, tRaw, step), lo, hi);
    const blocked = (x: number): FaceGap | undefined => face.gaps.find((g) => x + width / 2 > g.from + EPS && x - width / 2 < g.to - EPS);
    const g = blocked(t);
    if (g) {
      const options = [g.from - width / 2, g.to + width / 2]
        .filter((x) => x >= lo - EPS && x <= hi + EPS && !blocked(x))
        .sort((x, y) => Math.abs(x - tRaw) - Math.abs(y - tRaw));
      if (options.length > 0) t = options[0];
      else {
        valid = false;
        reason = "opening";
      }
    }
  }
  const position = add(add(face.a, mul(along, t)), mul(face.out, depth / 2));
  return { face, t, position, rotationDeg: rotationFacing(face.out), valid, reason };
}

/**
 * The face nearest the cursor within `reach` mm, with the object mounted on
 * it under the cursor. Null when no face is in reach.
 */
export function snapToWallFace(cursor: P, faces: readonly WallFace[], width: number, depth: number, reach: number, step: number): WallMount | null {
  let best: WallFace | null = null;
  let bestD = reach;
  for (const f of faces) {
    const d = closestOnSegment(cursor, f.a, f.b).dist;
    if (d <= bestD) {
      bestD = d;
      best = f;
    }
  }
  if (!best) return null;
  const along = unit(sub(best.b, best.a));
  return mountOnFace(best, dot(sub(cursor, best.a), along), width, depth, step);
}

// ---------------------------------------------------------------- the latch-side switch guide

export interface LatchGuide {
  doorId: string;
  wallId: string;
  /** Side of the wall direction the guide is on, like `WallFace.side`. */
  side: 1 | -1;
  /** The latch jamb, on the face. The 200 mm is measured from here. */
  jamb: P;
  /** Where the switch center goes, on the face. */
  point: P;
  /** Face normal, away from the wall. */
  out: P;
  /** Along the face, from the jamb away from the door. */
  away: P;
}

/**
 * The jambs a switch should sit beside: the one opposite the hinge of a
 * single swing door, both of a sliding door. A double swing door has hinges
 * on both jambs, so it has no latch side.
 */
export function latchJambs(door: Pick<Opening, "opening_type" | "style" | "flip_hinge">): ("a" | "b")[] {
  if (door.opening_type !== "door") return [];
  if (door.style === "swing_single") return [door.flip_hinge ? "a" : "b"];
  if (door.style === "sliding") return ["a", "b"];
  return [];
}

/**
 * Switch guides of a door on both faces of its wall: `offset` mm from each
 * latch jamb, away from the door. Hinge jambs never get one, so a switch
 * placed by the guide is never behind the open leaf.
 */
export function latchGuides(
  door: Pick<Opening, "id" | "opening_type" | "style" | "flip_hinge" | "offset_mm" | "width_mm">,
  wall: Pick<Wall, "id" | "start" | "end" | "thickness_mm">,
  offset = LATCH_OFFSET_MM,
): LatchGuide[] {
  const dir = unit(sub(wall.end, wall.start));
  const normal = { x: -dir.y, y: dir.x };
  const out: LatchGuide[] = [];
  for (const j of latchJambs(door)) {
    const at = door.offset_mm + (j === "a" ? -door.width_mm / 2 : door.width_mm / 2);
    const away = j === "a" ? mul(dir, -1) : dir;
    for (const side of [1, -1] as const) {
      const n = mul(normal, side);
      const jamb = add(add(wall.start, mul(dir, at)), mul(n, wall.thickness_mm / 2));
      out.push({ doorId: door.id, wallId: wall.id, side, jamb, point: add(jamb, mul(away, offset)), out: n, away });
    }
  }
  return out;
}

/**
 * Keeps the guides where a `width` wide switch fits on the face: on the
 * wall, clear of every door and window.
 */
export function usableGuides(guides: readonly LatchGuide[], faces: readonly WallFace[], width: number): LatchGuide[] {
  return guides.filter((g) => {
    const face = faces.find((f) => f.wallId === g.wallId && f.side === g.side);
    if (!face) return false;
    const along = unit(sub(face.b, face.a));
    const t = dot(sub(g.point, face.a), along);
    const L = dist(face.a, face.b);
    if (t < width / 2 - EPS || t > L - width / 2 + EPS) return false;
    return !face.gaps.some((gap) => t + width / 2 > gap.from + EPS && t - width / 2 < gap.to - EPS);
  });
}

/** The guide nearest the cursor, with its distance. */
export function nearestGuide(cursor: P, guides: readonly LatchGuide[]): { guide: LatchGuide; d: number } | null {
  let best: { guide: LatchGuide; d: number } | null = null;
  for (const g of guides) {
    const d = dist(cursor, g.point);
    if (!best || d < best.d) best = { guide: g, d };
  }
  return best;
}

/** A switch mounted at a guide: its back on the face, its center on the guide point. */
export function mountAtGuide(g: LatchGuide, depth: number): { position: P; rotationDeg: number } {
  return { position: add(g.point, mul(g.out, depth / 2)), rotationDeg: rotationFacing(g.out) };
}

// ---------------------------------------------------------------- ceiling

/** Slab under an upper level, as the 3D view draws it (docs/CONTRACT.md). */
export const UPPER_SLAB_MM = 200;

/**
 * Ceiling height of a level above its floor: its height, or the underside of
 * the next level's slab when that is lower (docs/CONTRACT.md, "Devices,
 * fixtures and links").
 */
export function ceilingHeightMm(
  level: { elevation_mm: number; height_mm: number } | undefined,
  levels: readonly { elevation_mm: number }[],
): number {
  if (!level) return CATALOG_LEVEL_HEIGHT_MM;
  let ceiling = level.height_mm;
  for (const l of levels) {
    if (l.elevation_mm > level.elevation_mm + 1) {
      ceiling = Math.min(ceiling, l.elevation_mm - UPPER_SLAB_MM - level.elevation_mm);
    }
  }
  return Math.max(0, ceiling);
}

/**
 * Underside height of a ceiling object on a level `levelHeight` mm high: it
 * hangs from the level height, so the underside is the level height minus
 * the object height. A catalog item written to hang lower than flush (a
 * pendant on its cord) keeps its catalog underside, lowered only when the
 * ceiling would cut it.
 */
export function ceilingElevation(levelHeight: number, item: Pick<CatalogItem, "height_mm" | "elevation_mm">): number {
  const flush = levelHeight - item.height_mm;
  const catalogFlush = CATALOG_LEVEL_HEIGHT_MM - item.height_mm;
  if (item.elevation_mm < catalogFlush - 1) return Math.max(0, Math.min(item.elevation_mm, flush));
  return Math.max(0, flush);
}

/**
 * The middle of the room around the cursor (its area centroid, or the label
 * point when the centroid falls outside an L shaped room), when the cursor
 * is within `tol` mm of it.
 */
export function roomCenterSnap(cursor: P, rooms: readonly { polygon: readonly P[]; label_point: P }[], tol: number): P | null {
  for (const r of rooms) {
    if (r.polygon.length < 3 || !pointInPolygon(cursor, r.polygon)) continue;
    const c = polygonCentroid(r.polygon);
    const center = c && pointInPolygon(c, r.polygon) ? c : r.label_point;
    return dist(cursor, center) <= tol ? center : null;
  }
  return null;
}

export function polygonCentroid(poly: readonly P[]): P | null {
  const A = polygonArea(poly);
  if (Math.abs(A) < EPS) return null;
  let x = 0;
  let y = 0;
  for (let i = 0; i < poly.length; i++) {
    const p = poly[i];
    const q = poly[(i + 1) % poly.length];
    const k = cross(p, q);
    x += (p.x + q.x) * k;
    y += (p.y + q.y) * k;
  }
  return { x: x / (6 * A), y: y / (6 * A) };
}

// ---------------------------------------------------------------- window aircon

export interface WindowHost {
  opening: Pick<Opening, "id" | "opening_type" | "offset_mm" | "width_mm" | "height_mm" | "sill_mm">;
  wall: Pick<Wall, "id" | "start" | "end" | "thickness_mm">;
}

/** The window whose opening is nearest the cursor, within `reach` mm of its span. */
export function nearestWindow<T extends WindowHost>(cursor: P, hosts: readonly T[], reach: number): T | null {
  let best: T | null = null;
  let bestD = reach;
  for (const h of hosts) {
    if (h.opening.opening_type !== "window") continue;
    const dir = unit(sub(h.wall.end, h.wall.start));
    const a = add(h.wall.start, mul(dir, h.opening.offset_mm - h.opening.width_mm / 2));
    const b = add(h.wall.start, mul(dir, h.opening.offset_mm + h.opening.width_mm / 2));
    const d = closestOnSegment(cursor, a, b).dist;
    if (d <= bestD) {
      bestD = d;
      best = h;
    }
  }
  return best;
}

export interface WindowMount {
  openingId: string;
  wallId: string;
  /** Center of the unit, on the wall centerline. */
  position: P;
  /** The back (local +y) points to `outside`. */
  rotationDeg: number;
  /** Underside height: the catalog height, kept inside the opening. */
  elevation: number;
  /** False when the unit is wider than the window or taller than its opening. */
  fits: boolean;
  /** True when it snapped to the middle of the window. */
  centered: boolean;
  /** The window opening on the centerline, for the ghost. */
  span: Seg;
}

/**
 * A window aircon set into `host`: across the wall, centered on its
 * centerline, sliding along the window under the cursor and snapping to its
 * middle within `tol` mm. `outside` is the side of the wall direction the
 * back (the condenser) faces: +1 left, -1 right.
 */
export function mountInWindow(
  cursor: P,
  host: WindowHost,
  size: { width: number; depth: number; height: number; elevation: number },
  outside: 1 | -1,
  tol: number,
): WindowMount {
  const { opening: o, wall } = host;
  const dir = unit(sub(wall.end, wall.start));
  const normal = { x: -dir.y, y: dir.x };
  const lo = o.offset_mm - o.width_mm / 2 + size.width / 2;
  const hi = o.offset_mm + o.width_mm / 2 - size.width / 2;
  const fitsWidth = hi >= lo - EPS;
  let t = o.offset_mm;
  let centered = true;
  if (fitsWidth) {
    const raw = dot(sub(cursor, wall.start), dir);
    if (Math.abs(raw - o.offset_mm) > tol) {
      t = clamp(raw, lo, hi);
      centered = Math.abs(t - o.offset_mm) < 0.5;
    }
  }
  const top = o.sill_mm + o.height_mm - size.height;
  const fitsHeight = top >= o.sill_mm - EPS;
  const elevation = fitsHeight ? clamp(size.elevation, o.sill_mm, top) : o.sill_mm;
  return {
    openingId: o.id,
    wallId: wall.id,
    position: add(wall.start, mul(dir, t)),
    rotationDeg: rotationFacing(mul(normal, -outside)),
    elevation,
    fits: fitsWidth && fitsHeight,
    centered,
    span: {
      a: add(wall.start, mul(dir, o.offset_mm - o.width_mm / 2)),
      b: add(wall.start, mul(dir, o.offset_mm + o.width_mm / 2)),
    },
  };
}

// ---------------------------------------------------------------- the height readout

/**
 * The mounting height shown near the cursor while placing: the box center
 * for wall devices (switches at 1200 to center), else the underside. Nothing
 * for an object standing on the floor.
 */
export function mountHeightLabel(kind: Mount, el: Pick<Asset, "category" | "elevation_mm" | "height_mm">, unit: DisplayUnit): string | null {
  if (kind === "floor" && Math.abs(el.elevation_mm) < 0.5) return null;
  // Centers read to the 5 mm: the catalog's 1143 + 115 / 2 is the 1200 of the rule.
  if (kind === "wall" && el.category === "electrical") return `Center ${formatHeight(Math.round((el.elevation_mm + el.height_mm / 2) / 5) * 5, unit, true)}`;
  return `Underside ${formatHeight(el.elevation_mm, unit, true)}`;
}
