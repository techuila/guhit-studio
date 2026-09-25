// Which lamps a render keeps (DECISIONS D23, docs/CONTRACT.md "Render").
//
// three-gpu-pathtracer 0.0.24 picks one light per bounce, uniformly among all
// the lights it has (and the sky). A lamp in another room still takes its
// share of those picks and, seen through walls, gives nothing back: in a
// house with ten lamps a room lit by one gets a tenth of the light samples it
// could. So a render keeps only the lamps that can light what the camera
// sees:
// - lamps in the camera's room;
// - lamps in rooms reached through an opening (a door or a window) of that
//   room, and the lamps outdoors when the room has an opening to the outside;
// - any lamp inside the camera's view.
// A lamp two rooms away, behind two doors, adds light too faint to see. A
// camera outdoors keeps the outdoor lamps and the lamps of every room with
// an opening to the outside, on every level: they light the windows.
// Lamps the rules cannot place (no room known) are kept. A camera standing in
// a doorway belongs to the rooms on both sides. "In view" means seen: inside
// the view and not behind a wall (the caller's test).
//
// Of the lamps kept, the ones in the camera's room light most of what it
// sees, so the tracer should pick them more often. It cannot be told to, but
// a lamp given to it k times at 1/k of its intensity is picked k times as
// often and adds up to the same light: the picks lean towards the room
// without changing the picture (`lightCopies`).

import type { DocState } from "../../contract/bindings";
import { pointInPolygon } from "../geom/polygon";
import type { Pt } from "../geom/coords";

/** The open air, as a room id. */
export const OUTSIDE = "outside";

/** A room's net floor polygon on its level, plan mm. */
export interface RoomArea {
  id: string;
  levelId: string;
  polygon: Pt[];
}

/** A door or window: its middle, the unit normal of its wall, how far to look past each face, half its width, plan mm. */
export interface OpeningSpot {
  levelId: string;
  center: Pt;
  normal: Pt;
  reach: number;
  halfWidth: number;
}

export interface Storey {
  id: string;
  elevationMm: number;
  heightMm: number;
}

/** A light as the selection sees it. */
export interface LightSpot {
  key: string;
  /** Room ids the light belongs to (several for fixtures sharing one light); OUTSIDE outdoors. Empty: unknown. */
  rooms: string[];
  levelId: string | null;
  /** World meters. */
  position: [number, number, number];
}

/** How far past the wall face an opening looks for a room, mm. */
const PROBE_MM = 250;
/** Copies of a lamp in the camera's room: picked this many times as often as a lamp in the next room. */
export const ROOM_WEIGHT = 4;
/** Most lights the tracer gets, copies included: every light costs it a few texture reads per bounce. */
export const MAX_TRACER_LIGHTS = 16;

/** Rooms, openings and levels of a document, plan mm. */
export function planOf(doc: DocState): { rooms: RoomArea[]; openings: OpeningSpot[]; storeys: Storey[] } {
  const project = doc.project;
  const levelOfRoom = new Map<string, string>();
  const walls = new Map<string, Extract<DocState["project"]["elements"][number], { kind: "wall" }>>();
  const openings: OpeningSpot[] = [];
  for (const e of project.elements) {
    if (e.kind === "room") levelOfRoom.set(e.id, e.level_id);
    else if (e.kind === "wall") walls.set(e.id, e);
  }
  const rooms: RoomArea[] = [];
  for (const g of doc.derived.rooms) {
    const levelId = levelOfRoom.get(g.room_id);
    if (!levelId || g.polygon.length < 3) continue;
    rooms.push({ id: g.room_id, levelId, polygon: g.polygon });
  }
  for (const e of project.elements) {
    if (e.kind !== "opening") continue;
    const w = walls.get(e.wall_id);
    if (!w) continue;
    const dx = w.end.x - w.start.x;
    const dy = w.end.y - w.start.y;
    const len = Math.hypot(dx, dy);
    if (!(len > 1)) continue;
    const ux = dx / len;
    const uy = dy / len;
    openings.push({
      levelId: w.level_id,
      center: { x: w.start.x + ux * e.offset_mm, y: w.start.y + uy * e.offset_mm },
      normal: { x: -uy, y: ux },
      reach: Math.max(w.thickness_mm, 0) / 2 + PROBE_MM,
      halfWidth: Math.max(e.width_mm, 0) / 2,
    });
  }
  const storeys = project.levels.map((l) => ({ id: l.id, elevationMm: l.elevation_mm, heightMm: l.height_mm }));
  return { rooms, openings, storeys };
}

/** The room a plan point is in on a level, or OUTSIDE. */
export function roomAt(rooms: readonly RoomArea[], levelId: string, p: Pt): string {
  return rooms.find((r) => r.levelId === levelId && pointInPolygon(p, r.polygon))?.id ?? OUTSIDE;
}

/** The level whose floor to floor span holds a height, mm, or null (above or below every level). */
export function storeyAt(storeys: readonly Storey[], zMm: number): string | null {
  let best: Storey | null = null;
  for (const s of storeys) {
    if (zMm >= s.elevationMm - 1 && zMm < s.elevationMm + s.heightMm && (!best || s.elevationMm > best.elevationMm)) best = s;
  }
  return best?.id ?? null;
}

/**
 * Rooms joined by openings: for each room, the rooms (or OUTSIDE) on the
 * other side of its doors and windows. OUTSIDE gets every room with an
 * opening to the outside, on any level.
 */
export function roomLinks(rooms: readonly RoomArea[], openings: readonly OpeningSpot[]): Map<string, Set<string>> {
  const links = new Map<string, Set<string>>();
  const link = (a: string, b: string) => {
    if (a === b) return;
    if (!links.has(a)) links.set(a, new Set());
    if (!links.has(b)) links.set(b, new Set());
    links.get(a)!.add(b);
    links.get(b)!.add(a);
  };
  for (const o of openings) link(openingSide(rooms, o, 1), openingSide(rooms, o, -1));
  return links;
}

/** The room (or OUTSIDE) on one side of an opening: `k` 1 for the side its normal points to, -1 for the other. */
function openingSide(rooms: readonly RoomArea[], o: OpeningSpot, k: number): string {
  return roomAt(rooms, o.levelId, { x: o.center.x + o.normal.x * o.reach * k, y: o.center.y + o.normal.y * o.reach * k });
}

/**
 * The rooms a camera is in: the room under it on its level, or, standing in
 * a door or window opening, the rooms on both sides. OUTSIDE above or below
 * every level and in the open air.
 */
export function cameraRooms(
  plan: { rooms: readonly RoomArea[]; openings: readonly OpeningSpot[]; storeys: readonly Storey[] },
  p: Pt,
  zMm: number,
): { levelId: string | null; rooms: string[] } {
  const levelId = storeyAt(plan.storeys, zMm);
  if (levelId === null) return { levelId, rooms: [OUTSIDE] };
  const here = roomAt(plan.rooms, levelId, p);
  if (here !== OUTSIDE) return { levelId, rooms: [here] };
  for (const o of plan.openings) {
    if (o.levelId !== levelId) continue;
    const dx = p.x - o.center.x;
    const dy = p.y - o.center.y;
    const across = Math.abs(dx * o.normal.x + dy * o.normal.y);
    const along = Math.abs(dx * -o.normal.y + dy * o.normal.x);
    if (across <= o.reach && along <= o.halfWidth) {
      return { levelId, rooms: [...new Set([openingSide(plan.rooms, o, 1), openingSide(plan.rooms, o, -1)])] };
    }
  }
  return { levelId, rooms: [OUTSIDE] };
}

/**
 * The lights to keep, by key: see the rules at the top of this file.
 * `inView` says whether a light at a world point is seen by the camera.
 */
export function selectLights(
  lights: readonly LightSpot[],
  camera: { rooms: readonly string[]; levelId: string | null },
  links: ReadonlyMap<string, ReadonlySet<string>>,
  inView: (p: [number, number, number]) => boolean,
): Set<string> {
  return new Set([...lightCopies(lights, camera, links, inView, 1)].filter(([, n]) => n > 0).map(([k]) => k));
}

/**
 * How many copies of each light the tracer gets, by key: 0 drops it, 1 keeps
 * it, more make it picked more often (see the top of this file). Lamps in
 * the camera's own room get `roomWeight` copies, fewer when that would pass
 * MAX_TRACER_LIGHTS; a camera outdoors weighs no lamp up.
 */
export function lightCopies(
  lights: readonly LightSpot[],
  camera: { rooms: readonly string[]; levelId: string | null },
  links: ReadonlyMap<string, ReadonlySet<string>>,
  inView: (p: [number, number, number]) => boolean,
  roomWeight = ROOM_WEIGHT,
): Map<string, number> {
  const out = new Map<string, number>();
  const near = new Set<string>();
  for (const room of camera.rooms) {
    near.add(room);
    for (const other of links.get(room) ?? []) near.add(other);
  }
  const outdoors = camera.rooms.includes(OUTSIDE);
  const own: string[] = [];
  let kept = 0;
  for (const l of lights) {
    // Indoors, rooms are joined on the camera's level only; the open air
    // joins every level.
    const sameLevel = camera.levelId === null || l.levelId === null || l.levelId === camera.levelId;
    const reached = l.rooms.some((room) => near.has(room) && (outdoors || room === OUTSIDE || sameLevel));
    const keep = l.rooms.length === 0 || reached || inView(l.position);
    out.set(l.key, keep ? 1 : 0);
    if (!keep) continue;
    kept++;
    if (!outdoors && sameLevel && l.rooms.some((room) => camera.rooms.includes(room))) own.push(l.key);
  }
  if (own.length > 0 && roomWeight > 1) {
    const room = Math.max(1, Math.min(roomWeight, Math.floor((MAX_TRACER_LIGHTS - (kept - own.length)) / own.length)));
    for (const key of own) out.set(key, room);
  }
  return out;
}

