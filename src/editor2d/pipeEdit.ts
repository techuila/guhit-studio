// Editing a drawn run (DECISIONS D39): which part a click picks, which points
// an edit moves, the command each edit sends, and the preview of a move with
// the joined runs that follow it. Pure: no DOM, no store. The plan canvas and
// the inspector share it. Tested in pipeEdit.test.ts.
//
// A pick is the whole run (null), one segment (point `index` to `index + 1`)
// or one point. A point stands for its plan node: the points a riser joins
// move together in plan, so a riser stays vertical, and a height edit moves
// the whole node. Geometry only: nothing here sizes a pipe (DECISIONS D19).

import type { Command, PipeEnd, PipeSystem, Vec3 } from "../contract/bindings";
import { pipeFalls } from "../contract/pipes";
import type { P } from "./geom";
import { dirDeg, dist, distToSegment, dot, mul, projectParam, sub, unit } from "./geom";
import type { PipeEl } from "./pipe";
import { canJoin, isVertical, pipeNodes, planDist, planOf, segmentFallPct } from "./pipe";
import type { PipePick } from "./pipePick";
import type { TypedState } from "./typed";
import { typedValues } from "./typed";

/** Heights closer than this read as the same height, mm. */
export const LEVEL_EPS_MM = 0.5;

/** The plan node (a point, or the points a riser joins) that holds point `index`. */
export function nodeOf(points: readonly Vec3[], index: number): { index: number; count: number } {
  for (const n of pipeNodes(points)) if (index >= n.index && index < n.index + n.count) return { index: n.index, count: n.count };
  return { index, count: 1 };
}

function range(from: number, count: number): number[] {
  return Array.from({ length: count }, (_, i) => from + i);
}

/** True when segment `i` is a riser (drawn as a circle in plan). */
export function segmentIsRiser(points: readonly Vec3[], i: number): boolean {
  return i >= 0 && i + 1 < points.length && isVertical(points[i], points[i + 1]);
}

/** 3D length of segment `i`. */
export function segmentLength(points: readonly Vec3[], i: number): number {
  const a = points[i];
  const b = points[i + 1];
  return a && b ? Math.hypot(b.x - a.x, b.y - a.y, b.z - a.z) : 0;
}

/** The segment under plan point `p`: the nearest one that is not a riser, within `tol` mm of its centerline. */
export function segmentAt(points: readonly Vec3[], p: P, tol: number): number | null {
  let best: number | null = null;
  let bestD = tol;
  for (let i = 0; i + 1 < points.length; i++) {
    if (segmentIsRiser(points, i)) continue;
    const d = distToSegment(p, planOf(points[i]), planOf(points[i + 1]));
    if (d <= bestD) {
      bestD = d;
      best = i;
    }
  }
  return best;
}

/** The point handle under `p` (within `tol` mm), as the point that starts its node. */
export function nodeAt(points: readonly Vec3[], p: P, tol: number): number | null {
  let best: number | null = null;
  let bestD = tol;
  for (const n of pipeNodes(points)) {
    const d = dist(n.point, p);
    if (d <= bestD) {
      bestD = d;
      best = n.index;
    }
  }
  return best;
}

/**
 * What a click on the selected run picks: a point handle within `handleTol`,
 * else the segment under the cursor within `tol`. Null when it hits neither
 * (a riser circle, say): the run stays picked as a whole.
 */
export function pickAt(pipe: Pick<PipeEl, "id" | "points">, p: P, handleTol: number, tol: number): PipePick | null {
  const node = nodeAt(pipe.points, p, handleTol);
  if (node !== null) return { pipeId: pipe.id, kind: "point", index: node };
  const seg = segmentAt(pipe.points, p, tol);
  return seg === null ? null : { pipeId: pipe.id, kind: "segment", index: seg };
}

/** A pick that still fits the run after an edit or an undo, else null. */
export function validPick(points: readonly Vec3[], pick: PipePick | null): PipePick | null {
  if (!pick) return null;
  if (pick.kind === "point") return pick.index >= 0 && pick.index < points.length ? { ...pick, index: nodeOf(points, pick.index).index } : null;
  return pick.index >= 0 && pick.index + 1 < points.length ? pick : null;
}

/** Points a plan move of the pick carries: whole nodes, so risers stay vertical. Empty is the whole run. */
export function planIndices(points: readonly Vec3[], pick: PipePick | null): number[] {
  if (!pick) return [];
  if (pick.kind === "point") {
    const n = nodeOf(points, pick.index);
    return range(n.index, n.count);
  }
  const a = nodeOf(points, pick.index);
  const b = nodeOf(points, pick.index + 1);
  return [...new Set([...range(a.index, a.count), ...range(b.index, b.count)])].sort((x, y) => x - y);
}

/**
 * Points a height change of the pick carries. A segment moves its own two
 * points, so the risers at its ends stretch; a point moves its whole node.
 * Empty is the whole run.
 */
export function heightIndices(points: readonly Vec3[], pick: PipePick | null): number[] {
  if (!pick) return [];
  if (pick.kind === "segment") return [pick.index, pick.index + 1];
  const n = nodeOf(points, pick.index);
  return range(n.index, n.count);
}

/** The height a typed `h` value sets: the first point of the pick (where a riser node is arrived at). */
export function refZ(points: readonly Vec3[], pick: PipePick | null): number {
  if (!pick || points.length === 0) return points[0]?.z ?? 0;
  if (pick.kind === "point") return points[nodeOf(points, pick.index).index]?.z ?? 0;
  return points[pick.index]?.z ?? 0;
}

/** True when every point of the run sits at one height. */
export function isLevelRun(points: readonly Vec3[]): boolean {
  return points.every((v) => Math.abs(v.z - points[0].z) < LEVEL_EPS_MM);
}

/** Plan length of the segments that are not risers, and the drop of the risers, from the first point to the last. */
export function runPlanAndRiserDrop(points: readonly Vec3[], from = 0, to = points.length - 2): { planMm: number; riserDropMm: number } {
  let planMm = 0;
  let riserDropMm = 0;
  for (let i = Math.max(0, from); i <= to && i + 1 < points.length; i++) {
    if (segmentIsRiser(points, i)) riserDropMm += points[i].z - points[i + 1].z;
    else planMm += planDist(points[i], points[i + 1]);
  }
  return { planMm, riserDropMm };
}

/**
 * Average fall of segments `from` to `to` in percent of their plan length,
 * risers left out: positive falls, negative rises. Null when they are all risers.
 */
export function rangeSlopePct(points: readonly Vec3[], from = 0, to = points.length - 2): number | null {
  if (from === to) return segmentFallPct(points[from], points[from + 1]);
  const { planMm, riserDropMm } = runPlanAndRiserDrop(points, from, to);
  if (planMm < 1) return null;
  const last = Math.min(to + 1, points.length - 1);
  return ((points[from].z - points[last].z - riserDropMm) / planMm) * 100;
}

/** Leading (start) or trailing (end) points that sit at that end's height: what a Start or End height edit moves. */
export function endStretch(points: readonly Vec3[], end: PipeEnd): number[] {
  const n = points.length;
  const out: number[] = [];
  if (end === "start") {
    for (let i = 0; i < n && Math.abs(points[i].z - points[0].z) < LEVEL_EPS_MM; i++) out.push(i);
  } else {
    for (let i = n - 1; i >= 0 && Math.abs(points[i].z - points[n - 1].z) < LEVEL_EPS_MM; i--) out.unshift(i);
  }
  return out;
}

// ---------------------------------------------------------------- commands

function round3(v: number): number {
  return Math.round(v * 1000) / 1000;
}

export function moveCommand(pipeId: string, indices: readonly number[], delta: Vec3, keepJoins = true): Command {
  return {
    type: "move_pipe_points",
    targets: [{ pipe_id: pipeId, indices: [...indices] }],
    delta: { x: round3(delta.x), y: round3(delta.y), z: round3(delta.z) },
    keep_joins: keepJoins,
  };
}

/** Moves the pick up or down by `dz` (PageUp, PageDown). Null for no change. */
export function heightStepCommand(pipe: Pick<PipeEl, "id" | "points">, pick: PipePick | null, dz: number): Command | null {
  if (Math.abs(dz) < 1e-6) return null;
  return moveCommand(pipe.id, heightIndices(pipe.points, pick), { x: 0, y: 0, z: dz });
}

/** Moves the pick up or down so its reference height reads `z` (h then a number). Null for no change. */
export function heightSetCommand(pipe: Pick<PipeEl, "id" | "points">, pick: PipePick | null, z: number): Command | null {
  if (!Number.isFinite(z)) return null;
  return heightStepCommand(pipe, pick, z - refZ(pipe.points, pick));
}

/** One point (`only`) or the node of point `index` moved to height `z`. */
export function pointHeightCommand(pipe: Pick<PipeEl, "id" | "points">, index: number, z: number, only = false): Command | null {
  const p = pipe.points[index];
  if (!p || !Number.isFinite(z) || Math.abs(z - p.z) < 1e-6) return null;
  const indices = only ? [index] : heightIndices(pipe.points, { pipeId: pipe.id, kind: "point", index });
  return moveCommand(pipe.id, indices, { x: 0, y: 0, z: z - p.z });
}

/** A point's plan position typed in: its whole node moves (`only` moves that point alone). */
export function pointPlanCommand(pipe: Pick<PipeEl, "id" | "points">, index: number, to: P, only = false): Command | null {
  const p = pipe.points[index];
  if (!p || !Number.isFinite(to.x) || !Number.isFinite(to.y)) return null;
  const d = sub(to, planOf(p));
  if (Math.hypot(d.x, d.y) < 1e-6) return null;
  const indices = only ? [index] : planIndices(pipe.points, { pipeId: pipe.id, kind: "point", index });
  return moveCommand(pipe.id, indices, { x: d.x, y: d.y, z: 0 });
}

// Lengths and slopes the engine refuses (a segment under 10 mm, a slope past
// 100 percent) are still sent: its refusal says why, in a toast.

export function lengthCommand(pipe: Pick<PipeEl, "id" | "points">, segment: number, lengthMm: number, hold: PipeEnd, keepJoins = true): Command | null {
  if (!Number.isFinite(lengthMm) || Math.abs(lengthMm - segmentLength(pipe.points, segment)) < 1e-6) return null;
  return { type: "set_pipe_segment_length", pipe_id: pipe.id, segment, length_mm: round3(lengthMm), hold, keep_joins: keepJoins };
}

export function slopeCommand(pipe: Pick<PipeEl, "id">, from: number, to: number, pct: number, hold: PipeEnd, keepJoins = true): Command | null {
  if (!Number.isFinite(pct)) return null;
  return { type: "set_pipe_slope", pipe_id: pipe.id, from_segment: from, to_segment: to, slope_pct: Math.round(pct * 1e4) / 1e4, hold, keep_joins: keepJoins };
}

/**
 * The run's Start or End height typed in. A run that falls keeps the other
 * end and takes the one fall that joins them (risers keep their drop), the
 * way Revit edits a sloped pipe's end. Any other run moves that end and the
 * points level with it, so a riser between them stretches.
 */
export function runEndHeightCommand(pipe: Pick<PipeEl, "id" | "points" | "system">, end: PipeEnd, z: number): Command | null {
  const pts = pipe.points;
  const n = pts.length;
  if (n < 2 || !Number.isFinite(z)) return null;
  const at = end === "start" ? pts[0].z : pts[n - 1].z;
  if (Math.abs(z - at) < 1e-6) return null;
  if (pipeFalls(pipe.system)) {
    const { planMm, riserDropMm } = runPlanAndRiserDrop(pts);
    if (planMm >= 1) {
      const startZ = end === "start" ? z : pts[0].z;
      const endZ = end === "end" ? z : pts[n - 1].z;
      const pct = ((startZ - endZ - riserDropMm) / planMm) * 100;
      return slopeCommand(pipe, 0, n - 2, pct, end === "start" ? "end" : "start");
    }
  }
  return moveCommand(pipe.id, endStretch(pts, end), { x: 0, y: 0, z: z - at });
}

/** A bend in the middle of segment `segment`. */
export function insertBendCommand(pipe: Pick<PipeEl, "id" | "points">, segment: number): Command | null {
  const a = pipe.points[segment];
  const b = pipe.points[segment + 1];
  if (!a || !b) return null;
  const at = { x: round3((a.x + b.x) / 2), y: round3((a.y + b.y) / 2), z: round3((a.z + b.z) / 2) };
  return { type: "insert_pipe_point", pipe_id: pipe.id, segment, at };
}

export function removePointCommand(pipe: Pick<PipeEl, "id" | "points">, index: number): Command | null {
  if (pipe.points.length <= 2 || index < 0 || index >= pipe.points.length) return null;
  return { type: "remove_pipe_point", pipe_id: pipe.id, index };
}

// ---------------------------------------------------------------- preview

/** The run with `indices` (empty: every point) moved by `delta`. */
export function movedPoints(points: readonly Vec3[], indices: readonly number[], delta: Vec3): Vec3[] {
  const all = indices.length === 0;
  const set = new Set(indices);
  return points.map((v, i) => (all || set.has(i) ? { x: v.x + delta.x, y: v.y + delta.y, z: v.z + delta.z } : v));
}

/** How close an end must be to rest on another run: the larger radius, at least 1 mm, as the engine joins them. */
function joinTol(a: { diameter_mm: number }, b: { diameter_mm: number }): number {
  return Math.max(a.diameter_mm / 2, b.diameter_mm / 2, MIN_REACH_MM);
}

function dist3(a: Vec3, b: Vec3): number {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
}

/** Where `p` lies along segment a-b in 3D (0 to 1), and how far from it. */
function onSegment3(p: Vec3, a: Vec3, b: Vec3): { t: number; d: number } {
  const ab = { x: b.x - a.x, y: b.y - a.y, z: b.z - a.z };
  const L2 = ab.x * ab.x + ab.y * ab.y + ab.z * ab.z;
  const t = L2 < 1e-12 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * ab.x + (p.y - a.y) * ab.y + (p.z - a.z) * ab.z) / L2));
  const q = { x: a.x + ab.x * t, y: a.y + ab.y * t, z: a.z + ab.z * t };
  return { t, d: dist3(p, q) };
}

type RunShape = Pick<PipeEl, "id" | "system" | "diameter_mm" | "points"> & { level_id?: string };

// The engine's join reach and point tolerance (crates/guhit-core/src/pipes.rs
// `MIN_REACH_MM`, validate.rs `PIPE_POINT_EPS_MM`) and its follow rounds.
const MIN_REACH_MM = 1;
const POINT_EPS_MM = 1;
const MAX_FOLLOW_ROUNDS = 64;
const NO_MOVE_MM = 1e-9;

/** A run as the follow rules see it: absolute heights. */
interface FollowRun {
  system: PipeSystem;
  radius: number;
  pts: Vec3[];
}

/** What one end rests on: a point of another run (it moves as that point), or a segment's body (a tee branch). */
type Rest = { kind: "point"; run: number; index: number } | { kind: "body"; run: number; seg: number };

const sub3 = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x - b.x, y: a.y - b.y, z: a.z - b.z });
const add3 = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x + b.x, y: a.y + b.y, z: a.z + b.z });
const mul3 = (a: Vec3, k: number): Vec3 => ({ x: a.x * k, y: a.y * k, z: a.z * k });
const dot3 = (a: Vec3, b: Vec3): number => a.x * b.x + a.y * b.y + a.z * b.z;
const lerp3 = (a: Vec3, b: Vec3, t: number): Vec3 => add3(a, mul3(sub3(b, a), t));
const len3 = (a: Vec3): number => Math.hypot(a.x, a.y, a.z);

/** Direction from an end into its run, unit length (zero when it has none). */
function away(r: FollowRun, last: boolean): Vec3 {
  const n = r.pts.length;
  const d = last ? sub3(r.pts[n - 2], r.pts[n - 1]) : sub3(r.pts[1], r.pts[0]);
  const l = len3(d);
  return Number.isFinite(l) && l > 1e-12 ? mul3(d, 1 / l) : { x: 0, y: 0, z: 0 };
}

/**
 * The support of every run end, `[first, last]` per run: a port of `rests`
 * in crates/guhit-core/src/pipe_edit.rs. An end on another joinable run's
 * body, away from its ends, is a tee branch on the nearest body (one landing
 * on a bend moves with the bend); two free ends in reach follow each other;
 * at three or more the two that line up best run through and the others
 * follow the main, which follows nothing there.
 */
function restsOf(runs: readonly FollowRun[]): [Rest | null, Rest | null][] {
  const out: [Rest | null, Rest | null][] = runs.map(() => [null, null]);
  const reach = (a: FollowRun, b: FollowRun): number => Math.max(a.radius, b.radius, MIN_REACH_MM);
  const endIndex = (r: FollowRun, last: boolean): number => (last ? r.pts.length - 1 : 0);
  const free: { run: number; last: boolean }[] = [];
  runs.forEach((q, qi) => {
    for (const last of [false, true]) {
      const end = q.pts[endIndex(q, last)];
      let best: { d: number; run: number; seg: number; t: number } | null = null;
      runs.forEach((r, ri) => {
        if (ri === qi || !canJoin(q.system, r.system)) return;
        const near = reach(q, r);
        if (dist3(end, r.pts[0]) <= near || dist3(end, r.pts[r.pts.length - 1]) <= near) return;
        let on: { d: number; seg: number; t: number } | null = null;
        for (let seg = 0; seg + 1 < r.pts.length; seg++) {
          const d = sub3(r.pts[seg + 1], r.pts[seg]);
          const l2 = dot3(d, d);
          const t = l2 > 0 ? Math.max(0, Math.min(1, dot3(sub3(end, r.pts[seg]), d) / l2)) : 0;
          const dd = dist3(end, lerp3(r.pts[seg], r.pts[seg + 1], t));
          if (!on || dd < on.d) on = { d: dd, seg, t };
        }
        if (on && on.d <= near && (!best || on.d < best.d)) best = { d: on.d, run: ri, seg: on.seg, t: on.t };
      });
      const slot = last ? 1 : 0;
      if (best) {
        const { run, seg, t } = best as { run: number; seg: number; t: number };
        const r = runs[run];
        const at = lerp3(r.pts[seg], r.pts[seg + 1], t);
        out[qi][slot] =
          dist3(at, r.pts[seg]) < POINT_EPS_MM
            ? { kind: "point", run, index: seg }
            : dist3(at, r.pts[seg + 1]) < POINT_EPS_MM
              ? { kind: "point", run, index: seg + 1 }
              : { kind: "body", run, seg };
      } else free.push({ run: qi, last });
    }
  });

  // Free ends within reach of each other form one joint.
  const n = free.length;
  const parent = Array.from({ length: n }, (_, i) => i);
  const root = (i: number): number => {
    let r = i;
    while (parent[r] !== r) r = parent[r];
    let c = i;
    while (parent[c] !== r) {
      const next = parent[c];
      parent[c] = r;
      c = next;
    }
    return r;
  };
  const endPt = (k: number): Vec3 => runs[free[k].run].pts[endIndex(runs[free[k].run], free[k].last)];
  for (let i = 0; i < n; i++) {
    for (let j = i + 1; j < n; j++) {
      const a = runs[free[i].run];
      const b = runs[free[j].run];
      if (free[i].run === free[j].run || !canJoin(a.system, b.system)) continue;
      if (dist3(endPt(i), endPt(j)) <= reach(a, b)) {
        const x = root(i);
        const y = root(j);
        if (x !== y) parent[Math.max(x, y)] = Math.min(x, y);
      }
    }
  }
  const groups: number[][] = [];
  const slotOf = new Map<number, number>();
  for (let i = 0; i < n; i++) {
    const r = root(i);
    const g = slotOf.get(r);
    if (g === undefined) {
      slotOf.set(r, groups.length);
      groups.push([i]);
    } else groups[g].push(i);
  }
  const point = (k: number): Rest => ({ kind: "point", run: free[k].run, index: endIndex(runs[free[k].run], free[k].last) });
  for (const g of groups) {
    if (g.length < 2) continue;
    if (g.length === 2) {
      const [a, b] = g;
      if (free[a].run !== free[b].run) {
        out[free[a].run][free[a].last ? 1 : 0] = point(b);
        out[free[b].run][free[b].last ? 1 : 0] = point(a);
      }
      continue;
    }
    // The pair of two runs that lines up best, as the fittings pick it.
    const dir = (k: number): Vec3 => away(runs[free[k].run], free[k].last);
    let pair: [number, number] | null = null;
    let straightest = Infinity;
    for (let x = 0; x < g.length; x++) {
      for (let y = x + 1; y < g.length; y++) {
        if (free[g[x]].run === free[g[y]].run) continue;
        const d = dot3(dir(g[x]), dir(g[y]));
        if (d < straightest - 1e-12) {
          straightest = d;
          pair = [g[x], g[y]];
        }
      }
    }
    if (!pair) continue;
    const [pa, pb] = pair;
    const ra = runs[free[pa].run].radius;
    const rb = runs[free[pb].run].radius;
    const [main, other] = rb > ra || (rb === ra && free[pb].last && !free[pa].last) ? [pb, pa] : [pa, pb];
    for (const k of g) {
      if (k === main) continue;
      // The other end of a loop that closes here follows the second run of the pair.
      const to = free[k].run === free[main].run ? other : main;
      out[free[k].run][free[k].last ? 1 : 0] = point(to);
    }
  }
  return out;
}

/** How an end at `p` moves with what it rests on, or null while that stays (the engine's `support`). */
function supportOf(runs: readonly FollowRun[], deltas: readonly (Vec3 | null)[][], rest: Rest, p: Vec3): Vec3 | null {
  if (rest.kind === "point") return deltas[rest.run][rest.index];
  const da = deltas[rest.run][rest.seg];
  const db = deltas[rest.run][rest.seg + 1];
  if (!da && !db) return null;
  const zero = { x: 0, y: 0, z: 0 };
  const a = runs[rest.run].pts[rest.seg];
  const b = runs[rest.run].pts[rest.seg + 1];
  const a2 = add3(a, da ?? zero);
  const b2 = add3(b, db ?? zero);
  const along = sub3(b, a);
  const l2 = dot3(along, along);
  if (!(Number.isFinite(l2) && l2 > 0)) return null;
  // Where the end sits along the segment and how far off it: both are kept.
  const t = Math.max(0, Math.min(1, dot3(sub3(p, a), along) / l2));
  const on = lerp3(a, b, t);
  const off = sub3(p, on);
  // The point of the moved segment with no move along the old direction.
  const u = mul3(along, 1 / Math.sqrt(l2));
  const k = dot3(sub3(b2, a2), u);
  const s = Math.abs(k) > 1e-9 ? dot3(sub3(on, a2), u) / k : t;
  return sub3(add3(lerp3(a2, b2, Math.max(0, Math.min(1, s))), off), p);
}

/**
 * The other runs that follow a move with `keep_joins`, as the engine moves
 * them (crates/guhit-core/src/pipe_edit.rs, DECISIONS D39): an elbow end
 * follows its joint exactly; a tee branch keeps its place along the run it
 * rests on and follows only the move across it; at three or more ends
 * branches follow the through run, never the other way round; and what
 * follows carries what rests on it in turn. `floorOf` gives each level's
 * floor height, so runs on other levels join with absolute heights. A
 * preview: the engine stays the authority.
 */
export function followers(
  moved: RunShape,
  indices: readonly number[],
  delta: Vec3,
  others: readonly RunShape[],
  floorOf: (levelId: string | undefined) => number = () => 0,
): RunShape[] {
  const shapes = [moved, ...others.filter((o) => o.id !== moved.id && o.points.length >= 2)];
  const runs: FollowRun[] = shapes.map((r) => {
    const floor = floorOf(r.level_id);
    return { system: r.system as PipeSystem, radius: r.diameter_mm / 2, pts: r.points.map((v) => ({ x: v.x, y: v.y, z: v.z + floor })) };
  });
  const deltas: (Vec3 | null)[][] = runs.map((r) => r.pts.map(() => null));
  const all = indices.length === 0;
  const set = new Set(indices);
  if (len3(delta) <= NO_MOVE_MM) return [];
  deltas[0] = runs[0].pts.map((_, i) => (all || set.has(i) ? delta : null));
  const rests = restsOf(runs);
  for (let round = 0; round < MAX_FOLLOW_ROUNDS; round++) {
    let changed = false;
    for (let qi = 1; qi < runs.length; qi++) {
      const q = runs[qi];
      for (const last of [false, true]) {
        const rest = rests[qi][last ? 1 : 0];
        if (!rest) continue;
        const index = last ? q.pts.length - 1 : 0;
        const d = supportOf(runs, deltas, rest, q.pts[index]);
        if (!d || len3(d) <= NO_MOVE_MM) continue;
        const old = deltas[qi][index];
        if (!old || dist3(old, d) > NO_MOVE_MM) {
          deltas[qi][index] = d;
          changed = true;
        }
      }
    }
    if (!changed) break;
  }
  const out: RunShape[] = [];
  for (let qi = 1; qi < runs.length; qi++) {
    if (!deltas[qi].some((d) => d)) continue;
    const o = shapes[qi];
    out.push({ ...o, points: o.points.map((v, i) => (deltas[qi][i] ? add3(v, deltas[qi][i] as Vec3) : v)) });
  }
  return out;
}

// ---------------------------------------------------------------- typed distance

/** A drag within this many degrees of a 45 degree step goes exactly along it when a distance is typed. */
export const TYPED_LOCK_DEG = 8;

/**
 * A distance typed while dragging (SketchUp's Measurements box): the typed
 * length along the drag so far (exactly along a 45 degree step when it is
 * close to one), or along a typed angle ("600<90"). Null until a length is
 * typed. Lengths in the display unit.
 */
export function typedDragDelta(drag: P, typed: TypedState, displayUnit: "mm" | "m", lock = true): P | null {
  const v = typedValues(typed, displayUnit);
  if (v.a === null) return null;
  let dir: P;
  if (!lock) dir = Math.hypot(drag.x, drag.y) < 1e-6 ? { x: 1, y: 0 } : unit(drag);
  else if (v.b !== null) dir = dirDeg(v.b);
  else if (Math.hypot(drag.x, drag.y) < 1e-6) dir = { x: 1, y: 0 };
  else {
    const deg = (Math.atan2(drag.y, drag.x) * 180) / Math.PI;
    const step = Math.round(deg / 45) * 45;
    dir = Math.abs(deg - step) <= TYPED_LOCK_DEG ? dirDeg(step) : unit(drag);
  }
  const d = mul(dir, v.a);
  return { x: round3(d.x), y: round3(d.y) };
}

/** Plan point `p` projected onto segment a-b, as the share along it (0 to 1). */
export function alongSegment(p: P, a: Vec3, b: Vec3): number {
  return Math.max(0, Math.min(1, projectParam(p, planOf(a), planOf(b))));
}

/** Plan delta of a drag from `from` to `to` on the axis that moved most, for ortho moves. */
export function orthoDelta(d: P): P {
  return Math.abs(d.x) >= Math.abs(d.y) ? { x: d.x, y: 0 } : { x: 0, y: d.y };
}

/** Plan delta that moves a segment sideways only: along its normal. */
export function normalOnly(d: P, a: Vec3, b: Vec3): P {
  const dir = unit(sub(planOf(b), planOf(a)));
  const n = { x: -dir.y, y: dir.x };
  return mul(n, dot(d, n));
}

// ---------------------------------------------------------------- handles

/** Plan middle of segment `i`, where its sideways handle sits. */
export function segmentMid(points: readonly Vec3[], i: number): P | null {
  const a = points[i];
  const b = points[i + 1];
  return a && b ? { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 } : null;
}

/** How far from an open end its "+" handle sits, CSS pixels. */
export const CONTINUE_HANDLE_PX = 22;

/** Plan direction a run leaves `end` by: along its last segment that is not a riser, else east. */
export function endDirection(points: readonly Vec3[], end: PipeEnd): P {
  const n = points.length;
  if (end === "end") {
    for (let i = n - 1; i > 0; i--) if (!isVertical(points[i - 1], points[i])) return unit(sub(planOf(points[i]), planOf(points[i - 1])));
  } else {
    for (let i = 0; i + 1 < n; i++) if (!isVertical(points[i], points[i + 1])) return unit(sub(planOf(points[i]), planOf(points[i + 1])));
  }
  return { x: 1, y: 0 };
}

/**
 * The ends of a run that join nothing: no joinable run's point or body in
 * reach (the engine's join rule) and no fixture point within `fixtureTolMm`
 * in plan. Those get a "+" handle that continues the run (Plant 3D's
 * continuation grip).
 */
export function openEnds(
  pipe: RunShape,
  others: readonly RunShape[],
  fixtures: readonly P[],
  fixtureTolMm = 60,
): PipeEnd[] {
  const pts = pipe.points;
  if (pts.length < 2) return [];
  const out: PipeEnd[] = [];
  for (const end of ["start", "end"] as const) {
    const p = end === "start" ? pts[0] : pts[pts.length - 1];
    let joined = fixtures.some((f) => planDist(f, p) <= fixtureTolMm);
    for (const o of others) {
      if (joined) break;
      if (o.id === pipe.id || !canJoin(pipe.system as PipeSystem, o.system as PipeSystem)) continue;
      if (pipe.level_id !== undefined && o.level_id !== undefined && o.level_id !== pipe.level_id) continue;
      const tol = joinTol(pipe, o);
      for (let i = 0; i < o.points.length && !joined; i++) {
        if (dist3(p, o.points[i]) <= tol) joined = true;
        else if (i + 1 < o.points.length && onSegment3(p, o.points[i], o.points[i + 1]).d <= tol) joined = true;
      }
    }
    if (!joined) out.push(end);
  }
  return out;
}

/**
 * Points of a run continued from one end by `drawn` (which starts at that
 * end, in drawing order): appended at the end, or reversed in front of the
 * start so the flow order holds.
 */
export function continuedPoints(points: readonly Vec3[], end: PipeEnd, drawn: readonly Vec3[]): Vec3[] {
  if (drawn.length < 2) return [...points];
  return end === "end" ? [...points, ...drawn.slice(1)] : [...[...drawn].reverse().slice(0, -1), ...points];
}
