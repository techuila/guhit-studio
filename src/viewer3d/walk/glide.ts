// Where a glide lands. Pure plan math in millimeters, no three.js, no DOM.
//
// A glide carries the walker to a picked spot, through anything, and lands it
// at eye height on the floor there (engine/walker.ts, `startGlide`). The spot
// comes from a minimap click (a plan point on the walker's level) or from a
// double click in the 3D view (what the ray hit). A floor keeps the eye
// height and the walk mode: Enscape dropped users onto the floor on a double
// click, and they asked for it to stop. The landing point is always stepped
// out of walls and furniture before the glide starts, so it never ends in one.

import { dur } from "../../ui/motion";
import { pushOut, WALKER_RADIUS_MM } from "../geom/collision";
import type { Pt } from "../geom/coords";
import type { GlideTarget } from "../engine/walker";
import { flightLevelId, onFlight, rampHeight, stairLocal } from "./stairs";
import type { WalkWorlds } from "./worlds";

/** A surface whose normal points up more than this counts as a floor. */
export const FLOOR_NORMAL_UP = 0.7;
/** A double click on a wall or an object glides to this far in front of it. */
export const FACE_STANDOFF_MM = WALKER_RADIUS_MM + 150;
/** A floor hit this close to a flight's walking line lands on the flight. */
const FLIGHT_HIT_MM = 400;

/** What a double click in the 3D view hit: plan point, height (mm above the project zero) and whether it faces up. */
export interface GlideHit {
  x: number;
  y: number;
  z: number;
  /** The world normal's up component, -1 to 1. */
  up: number;
}

/**
 * Glide length. `--dur-scene` for a short hop, up to twice that across a big
 * house, so a far spot does not rush past. 0 under reduced motion.
 */
export function glideDurationMs(distanceMm: number): number {
  const k = Math.min(Math.max(1 + distanceMm / 8000, 1), 2);
  return dur("scene") * k;
}

/**
 * A spot on a level: stepped out of anything there, onto a flight when it is
 * over one that starts or ends on this level. On a flight the walker counts as
 * on its lower level below half the rise and on its upper level above.
 */
export function levelTarget(worlds: WalkWorlds, levelId: string, p: Pt, r = WALKER_RADIUS_MM): GlideTarget {
  const s = worlds.stairAt(levelId, p);
  if (s) {
    const high = rampHeight(s, stairLocal(s, p).u) >= s.rise / 2;
    const q = pushOut(p, r, worlds.flightWorld(s, high));
    if (onFlight(s, q)) {
      const { u } = stairLocal(s, q);
      return { x: q.x, y: q.y, levelId: flightLevelId(s, u), stair: s, floorZ: s.bottomZ + rampHeight(s, u) };
    }
  }
  const q = pushOut(p, r, worlds.levelWorld(levelId));
  const on = worlds.stairAt(levelId, q);
  if (on) {
    const { u } = stairLocal(on, q);
    return { x: q.x, y: q.y, levelId: flightLevelId(on, u), stair: on, floorZ: on.bottomZ + rampHeight(on, u) };
  }
  return { x: q.x, y: q.y, levelId, stair: null, floorZ: worlds.elevation(levelId) + worlds.approach(levelId, q) };
}

/**
 * A double click in the 3D view. A floor (a room, a slab, a tread) is where
 * to go: its level is the one whose floor is at that height, a tread lands on
 * its flight. A wall or an object is where to go up to: the spot in front of
 * it, toward the walker, on the walker's level. Null when nothing works.
 */
export function hitTarget(worlds: WalkWorlds, hit: GlideHit, from: { x: number; y: number; levelId: string | null }, r = WALKER_RADIUS_MM): GlideTarget | null {
  const p = { x: hit.x, y: hit.y };
  if (hit.up >= FLOOR_NORMAL_UP) {
    for (const s of worlds.stairs) {
      if (!onFlight(s, p)) continue;
      const { u } = stairLocal(s, p);
      if (Math.abs(hit.z - (s.bottomZ + rampHeight(s, u))) > FLIGHT_HIT_MM) continue;
      return levelTarget(worlds, u < s.run / 2 || !s.topLevelId ? s.levelId : s.topLevelId, p, r);
    }
    const levelId = worlds.levelAtHeight(hit.z);
    return levelId ? levelTarget(worlds, levelId, p, r) : null;
  }
  const levelId = worlds.levelId(from.levelId);
  if (!levelId) return null;
  const dx = from.x - hit.x;
  const dy = from.y - hit.y;
  const d = Math.hypot(dx, dy);
  // Right under the walker (a click straight down on an object): stay.
  if (d < 1) return levelTarget(worlds, levelId, { x: from.x, y: from.y }, r);
  const back = Math.min(FACE_STANDOFF_MM, d);
  return levelTarget(worlds, levelId, { x: hit.x + (dx / d) * back, y: hit.y + (dy / d) * back }, r);
}
