// Door leaves in walk mode. View only: the model is never touched.
//
// While walking every door leaf rests closed. It swings open (a sliding panel
// slides open) when the walker comes within 1.2 m of the doorway facing
// roughly toward it, stays open while the walker is in the doorway, and
// closes behind the walker once it is past or has walked away. Leaving walk
// mode puts every leaf back at the ajar angle the model is drawn with. Doors
// never block walking: geom/collision.ts cuts the wall open at every door.
//
// The leaves move on their own `Animator` with motion tokens (`--dur-scene`,
// ease in-out: a leaf moves between two places) and jump under reduced
// motion. The engine samples it inside its one frame loop, so a frame is
// scheduled only while a leaf moves, and a leaf that moved redraws the sun's
// shadow map (it casts a shadow).

import type * as THREE from "three";
import type { DocState } from "../../contract/bindings";
import { dur, ease } from "../../ui/motion";
import { Animator } from "../engine/animator";
import { levelResolver, WALKER_RADIUS_MM } from "../geom/collision";
import type { Pt } from "../geom/coords";
import { closestOnSegment } from "../geom/polygon";
import { setLeafOpen, type DoorLeafTag } from "../scene/openings";

/** A door opens when the walker is this close to its doorway, facing it. */
export const DOOR_OPEN_MM = 1200;
/** Facing it: the doorway's middle within this angle of the view direction. */
export const DOOR_FACING_DEG = 70;
/** An open door stays open this much further out, */
export const DOOR_KEEP_MM = 200;
/** as long as it is not behind the walker (further round than this). */
export const DOOR_BEHIND_DEG = 100;
/** In the doorway (this close to the opening): open, whichever way the walker looks. */
export const DOORWAY_MM = WALKER_RADIUS_MM + 150;

export interface DoorInfo {
  id: string;
  levelId: string;
  /** The opening on the wall centerline, jamb to jamb. */
  a: Pt;
  b: Pt;
  center: Pt;
}

export interface DoorPose {
  x: number;
  y: number;
  /** Plan heading of the view, radians counter-clockwise from east. */
  yaw: number;
}

/** Every door of the document, on the level of its wall. */
export function doorsOf(doc: DocState | null): DoorInfo[] {
  if (!doc) return [];
  const levelOf = levelResolver(doc);
  const walls = new Map(doc.project.elements.flatMap((e) => (e.kind === "wall" ? [[e.id, e] as const] : [])));
  const out: DoorInfo[] = [];
  for (const e of doc.project.elements) {
    if (e.kind !== "opening" || e.opening_type !== "door") continue;
    const wall = walls.get(e.wall_id);
    const level = wall ? levelOf(wall.level_id) : null;
    if (!wall || !level) continue;
    const dx = wall.end.x - wall.start.x;
    const dy = wall.end.y - wall.start.y;
    const len = Math.hypot(dx, dy);
    if (!(len > 1)) continue;
    const at = (u: number): Pt => ({ x: wall.start.x + (dx / len) * u, y: wall.start.y + (dy / len) * u });
    out.push({ id: e.id, levelId: level.id, a: at(e.offset_mm - e.width_mm / 2), b: at(e.offset_mm + e.width_mm / 2), center: at(e.offset_mm) });
  }
  return out;
}

/**
 * Whether a door should be open for a walker at `pose`, given whether it is
 * open now. Opens within 1.2 m of the doorway when the walker faces it (the
 * doorway's middle within 70 degrees of the view), or when the walker is in
 * the doorway. Stays open out to 1.4 m while it is not behind the walker.
 */
export function doorWantsOpen(door: DoorInfo, pose: DoorPose, open: boolean): boolean {
  const q = closestOnSegment(pose, door.a, door.b);
  const d = Math.hypot(q.x - pose.x, q.y - pose.y);
  if (d <= DOORWAY_MM) return true;
  const tx = door.center.x - pose.x;
  const ty = door.center.y - pose.y;
  const tl = Math.hypot(tx, ty);
  const cos = tl > 1 ? (Math.cos(pose.yaw) * tx + Math.sin(pose.yaw) * ty) / tl : 1;
  const angle = (Math.acos(Math.min(Math.max(cos, -1), 1)) * 180) / Math.PI;
  if (open) return d <= DOOR_OPEN_MM + DOOR_KEEP_MM && angle <= DOOR_BEHIND_DEG;
  return d <= DOOR_OPEN_MM && angle <= DOOR_FACING_DEG;
}

interface Leaf {
  obj: THREE.Object3D;
  tag: DoorLeafTag;
}

/** Opens and closes the door leaves of the built scene around a walker. */
export class DoorSwing {
  private leaves = new Map<string, Leaf[]>();
  private doors: DoorInfo[] = [];
  private doorsDoc: DocState | null = null;
  private anim = new Animator();
  private open = new Set<string>();
  private walking = false;
  /** A leaf was put somewhere outside `sample` (a jump under reduced motion): the next frame shows it. */
  private jumped = false;

  /**
   * After every scene build: finds the tagged leaves and puts each at the
   * opening it has now, without animating, so a model change while walking
   * never snaps a door back.
   */
  attach(root: THREE.Object3D | null, doc: DocState | null): void {
    this.leaves.clear();
    root?.traverse((o) => {
      const tag = o.userData.doorLeaf as DoorLeafTag | undefined;
      const id = o.userData.elementId as string | undefined;
      if (!tag || !id) return;
      const list = this.leaves.get(id) ?? [];
      list.push({ obj: o, tag });
      this.leaves.set(id, list);
    });
    if (doc !== this.doorsDoc) {
      this.doorsDoc = doc;
      this.doors = doorsOf(doc);
    }
    for (const id of this.anim.keys()) {
      if (this.leaves.has(id)) continue;
      this.anim.remove(id);
      this.open.delete(id);
    }
    // Every door has a track from here on: a new one starts where it belongs now.
    for (const [id, leaves] of this.leaves) {
      if (!this.anim.has(id)) this.anim.set(id, this.walking ? 0 : leaves[0].tag.rest);
      this.apply(id);
    }
  }

  /** Walking: every leaf closes (or opens, near the walker). Not walking: back to ajar. */
  setWalking(on: boolean, now: number): void {
    if (on === this.walking) return;
    this.walking = on;
    this.open.clear();
    for (const [id, leaves] of this.leaves) this.retarget(id, on ? 0 : leaves[0].tag.rest, now);
  }

  /** Opens the doors the walker comes up to on its level, closes the ones it left. */
  update(pose: DoorPose, levelId: string | null, now: number): void {
    if (!this.walking) return;
    for (const door of this.doors) {
      if (!this.leaves.has(door.id)) continue;
      const was = this.open.has(door.id);
      const want = door.levelId === levelId && doorWantsOpen(door, pose, was);
      if (want === was) continue;
      if (want) this.open.add(door.id);
      else this.open.delete(door.id);
      this.retarget(door.id, want ? 1 : 0, now);
    }
  }

  /** Advances the leaves. `moved`: a leaf moved since the last frame (render, redraw shadows). `animating`: one still moves. */
  sample(now: number): { moved: boolean; animating: boolean } {
    const stepped = this.anim.sample(now);
    if (stepped) for (const id of this.anim.moved()) this.apply(id);
    const moved = stepped || this.jumped;
    this.jumped = false;
    return { moved, animating: this.anim.animating() };
  }

  /** Lands every swing where it is going. */
  finish(): void {
    this.anim.finishAll();
    for (const id of this.leaves.keys()) this.apply(id);
  }

  stats(): { open: string[]; moving: number; leaves: number } {
    let leaves = 0;
    for (const list of this.leaves.values()) leaves += list.length;
    return { open: [...this.open], moving: this.anim.active(), leaves };
  }

  /** How open a door is now, 0 to 1. */
  openness(id: string): number {
    return this.anim.value(id, 0);
  }

  private retarget(id: string, target: number, now: number): void {
    if (this.openness(id) === target) {
      // Already there: no running track, so no frames for nothing.
      this.anim.set(id, target);
      return;
    }
    this.anim.to(id, target, now, { duration: dur("scene"), easing: ease.inOut });
    // Under reduced motion the track has landed already: put the leaf there.
    if (this.anim.value(id) === target) {
      this.apply(id);
      this.jumped = true;
    }
  }

  private apply(id: string): void {
    const leaves = this.leaves.get(id);
    if (!leaves) return;
    const v = this.openness(id);
    for (const l of leaves) setLeafOpen(l.obj, l.tag, v);
  }
}
