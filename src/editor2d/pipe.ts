// Pipe tool math: tool defaults, drainage fall, risers, plan nodes, snapping
// to pipes and plumbing fixtures, node edits and hit testing. Pure: no DOM,
// no store. Tested in pipe.test.ts. Contract: docs/CONTRACT.md, "Pipes".
//
// Guhit coordinates pipes. Nothing here sizes a pipe or checks a code: the
// fall is the review default the engine also uses (drain_min_slope_pct).

import type { Asset, DeviceKind, DisplayUnit, Element, PipeMaterial, PipeSystem, Vec3 } from "../contract/bindings";
import { PIPE_DEFAULTS, PIPE_GROUP, PIPE_SYSTEM_LABEL, drainMinSlopePct, pipeFalls } from "../contract/pipes";
import type { P } from "./geom";
import { add, closestOnSegment, cross, dirDeg, dist, distToSegment, dot, lineLineIntersection, mul, projectParam, sub } from "./geom";
import { backDir } from "./model";
import { nearestLockAngle } from "./snap";
import { formatLength } from "./typed";

export type PipeEl = Extract<Element, { kind: "pipe" }>;

/** The parts of a pipe that drawing and hit testing need. */
export type PipeShape = Pick<PipeEl, "system" | "diameter_mm" | "points">;

// Tool defaults and the drainage fall come from src/contract/pipes.ts.
export { PIPE_DEFAULTS, PIPE_SYSTEM_LABEL };
export type { PipeDefaults } from "../contract/pipes";

/** The drainage fall the tool draws with: `drainMinSlopePct`. */
export const drainFallPct = drainMinSlopePct;

/** Two pipe points closer than this are the same point (contract). */
export const PIPE_EPS_MM = 1;

/** Narrowest a pipe is drawn, in CSS pixels, however far out the view is. */
export const PIPE_MIN_PX = 2;

/** Height steps of PageUp and PageDown while drawing, mm. Shift takes the fine one. */
export const PIPE_HEIGHT_STEP = 100;
export const PIPE_HEIGHT_STEP_FINE = 10;

export interface PipeSpec {
  system: PipeSystem;
  material: PipeMaterial;
  diameterMm: number;
  startHeightMm: number;
}

/** What the pipe tool draws: the tool options with the per system defaults filled in. */
export function pipeSpec(o: {
  pipeSystem: PipeSystem;
  pipeMaterial: PipeMaterial | null;
  pipeDiameterMm: number | null;
  pipeElevationMm: number | null;
}): PipeSpec {
  const d = PIPE_DEFAULTS[o.pipeSystem] ?? PIPE_DEFAULTS.cold_water;
  return {
    system: o.pipeSystem,
    material: o.pipeMaterial ?? d.material,
    diameterMm: o.pipeDiameterMm ?? d.diameterMm,
    startHeightMm: o.pipeElevationMm ?? d.startHeightMm,
  };
}

/**
 * Fall the tool gives new horizontal segments: the drain default for the
 * systems that fall (drainage, storm, condensate), none for the others. No
 * aircon manual gives a condensate number, so it uses the drain default too.
 */
export function toolFallPct(spec: Pick<PipeSpec, "system" | "diameterMm">): number | null {
  return pipeFalls(spec.system) ? drainFallPct(spec.diameterMm) : null;
}

// ---------------------------------------------------------------- line sets

/** The liquid line of every PH split unit line set, mm (docs/CONTRACT.md). */
export const LIQUID_LINE_MM = 6.35;
/** Foam insulation on each line of a line set, mm. Only sets the drawn spacing. */
export const LINESET_INSULATION_MM = 10;

/** Center spacing of the gas and liquid lines of a line set, taped side by side. */
export function lineSetSpacingMm(gasMm: number): number {
  return (gasMm + LIQUID_LINE_MM) / 2 + 2 * LINESET_INSULATION_MM;
}

export interface LineSetPx {
  /** Center spacing of the two lines, CSS pixels. */
  sep: number;
  gasW: number;
  liquidW: number;
}

/**
 * A refrigerant line set drawn at plan scale: the gas line on the left of
 * the run direction, the liquid line on the right, each at its size and
 * spacing at this zoom, never so thin or close that they merge.
 */
export function lineSetPx(gasMm: number, scale: number, uiScale = 1): LineSetPx {
  return {
    sep: Math.max(lineSetSpacingMm(gasMm) * scale, 4.5 * uiScale),
    gasW: Math.max(1.8 * uiScale, gasMm * scale),
    liquidW: Math.max(1.3 * uiScale, LIQUID_LINE_MM * scale),
  };
}

/** Half the drawn band of a run, CSS pixels: its width, or both lines of a line set. */
export function pipeBandHalfPx(pipe: Pick<PipeEl, "system" | "diameter_mm">, scale: number, uiScale = 1): number {
  if (pipe.system === "refrigerant") {
    const l = lineSetPx(pipe.diameter_mm, scale, uiScale);
    return l.sep / 2 + Math.max(l.gasW, l.liquidW) / 2;
  }
  return pipeWidthPx(pipe.diameter_mm, scale, uiScale) / 2;
}

export const planOf = (v: Vec3 | P): P => ({ x: v.x, y: v.y });

export function planDist(a: Vec3 | P, b: Vec3 | P): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

function dist3(a: Vec3, b: Vec3): number {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
}

/** Riser rule (docs/CONTRACT.md, "Pipes"): plan length under 1 mm, or at most this while rising ten times as much. */
export const RISER_MAX_PLAN_MM = 50;

/**
 * True when a segment is a riser: under 1 mm apart in plan, or at most 50 mm
 * apart while rising at least ten times that. Plans draw it as a circle,
 * never as a line. Same rule as the exports (`guhit-export` pipes.rs).
 */
export function isVertical(a: Vec3, b: Vec3): boolean {
  const h = planDist(a, b);
  return h < PIPE_EPS_MM || (h <= RISER_MAX_PLAN_MM && Math.abs(b.z - a.z) >= 10 * h);
}

/**
 * Height at the end of a new horizontal segment that starts at `fromZ` and
 * falls `pct` percent of its plan length. Rounded down to the micron, so the
 * model carries no float noise and never comes out flatter than asked.
 */
export function fallEnd(fromZ: number, from: P, to: P, pct: number): number {
  return Math.floor((fromZ - (planDist(from, to) * pct) / 100) * 1000) / 1000;
}

/** Fall of a segment in percent of its plan length: positive falls, negative rises. Null for a riser. */
export function segmentFallPct(a: Vec3, b: Vec3): number | null {
  if (isVertical(a, b)) return null;
  return ((a.z - b.z) / planDist(a, b)) * 100;
}

// ---------------------------------------------------------------- plan nodes

export interface PipeNode {
  /** Index of the first point of the node. */
  index: number;
  /** Points joined by risers. More than one is a riser (a vertical run). */
  count: number;
  /** Plan position: the point, or the middle of a riser, where its circle is drawn. */
  point: P;
  /** Height where the run arrives at the node and where it leaves, in run order. */
  zIn: number;
  zOut: number;
  zMin: number;
  zMax: number;
}

/** The run as plan nodes: points joined by risers merge into one node, handled as one in plan. */
export function pipeNodes(points: readonly Vec3[]): PipeNode[] {
  const out: PipeNode[] = [];
  for (let i = 0; i < points.length; i++) {
    const v = points[i];
    const last = out[out.length - 1];
    if (last && isVertical(points[i - 1], v)) {
      const first = points[last.index];
      last.count++;
      last.point = { x: (first.x + v.x) / 2, y: (first.y + v.y) / 2 };
      last.zOut = v.z;
      last.zMin = Math.min(last.zMin, v.z);
      last.zMax = Math.max(last.zMax, v.z);
      continue;
    }
    out.push({ index: i, count: 1, point: { x: v.x, y: v.y }, zIn: v.z, zOut: v.z, zMin: v.z, zMax: v.z });
  }
  return out;
}

export interface PlanRiser {
  /** Middle of the riser in plan, where its circle goes. */
  point: P;
  /** Heights where it starts and ends in run order: it rises when `zTo` is higher. */
  zFrom: number;
  zTo: number;
}

/** What a plan shows of a run: its horizontal and sloped stretches, and its risers. */
export interface PipePlan {
  /** Plan polylines, split at every riser. */
  runs: P[][];
  /** One circle per plan position. */
  risers: PlanRiser[];
}

/** The plan of a run, split the way the exports split it (`guhit-export` pipes.rs `plan_view`). */
export function pipePlan(points: readonly Vec3[]): PipePlan {
  const runs: P[][] = [];
  const risers: PlanRiser[] = [];
  let cur: P[] = [];
  for (let i = 0; i + 1 < points.length; i++) {
    const a = points[i];
    const b = points[i + 1];
    if (isVertical(a, b)) {
      if (cur.length >= 2) runs.push(cur);
      cur = [];
      const c = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
      const same = risers.find((r) => planDist(r.point, c) < PIPE_EPS_MM);
      if (same) same.zTo = b.z;
      else risers.push({ point: c, zFrom: a.z, zTo: b.z });
    } else {
      if (cur.length === 0) cur.push(planOf(a));
      cur.push(planOf(b));
    }
  }
  if (cur.length >= 2) runs.push(cur);
  return { runs, risers };
}

/** True when a node climbs or drops at least 1 mm: it is drawn as a riser. */
export function isRiser(n: PipeNode): boolean {
  return n.count > 1 && n.zMax - n.zMin >= PIPE_EPS_MM;
}

/** Removes points closer than 1 mm to the one before them. */
export function cleanRun(points: readonly Vec3[]): Vec3[] {
  const out: Vec3[] = [];
  for (const v of points) {
    const last = out[out.length - 1];
    if (last && dist3(last, v) < PIPE_EPS_MM) continue;
    out.push(v);
  }
  return out;
}

// ---------------------------------------------------------------- drawing a run

/**
 * A run being drawn. `penZ` is the height the run continues at from its last
 * point. When it differs from that point's height, a riser is pending there.
 */
export interface RunDraft {
  points: Vec3[];
  penZ: number;
}

export function pendingRiser(d: RunDraft): boolean {
  const last = d.points[d.points.length - 1];
  return !!last && Math.abs(d.penZ - last.z) >= PIPE_EPS_MM;
}

/** The run as it stands, with the pending riser at its last point. */
export function withPendingRiser(d: RunDraft): Vec3[] {
  if (!pendingRiser(d)) return d.points;
  const last = d.points[d.points.length - 1];
  return [...d.points, { x: last.x, y: last.y, z: d.penZ }];
}

export interface AddPointOptions {
  /** Fall of the new horizontal segment in percent, null for level. */
  fallPct: number | null;
  /** Height of a pipe the point snapped to. The run joins it there. */
  snapZ: number | null;
}

/**
 * Adds the next point of a run. A pen height that moved since the last point
 * first adds a riser there. The new segment runs level at the pen height, or
 * falls by `fallPct` (drainage). A point snapped to another pipe takes that
 * pipe's height: when the segment arrives higher or lower, a riser joins it,
 * except that a falling run arriving below its join slopes up to it instead.
 */
export function addRunPoint(d: RunDraft, next: P, o: AddPointOptions): RunDraft {
  if (d.points.length === 0) {
    const z = o.snapZ ?? d.penZ;
    return { points: [{ x: next.x, y: next.y, z }], penZ: z };
  }
  const pts = [...withPendingRiser(d)];
  const from = pts[pts.length - 1];
  if (planDist(from, next) < PIPE_EPS_MM) {
    // Same plan position: only a vertical move, when a snapped height asks for one.
    const z = o.snapZ ?? from.z;
    if (Math.abs(z - from.z) >= PIPE_EPS_MM) pts.push({ x: from.x, y: from.y, z });
    return { points: pts, penZ: pts[pts.length - 1].z };
  }
  let endZ = o.fallPct !== null ? fallEnd(from.z, from, next, o.fallPct) : from.z;
  // A falling run never climbs a riser into a join: it slopes up to it, so its
  // fall label and the drain slope review show the problem.
  if (o.snapZ !== null && (Math.abs(o.snapZ - endZ) < PIPE_EPS_MM || (o.fallPct !== null && o.snapZ > endZ))) endZ = o.snapZ;
  pts.push({ x: next.x, y: next.y, z: endZ });
  if (o.snapZ !== null && Math.abs(o.snapZ - endZ) >= PIPE_EPS_MM) pts.push({ x: next.x, y: next.y, z: o.snapZ });
  return { points: pts, penZ: pts[pts.length - 1].z };
}

/** Escape while drawing: drops the pending riser first, then the last point. Null ends the run. */
export function popRunPoint(d: RunDraft): RunDraft | null {
  if (pendingRiser(d)) return { points: d.points, penZ: d.points[d.points.length - 1].z };
  if (d.points.length <= 1) return null;
  const points = d.points.slice(0, -1);
  return { points, penZ: points[points.length - 1].z };
}

/** Points to commit when the run finishes, or null when it has fewer than two. */
export function finishRun(d: RunDraft): Vec3[] | null {
  const pts = cleanRun(withPendingRiser(d));
  return pts.length >= 2 ? pts : null;
}

// ---------------------------------------------------------------- snapping

/** Drainage and vent join each other. Every other system only joins itself. */
export function canJoin(a: PipeSystem, b: PipeSystem): boolean {
  if (a === b) return true;
  const waste = (s: PipeSystem): boolean => s === "drainage" || s === "vent";
  return waste(a) && waste(b);
}

/** Objects that take water or drain: the sanitary ones and the kitchen sink. */
export function isPlumbingFixture(a: Pick<Asset, "category" | "catalog_key">): boolean {
  return a.category === "sanitary" || a.catalog_key === "kitchen-sink";
}

const WIRED: ReadonlySet<DeviceKind> = new Set<DeviceKind>([
  "lighting_outlet",
  "convenience_receptacle",
  "special_purpose_outlet",
  "switch",
  "panelboard",
  "smoke_detector",
  "buzzer",
  "push_button",
]);

/**
 * True when a run of `system` starts or ends at this object: plumbing
 * fixtures for water, drain and vent; electrical devices and lights for
 * conduit; split units for a line set; indoor and window units and floor
 * drains for condensate. Storm drains have no catalog object to start at.
 */
export function isServiceFixture(a: Pick<Asset, "category" | "catalog_key">, system: PipeSystem, device: DeviceKind | null): boolean {
  switch (system) {
    case "storm":
      return false;
    case "conduit":
      return a.category === "lighting" || (device !== null && WIRED.has(device));
    case "refrigerant":
      return device === "aircon_indoor" || device === "aircon_outdoor";
    case "condensate":
      return device === "aircon_indoor" || device === "aircon_window" || a.catalog_key === "floor-drain";
    default:
      return PIPE_GROUP[system] === "plumbing" && isPlumbingFixture(a);
  }
}

export interface FixturePoint {
  point: P;
  label: string;
}

/** Snap points of a fixture: its center and the middle of its back edge (local +y). */
export function fixturePoints(a: Pick<Asset, "position" | "rotation_deg" | "depth_mm" | "name">): FixturePoint[] {
  const back = add(a.position, mul(backDir(a.rotation_deg), a.depth_mm / 2));
  return [
    { point: { x: a.position.x, y: a.position.y }, label: `${a.name}, center` },
    { point: back, label: `${a.name}, back` },
  ];
}

export type PipeSnapKind = "pipe_end" | "pipe_joint" | "tee" | "fixture";

export interface PipeSnap {
  point: P;
  /** Height the run joins at. Null keeps the pen height (fixtures). */
  z: number | null;
  kind: PipeSnapKind;
  label: string;
  /** The pipe snapped to, or null for a fixture. */
  pipeId: string | null;
}

export interface PipeSnapScene {
  pipes: readonly Pick<PipeEl, "id" | "system" | "points">[];
  fixtures: readonly FixturePoint[];
}

export interface PipeSnapOptions {
  /** Pick radius in mm. */
  tol: number;
  /** The system being drawn. Only pipes it can join are targets. */
  system: PipeSystem;
  /** Current pen height: where a riser in the middle of a run is joined. */
  penZ: number;
  /** Previous point of the run, for the angle locks. */
  anchor: P | null;
  /** Forced 0/90 lock from the anchor (Shift or ortho): targets must lie on that ray. */
  ortho: boolean;
}

/** Soft angle lock of the plan snap (snap.ts): 45 degree steps within 4 degrees. */
const SOFT_STEP_DEG = 45;
const SOFT_TOL_DEG = 4;

/** Height of the segment a-b at plan point `p`, interpolated along the plan. */
export function zOnSegment(a: Vec3, b: Vec3, p: P): number {
  const L = planDist(a, b);
  if (L < 1e-9) return a.z;
  const t = Math.max(0, Math.min(1, projectParam(p, a, b)));
  return a.z + (b.z - a.z) * t;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/** Plan point of a node's riser at height `z`, along its vertical segments. */
function riserPointAt(points: readonly Vec3[], n: PipeNode, z: number): P {
  for (let i = n.index; i < n.index + n.count - 1; i++) {
    const a = points[i];
    const b = points[i + 1];
    if (z < Math.min(a.z, b.z) - 1e-9 || z > Math.max(a.z, b.z) + 1e-9) continue;
    const t = Math.abs(b.z - a.z) < 1e-9 ? 0 : (z - a.z) / (b.z - a.z);
    return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
  }
  return n.point;
}

/**
 * Pipe snapping, in priority order: a point of a joinable pipe (its ends and
 * interior points, inheriting their height so joints connect in 3D), then a
 * fixture's center or back edge, then the middle of a joinable pipe segment
 * (a tee, at that pipe's height there). Null when nothing is in reach: the
 * caller falls back to the plan snaps (grid, walls, angles).
 */
export function snapToPipes(cursor: P, scene: PipeSnapScene, o: PipeSnapOptions): PipeSnap | null {
  // Angle locks from the anchor, like the plan snap: forced ortho searches only
  // along its ray; a soft 45 degree lock lets a tee land on the straight ray.
  let probe = cursor;
  let lock: { a: P; d: P } | null = null;
  if (o.anchor && dist(o.anchor, cursor) > 1e-9) {
    const l = nearestLockAngle(o.anchor, cursor, o.ortho ? 90 : SOFT_STEP_DEG);
    if (o.ortho || l.diff <= SOFT_TOL_DEG) {
      const d = dirDeg(l.angle);
      lock = { a: o.anchor, d };
      if (o.ortho) probe = add(o.anchor, mul(d, Math.max(0, dot(sub(cursor, o.anchor), d))));
    }
  }
  const ray = o.ortho ? lock : null;
  const onLock = (p: P, l: { a: P; d: P }): boolean => {
    const v = sub(p, l.a);
    return Math.abs(cross(v, l.d)) < 0.5 && dot(v, l.d) > 0;
  };
  const onRay = (p: P): boolean => !ray || onLock(p, ray);
  const pipes = scene.pipes.filter((p) => canJoin(o.system, p.system));

  // 1. Pipe points. A riser in the middle of a run is joined at the pen height.
  let best: PipeSnap | null = null;
  let bestD = o.tol;
  for (const pipe of pipes) {
    const nodes = pipeNodes(pipe.points);
    for (let i = 0; i < nodes.length; i++) {
      const n = nodes[i];
      if (!onRay(n.point)) continue;
      const d = dist(probe, n.point);
      if (d > bestD) continue;
      const first = i === 0;
      const last = i === nodes.length - 1;
      // An end joins at its end point. A riser elsewhere is joined at the pen
      // height, on the riser itself, so the joint sits on the centerline in 3D.
      let at: Vec3;
      if (first && !last) at = pipe.points[0];
      else if (last && !first) at = pipe.points[pipe.points.length - 1];
      else {
        const z = clamp(o.penZ, n.zMin, n.zMax);
        at = { ...riserPointAt(pipe.points, n, z), z };
      }
      const end = first || last;
      best = { point: planOf(at), z: at.z, kind: end ? "pipe_end" : "pipe_joint", label: end ? "Pipe end" : "Pipe joint", pipeId: pipe.id };
      bestD = d;
    }
  }
  if (best) return best;

  // 2. Fixtures: where the run starts at a basin, a WC or a sink.
  for (const f of scene.fixtures) {
    if (!onRay(f.point)) continue;
    const d = dist(probe, f.point);
    if (d > bestD) continue;
    best = { point: f.point, z: null, kind: "fixture", label: f.label, pipeId: null };
    bestD = d;
  }
  if (best) return best;

  // 3. A tee on the middle of a joinable pipe: where the locked ray crosses it
  // near the cursor, else (no forced lock) the nearest point on it.
  const onLockTee: PipeSnap | null = lock ? teeOnRay(pipes, lock, cursor, probe, o.tol, onLock) : null;
  if (onLockTee || ray) return onLockTee;
  for (const pipe of pipes) {
    for (let i = 0; i + 1 < pipe.points.length; i++) {
      const a = pipe.points[i];
      const b = pipe.points[i + 1];
      if (isVertical(a, b)) continue;
      const p = closestOnSegment(probe, a, b).point;
      const d = dist(probe, p);
      if (d > bestD) continue;
      best = { point: p, z: zOnSegment(a, b, p), kind: "tee", label: "Tee", pipeId: pipe.id };
      bestD = d;
    }
  }
  return best;
}

/** The nearest crossing of the locked ray with a joinable pipe segment, within `tol` of the cursor on the ray. */
function teeOnRay(
  pipes: readonly Pick<PipeEl, "id" | "points">[],
  lock: { a: P; d: P },
  cursor: P,
  probe: P,
  tol: number,
  onLock: (p: P, l: { a: P; d: P }) => boolean,
): PipeSnap | null {
  // The cursor's place along the ray.
  const along = add(lock.a, mul(lock.d, Math.max(0, dot(sub(cursor, lock.a), lock.d))));
  let best: PipeSnap | null = null;
  let bestD = tol;
  for (const pipe of pipes) {
    for (let i = 0; i + 1 < pipe.points.length; i++) {
      const a = pipe.points[i];
      const b = pipe.points[i + 1];
      if (isVertical(a, b)) continue;
      const x = lineLineIntersection(lock.a, add(lock.a, lock.d), a, b);
      if (!x || !onLock(x, lock)) continue;
      const t = projectParam(x, a, b);
      if (t < 0 || t > 1) continue;
      const d = Math.min(dist(x, along), dist(x, probe));
      if (d > bestD) continue;
      best = { point: x, z: zOnSegment(a, b, x), kind: "tee", label: "Tee", pipeId: pipe.id };
      bestD = d;
    }
  }
  return best;
}

// ---------------------------------------------------------------- editing

/**
 * Moves plan node `index` (its first point) with `count` points so its plan
 * position lands on `to`. All points of the node move by the same step, so a
 * riser stays a riser. `z`, when given, is the height a single point joins at
 * (it snapped to another pipe). Points that land on their neighbour merge.
 * Null when fewer than two remain.
 */
export function movePipeNode(points: readonly Vec3[], index: number, count: number, to: P, z: number | null): Vec3[] | null {
  const a = points[index];
  const b = points[index + count - 1];
  if (!a || !b) return null;
  const dx = to.x - (a.x + b.x) / 2;
  const dy = to.y - (a.y + b.y) / 2;
  const out = points.map((v, i) =>
    i >= index && i < index + count ? { x: v.x + dx, y: v.y + dy, z: count === 1 && z !== null ? z : v.z } : v,
  );
  const clean = cleanRun(out);
  return clean.length >= 2 ? clean : null;
}

// ---------------------------------------------------------------- hit testing and sizes

/** Drawn width of a pipe in CSS pixels: its size at this zoom, never thinner than the minimum. */
export function pipeWidthPx(diameterMm: number, scale: number, uiScale = 1): number {
  return Math.max(PIPE_MIN_PX * uiScale, diameterMm * scale);
}

/** Radius of the riser circle in CSS pixels. */
export function riserRadiusPx(widthPx: number, uiScale = 1): number {
  return Math.max(6 * uiScale, widthPx / 2 + 2.5 * uiScale);
}

/** Riser circle radius of a run in CSS pixels: around its whole band (both lines of a line set). */
export function pipeRiserRadiusPx(pipe: Pick<PipeEl, "system" | "diameter_mm">, scale: number, uiScale = 1): number {
  return riserRadiusPx(pipeBandHalfPx(pipe, scale, uiScale) * 2, uiScale);
}

/** Plan distance from `p` to the run. A riser counts as its plan point. */
export function distToPipe(p: P, points: readonly Vec3[]): number {
  if (points.length === 1) return planDist(p, points[0]);
  let best = Infinity;
  for (let i = 0; i + 1 < points.length; i++) best = Math.min(best, distToSegment(p, planOf(points[i]), planOf(points[i + 1])));
  return best;
}

/**
 * True when `p` picks the pipe: near its centerline by half its drawn width
 * (both lines of a line set) plus half the pick tolerance, or inside a riser
 * circle. `pxMm` is the size of one screen pixel in mm, `tol` the pick radius in mm.
 */
export function hitsPipe(p: P, pipe: Pick<PipeEl, "diameter_mm" | "points"> & { system?: PipeSystem }, pxMm: number, tol: number): boolean {
  const band = pipe.system ? pipeBandHalfPx({ system: pipe.system, diameter_mm: pipe.diameter_mm }, 1 / pxMm) * pxMm : 0;
  const half = Math.max(pipe.diameter_mm / 2, (PIPE_MIN_PX / 2) * pxMm, band);
  if (distToPipe(p, pipe.points) <= half + tol * 0.5) return true;
  const r = (pipe.system ? pipeRiserRadiusPx({ system: pipe.system, diameter_mm: pipe.diameter_mm }, 1 / pxMm) : riserRadiusPx(pipeWidthPx(pipe.diameter_mm, 1 / pxMm))) * pxMm;
  return pipePlan(pipe.points).risers.some((q) => planDist(p, q.point) <= r + tol * 0.25);
}

// ---------------------------------------------------------------- readouts

/** A height with its sign, in the display unit: "+300", "-300", "0". */
export function formatHeight(z: number, unit: DisplayUnit, withUnit = false): string {
  const r = Math.abs(z) < 0.5 ? 0 : z;
  const s = formatLength(r, unit, withUnit);
  return r > 0 ? `+${s}` : s;
}

/** A fall percentage for labels: "2%", "1.5%". */
export function formatPct(pct: number): string {
  const r = Math.round(Math.abs(pct) * 10) / 10;
  return `${Number.isInteger(r) ? r.toFixed(0) : r.toFixed(1)}%`;
}
