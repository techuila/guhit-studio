// Where a walk starts, and where to stand to look at a finding. Pure plan
// math in millimeters. A pose is a plan point plus a heading: `yaw` is the
// plan angle of the view direction (counter-clockwise from east, radians),
// `pitch` looks up when positive.

import type { DocState, Pipe, Vec3, Wall } from "../../contract/bindings";
import { groupFootprints } from "../scene/buildScene";
import { EYE_HEIGHT_MM } from "./cameraMath";
import { crossesWall, isClear, levelResolver, pushOut, WALKER_RADIUS_MM, type CollisionWorld } from "./collision";
import type { Pt } from "./coords";
import { boundsOf, ensureCCW, longAxis, orientedRect, pointInPolygon } from "./polygon";

export interface WalkPose {
  x: number;
  y: number;
  yaw: number;
  pitch: number;
}

export interface WalkStart extends WalkPose {
  /** Which rule placed the walker. */
  from: "room" | "door" | "outside";
}

/** How far outside an exterior door a walk starts, from the wall face. */
export const DOOR_STANDOFF_MM = 1200;
/** How far from a finding `walk_to` stands. */
export const FINDING_STANDOFF_MM = 1500;
const PITCH_LIMIT = 1.35;

/** Room polygons on a level, counter-clockwise, with their label points. */
export function roomsOn(doc: DocState, levelId: string): { id: string; polygon: Pt[]; label: Pt }[] {
  const levelOf = levelResolver(doc);
  const level = levelOf(levelId);
  const byId = new Map(doc.project.elements.map((e) => [e.id, e]));
  const out: { id: string; polygon: Pt[]; label: Pt }[] = [];
  for (const g of doc.derived?.rooms ?? []) {
    const room = byId.get(g.room_id);
    if (!room || room.kind !== "room" || levelOf(room.level_id)?.id !== level?.id) continue;
    const polygon = ensureCCW(g.polygon);
    if (polygon.length >= 3) out.push({ id: g.room_id, polygon, label: g.label_point });
  }
  return out;
}

function roomAt(rooms: { id: string; polygon: Pt[] }[], p: Pt): string | null {
  for (const r of rooms) if (pointInPolygon(p, r.polygon)) return r.id;
  return null;
}

/** A clear point inside `polygon` near `near`, searching outward in rings. */
function clearPointInRoom(polygon: Pt[], near: Pt, world: CollisionWorld, r: number): Pt | null {
  const ok = (p: Pt) => pointInPolygon(p, polygon) && isClear(p, r + 20, world);
  if (ok(near)) return near;
  for (let ring = 1; ring <= 12; ring++) {
    const dist = ring * 250;
    const n = 8 + ring * 4;
    for (let k = 0; k < n; k++) {
      const a = (k / n) * Math.PI * 2;
      const p = { x: near.x + Math.cos(a) * dist, y: near.y + Math.sin(a) * dist };
      if (ok(p)) return p;
    }
  }
  return null;
}

/**
 * The start rule. Inside the room under the orbit target when there is one,
 * keeping the orbit camera's heading. Otherwise just outside the first
 * exterior door, facing in. Otherwise south of the model, facing north.
 */
export function walkStartPose(
  doc: DocState,
  levelId: string,
  world: CollisionWorld,
  target: Pt | null,
  heading: number | null,
  r = WALKER_RADIUS_MM,
): WalkStart {
  const rooms = roomsOn(doc, levelId);
  if (target) {
    const room = rooms.find((rm) => pointInPolygon(target, rm.polygon));
    if (room) {
      const p = clearPointInRoom(room.polygon, target, world, r) ?? clearPointInRoom(room.polygon, room.label, world, r);
      if (p) {
        const yaw = heading ?? Math.atan2(longAxis(room.polygon).dir.y, longAxis(room.polygon).dir.x);
        return { x: p.x, y: p.y, yaw, pitch: -0.05, from: "room" };
      }
    }
  }

  const door = firstExteriorDoor(doc, levelId);
  if (door) {
    const p = pushOut(door.stand, r, world);
    return { x: p.x, y: p.y, yaw: Math.atan2(door.center.y - p.y, door.center.x - p.x), pitch: -0.05, from: "door" };
  }

  // Nothing to walk into: stand south of whatever there is and face it.
  const pts: Pt[] = [...world.wallPieces.flat(), ...rooms.flatMap((rm) => rm.polygon)];
  const b = boundsOf(pts);
  const cx = b ? (b.minX + b.maxX) / 2 : 0;
  const cy = b ? (b.minY + b.maxY) / 2 : 0;
  const south = b ? b.minY - 3000 : -3000;
  const p = pushOut({ x: cx, y: south }, r, world);
  return { x: p.x, y: p.y, yaw: Math.atan2(cy - p.y, cx - p.x), pitch: -0.05, from: "outside" };
}

/**
 * The first door, in element order, on an exterior wall of the level: its
 * center on the wall and the point `DOOR_STANDOFF_MM` outside the wall face.
 * Outside is the side that falls outside the building footprint.
 */
export function firstExteriorDoor(doc: DocState, levelId: string): { id: string; center: Pt; stand: Pt } | null {
  const levelOf = levelResolver(doc);
  const level = levelOf(levelId);
  if (!level) return null;
  const exterior = new Map((doc.derived?.walls ?? []).map((g) => [g.wall_id, g.exterior]));
  const walls = new Map<string, Wall>();
  for (const e of doc.project.elements) if (e.kind === "wall" && levelOf(e.level_id)?.id === level.id) walls.set(e.id, e);
  const footprints = groupFootprints(doc.derived?.footprints, level.id);
  const rooms = roomsOn(doc, level.id);
  const inside = (p: Pt) => footprints.some((fp) => pointInPolygon(p, fp)) || roomAt(rooms, p) !== null;

  for (const e of doc.project.elements) {
    if (e.kind !== "opening" || e.opening_type !== "door") continue;
    const wall = walls.get(e.wall_id);
    if (!wall || exterior.get(wall.id) !== true) continue;
    const dx = wall.end.x - wall.start.x;
    const dy = wall.end.y - wall.start.y;
    const len = Math.hypot(dx, dy);
    if (!(len > 1)) continue;
    const ux = dx / len;
    const uy = dy / len;
    const center = { x: wall.start.x + ux * e.offset_mm, y: wall.start.y + uy * e.offset_mm };
    const off = wall.thickness_mm / 2 + DOOR_STANDOFF_MM;
    const left = { x: center.x - uy * off, y: center.y + ux * off };
    const right = { x: center.x + uy * off, y: center.y - ux * off };
    const leftIn = inside(left);
    const rightIn = inside(right);
    const stand = leftIn && !rightIn ? right : !leftIn && rightIn ? left : leftIn ? right : left;
    return { id: e.id, center, stand };
  }
  return null;
}

/**
 * Where `walk_to` stands: about 1.5 m from the finding, on a room side, clear
 * of blockers, with a clear line to it when there is one, facing it. The
 * finding is plan x, y and z above the level floor.
 */
export function walkToPose(doc: DocState, levelId: string, world: CollisionWorld, location: Vec3, r = WALKER_RADIUS_MM): WalkPose {
  const rooms = roomsOn(doc, levelId);
  const target = { x: location.x, y: location.y };
  const targetRoom = roomAt(rooms, target);
  // A finding inside a wall still has to be looked at from one side of it:
  // ignore the wall piece it sits in when testing the line of sight.
  const seeWorld: CollisionWorld = {
    ...world,
    wallPieces: world.wallPieces.filter((piece) => !pointInPolygon(target, piece)),
  };
  const toward = rooms.length > 0 ? centerOf(rooms.map((rm) => rm.label)) : null;
  let best: { p: Pt; score: number } | null = null;
  const n = 24;
  for (let k = 0; k < n; k++) {
    const a = (k / n) * Math.PI * 2;
    const raw = { x: target.x + Math.cos(a) * FINDING_STANDOFF_MM, y: target.y + Math.sin(a) * FINDING_STANDOFF_MM };
    const p = pushOut(raw, r, world);
    const moved = Math.hypot(p.x - raw.x, p.y - raw.y);
    const room = roomAt(rooms, p);
    let score = 0;
    if (moved < 50) score += 8;
    else score -= moved / 100;
    if (room) score += 6;
    if (room && room === targetRoom) score += 3;
    if (!crossesWall(p, target, seeWorld)) score += 3;
    // Tie break: toward the middle of the building.
    if (toward) score -= Math.hypot(p.x - toward.x, p.y - toward.y) / 100000;
    if (!best || score > best.score) best = { p, score };
  }
  const stand = best?.p ?? target;
  return facing(stand, location);
}

/** A pose at `stand` looking at `location` (z above the floor) from eye height. */
export function facing(stand: Pt, location: Vec3): WalkPose {
  const dx = location.x - stand.x;
  const dy = location.y - stand.y;
  const flat = Math.hypot(dx, dy);
  const yaw = flat > 1 ? Math.atan2(dy, dx) : 0;
  const pitch = Math.max(-PITCH_LIMIT, Math.min(PITCH_LIMIT, Math.atan2(location.z - EYE_HEIGHT_MM, Math.max(flat, 1))));
  return { x: stand.x, y: stand.y, yaw, pitch };
}

function centerOf(pts: Pt[]): Pt {
  let x = 0;
  let y = 0;
  for (const p of pts) {
    x += p.x;
    y += p.y;
  }
  return { x: x / pts.length, y: y / pts.length };
}

export type HiddenReason = "wall" | "column" | "below" | "above";

/**
 * Why a point would be hidden by a solid building, or null when it is in the
 * open: inside a wall (and not in one of its door or window holes) or a
 * column, under the floor, or above the level's ceiling. `location` is plan
 * x, y and z above the level floor.
 */
export function hiddenAt(doc: DocState, levelId: string, location: Vec3): HiddenReason | null {
  const levelOf = levelResolver(doc);
  const level = levelOf(levelId);
  if (location.z < 0) return "below";
  if (level && location.z > level.height_mm) return "above";
  const p = { x: location.x, y: location.y };
  const outlines = new Map((doc.derived?.walls ?? []).map((g) => [g.wall_id, g.outline]));
  for (const e of doc.project.elements) {
    if (e.kind !== "wall" && e.kind !== "column") continue;
    if (levelOf(e.level_id)?.id !== level?.id) continue;
    if (e.kind === "wall") {
      const outline = outlines.get(e.id);
      if (outline && outline.length >= 3 && pointInPolygon(p, outline)) {
        const top = e.height_mm ?? level?.height_mm ?? Infinity;
        if (location.z <= top && !inOpening(doc, e, location)) return "wall";
      }
    } else if (e.kind === "column") {
      const inside =
        e.shape === "round"
          ? Math.hypot(p.x - e.center.x, p.y - e.center.y) <= e.width_mm / 2
          : pointInPolygon(p, orientedRect(e.center, e.width_mm, e.depth_mm, e.rotation_deg));
      if (inside) return "column";
    }
  }
  return null;
}

/** True when a point in a wall's thickness is in the hole of one of its doors or windows. */
function inOpening(doc: DocState, wall: Wall, location: Vec3): boolean {
  const dx = wall.end.x - wall.start.x;
  const dy = wall.end.y - wall.start.y;
  const len = Math.hypot(dx, dy);
  if (!(len > 1)) return false;
  const u = ((location.x - wall.start.x) * dx + (location.y - wall.start.y) * dy) / len;
  for (const o of doc.project.elements) {
    if (o.kind !== "opening" || o.wall_id !== wall.id) continue;
    if (Math.abs(u - o.offset_mm) <= o.width_mm / 2 && location.z >= o.sill_mm && location.z <= o.sill_mm + o.height_mm) return true;
  }
  return false;
}

/**
 * True when more than a third of a pipe's length would be hidden by a solid
 * building (inside walls or columns, under the floor, above the ceiling).
 */
export function pipeMostlyHidden(doc: DocState, pipe: Pipe): boolean {
  let total = 0;
  let hidden = 0;
  for (let i = 0; i + 1 < pipe.points.length; i++) {
    const a = pipe.points[i];
    const b = pipe.points[i + 1];
    const len = Math.hypot(b.x - a.x, b.y - a.y, b.z - a.z);
    if (!(len > 0)) continue;
    const n = Math.max(1, Math.ceil(len / 100));
    for (let k = 0; k < n; k++) {
      const t = (k + 0.5) / n;
      const p = { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, z: a.z + (b.z - a.z) * t };
      total += len / n;
      if (hiddenAt(doc, pipe.level_id, p)) hidden += len / n;
    }
  }
  return total > 0 && hidden / total > 1 / 3;
}
