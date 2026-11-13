// Slabs, railings, stairs seen from the level they arrive at, and the level
// below as an underlay (docs/CONTRACT.md "Slabs and railings", DECISIONS
// D44, D50). Pure, tested in slab.test.ts.

import type { DocState, Element, Level, RailingRun, SlabKind } from "../contract/bindings";
import type { P } from "./geom";
import { add, distToPolygonEdge, dist, expandRect, emptyRect, mul, pointInPolygon, polygonArea, sub } from "./geom";

export type SlabEl = Extract<Element, { kind: "slab" }>;
export type StairEl = Extract<Element, { kind: "stair" }>;

/** Slab tool defaults: a 150 mm slab flush with the floor, a 1000 mm bar railing. */
export const SLAB_THICKNESS_MM = 150;
export const SLAB_RAILING_HEIGHT_MM = 1000;
/** The level a stair arrives at has its floor within this of the stair's top. */
export const ARRIVAL_TOL_MM = 300;
/** The engine refuses outlines under 0.01 m2 (`slab_outline`). */
export const SLAB_MIN_AREA_MM2 = 10_000;
/** Bars and glass: two lines this far apart. A solid parapet: a band this wide. */
export const RAIL_GAP_MM = 50;
export const RAIL_SOLID_MM = 100;

export const SLAB_KIND_LABEL: Record<SlabKind, string> = {
  floor: "Floor",
  balcony: "Balcony",
  deck: "Deck",
  void: "Void",
};

// ---------------------------------------------------------------- outlines

/** Plan area of an outline in mm2, either winding. */
export function slabAreaMm2(polygon: readonly P[]): number {
  return Math.abs(polygonArea(polygon));
}

/** The outline as the engine keeps it: no point within 1 mm of the one before, no closing duplicate. */
export function cleanOutline(points: readonly P[]): P[] {
  const out: P[] = [];
  for (const p of points) {
    if (out.length > 0 && dist(p, out[out.length - 1]) < 1) continue;
    out.push({ x: p.x, y: p.y });
  }
  while (out.length > 1 && dist(out[0], out[out.length - 1]) < 1) out.pop();
  return out;
}

/** True when the points so far make an outline the slab tool may close. */
export function canCloseSlab(points: readonly P[]): boolean {
  const c = cleanOutline(points);
  return c.length >= 3 && slabAreaMm2(c) >= SLAB_MIN_AREA_MM2;
}

export type SlabClick = { kind: "add"; points: P[] } | { kind: "close"; points: P[] } | { kind: "none" };

/**
 * One click of the slab tool at `p`: a click on the first point (within
 * `closeTol` mm) closes the outline once it has three points and adds
 * nothing before that, a click on the last point adds nothing, anything
 * else adds a point.
 */
export function slabClick(points: readonly P[], p: P, closeTol: number): SlabClick {
  if (points.length > 0 && dist(p, points[0]) <= closeTol) {
    // On the first point: it closes, or (too few points so far) adds nothing.
    return points.length >= 3 && canCloseSlab(points) ? { kind: "close", points: cleanOutline(points) } : { kind: "none" };
  }
  if (points.length > 0 && dist(p, points[points.length - 1]) < 1) return { kind: "none" };
  return { kind: "add", points: [...points, { x: p.x, y: p.y }] };
}

/** Backspace: the last point goes. Null when nothing is left, so the draft ends. */
export function popSlabPoint(points: readonly P[]): P[] | null {
  return points.length > 1 ? points.slice(0, -1) : null;
}

/** A new slab as the tool adds it. The engine gives it its id. */
export function newSlab(levelId: string, kind: SlabKind, railing: boolean, polygon: readonly P[]): SlabEl {
  return {
    kind: "slab",
    id: "",
    level_id: levelId,
    slab_kind: kind,
    polygon: cleanOutline(polygon),
    thickness_mm: SLAB_THICKNESS_MM,
    top_offset_mm: 0,
    material_id: null,
    railing: railing ? { height_mm: SLAB_RAILING_HEIGHT_MM, style: "bars" } : null,
  };
}

/** The outline with vertex `i` at `p`. */
export function moveVertex(polygon: readonly P[], i: number, p: P): P[] {
  return polygon.map((q, k) => (k === i ? { x: p.x, y: p.y } : q));
}

/** The outline with a new vertex `p` on edge `i` (from vertex i to i + 1). */
export function insertVertex(polygon: readonly P[], i: number, p: P): P[] {
  const out = polygon.slice();
  out.splice(i + 1, 0, { x: p.x, y: p.y });
  return out;
}

/** The outline without vertex `i`, or null when that would leave fewer than three. */
export function removeVertex(polygon: readonly P[], i: number): P[] | null {
  if (polygon.length <= 3 || i < 0 || i >= polygon.length) return null;
  return polygon.filter((_, k) => k !== i);
}

/** True when two outlines have the same corners (within `tol` mm), in any order or winding. */
export function sameOutline(a: readonly P[], b: readonly P[], tol = 1): boolean {
  const ca = cleanOutline(a);
  const cb = cleanOutline(b);
  return ca.length === cb.length && ca.every((p) => cb.some((q) => dist(p, q) <= tol));
}

/** Midpoint of each edge, edge i running from vertex i to i + 1. */
export function edgeMidpoints(polygon: readonly P[]): P[] {
  return polygon.map((p, i) => {
    const q = polygon[(i + 1) % polygon.length];
    return { x: (p.x + q.x) / 2, y: (p.y + q.y) / 2 };
  });
}

/**
 * The void cross: the two diagonals of a four-corner outline, otherwise the
 * diagonals of its bounding box clipped to it (docs/CONTRACT.md).
 */
export function voidCross(polygon: readonly P[]): [P, P][] {
  if (polygon.length < 3) return [];
  if (polygon.length === 4) {
    return [
      [polygon[0], polygon[2]],
      [polygon[1], polygon[3]],
    ];
  }
  const r = emptyRect();
  for (const p of polygon) expandRect(r, p);
  return [
    ...clipToPolygon({ x: r.minX, y: r.minY }, { x: r.maxX, y: r.maxY }, polygon),
    ...clipToPolygon({ x: r.minX, y: r.maxY }, { x: r.maxX, y: r.minY }, polygon),
  ];
}

/** The parts of segment a-b inside `polygon`. */
export function clipToPolygon(a: P, b: P, polygon: readonly P[]): [P, P][] {
  const d = sub(b, a);
  const ts = [0, 1];
  for (let i = 0; i < polygon.length; i++) {
    const c = polygon[i];
    const e = polygon[(i + 1) % polygon.length];
    const s = sub(e, c);
    const den = d.x * s.y - d.y * s.x;
    if (Math.abs(den) < 1e-9) continue;
    const ac = sub(c, a);
    const t = (ac.x * s.y - ac.y * s.x) / den;
    const u = (ac.x * d.y - ac.y * d.x) / den;
    if (t > 0 && t < 1 && u >= 0 && u <= 1) ts.push(t);
  }
  ts.sort((x, y) => x - y);
  const out: [P, P][] = [];
  for (let i = 0; i + 1 < ts.length; i++) {
    if (ts[i + 1] - ts[i] < 1e-9) continue;
    const m = add(a, mul(d, (ts[i] + ts[i + 1]) / 2));
    if (pointInPolygon(m, polygon)) out.push([add(a, mul(d, ts[i])), add(a, mul(d, ts[i + 1]))]);
  }
  return out;
}

/** Where "OPEN TO BELOW" goes: the centroid when it is inside, else the middle of the longest inside run of the horizontal through it. */
export function labelPoint(polygon: readonly P[]): P {
  const a = polygonArea(polygon);
  let cx = 0;
  let cy = 0;
  if (Math.abs(a) > 1e-9) {
    for (let i = 0; i < polygon.length; i++) {
      const p = polygon[i];
      const q = polygon[(i + 1) % polygon.length];
      const f = p.x * q.y - q.x * p.y;
      cx += (p.x + q.x) * f;
      cy += (p.y + q.y) * f;
    }
    cx /= 6 * a;
    cy /= 6 * a;
  } else if (polygon.length > 0) {
    cx = polygon[0].x;
    cy = polygon[0].y;
  }
  const c = { x: cx, y: cy };
  if (pointInPolygon(c, polygon)) return c;
  const r = emptyRect();
  for (const p of polygon) expandRect(r, p);
  const runs = clipToPolygon({ x: r.minX - 1, y: cy }, { x: r.maxX + 1, y: cy }, polygon);
  let best: [P, P] | null = null;
  for (const run of runs) if (!best || dist(run[0], run[1]) > dist(best[0], best[1])) best = run;
  return best ? { x: (best[0].x + best[1].x) / 2, y: cy } : c;
}

/**
 * A slab under `p`: near its outline, or inside it where no room of the
 * level is (a click on a room's floor selects the room, DECISIONS D44).
 */
export function hitsSlab(p: P, polygon: readonly P[], tol: number, roomPolygons: readonly (readonly P[])[]): boolean {
  if (polygon.length < 3) return false;
  if (distToPolygonEdge(p, polygon) <= tol) return true;
  if (!pointInPolygon(p, polygon)) return false;
  return !roomPolygons.some((r) => r.length >= 3 && pointInPolygon(p, r));
}

// ---------------------------------------------------------------- levels

function sortedLevels(levels: readonly Level[]): Level[] {
  return levels.slice().sort((a, b) => a.elevation_mm - b.elevation_mm);
}

/** The next lower level by elevation, or null on the lowest. */
export function levelBelow(levels: readonly Level[], levelId: string | null): Level | null {
  const here = levels.find((l) => l.id === levelId);
  if (!here) return null;
  let best: Level | null = null;
  for (const l of sortedLevels(levels)) {
    if (l.id === here.id || l.elevation_mm >= here.elevation_mm) continue;
    best = l;
  }
  return best;
}

/**
 * The level a stair on `levelId` arrives at: the level above whose
 * elevation is within 300 mm of the stair's top (its level's elevation plus
 * height), the closest one. Null when there is none.
 */
export function arrivalLevel(levels: readonly Level[], levelId: string): Level | null {
  const here = levels.find((l) => l.id === levelId);
  if (!here) return null;
  const top = here.elevation_mm + here.height_mm;
  let best: Level | null = null;
  let bestD = Infinity;
  for (const l of levels) {
    if (l.id === here.id || l.elevation_mm <= here.elevation_mm) continue;
    const d = Math.abs(l.elevation_mm - top);
    if (d <= ARRIVAL_TOL_MM && d < bestD) {
      best = l;
      bestD = d;
    }
  }
  return best;
}

/** Stairs of other levels that arrive at `levelId`. */
export function stairsArrivingAt(doc: DocState, levelId: string | null): StairEl[] {
  if (!levelId) return [];
  const levels = doc.project.levels;
  const arrive = new Map<string, string | null>();
  const out: StairEl[] = [];
  for (const e of doc.project.elements) {
    if (e.kind !== "stair" || e.level_id === levelId) continue;
    if (!arrive.has(e.level_id)) arrive.set(e.level_id, arrivalLevel(levels, e.level_id)?.id ?? null);
    if (arrive.get(e.level_id) === levelId) out.push(e);
  }
  return out;
}

// ---------------------------------------------------------------- stairs

/**
 * Distances along the run of the tread lines inside a straight flight of
 * `risers` risers (DECISIONS D44): n - 1 treads of run / (n - 1), so the
 * lines are at k * run / (n - 1) for k = 1 .. n - 2.
 */
export function stairTreadOffsets(runMm: number, risers: number): number[] {
  const treads = Math.max(1, Math.round(risers) - 1);
  const out: number[] = [];
  for (let k = 1; k < treads; k++) out.push((runMm * k) / treads);
  return out;
}

// ---------------------------------------------------------------- railings

/** Layer a rail hides and shows with: slab rails the slabs layer, stair rails the stairs layer. */
export function railingLayer(run: Pick<RailingRun, "kind">): "slabs" | "stairs" {
  return run.kind === "slab_edge" ? "slabs" : "stairs";
}

/** Rails standing on `levelId` whose layer is visible. */
export function railingsOn(doc: DocState, levelId: string | null, hidden: ReadonlySet<string>): RailingRun[] {
  if (!levelId) return [];
  return (doc.derived.railings ?? []).filter((r) => r.level_id === levelId && r.points.length >= 2 && !hidden.has(railingLayer(r)));
}

/** True when a run goes all the way round (it repeats its first point). */
export function isClosedRun(points: readonly P[]): boolean {
  return points.length >= 4 && dist(points[0], points[points.length - 1]) < 1;
}

/**
 * The run moved `d` mm to its left, with mitred corners (capped so a sharp
 * turn never spikes). A closed run is mitred at its start too.
 */
export function offsetRun(points: readonly P[], d: number): P[] {
  const closed = isClosedRun(points);
  const pts = closed ? points.slice(0, -1) : points.slice();
  const n = pts.length;
  if (n < 2) return pts.map((p) => ({ x: p.x, y: p.y }));
  const segs = closed ? n : n - 1;
  const normals: P[] = [];
  for (let i = 0; i < segs; i++) {
    const a = pts[i];
    const b = pts[(i + 1) % n];
    const L = Math.hypot(b.x - a.x, b.y - a.y) || 1;
    normals.push({ x: -(b.y - a.y) / L, y: (b.x - a.x) / L });
  }
  const out: P[] = [];
  for (let i = 0; i < n; i++) {
    const na = closed ? normals[(i - 1 + segs) % segs] : normals[Math.max(0, i - 1)];
    const nb = closed ? normals[i % segs] : normals[Math.min(segs - 1, i)];
    let mx = na.x + nb.x;
    let my = na.y + nb.y;
    const ml = Math.hypot(mx, my);
    if (ml < 1e-9) {
      mx = nb.x;
      my = nb.y;
    } else {
      mx /= ml;
      my /= ml;
    }
    const cos = mx * nb.x + my * nb.y;
    const k = Math.min(4, 1 / Math.max(0.25, cos));
    out.push({ x: pts[i].x + mx * d * k, y: pts[i].y + my * d * k });
  }
  if (closed) out.push({ ...out[0] });
  return out;
}

/**
 * What a rail draws on the plan: bars and glass as two lines 50 mm apart,
 * a solid parapet as a 100 mm band (a closed outline to fill).
 */
export function railingPlan(run: Pick<RailingRun, "points" | "style">): { lines: P[][]; band: P[] | null } {
  const pts = run.points.map((v) => ({ x: v.x, y: v.y }));
  if (run.style === "solid") {
    const half = RAIL_SOLID_MM / 2;
    const l = offsetRun(pts, half);
    const r = offsetRun(pts, -half);
    if (isClosedRun(pts)) return { lines: [l, r], band: null };
    return { lines: [l, r], band: [...l, ...r.reverse()] };
  }
  const half = RAIL_GAP_MM / 2;
  return { lines: [offsetRun(pts, half), offsetRun(pts, -half)], band: null };
}

// ---------------------------------------------------------------- rooms for the slab tool

export interface RoomOutline {
  roomId: string;
  levelId: string;
  name: string;
  polygon: P[];
  /** True when the room is on the level below (picked through the underlay). */
  below: boolean;
}

function roomsAt(doc: DocState, levelId: string, p: P, below: boolean): RoomOutline | null {
  const geo = new Map(doc.derived.rooms.map((g) => [g.room_id, g] as const));
  let best: RoomOutline | null = null;
  let bestArea = Infinity;
  for (const e of doc.project.elements) {
    if (e.kind !== "room" || e.level_id !== levelId) continue;
    const g = geo.get(e.id);
    if (!g || g.polygon.length < 3 || !pointInPolygon(p, g.polygon)) continue;
    if (g.area_mm2 < bestArea) {
      bestArea = g.area_mm2;
      best = { roomId: e.id, levelId, name: e.name, polygon: g.polygon.map((q) => ({ x: q.x, y: q.y })), below };
    }
  }
  return best;
}

/**
 * Whether the slab tool offers a room's outline under the pointer: only
 * away from every snap (a corner, face or line starts a free outline), with
 * the Slabs layer unlocked, and not while Alt is held, so a free outline can
 * start inside a room.
 */
export function offersRoom(snap: "none" | "grid" | string, alt: boolean, locked: boolean): boolean {
  return (snap === "none" || snap === "grid") && !alt && !locked;
}

/**
 * The room outline a single slab tool click at `p` takes: a room of the
 * active level, else (with the underlay on) one of the level below, so a
 * void over a double height room is one click on the floor above. The
 * smallest room wins where rooms nest.
 */
export function roomOutlineAt(doc: DocState, levelId: string | null, p: P, withBelow: boolean): RoomOutline | null {
  if (!levelId) return null;
  const here = roomsAt(doc, levelId, p, false);
  if (here || !withBelow) return here;
  const below = levelBelow(doc.project.levels, levelId);
  return below ? roomsAt(doc, below.id, p, true) : null;
}

/** Wall outlines of the level below `levelId`, for the underlay. */
export function underlayWalls(doc: DocState, levelId: string | null): P[][] {
  const below = levelBelow(doc.project.levels, levelId);
  if (!below) return [];
  const onBelow = new Set<string>();
  for (const e of doc.project.elements) if (e.kind === "wall" && e.level_id === below.id) onBelow.add(e.id);
  return doc.derived.walls.filter((g) => onBelow.has(g.wall_id) && g.outline.length >= 3).map((g) => g.outline.map((q) => ({ x: q.x, y: q.y })));
}

// ---------------------------------------------------------------- underlay toggle storage

export const UNDERLAY_KEY = "guhit.planUnderlay.";

/** The underlay switch of a project: on unless it was turned off. */
export function readUnderlay(projectId: string | null, storage: Pick<Storage, "getItem"> | null = safeStorage()): boolean {
  if (!projectId || !storage) return true;
  try {
    return storage.getItem(UNDERLAY_KEY + projectId) !== "0";
  } catch {
    return true;
  }
}

export function writeUnderlay(projectId: string | null, on: boolean, storage: Pick<Storage, "setItem" | "removeItem"> | null = safeStorage()): void {
  if (!projectId || !storage) return;
  try {
    // On is the default, so only "off" needs remembering.
    if (on) storage.removeItem(UNDERLAY_KEY + projectId);
    else storage.setItem(UNDERLAY_KEY + projectId, "0");
  } catch {
    // Private windows and blocked storage: the switch still works for this session.
  }
}

function safeStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}
