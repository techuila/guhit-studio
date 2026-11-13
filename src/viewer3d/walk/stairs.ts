// Stairs in walk mode. Pure plan math in millimeters, no three.js, no DOM.
//
// Every stair comes from the engine's `Derived.stairs` (DECISIONS D54):
// straight, L or U, its flights, landings and walking line. A stair the
// engine has not answered for yet (a document still loading, a test model)
// is read from its fields as one straight flight: `origin` is the middle of
// the first riser, the flight climbs along the rotated +y and is `width_mm`
// wide across it, and its total rise is the height of its level.
//
// Walking, a stair is a ramp: the floor under the walker follows the walking
// line through the middle of every tread, flat across a landing, so the eye
// rises smoothly instead of stepping, and it never climbs past the level
// height. `u` is the distance along that line from the first riser (the line
// starts half a going before it), `v` the distance to its right; on an L or
// U the line turns on the landing and `u` follows it round.
//
// A stair joins its own level to the level whose floor sits at its top.
// Around it the walker meets thin strips of wall, so the rest of the stair is
// solid: on the lower level everything but the foot is closed, on the upper
// level everything but the head (never into the stairwell). On a straight
// flight the first and last treads, within `STAIR_OPEN_RISE_MM` of either
// floor, stay open at the sides where that floor is beside them: a walker can
// step on or off the flight there at an angle, or off the top sideways when
// the head meets a wall.
//
// On the stair the walker is held by its own sides, not by strips:
// `holdOnFlight` keeps it within the flights and landings where their sides
// are closed (on a straight flight narrowing over one walker radius where a
// closed side begins, a funnel with no corner to catch on). Walls and
// objects of either level that stand on the stair's footprint do not block
// a walker on it. A stair with no level at its top is closed at the head:
// the walker stops at the top.

import type { DocState, StairGeometry } from "../../contract/bindings";
import { levelResolver, polyCollider, type Collider } from "../geom/collision";
import type { Pt } from "../geom/coords";
import { pointInPolygon } from "../geom/polygon";

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

/** One straight flight of a stair, in its own frame. */
export interface FlightInfo {
  /** Middle of its first riser. */
  origin: Pt;
  /** Unit vector up the flight. */
  dir: Pt;
  /** Unit vector across it, to the right facing up. */
  across: Pt;
  width: number;
  /** Its treads' going, first riser to top riser. */
  run: number;
  treads: number;
}

/** A flat part of a stair: a landing, its plan outline and its top above the stair's floor. */
export interface LandingInfo {
  polygon: Pt[];
  z: number;
}

export interface StairInfo {
  id: string;
  /** The level the stair stands on. */
  levelId: string;
  /** The level at its top, or null when none matches: the stair ends at a wall. */
  topLevelId: string | null;
  /** Middle of the first riser. */
  origin: Pt;
  /** Unit vector up the first flight. */
  dir: Pt;
  /** Unit vector across the first flight, to the right when facing up it. */
  across: Pt;
  width: number;
  /** Along the walking line from the first riser to the top riser: the flight's run when straight. */
  run: number;
  risers: number;
  /** Treads of every flight: one fewer than the risers straight, two fewer on an L or U (DECISIONS D44, D54). At least one. */
  treads: number;
  /** Tread depth. */
  going: number;
  /** Floor of `levelId`. */
  bottomZ: number;
  /** Total rise: the height of `levelId`. */
  rise: number;
  /** In walking order; one for a straight stair. */
  flights: FlightInfo[];
  landings: LandingInfo[];
  /** The walking line in plan, from half a going before the first riser to half a going past the top, its length at each point and its height above the stair's floor. */
  path: Pt[];
  pathAt: number[];
  pathZ: number[];
  /** The hole it makes in the floor it arrives at; empty for none. */
  opening: Pt[];
}

/** Position relative to a stair: `u` along its walking line from the first riser, `v` to its right. */
export interface StairLocal {
  u: number;
  v: number;
}

/** True for an L or U: two flights and a landing. */
export function turning(s: StairInfo): boolean {
  return s.flights.length > 1;
}

function lengths(path: Pt[]): number[] {
  const at = [0];
  for (let i = 1; i < path.length; i++) at.push(at[i - 1] + Math.hypot(path[i].x - path[i - 1].x, path[i].y - path[i - 1].y));
  return at;
}

/** A straight flight's walking line, as the engine draws it: half a going either side of the treads. */
function straightPath(origin: Pt, dir: Pt, run: number, going: number, rise: number): { path: Pt[]; z: number[] } {
  const at = (u: number) => ({ x: origin.x + dir.x * u, y: origin.y + dir.y * u });
  return { path: [at(-going / 2), at(run + going / 2)], z: [0, rise] };
}

/** The engine's stair as walk mode reads it. */
function fromGeometry(g: StairGeometry, id: string, levelId: string, topLevelId: string | null, bottomZ: number, rise: number): StairInfo | null {
  const flights: FlightInfo[] = [];
  for (const f of g.flights) {
    if (f.outline.length !== 4) continue;
    const [b0, b1, , t0] = f.outline;
    const width = Math.hypot(b1.x - b0.x, b1.y - b0.y);
    if (!(width > 10)) continue;
    const across = { x: (b1.x - b0.x) / width, y: (b1.y - b0.y) / width };
    const run = (t0.x - b0.x) * f.up.x + (t0.y - b0.y) * f.up.y;
    flights.push({ origin: { x: (b0.x + b1.x) / 2, y: (b0.y + b1.y) / 2 }, dir: { x: f.up.x, y: f.up.y }, across, width, run, treads: Math.max(f.nosings.length, 1) });
  }
  const first = flights[0];
  if (!first || g.walk_line.length < 2 || !(rise > 10)) return null;
  const path = g.walk_line.map((p) => ({ x: p.x, y: p.y }));
  const pathAt = lengths(path);
  const going = g.tread_mm;
  const treads = flights.reduce((n, f) => n + f.treads, 0);
  return {
    id,
    levelId,
    topLevelId,
    origin: first.origin,
    dir: first.dir,
    across: first.across,
    width: first.width,
    run: flights.length === 1 ? first.run : pathAt[pathAt.length - 1] - going,
    risers: Math.max(Math.round(rise / g.riser_mm), 1),
    treads,
    going,
    bottomZ,
    rise,
    flights,
    landings: g.landings.map((l) => ({ polygon: l.polygon.map((p) => ({ x: p.x, y: p.y })), z: l.z })),
    path,
    pathAt,
    pathZ: g.walk_line.map((p) => p.z),
    opening: g.opening.map((p) => ({ x: p.x, y: p.y })),
  };
}

/** Every stair on a shown stairs layer, with the level at its top. */
export function stairsOf(doc: DocState | null): StairInfo[] {
  if (!doc) return [];
  const project = doc.project;
  if (project.layers?.find((l) => l.key === "stairs")?.visible === false) return [];
  const levelOf = levelResolver(doc);
  const levels = project.levels ?? [];
  const derived = new Map((doc.derived?.stairs ?? []).map((g) => [g.stair_id, g]));
  const out: StairInfo[] = [];
  for (const e of project.elements) {
    if (e.kind !== "stair") continue;
    const level = levelOf(e.level_id);
    if (!level) continue;
    const rise = level.height_mm;
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
    const g = derived.get(e.id);
    const fromEngine = g ? fromGeometry(g, e.id, level.id, topLevelId, level.elevation_mm, rise) : null;
    if (fromEngine) {
      out.push(fromEngine);
      continue;
    }
    // Same limits as the 3D form (scene/stairs.ts, buildStair).
    const risers = Math.min(Math.max(Math.round(e.riser_count), 1), 200);
    const run = e.run_mm;
    const width = e.width_mm;
    if (!(run > 10 && width > 10 && rise > 10)) continue;
    const a = (e.rotation_deg * Math.PI) / 180;
    const origin = { x: e.origin.x, y: e.origin.y };
    const dir = { x: -Math.sin(a), y: Math.cos(a) };
    const across = { x: Math.cos(a), y: Math.sin(a) };
    const treads = Math.max(risers - 1, 1);
    const going = run / treads;
    const line = straightPath(origin, dir, run, going, rise);
    out.push({
      id: e.id,
      levelId: level.id,
      topLevelId,
      origin,
      dir,
      across,
      width,
      run,
      risers,
      treads,
      going,
      bottomZ: level.elevation_mm,
      rise,
      flights: [{ origin, dir, across, width, run, treads }],
      landings: [],
      path: line.path,
      pathAt: lengths(line.path),
      pathZ: line.z,
      opening: [],
    });
  }
  return out;
}

/** The nearest point of the walking line: its length there and the signed distance to its right. */
function onPath(s: StairInfo, p: Pt): { at: number; v: number } {
  let best = { at: 0, v: 0, d: Infinity };
  for (let i = 0; i + 1 < s.path.length; i++) {
    const a = s.path[i];
    const b = s.path[i + 1];
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const len = Math.hypot(dx, dy);
    if (len < 1e-9) continue;
    const t = Math.min(Math.max(((p.x - a.x) * dx + (p.y - a.y) * dy) / (len * len), 0), 1);
    const q = { x: a.x + dx * t, y: a.y + dy * t };
    const d = Math.hypot(p.x - q.x, p.y - q.y);
    if (d < best.d - 1e-9) {
      // Right of a-b is positive: the cross of the way along and the offset, negated.
      const side = -(dx * (p.y - q.y) - dy * (p.x - q.x)) / len;
      best = { at: s.pathAt[i] + len * t, v: side, d };
    }
  }
  return { at: best.at, v: best.v };
}

export function stairLocal(s: StairInfo, p: Pt): StairLocal {
  if (turning(s)) {
    const { at, v } = onPath(s, p);
    return { u: at - s.going / 2, v };
  }
  const dx = p.x - s.origin.x;
  const dy = p.y - s.origin.y;
  return { u: dx * s.dir.x + dy * s.dir.y, v: dx * s.across.x + dy * s.across.y };
}

export function stairPoint(s: StairInfo, u: number, v: number): Pt {
  if (turning(s)) {
    const at = Math.min(Math.max(u + s.going / 2, 0), s.pathAt[s.pathAt.length - 1]);
    for (let i = 0; i + 1 < s.path.length; i++) {
      if (at > s.pathAt[i + 1] + 1e-9 && i + 2 < s.path.length) continue;
      const a = s.path[i];
      const b = s.path[i + 1];
      const len = s.pathAt[i + 1] - s.pathAt[i] || 1;
      const t = (at - s.pathAt[i]) / len;
      const d = { x: (b.x - a.x) / len, y: (b.y - a.y) / len };
      return { x: a.x + (b.x - a.x) * t + d.y * v, y: a.y + (b.y - a.y) * t - d.x * v };
    }
  }
  return { x: s.origin.x + s.dir.x * u + s.across.x * v, y: s.origin.y + s.dir.y * u + s.across.y * v };
}

/**
 * Height of the walking line above the stair's floor at `u`: through the
 * middle of every tread, from 0 half a going before the first riser up to
 * the level height half a going past the head (the top riser lands on the
 * floor above, DECISIONS D44: the walker steps up that last half riser as it
 * leaves the flight), flat across a landing and beyond both ends.
 */
export function rampHeight(s: StairInfo, u: number): number {
  if (turning(s)) {
    const at = u + s.going / 2;
    const n = s.pathAt.length;
    if (at <= 0) return s.pathZ[0];
    for (let i = 0; i + 1 < n; i++) {
      if (at <= s.pathAt[i + 1]) {
        const len = s.pathAt[i + 1] - s.pathAt[i];
        const t = len > 0 ? (at - s.pathAt[i]) / len : 1;
        return Math.min(Math.max(s.pathZ[i] + (s.pathZ[i + 1] - s.pathZ[i]) * t, 0), s.rise);
      }
    }
    return Math.min(s.pathZ[n - 1], s.rise);
  }
  const step = s.rise / s.risers;
  return Math.min(Math.max(step * (u / s.going + 0.5), 0), s.rise);
}

/** A flight's pieces and landings: what a stair stands on in plan. */
export function stairPieces(s: StairInfo): Pt[][] {
  return [...s.flights.map((f) => flightOutline(f)), ...s.landings.map((l) => l.polygon)];
}

function flightOutline(f: FlightInfo): Pt[] {
  const at = (u: number, v: number) => ({ x: f.origin.x + f.dir.x * u + f.across.x * v, y: f.origin.y + f.dir.y * u + f.across.y * v });
  const hw = f.width / 2;
  return [at(0, -hw), at(0, hw), at(f.run, hw), at(f.run, -hw)];
}

/** True when `p` is within `margin` of a convex polygon (or inside it). */
function nearPolygon(p: Pt, poly: Pt[], margin: number): boolean {
  if (pointInPolygon(p, poly)) return true;
  if (margin <= 0) return false;
  for (let i = 0; i < poly.length; i++) {
    const a = poly[i];
    const b = poly[(i + 1) % poly.length];
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const l2 = dx * dx + dy * dy;
    const t = l2 > 0 ? Math.min(Math.max(((p.x - a.x) * dx + (p.y - a.y) * dy) / l2, 0), 1) : 0;
    if (Math.hypot(a.x + dx * t - p.x, a.y + dy * t - p.y) <= margin) return true;
  }
  return false;
}

/** True when `p` is on the stair: between the first riser and the head, within the width (on an L or U, on a flight or the landing). */
export function onFlight(s: StairInfo, p: Pt): boolean {
  if (turning(s)) return stairPieces(s).some((poly) => pointInPolygon(p, poly));
  const { u, v } = stairLocal(s, p);
  return u >= 0 && u <= s.run && Math.abs(v) <= s.width / 2;
}

/** True when a walker at `p` is still on the stair: `onFlight` with a millimeter of slack at the sides. */
export function staysOnStair(s: StairInfo, p: Pt): boolean {
  if (turning(s)) {
    const { u } = stairLocal(s, p);
    return u >= 0 && u <= s.run && stairPieces(s).some((poly) => nearPolygon(p, poly, 1));
  }
  const { u, v } = stairLocal(s, p);
  return u >= 0 && u <= s.run && Math.abs(v) <= s.width / 2 + 1;
}

/** True when `p` is on the stair, its approach or its head, `margin` round it. */
export function nearStair(s: StairInfo, p: Pt, margin: number): boolean {
  if (turning(s)) {
    const f = s.flights[0];
    const local = { u: (p.x - f.origin.x) * f.dir.x + (p.y - f.origin.y) * f.dir.y, v: (p.x - f.origin.x) * f.across.x + (p.y - f.origin.y) * f.across.y };
    const approach = local.u > -s.going - margin && local.u < 0 && Math.abs(local.v) < f.width / 2 + margin;
    return approach || stairPieces(s).some((poly) => nearPolygon(p, poly, margin + s.going));
  }
  const { u, v } = stairLocal(s, p);
  return u > -s.going - margin && u < s.run + margin && Math.abs(v) < s.width / 2 + margin;
}

/**
 * Near the foot of a flight the walking line has already started to rise:
 * half a going before the first riser, within the width. Returns the height
 * above the stair's floor there, or null when `p` is not in that strip.
 */
export function approachHeight(s: StairInfo, p: Pt): number | null {
  const f = s.flights[0];
  const dx = p.x - f.origin.x;
  const dy = p.y - f.origin.y;
  const u = dx * f.dir.x + dy * f.dir.y;
  const v = dx * f.across.x + dy * f.across.y;
  if (u < -s.going / 2 || u >= 0 || Math.abs(v) > f.width / 2) return null;
  return rampHeight(s, u);
}

/** Level to count a walker on the stair at `u` as standing on: the lower one below half the rise. */
export function flightLevelId(s: StairInfo, u: number): string {
  return rampHeight(s, u) < s.rise / 2 || !s.topLevelId ? s.levelId : s.topLevelId;
}

function strip(s: StairInfo, u0: number, u1: number, v0: number, v1: number): Collider | null {
  if (!(u1 - u0 > 1)) return null;
  return polyCollider([stairPoint(s, u0, v0), stairPoint(s, u0, v1), stairPoint(s, u1, v1), stairPoint(s, u1, v0)], "stair", s.id);
}

/** `u` where the walking line is `h` above the stair's floor (the inverse of `rampHeight`), within the stair. */
export function uAtHeight(s: StairInfo, h: number): number {
  if (turning(s)) {
    for (let i = 0; i + 1 < s.pathZ.length; i++) {
      const z0 = s.pathZ[i];
      const z1 = s.pathZ[i + 1];
      if (h <= z1 && z1 > z0) {
        const t = Math.min(Math.max((h - z0) / (z1 - z0), 0), 1);
        return Math.min(Math.max(s.pathAt[i] + (s.pathAt[i + 1] - s.pathAt[i]) * t - s.going / 2, 0), s.run);
      }
    }
    return s.run;
  }
  const step = s.rise / s.risers;
  return Math.min(Math.max((h / step - 0.5) * s.going, 0), s.run);
}

/** Where a side of a flight is closed: `u` from `from` to `to`. Side 0 is the left facing up, 1 the right. */
export function closedSide(s: StairInfo, open: StairOpen, side: 0 | 1): { from: number; to: number } {
  const from = open.low[side] ? uAtHeight(s, STAIR_OPEN_RISE_MM) : 0;
  const to = open.high[side] && s.topLevelId ? uAtHeight(s, s.rise - STAIR_OPEN_RISE_MM) : s.run;
  return { from, to: Math.max(from, to) };
}

/** A thin strip just outside edge a-b of a convex counter-clockwise piece. */
function edgeStrip(a: Pt, b: Pt, id: string): Collider | null {
  const len = Math.hypot(b.x - a.x, b.y - a.y);
  if (len < 1) return null;
  const n = { x: ((b.y - a.y) / len) * STAIR_STRIP_MM, y: (-(b.x - a.x) / len) * STAIR_STRIP_MM };
  return polyCollider([a, b, { x: b.x + n.x, y: b.y + n.y }, { x: a.x + n.x, y: a.y + n.y }], "stair", id);
}

/** Counter-clockwise copy of a polygon. */
function ccw(poly: Pt[]): Pt[] {
  let s = 0;
  for (let i = 0; i < poly.length; i++) {
    const a = poly[i];
    const b = poly[(i + 1) % poly.length];
    s += a.x * b.y - b.x * a.y;
  }
  return s < 0 ? [...poly].reverse() : poly;
}

/** True when segment a-b lies along segment c-d (within a millimeter). */
function along(a: Pt, b: Pt, c: Pt, d: Pt): boolean {
  const on = (p: Pt) => {
    const dx = d.x - c.x;
    const dy = d.y - c.y;
    const l2 = dx * dx + dy * dy;
    const t = l2 > 0 ? ((p.x - c.x) * dx + (p.y - c.y) * dy) / l2 : 0;
    return t >= -1e-6 && t <= 1 + 1e-6 && Math.hypot(c.x + dx * t - p.x, c.y + dy * t - p.y) < 1;
  };
  return on(a) && on(b);
}

/**
 * The wall strips of a stair. `lower`: seen from the level it stands on
 * (everything but the foot). `upper`: seen from the level at its top
 * (everything but the head). On a straight flight the sides leave the open
 * treads of `open` free.
 */
export function stairColliders(s: StairInfo, part: "lower" | "upper", open: StairOpen = STAIR_CLOSED): Collider[] {
  if (turning(s)) {
    const first = s.flights[0];
    const last = s.flights[s.flights.length - 1];
    const foot = flightOutline(first).slice(0, 2);
    const fo = flightOutline(last);
    const head = [fo[3], fo[2]];
    const out: Collider[] = [];
    for (const piece of stairPieces(s).map(ccw)) {
      for (let i = 0; i < piece.length; i++) {
        const a = piece[i];
        const b = piece[(i + 1) % piece.length];
        if (part === "lower" && along(a, b, foot[0], foot[1])) continue;
        if (part === "upper" && along(a, b, head[0], head[1])) continue;
        const c = edgeStrip(a, b, s.id);
        if (c) out.push(c);
      }
    }
    return out;
  }
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

/** A piece of a turning stair as a box in a flight's frame, with which of its sides hold the walker. */
interface HeldBox {
  f: FlightInfo;
  u0: number;
  u1: number;
  v0: number;
  v1: number;
  /** Closed: bottom (u0), right (v1), top (u1), left (v0). */
  closed: [boolean, boolean, boolean, boolean];
}

function heldBoxes(s: StairInfo): HeldBox[] {
  const first = s.flights[0];
  const pieces: { f: FlightInfo; u0: number; u1: number; v0: number; v1: number }[] = s.flights.map((f) => ({ f, u0: 0, u1: f.run, v0: -f.width / 2, v1: f.width / 2 }));
  for (const l of s.landings) {
    const local = l.polygon.map((p) => ({ u: (p.x - first.origin.x) * first.dir.x + (p.y - first.origin.y) * first.dir.y, v: (p.x - first.origin.x) * first.across.x + (p.y - first.origin.y) * first.across.y }));
    pieces.push({ f: first, u0: Math.min(...local.map((q) => q.u)), u1: Math.max(...local.map((q) => q.u)), v0: Math.min(...local.map((q) => q.v)), v1: Math.max(...local.map((q) => q.v)) });
  }
  const at = (f: FlightInfo, u: number, v: number) => ({ x: f.origin.x + f.dir.x * u + f.across.x * v, y: f.origin.y + f.dir.y * u + f.across.y * v });
  const others = (k: number) => pieces.filter((_, j) => j !== k).map((q) => [at(q.f, q.u0, q.v0), at(q.f, q.u0, q.v1), at(q.f, q.u1, q.v1), at(q.f, q.u1, q.v0)]);
  const lastIndex = s.flights.length - 1;
  return pieces.map((q, k) => {
    // A side is open where another piece carries on past any of it (a U's
    // landing meets each flight over part of its edge), at the foot of the
    // first flight and the head of the last (when it arrives somewhere).
    const polys = others(k);
    const probe = (u: number, v: number) => polys.some((poly) => pointInPolygon(at(q.f, u, v), poly));
    const alongU = (v: number) => [0.1, 0.5, 0.9].some((t) => probe(q.u0 + (q.u1 - q.u0) * t, v));
    const alongV = (u: number) => [0.1, 0.5, 0.9].some((t) => probe(u, q.v0 + (q.v1 - q.v0) * t));
    const closed: [boolean, boolean, boolean, boolean] = [
      !(k === 0) && !alongV(q.u0 - 2),
      !alongU(q.v1 + 2),
      !(k === lastIndex && s.topLevelId) && !alongV(q.u1 + 2),
      !alongU(q.v0 - 2),
    ];
    return { ...q, closed };
  });
}

/**
 * Keeps a walker of radius `r` at `p` on a stair between its sides where
 * they are closed, and short of the head when no level is at the top. On a
 * straight flight the allowed width narrows at 45 degrees toward a closed
 * stretch, so walking in at an angle slides inward smoothly instead of
 * catching on a corner. On an L or U the walker stays within the flights and
 * the landing, a radius in from their closed sides.
 */
export function holdOnFlight(s: StairInfo, p: Pt, r: number, open: StairOpen = STAIR_CLOSED): Pt {
  if (turning(s)) {
    let best: Pt | null = null;
    let bestD = Infinity;
    const boxes = heldBoxes(s);
    const last = s.flights.length - 1;
    for (const [k, b] of boxes.entries()) {
      // Off the foot and off the head (when it arrives somewhere) the walker
      // leaves the stair: nothing holds it back there.
      const u0 = k === 0 ? -Infinity : b.u0 + (b.closed[0] ? r : 0);
      const u1 = k === last && s.topLevelId ? Infinity : b.u1 - (b.closed[2] ? r : 0);
      const v0 = b.v0 + (b.closed[3] ? r : 0);
      const v1 = b.v1 - (b.closed[1] ? r : 0);
      if (!(u1 > u0 && v1 > v0)) continue;
      const u = (p.x - b.f.origin.x) * b.f.dir.x + (p.y - b.f.origin.y) * b.f.dir.y;
      const v = (p.x - b.f.origin.x) * b.f.across.x + (p.y - b.f.origin.y) * b.f.across.y;
      const cu = Math.min(Math.max(u, u0), u1);
      const cv = Math.min(Math.max(v, v0), v1);
      if (cu === u && cv === v) return p;
      const d = Math.hypot(cu - u, cv - v);
      if (d < bestD) {
        bestD = d;
        best = { x: b.f.origin.x + b.f.dir.x * cu + b.f.across.x * cv, y: b.f.origin.y + b.f.dir.y * cu + b.f.across.y * cv };
      }
    }
    return best ?? p;
  }
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

/** True when a blocker stands on a box `u0..u1`, `-hw..hw` of a flight's frame. */
function onBox(f: FlightInfo, u0: number, u1: number, v0: number, v1: number, c: Collider): boolean {
  if (!(v1 > v0 && u1 > u0)) return false;
  const local = (q: Pt): StairLocal => ({ u: (q.x - f.origin.x) * f.dir.x + (q.y - f.origin.y) * f.dir.y, v: (q.x - f.origin.x) * f.across.x + (q.y - f.origin.y) * f.across.y });
  if (c.kind === "circle") {
    const { u, v } = local(c.c);
    const du = Math.max(u0 - u, 0, u - u1);
    const dv = Math.max(v0 - v, 0, v - v1);
    return Math.hypot(du, dv) < c.r;
  }
  const pts = c.pts.map(local);
  const inRect = (q: StairLocal) => q.u > u0 && q.u < u1 && q.v > v0 && q.v < v1;
  if (pts.some(inRect)) return true;
  const rect: StairLocal[] = [
    { u: u0, v: v0 },
    { u: u1, v: v0 },
    { u: u1, v: v1 },
    { u: u0, v: v1 },
  ];
  if (rect.some((q) => pointInLocal(q, pts))) return true;
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i];
    const b = pts[(i + 1) % pts.length];
    for (let k = 0; k < 4; k++) if (crosses(a, b, rect[k], rect[(k + 1) % 4])) return true;
  }
  return false;
}

/** True when a blocker stands on the stair's footprint, more than `inset` inside its edges. */
export function onFootprint(s: StairInfo, c: Collider, inset = 30): boolean {
  if (turning(s)) {
    return heldBoxes(s).some((b) => onBox(b.f, b.u0 + inset, b.u1 - inset, b.v0 + inset, b.v1 - inset, c));
  }
  const hw = s.width / 2 - inset;
  return onBox(s.flights[0], inset, s.run - inset, -hw, hw, c);
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

/**
 * Outline, tread lines and the up arrow of a stair, for the minimap. On an L
 * or U `pieces` has every flight and landing and `path` the walking line
 * round the turn; `outline` is the first flight.
 */
export function stairOutline(s: StairInfo): { outline: Pt[]; treads: [Pt, Pt][]; arrow: [Pt, Pt]; pieces?: Pt[][]; path?: Pt[] } {
  const treads: [Pt, Pt][] = [];
  for (const f of s.flights) {
    const going = f.run / Math.max(f.treads, 1);
    const hw = f.width / 2;
    const at = (u: number, v: number) => ({ x: f.origin.x + f.dir.x * u + f.across.x * v, y: f.origin.y + f.dir.y * u + f.across.y * v });
    for (let i = 1; i < f.treads; i++) treads.push([at(i * going, -hw), at(i * going, hw)]);
  }
  if (turning(s)) {
    const path = s.path.slice(1, -1);
    return {
      outline: flightOutline(s.flights[0]),
      treads,
      arrow: [path[0] ?? s.path[0], path[path.length - 1] ?? s.path[s.path.length - 1]],
      pieces: stairPieces(s),
      path,
    };
  }
  return {
    outline: flightOutline(s.flights[0]),
    treads,
    arrow: [stairPoint(s, s.going * 0.5, 0), stairPoint(s, s.run - s.going * 0.5, 0)],
  };
}

/**
 * The holes stairs cut in the floors they arrive at, by level, as convex
 * pieces (a straight flight's rectangle; an L or U's flights, landing and the
 * well between a U's flights): what the 3D view and the walker take out of
 * those floors.
 */
export function stairWells(stairs: StairInfo[]): Map<string, Pt[][]> {
  const wells = new Map<string, Pt[][]>();
  for (const s of stairs) {
    if (!s.topLevelId) continue;
    const list = wells.get(s.topLevelId) ?? [];
    list.push(...stairPieces(s));
    if (turning(s) && s.flights.length === 2 && s.flights[0].dir.x * s.flights[1].dir.x + s.flights[0].dir.y * s.flights[1].dir.y < -0.5) {
      // A U's well: between the flights as far as both run.
      const [one, two] = s.flights;
      const x2 = (two.origin.x - one.origin.x) * one.across.x + (two.origin.y - one.origin.y) * one.across.y;
      const side = Math.sign(x2);
      const hw = one.width / 2;
      const low = Math.max(one.run - two.run, 0);
      if (Math.abs(x2 - 2 * side * hw) > 1) {
        const at = (u: number, v: number) => ({ x: one.origin.x + one.dir.x * u + one.across.x * v, y: one.origin.y + one.dir.y * u + one.across.y * v });
        list.push(ccw([at(low, side * hw), at(low, x2 - side * hw), at(one.run, x2 - side * hw), at(one.run, side * hw)]));
      }
    }
    wells.set(s.topLevelId, list);
  }
  return wells;
}
