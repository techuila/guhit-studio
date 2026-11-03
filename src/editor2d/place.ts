// Placement math for openings on walls and assets against wall faces.
// Pure, tested in place.test.ts.

import type { OpeningStyle, RoomGeometry, Wall } from "../contract/bindings";
import type { P, Seg } from "./geom";
import { add, angleDeg, clamp, closestOnSegment, cross, dist, dot, mul, normDeg, roundTo, sub, unit } from "./geom";

export interface OpeningPlacement {
  wallId: string;
  /** Opening center from the wall start, mm. */
  offset: number;
  /** Clear distance from each jamb to the inside corner at that wall end. */
  clearStart: number;
  clearEnd: number;
  /** False when the opening cannot go here. See `reason`. */
  valid: boolean;
  reason: "short" | "overlap" | null;
  /** +1 when the cursor is on the left of the wall direction, else -1. */
  side: 1 | -1;
  snapped: "center" | "clear" | null;
}

/** Another opening on the same wall: center offset and width. */
export interface OpeningSpan {
  offset: number;
  width: number;
}

/** The wall whose body is under `p`, or the nearest one within `tol` of its faces. */
export function findHostWall(p: P, walls: readonly Wall[], tol: number): Wall | null {
  let best: Wall | null = null;
  let bestD = Infinity;
  for (const w of walls) {
    const c = closestOnSegment(p, w.start, w.end);
    const reach = w.thickness_mm / 2 + tol;
    if (c.dist <= reach && c.dist < bestD) {
      bestD = c.dist;
      best = w;
    }
  }
  return best;
}

/**
 * Slides an opening of `width` along `wall` under the cursor. `insetStart`
 * and `insetEnd` are the half thickness of joined walls at each end, so
 * clear distances are measured from the inside corner.
 */
export function placeOnWall(
  p: P,
  wall: Wall,
  width: number,
  insetStart: number,
  insetEnd: number,
  step: number,
  tol: number,
  snapEnabled: boolean,
  others: readonly OpeningSpan[] = [],
): OpeningPlacement {
  const d = unit(sub(wall.end, wall.start));
  const L = dist(wall.start, wall.end);
  const lo = insetStart + width / 2;
  const hi = L - insetEnd - width / 2;
  let valid = hi >= lo - 1e-6;
  let reason: OpeningPlacement["reason"] = valid ? null : "short";
  let t = dot(sub(p, wall.start), d);
  const raw = t;
  let snapped: OpeningPlacement["snapped"] = null;
  if (valid) {
    t = clamp(t, lo, hi);
    if (snapEnabled) {
      const center = (lo + hi) / 2;
      if (Math.abs(t - center) <= tol) {
        t = center;
        snapped = "center";
      } else if (step > 0) {
        const cs = t - lo;
        const ce = hi - t;
        if (cs <= ce) t = lo + roundTo(cs, step);
        else t = hi - roundTo(ce, step);
        t = clamp(t, lo, hi);
        snapped = "clear";
      }
    }
    // Other openings on this wall: bump against the nearest free edge.
    const hits = (at: number): OpeningSpan | undefined => others.find((o) => Math.abs(at - o.offset) < (width + o.width) / 2 - 1e-6);
    const hit = hits(t);
    if (hit) {
      const gap = (width + hit.width) / 2;
      const pushed = raw >= hit.offset ? hit.offset + gap : hit.offset - gap;
      if (pushed >= lo - 1e-6 && pushed <= hi + 1e-6 && !hits(pushed)) {
        t = pushed;
        snapped = "clear";
      } else {
        valid = false;
        reason = "overlap";
      }
    }
  } else {
    t = L / 2;
  }
  const side: 1 | -1 = cross(d, sub(p, wall.start)) >= 0 ? 1 : -1;
  return {
    wallId: wall.id,
    offset: t,
    clearStart: t - width / 2 - insetStart,
    clearEnd: L - insetEnd - (t + width / 2),
    valid,
    reason,
    side,
    snapped,
  };
}

/**
 * The narrowest each style still works at, mm: below this a door is no longer
 * a door you walk through, a window no longer one you open. The door and
 * window tools narrow to fit a short wall, never below these.
 */
export const OPENING_MIN_WIDTH: Record<"door" | "window", Partial<Record<OpeningStyle, number>>> = {
  door: { swing_single: 600, swing_double: 1000, sliding: 700 },
  window: { sliding: 600, casement: 400, jalousie: 300, fixed: 300 },
};

/** Narrowed widths step down in 50 mm, the sizes doors and windows are sold in. */
export const OPENING_FIT_STEP = 50;

export function openingMinWidth(type: "door" | "window", style: OpeningStyle): number {
  return OPENING_MIN_WIDTH[type][style] ?? (type === "door" ? 600 : 300);
}

/**
 * The free stretch of wall around `at` (mm from the wall start): between the
 * inside corners, less the other openings. Null when `at` is inside one.
 */
export function freeGapAt(at: number, length: number, insetStart: number, insetEnd: number, others: readonly OpeningSpan[]): { lo: number; hi: number } | null {
  let lo = insetStart;
  let hi = length - insetEnd;
  const t = clamp(at, lo, hi);
  for (const o of others) {
    const a = o.offset - o.width / 2;
    const b = o.offset + o.width / 2;
    if (t > a && t < b) return null;
    if (b <= t) lo = Math.max(lo, b);
    if (a >= t) hi = Math.min(hi, a);
  }
  return hi > lo ? { lo, hi } : null;
}

export interface OpeningFit {
  placement: OpeningPlacement;
  /** The width placed: `wanted`, or narrower to fit. */
  width: number;
  /** True when `width` is narrower than `wanted`. */
  narrowed: boolean;
  /** The free length the opening had to fit into. */
  clear: number;
  /** The style's narrowest width (`OPENING_MIN_WIDTH`). */
  min: number;
}

/**
 * Places an opening of `wanted` width, or when it does not fit the free
 * stretch under the cursor, the widest 50 mm size that does, down to `min`.
 * When even `min` does not fit, the placement is refused at `wanted`.
 */
export function fitOnWall(
  p: P,
  wall: Wall,
  wanted: number,
  min: number,
  insetStart: number,
  insetEnd: number,
  step: number,
  tol: number,
  snapEnabled: boolean,
  others: readonly OpeningSpan[] = [],
): OpeningFit {
  const first = placeOnWall(p, wall, wanted, insetStart, insetEnd, step, tol, snapEnabled, others);
  const L = dist(wall.start, wall.end);
  const at = dot(sub(p, wall.start), unit(sub(wall.end, wall.start)));
  const gap = freeGapAt(at, L, insetStart, insetEnd, others);
  const clear = gap ? gap.hi - gap.lo : 0;
  if (first.valid) return { placement: first, width: wanted, narrowed: false, clear, min };
  const width = Math.floor((clear + 1e-6) / OPENING_FIT_STEP) * OPENING_FIT_STEP;
  if (gap && width >= min && width < wanted) {
    const narrow = placeOnWall(p, wall, width, insetStart, insetEnd, step, tol, snapEnabled, others);
    if (narrow.valid) return { placement: narrow, width, narrowed: true, clear, min };
  }
  return { placement: first, width: wanted, narrowed: false, clear, min };
}

const STYLE_NOUN: Record<"door" | "window", Partial<Record<OpeningStyle, string>>> = {
  door: { swing_single: "single swing door", swing_double: "double swing door", sliding: "sliding door" },
  window: { sliding: "sliding window", casement: "casement window", jalousie: "jalousie window", fixed: "fixed window" },
};

/**
 * The short note beside the cursor for a door or window ghost: the width it
 * narrowed to, or why it cannot go here and what would. Null when it places
 * as chosen. `fmt` formats a length in the display unit.
 */
export function openingFitNote(type: "door" | "window", style: OpeningStyle, fit: OpeningFit, fmt: (mm: number) => string): string | null {
  if (fit.placement.valid) return fit.narrowed ? `Narrowed to ${fmt(fit.width)} to fit` : null;
  const noun = STYLE_NOUN[type][style] ?? type;
  // Nothing free under the cursor, or room enough that only a neighbor is in the way.
  if (fit.clear <= 0 || fit.clear >= fit.min) return fit.placement.reason === "short" ? "Wall too short" : "Overlaps another opening";
  const where = fit.placement.reason === "overlap" ? "free here" : "clear";
  const others = Object.entries(OPENING_MIN_WIDTH[type])
    .filter(([key, min]) => key !== style && min !== undefined && min <= fit.clear)
    .sort((a, b) => (b[1] ?? 0) - (a[1] ?? 0));
  const alt = others.length > 0 ? `. A ${STYLE_NOUN[type][others[0][0] as OpeningStyle]} fits` : "";
  return `${fmt(fit.clear)} ${where}, a ${noun} needs ${fmt(fit.min)}${alt}`;
}

/**
 * Which side of `wall`'s centerline (start -> end direction, left is +1) a
 * room bounded by it sits on, or null when no room among `rooms` uses this
 * wall. A wall can host at most one room per side, so the first match wins.
 */
export function roomSideOfWall(wall: Wall, rooms: readonly Pick<RoomGeometry, "wall_ids" | "label_point">[]): 1 | -1 | null {
  const room = rooms.find((r) => r.wall_ids.includes(wall.id));
  if (!room) return null;
  const d = unit(sub(wall.end, wall.start));
  return cross(d, sub(room.label_point, wall.start)) >= 0 ? 1 : -1;
}

/**
 * The side a door leaf swings toward, for a cursor at `p` on its host wall.
 * Outside a band of half the wall thickness around the centerline, the
 * cursor's side decides (matching `placeOnWall`'s `side`). Inside that band,
 * an exterior wall with a known room side defaults to swinging into the
 * room; an interior wall, or an exterior wall with no known room side,
 * keeps following the cursor.
 */
export function doorSwingSide(p: P, wall: Wall, exterior: boolean, roomSide: 1 | -1 | null): 1 | -1 {
  const d = unit(sub(wall.end, wall.start));
  const perp = cross(d, sub(p, wall.start));
  const cursorSide: 1 | -1 = perp >= 0 ? 1 : -1;
  if (exterior && roomSide !== null && Math.abs(perp) < wall.thickness_mm / 2) return roomSide;
  return cursorSide;
}

export interface FaceSnap {
  position: P;
  rotationDeg: number;
  face: Seg;
}

/**
 * Puts the back of a w x d footprint against the nearest wall face.
 * `turns` is the number of extra 90 degree turns the user asked for.
 * Returns null when no face is within reach of the cursor.
 */
export function snapToFace(
  cursor: P,
  faces: readonly Seg[],
  width: number,
  depth: number,
  turns: number,
  tol: number,
  step: number,
): FaceSnap | null {
  const half = turns % 2 === 0 ? depth / 2 : width / 2;
  let best: { seg: Seg; point: P; d: number } | null = null;
  for (const s of faces) {
    const c = closestOnSegment(cursor, s.a, s.b);
    if (c.dist <= half + tol && (!best || c.dist < best.d)) best = { seg: s, point: c.point, d: c.dist };
  }
  if (!best || best.d < 1e-6) return null;
  const away = unit(sub(cursor, best.point));
  const along = unit(sub(best.seg.b, best.seg.a));
  // Only snap when the cursor is off the face sideways, not past its ends.
  if (Math.abs(dot(away, along)) > 0.2) return null;
  const L = dist(best.seg.a, best.seg.b);
  let t = dot(sub(best.point, best.seg.a), along);
  if (step > 0) t = clamp(roundTo(t, step), 0, L);
  const onFace = add(best.seg.a, mul(along, t));
  const normal = { x: -along.y, y: along.x };
  const n = dot(normal, away) >= 0 ? normal : mul(normal, -1);
  // Local +y (the back) points into the wall.
  const rotationDeg = normDeg(angleDeg(mul(n, -1)) - 90 + turns * 90);
  return { position: add(onFace, mul(n, half)), rotationDeg, face: best.seg };
}
