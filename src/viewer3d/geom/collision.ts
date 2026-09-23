// Walk mode collision. Pure plan math in millimeters, no three.js, no DOM.
//
// The walker is a circle (250 mm radius) that stays out of every blocker on
// its level: the wall outlines from `Derived.walls`, cut open at each door
// (a window leaves the wall closed), columns, and objects taller than 300 mm
// that reach below eye height. A move is split into substeps shorter than the
// radius, and after each substep the circle is pushed out of whatever it
// overlaps along the shortest way. Pushing along the contact normal keeps the
// part of the move that runs along the surface, so the walker slides along a
// wall instead of sticking to it, and no step is ever long enough to jump a
// wall, however slow the frame was.

import type { DocState, LayerKey, Level, Opening, Wall } from "../../contract/bindings";
import { EYE_HEIGHT_MM } from "./cameraMath";
import type { Pt } from "./coords";
import { clipHalfPlane, closestOnSegment, ensureCCW, orientedRect, pointInPolygon, segmentsIntersect, signedArea } from "./polygon";

export const WALKER_RADIUS_MM = 250;
/** Objects this tall or lower do not block: rugs, low tables, a floor drain. */
export const OBJECT_BLOCK_MIN_HEIGHT_MM = 300;
/** A substep never moves further than this share of the radius. */
const MAX_STEP_OF_RADIUS = 0.4;
const PUSH_ITERATIONS = 4;

export type ColliderSource = "wall" | "column" | "object";

interface Box {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export type Collider =
  | ({ kind: "poly"; pts: Pt[]; source: ColliderSource; id: string } & Box)
  | ({ kind: "circle"; c: Pt; r: number; source: ColliderSource; id: string } & Box);

/** A span on a wall centerline, for the minimap. */
export interface WallSpan {
  a: Pt;
  b: Pt;
  wallId: string;
}

export interface CollisionWorld {
  levelId: string | null;
  colliders: Collider[];
  /** Wall outline pieces between door gaps, counter-clockwise. */
  wallPieces: Pt[][];
  doorGaps: WallSpan[];
  windows: WallSpan[];
}

export const EMPTY_WORLD: CollisionWorld = { levelId: null, colliders: [], wallPieces: [], doorGaps: [], windows: [] };

function boxOf(pts: Pt[]): Box {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const p of pts) {
    minX = Math.min(minX, p.x);
    minY = Math.min(minY, p.y);
    maxX = Math.max(maxX, p.x);
    maxY = Math.max(maxY, p.y);
  }
  return { minX, minY, maxX, maxY };
}

function polyCollider(pts: Pt[], source: ColliderSource, id: string): Collider | null {
  const ccw = ensureCCW(pts);
  if (ccw.length < 3 || Math.abs(signedArea(ccw)) < 1) return null;
  return { kind: "poly", pts: ccw, source, id, ...boxOf(ccw) };
}

/** Rectangle fallback for a wall whose derived outline is missing. */
function wallRect(w: Wall): Pt[] {
  const dx = w.end.x - w.start.x;
  const dy = w.end.y - w.start.y;
  const len = Math.hypot(dx, dy) || 1;
  const nx = (-dy / len) * (w.thickness_mm / 2);
  const ny = (dx / len) * (w.thickness_mm / 2);
  return [
    { x: w.start.x - nx, y: w.start.y - ny },
    { x: w.end.x - nx, y: w.end.y - ny },
    { x: w.end.x + nx, y: w.end.y + ny },
    { x: w.start.x + nx, y: w.start.y + ny },
  ];
}

/** Which level an element counts as on: its own, or the lowest when that is gone. */
export function levelResolver(doc: DocState): (levelId: string) => Level | null {
  const levels = new Map((doc.project.levels ?? []).map((l) => [l.id, l]));
  const lowest = [...levels.values()].sort((a, b) => a.elevation_mm - b.elevation_mm)[0] ?? null;
  return (id) => levels.get(id) ?? lowest;
}

/**
 * Cuts a wall outline into the solid pieces between its door openings. `u`
 * runs along the wall from its start; a door spans offset +- width / 2.
 */
export function wallPieces(outline: Pt[], wall: Wall, doors: Opening[]): Pt[][] {
  const dx = wall.end.x - wall.start.x;
  const dy = wall.end.y - wall.start.y;
  const len = Math.hypot(dx, dy);
  if (!(len > 1e-3)) return [outline];
  const ux = dx / len;
  const uy = dy / len;
  const base = wall.start.x * ux + wall.start.y * uy;
  const gaps = doors
    .map((o) => [o.offset_mm - o.width_mm / 2, o.offset_mm + o.width_mm / 2] as [number, number])
    .filter(([a, b]) => b - a > 1)
    .sort((p, q) => p[0] - q[0]);
  // Overlapping doors make one gap.
  const merged: [number, number][] = [];
  for (const g of gaps) {
    const last = merged[merged.length - 1];
    if (last && g[0] <= last[1]) last[1] = Math.max(last[1], g[1]);
    else merged.push([g[0], g[1]]);
  }
  const pieces: Pt[][] = [];
  let lo = -Infinity;
  const cut = (from: number, to: number) => {
    let p = outline;
    // Keep u >= from:  -u <= -from.
    if (Number.isFinite(from)) p = clipHalfPlane(p, -ux, -uy, -(from + base));
    // Keep u <= to.
    if (Number.isFinite(to)) p = clipHalfPlane(p, ux, uy, to + base);
    if (p.length >= 3 && Math.abs(signedArea(p)) > 1) pieces.push(p);
  };
  for (const [a, b] of merged) {
    cut(lo, a);
    lo = b;
  }
  cut(lo, Infinity);
  return pieces;
}

/**
 * Everything that blocks a walker on `levelId`. Hidden layers do not block:
 * what is not drawn is not there. Doors only open a gap while the openings
 * layer is shown, because the 3D wall is only cut for them then.
 */
export function buildCollisionWorld(doc: DocState | null, levelId: string | null): CollisionWorld {
  if (!doc) return EMPTY_WORLD;
  const project = doc.project;
  const layerOn = (key: LayerKey) => project.layers?.find((l) => l.key === key)?.visible !== false;
  const levelOf = levelResolver(doc);
  const level = levelId ? levelOf(levelId) : levelOf("");
  if (!level) return EMPTY_WORLD;
  const onLevel = (id: string) => levelOf(id)?.id === level.id;

  const colliders: Collider[] = [];
  const pieces: Pt[][] = [];
  const doorGaps: WallSpan[] = [];
  const windows: WallSpan[] = [];

  const outlines = new Map((doc.derived?.walls ?? []).map((g) => [g.wall_id, g.outline]));
  const hosted = new Map<string, Opening[]>();
  for (const e of project.elements) {
    if (e.kind !== "opening") continue;
    const list = hosted.get(e.wall_id) ?? [];
    list.push(e);
    hosted.set(e.wall_id, list);
  }

  if (layerOn("walls")) {
    const openingsOn = layerOn("openings");
    for (const e of project.elements) {
      if (e.kind !== "wall" || !onLevel(e.level_id)) continue;
      const derived = outlines.get(e.id);
      const outline = ensureCCW(derived && derived.length >= 3 ? derived : wallRect(e));
      if (outline.length < 3) continue;
      const ops = openingsOn ? (hosted.get(e.id) ?? []) : [];
      const doors = ops.filter((o) => o.opening_type === "door");
      const len = Math.hypot(e.end.x - e.start.x, e.end.y - e.start.y) || 1;
      const at = (u: number): Pt => ({
        x: e.start.x + ((e.end.x - e.start.x) / len) * u,
        y: e.start.y + ((e.end.y - e.start.y) / len) * u,
      });
      for (const piece of wallPieces(outline, e, doors)) {
        pieces.push(piece);
        const c = polyCollider(piece, "wall", e.id);
        if (c) colliders.push(c);
      }
      for (const o of ops) {
        const span = { a: at(o.offset_mm - o.width_mm / 2), b: at(o.offset_mm + o.width_mm / 2), wallId: e.id };
        if (o.opening_type === "door") doorGaps.push(span);
        else windows.push(span);
      }
    }
  }

  if (layerOn("columns")) {
    for (const e of project.elements) {
      if (e.kind !== "column" || !onLevel(e.level_id)) continue;
      if (e.shape === "round") {
        const r = Math.max(e.width_mm, 10) / 2;
        colliders.push({
          kind: "circle",
          c: { x: e.center.x, y: e.center.y },
          r,
          source: "column",
          id: e.id,
          minX: e.center.x - r,
          minY: e.center.y - r,
          maxX: e.center.x + r,
          maxY: e.center.y + r,
        });
      } else {
        const c = polyCollider(orientedRect(e.center, Math.max(e.width_mm, 10), Math.max(e.depth_mm, 10), e.rotation_deg), "column", e.id);
        if (c) colliders.push(c);
      }
    }
  }

  if (layerOn("assets")) {
    for (const e of project.elements) {
      if (e.kind !== "asset" || !onLevel(e.level_id)) continue;
      if (!objectBlocks(e.height_mm, e.elevation_mm)) continue;
      const c = polyCollider(orientedRect(e.position, e.width_mm, e.depth_mm, e.rotation_deg), "object", e.id);
      if (c) colliders.push(c);
    }
  }

  return { levelId: level.id, colliders, wallPieces: pieces, doorGaps, windows };
}

/** Taller than 300 mm, and not hung above the walker's head. */
export function objectBlocks(heightMm: number, elevationMm: number): boolean {
  return heightMm > OBJECT_BLOCK_MIN_HEIGHT_MM && Math.max(elevationMm, 0) < EYE_HEIGHT_MM;
}

/** The point on a polygon's boundary nearest to `p`, and the distance to it. */
function closestOnPolygon(p: Pt, pts: Pt[]): { q: Pt; d: number } {
  let best: Pt = pts[0];
  let bestD = Infinity;
  for (let i = 0; i < pts.length; i++) {
    const q = closestOnSegment(p, pts[i], pts[(i + 1) % pts.length]);
    const d = Math.hypot(p.x - q.x, p.y - q.y);
    if (d < bestD) {
      bestD = d;
      best = q;
    }
  }
  return { q: best, d: bestD };
}

/** Pushes a circle out of one collider. Null when they do not overlap. */
function pushOutOf(p: Pt, r: number, c: Collider): Pt | null {
  if (p.x < c.minX - r || p.x > c.maxX + r || p.y < c.minY - r || p.y > c.maxY + r) return null;
  if (c.kind === "circle") {
    const dx = p.x - c.c.x;
    const dy = p.y - c.c.y;
    const d = Math.hypot(dx, dy);
    const need = r + c.r;
    if (d >= need) return null;
    if (d < 1e-6) return { x: c.c.x + need, y: c.c.y };
    return { x: c.c.x + (dx / d) * need, y: c.c.y + (dy / d) * need };
  }
  const inside = pointInPolygon(p, c.pts);
  const { q, d } = closestOnPolygon(p, c.pts);
  if (!inside && d >= r) return null;
  if (inside) {
    // Out through the nearest face: the direction from the center to the
    // nearest boundary point points out of the polygon.
    if (d < 1e-6) {
      const cx = (c.minX + c.maxX) / 2;
      const cy = (c.minY + c.maxY) / 2;
      const ox = q.x - cx;
      const oy = q.y - cy;
      const ol = Math.hypot(ox, oy) || 1;
      return { x: q.x + (ox / ol) * r, y: q.y + (oy / ol) * r };
    }
    return { x: q.x + ((q.x - p.x) / d) * r, y: q.y + ((q.y - p.y) / d) * r };
  }
  if (d < 1e-6) return null;
  return { x: q.x + ((p.x - q.x) / d) * r, y: q.y + ((p.y - q.y) / d) * r };
}

/**
 * Moves a circle of radius `r` at `p` out of everything it overlaps. A few
 * passes settle corners, where two blockers push against each other.
 */
export function pushOut(p: Pt, r: number, world: CollisionWorld): Pt {
  let cur = { x: p.x, y: p.y };
  for (let it = 0; it < PUSH_ITERATIONS; it++) {
    let moved = false;
    for (const c of world.colliders) {
      const next = pushOutOf(cur, r, c);
      if (!next) continue;
      cur = next;
      moved = true;
    }
    if (!moved) break;
  }
  return cur;
}

/**
 * Walks from `p` by `delta` with collision. The move is cut into substeps no
 * longer than 40 percent of the radius, so even a long, slow frame at running
 * speed cannot carry the circle through a thin wall.
 */
export function moveWithCollision(p: Pt, delta: Pt, r: number, world: CollisionWorld): Pt {
  const len = Math.hypot(delta.x, delta.y);
  if (!(len > 0)) return pushOut(p, r, world);
  const steps = Math.max(1, Math.ceil(len / (r * MAX_STEP_OF_RADIUS)));
  const sx = delta.x / steps;
  const sy = delta.y / steps;
  let cur = { x: p.x, y: p.y };
  for (let i = 0; i < steps; i++) cur = pushOut({ x: cur.x + sx, y: cur.y + sy }, r, world);
  return cur;
}

/** True when a circle at `p` touches nothing (within `slack` mm). */
export function isClear(p: Pt, r: number, world: CollisionWorld, slack = 1): boolean {
  const q = pushOut(p, r, world);
  return Math.hypot(q.x - p.x, q.y - p.y) <= slack;
}

/** True when the plan segment a-b passes through a wall piece. */
export function crossesWall(a: Pt, b: Pt, world: CollisionWorld): boolean {
  for (const piece of world.wallPieces) {
    if (pointInPolygon(a, piece) || pointInPolygon(b, piece)) return true;
    for (let i = 0; i < piece.length; i++) {
      if (segmentsIntersect(a, b, piece[i], piece[(i + 1) % piece.length])) return true;
    }
  }
  return false;
}
