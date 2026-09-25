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
// solid: on the lower level both sides and the head are closed (walk in at
// the foot only), on the upper level both sides and the foot are closed (walk
// in at the head only, never into the stairwell). On the flight only the
// sides hold the walker, at the stair's width. A flight with no level at its
// top is closed at the head too: the walker stops at the top.

import type { DocState } from "../../contract/bindings";
import { levelResolver, polyCollider, type Collider } from "../geom/collision";
import type { Pt } from "../geom/coords";

/** A level counts as the top of a flight when its floor is this close to it. */
export const STAIR_TOP_MATCH_MM = 300;
/** Thickness of the wall strips around a flight. */
export const STAIR_STRIP_MM = 20;

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
  return polyCollider([stairPoint(s, u0, v0), stairPoint(s, u0, v1), stairPoint(s, u1, v1), stairPoint(s, u1, v0)], "stair", s.id);
}

/**
 * The wall strips of a flight. `lower`: seen from the level it stands on
 * (sides and head). `upper`: seen from the level at its top (sides and foot).
 * `flight`: for a walker on it (sides, and the head when no level is at the top).
 */
export function stairColliders(s: StairInfo, part: "lower" | "upper" | "flight"): Collider[] {
  const t = STAIR_STRIP_MM;
  const hw = s.width / 2;
  const out: (Collider | null)[] = [strip(s, 0, s.run, hw, hw + t), strip(s, 0, s.run, -hw - t, -hw)];
  if (part === "lower" || (part === "flight" && !s.topLevelId)) out.push(strip(s, s.run, s.run + t, -hw - t, hw + t));
  if (part === "upper") out.push(strip(s, -t, 0, -hw - t, hw + t));
  return out.filter((c): c is Collider => c !== null);
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
