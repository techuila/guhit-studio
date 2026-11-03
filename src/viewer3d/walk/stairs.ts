// Stairs in walk mode. Pure plan math in millimeters, no three.js, no DOM.
//
// A `Stair` is one straight flight: `origin` is the middle of the first
// riser, the flight climbs along the rotated +y and is `width_mm` wide across
// it, and its total rise is the height of its level (docs/CONTRACT.md,
// "Stairs"). Walking, a flight is a ramp: the floor under the walker follows
// the line through the middle of every tread, so the eye rises smoothly
// instead of stepping, and it never climbs past the level height.
//
// A flight joins its own level to the level whose floor sits at its top.
// Around it the walker meets thin strips of wall, so the rest of the stair is
// solid: on the lower level the sides and the head are closed, on the upper
// level the sides and the foot (never into the stairwell). The first and last
// treads, within `STAIR_OPEN_RISE_MM` of either floor, stay open at the sides
// where that floor is beside them: a walker can step on or off the flight
// there at an angle, or off the top sideways when the head meets a wall.
//
// On the flight the walker is held by the stair's own sides, not by strips:
// `holdOnFlight` keeps it within the width where the sides are closed, and
// narrows over one walker radius where a closed side begins, a funnel with no
// corner to catch on. Walls and objects of either level that stand on the
// flight's footprint do not block a walker on it. A flight with no level at
// its top is closed at the head: the walker stops at the top.

import type { DocState } from "../../contract/bindings";
import { levelResolver, polyCollider, type Collider } from "../geom/collision";
import type { Pt } from "../geom/coords";

/** A level counts as the top of a flight when its floor is this close to it. */
export const STAIR_TOP_MATCH_MM = 300;
/** Thickness of the wall strips around a flight. */
export const STAIR_STRIP_MM = 20;
/** Within this rise of the floor at either end, the sides of a flight are open where that floor is beside them. */
export const STAIR_OPEN_RISE_MM = 450;

/** Which sides of a flight are open at its foot and at its head: [left, right], facing up the flight. */
export interface StairOpen {
  low: [boolean, boolean];
  high: [boolean, boolean];
}

/** Every side closed, foot to head. */
export const STAIR_CLOSED: StairOpen = { low: [false, false], high: [false, false] };

export interface StairInfo {
  id: string;
  /** The level the flight stands on. */
  levelId: string;
  /** The level at its top, or null when none matches: the flight ends at a wall. */
  topLevelId: string | null;
  /** Middle of the first riser. */
  origin: Pt;
  /** Unit vector up the flight. */
  dir: Pt;
  /** Unit vector across the flight, to the right when facing up it. */
  across: Pt;
  width: number;
  run: number;
  risers: number;
  /** Tread depth: run / risers. */
  going: number;
  /** Floor of `levelId`. */
  bottomZ: number;
  /** Total rise: the height of `levelId`. */
  rise: number;
}

/** Position relative to a flight: `u` up the flight from the first riser, `v` across it. */
export interface StairLocal {
  u: number;
  v: number;
}

/** Every flight on a shown stairs layer, with the level at its top. */
export function stairsOf(doc: DocState | null): StairInfo[] {
  if (!doc) return [];
  const project = doc.project;
  if (project.layers?.find((l) => l.key === "stairs")?.visible === false) return [];
  const levelOf = levelResolver(doc);
  const levels = project.levels ?? [];
  const out: StairInfo[] = [];
  for (const e of project.elements) {
    if (e.kind !== "stair") continue;
    const level = levelOf(e.level_id);
    if (!level) continue;
    // Same limits as the 3D form (scene/buildScene.ts, buildStair).
    const risers = Math.min(Math.max(Math.round(e.riser_count), 1), 200);
    const run = e.run_mm;
    const width = e.width_mm;
    const rise = level.height_mm;
    if (!(run > 10 && width > 10 && rise > 10)) continue;
    const top = level.elevation_mm + rise;
    let topLevelId: string | null = null;
    let best = Infinity;
    for (const l of levels) {
      if (l.id === level.id || !(l.elevation_mm > level.elevation_mm)) continue;
      const d = Math.abs(l.elevation_mm - top);
      if (d <= STAIR_TOP_MATCH_MM && d < best) {
        best = d;
        topLevelId = l.id;
      }
    }
    const a = (e.rotation_deg * Math.PI) / 180;
    out.push({
      id: e.id,
      levelId: level.id,
      topLevelId,
      origin: { x: e.origin.x, y: e.origin.y },
      dir: { x: -Math.sin(a), y: Math.cos(a) },
      across: { x: Math.cos(a), y: Math.sin(a) },
      width,
      run,
      risers,
      going: run / risers,
      bottomZ: level.elevation_mm,
      rise,
    });
  }
  return out;
}

export function stairLocal(s: StairInfo, p: Pt): StairLocal {
  const dx = p.x - s.origin.x;
  const dy = p.y - s.origin.y;
  return { u: dx * s.dir.x + dy * s.dir.y, v: dx * s.across.x + dy * s.across.y };
}

export function stairPoint(s: StairInfo, u: number, v: number): Pt {
  return { x: s.origin.x + s.dir.x * u + s.across.x * v, y: s.origin.y + s.dir.y * u + s.across.y * v };
}

/**
 * Height of the walking line above the stair's floor at `u`: the line through
 * the middle of every tread, from 0 half a going before the first riser up to
 * the level height half a going before the head, flat beyond both.
 */
export function rampHeight(s: StairInfo, u: number): number {
  const step = s.rise / s.risers;
  return Math.min(Math.max(step * (u / s.going + 0.5), 0), s.rise);
}

/** True when `p` is on the flight: between the first riser and the head, within the width. */
export function onFlight(s: StairInfo, p: Pt): boolean {
  const { u, v } = stairLocal(s, p);
  return u >= 0 && u <= s.run && Math.abs(v) <= s.width / 2;
}

/**
 * Near the foot of a flight the walking line has already started to rise:
 * half a going before the first riser, within the width. Returns the height
 * above the stair's floor there, or null when `p` is not in that strip.
 */
export function approachHeight(s: StairInfo, p: Pt): number | null {
  const { u, v } = stairLocal(s, p);
  if (u < -s.going / 2 || u >= 0 || Math.abs(v) > s.width / 2) return null;
  return rampHeight(s, u);
}

/** Level to count a walker on the flight at `u` as standing on: the lower one below half the rise. */
export function flightLevelId(s: StairInfo, u: number): string {
  return rampHeight(s, u) < s.rise / 2 || !s.topLevelId ? s.levelId : s.topLevelId;
}

function strip(s: StairInfo, u0: number, u1: number, v0: number, v1: number): Collider | null {
  if (!(u1 - u0 > 1)) return null;
  return polyCollider([stairPoint(s, u0, v0), stairPoint(s, u0, v1), stairPoint(s, u1, v1), stairPoint(s, u1, v0)], "stair", s.id);
}

/** `u` where the walking line is `h` above the stair's floor (the inverse of `rampHeight`), within the flight. */
export function uAtHeight(s: StairInfo, h: number): number {
  const step = s.rise / s.risers;
  return Math.min(Math.max((h / step - 0.5) * s.going, 0), s.run);
}

/** Where a side of a flight is closed: `u` from `from` to `to`. Side 0 is the left facing up, 1 the right. */
export function closedSide(s: StairInfo, open: StairOpen, side: 0 | 1): { from: number; to: number } {
  const from = open.low[side] ? uAtHeight(s, STAIR_OPEN_RISE_MM) : 0;
  const to = open.high[side] && s.topLevelId ? uAtHeight(s, s.rise - STAIR_OPEN_RISE_MM) : s.run;
  return { from, to: Math.max(from, to) };
}

/**
 * The wall strips of a flight. `lower`: seen from the level it stands on
 * (sides and head). `upper`: seen from the level at its top (sides and
 * foot). The sides leave the open treads of `open` free.
 */
export function stairColliders(s: StairInfo, part: "lower" | "upper", open: StairOpen = STAIR_CLOSED): Collider[] {
  const t = STAIR_STRIP_MM;
  const hw = s.width / 2;
  const left = closedSide(s, open, 0);
  const right = closedSide(s, open, 1);
  const out: (Collider | null)[] =
    part === "lower"
      ? [strip(s, left.from, s.run, -hw - t, -hw), strip(s, right.from, s.run, hw, hw + t), strip(s, s.run, s.run + t, -hw - t, hw + t)]
      : [strip(s, 0, left.to, -hw - t, -hw), strip(s, 0, right.to, hw, hw + t), strip(s, -t, 0, -hw - t, hw + t)];
  return out.filter((c): c is Collider => c !== null);
}

/**
 * Keeps a walker of radius `r` at `p` on a flight between its sides where
 * they are closed, and short of the head when no level is at the top. Toward
 * a closed stretch the allowed width narrows at 45 degrees, so walking in at
 * an angle slides inward smoothly instead of catching on a corner.
 */
export function holdOnFlight(s: StairInfo, p: Pt, r: number, open: StairOpen = STAIR_CLOSED): Pt {
  const { u, v } = stairLocal(s, p);
  const inner = Math.max(s.width / 2 - r, 0);
  const limit = (side: 0 | 1) => {
    const c = closedSide(s, open, side);
    // A side closed from the very foot still lets the walker in at the foot's full width.
    const from = c.from > 0 ? c.from : Math.min(r, c.to);
    return inner + Math.max(0, from - u, u - c.to);
  };
  let nv = v;
  if (v < -limit(0)) nv = -limit(0);
  if (v > limit(1)) nv = limit(1);
  let nu = u;
  if (!s.topLevelId && u > s.run - r) nu = s.run - r;
  if (nu === u && nv === v) return p;
  return stairPoint(s, nu, nv);
}

/** True when a blocker stands on the flight's footprint, more than `inset` inside its edges. */
export function onFootprint(s: StairInfo, c: Collider, inset = 30): boolean {
  const hw = s.width / 2 - inset;
  const u0 = inset;
  const u1 = s.run - inset;
  if (!(hw > 0 && u1 > u0)) return false;
  if (c.kind === "circle") {
    const { u, v } = stairLocal(s, c.c);
    const du = Math.max(u0 - u, 0, u - u1);
    const dv = Math.max(-hw - v, 0, v - hw);
    return Math.hypot(du, dv) < c.r;
  }
  const local = c.pts.map((q) => stairLocal(s, q));
  const inRect = (q: StairLocal) => q.u > u0 && q.u < u1 && Math.abs(q.v) < hw;
  if (local.some(inRect)) return true;
  const rect: StairLocal[] = [
    { u: u0, v: -hw },
    { u: u1, v: -hw },
    { u: u1, v: hw },
    { u: u0, v: hw },
  ];
  const inPoly = (q: StairLocal) => pointInLocal(q, local);
  if (rect.some(inPoly)) return true;
  for (let i = 0; i < local.length; i++) {
    const a = local[i];
    const b = local[(i + 1) % local.length];
    for (let k = 0; k < 4; k++) if (crosses(a, b, rect[k], rect[(k + 1) % 4])) return true;
  }
  return false;
}

function pointInLocal(p: StairLocal, poly: StairLocal[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const a = poly[i];
    const b = poly[j];
    if (a.v > p.v !== b.v > p.v && p.u < ((b.u - a.u) * (p.v - a.v)) / (b.v - a.v) + a.u) inside = !inside;
  }
  return inside;
}

function crosses(a: StairLocal, b: StairLocal, c: StairLocal, d: StairLocal): boolean {
  const o = (p: StairLocal, q: StairLocal, r: StairLocal) => (q.u - p.u) * (r.v - p.v) - (q.v - p.v) * (r.u - p.u);
  const d1 = o(c, d, a);
  const d2 = o(c, d, b);
  const d3 = o(a, b, c);
  const d4 = o(a, b, d);
  return d1 * d2 < 0 && d3 * d4 < 0;
}

/** Outline, tread lines and the up arrow of a flight, for the minimap. */
export function stairOutline(s: StairInfo): { outline: Pt[]; treads: [Pt, Pt][]; arrow: [Pt, Pt] } {
  const hw = s.width / 2;
  const treads: [Pt, Pt][] = [];
  for (let i = 1; i < s.risers; i++) treads.push([stairPoint(s, i * s.going, -hw), stairPoint(s, i * s.going, hw)]);
  return {
    outline: [stairPoint(s, 0, -hw), stairPoint(s, 0, hw), stairPoint(s, s.run, hw), stairPoint(s, s.run, -hw)],
    treads,
    arrow: [stairPoint(s, s.going * 0.5, 0), stairPoint(s, s.run - s.going * 0.5, 0)],
  };
}
