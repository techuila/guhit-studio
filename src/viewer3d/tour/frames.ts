// A tour's frames as the engine works them out (`Query::TourFrames`,
// guhit-core tours.rs): the one source for playback, the timeline, the path
// preview and the video export, so all of them agree. Fetched once per tour,
// revision and frame rate, then cached.

import type { Query, Shot, TransitionKind, Vec3, ViewLight } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";
import { useApp } from "../../state/store";

/** How a frame is shown while a transition runs (tours.rs, `Fx`). */
export interface TourFx {
  kind: Exclude<TransitionKind, "cut">;
  /** 0 to 1 through the transition, eased for dissolves and dips. */
  mix: number;
  /** Whip pan: the sideways smear, 0 to 1. */
  blur?: number;
  /** Dissolves and dips: the frame held under this one. None: from black (or the dip color). */
  from_frame?: number;
}

export interface TourFrame {
  t: number;
  shot: number;
  position: Vec3;
  target: Vec3;
  fov_deg: number;
  light?: ViewLight;
  fx?: TourFx;
}

export interface TourShotSpan {
  id: string;
  name: string;
  move: Shot["kind"];
  start: number;
  seconds: number;
  transition: Shot["transition"];
  first_frame: number | null;
  frame_count: number;
}

/**
 * A door the tour moves in the view only (tours.rs, `door_moves`): from
 * where it was to `open` between `from` and `to` seconds, eased, then held.
 * Before a door's first move its leaf rests as drawn. The model is never
 * touched: a tour is a view.
 */
export interface TourDoor {
  /** The door opening's id. */
  id: string;
  from: number;
  to: number;
  /** 0 closed, 1 a swing leaf at 90 degrees (more turns it on round toward the wall, up to 165) or a sliding panel all the way over the fixed one. */
  open: number;
}

export interface TourFrames {
  tour_id: string;
  name: string;
  fps: number;
  seconds: number;
  frame_count: number;
  shots: TourShotSpan[];
  frames: TourFrame[];
  /** Doors opened for the camera to pass, in time order per door. */
  doors?: TourDoor[];
}

/** A door at a moment of a tour: between `from` (null: as drawn) and `to`, `k` of the way. */
export interface DoorAt {
  from: number | null;
  to: number;
  k: number;
}

/** 0 to 1, smooth at both ends: the swing of a door, as tours.rs eases it. */
export function smoothstep(w: number): number {
  const x = Math.min(Math.max(w, 0), 1);
  return x * x * (3 - 2 * x);
}

/**
 * Every door the tour has moved by `t` seconds, by opening id. Playback and
 * the video both read it, so a door swings the same in each. Doors not in
 * the map rest as drawn.
 */
export function doorsAt(data: Pick<TourFrames, "doors">, t: number): Map<string, DoorAt> {
  const byId = new Map<string, TourDoor[]>();
  for (const d of data.doors ?? []) {
    const list = byId.get(d.id) ?? [];
    list.push(d);
    byId.set(d.id, list);
  }
  const out = new Map<string, DoorAt>();
  for (const [id, moves] of byId) {
    moves.sort((a, b) => a.from - b.from);
    let held: number | null = null;
    let at: DoorAt | null = null;
    for (const m of moves) {
      if (t >= m.to) {
        held = m.open;
        continue;
      }
      if (t > m.from) at = { from: held, to: m.open, k: smoothstep((t - m.from) / Math.max(m.to - m.from, 1e-6)) };
      break;
    }
    if (at) out.set(id, at);
    else if (held !== null) out.set(id, { from: held, to: held, k: 1 });
  }
  return out;
}

const cache = new Map<string, Promise<TourFrames>>();
let cacheRev = -1;

/** The frames of a tour at the open document's revision. `fps` null: the tour's own. */
export function tourFrames(tourId: string, fps: number | null = null): Promise<TourFrames> {
  const rev = useApp.getState().doc?.revision ?? -1;
  const key = `${tourId}@${rev}@${fps ?? "own"}`;
  // Older revisions are never asked for again.
  if (rev !== cacheRev) {
    cache.clear();
    cacheRev = rev;
  }
  let p = cache.get(key);
  if (!p) {
    const query = { type: "tour_frames", tour_id: tourId, fps } as Query;
    p = ipc.docQuery(query).then((v) => v as TourFrames);
    p.catch(() => cache.delete(key));
    cache.set(key, p);
  }
  return p;
}

/** The frame at a time, seconds, clamped to the tour. */
export function frameIndexAt(data: Pick<TourFrames, "fps" | "frame_count">, t: number): number {
  if (data.frame_count === 0) return 0;
  return Math.min(Math.max(Math.floor(t * data.fps + 1e-6), 0), data.frame_count - 1);
}

function lerp(a: number, b: number, k: number): number {
  return a + (b - a) * k;
}

function lerp3(a: Vec3, b: Vec3, k: number): Vec3 {
  return { x: lerp(a.x, b.x, k), y: lerp(a.y, b.y, k), z: lerp(a.z, b.z, k) };
}

/**
 * The camera between two frames at time t, for a display faster than the
 * tour's frame rate. Never blends across shots or into a transition: a jump
 * stays a jump.
 */
export function poseAt(data: TourFrames, t: number): { frame: TourFrame; index: number; position: Vec3; target: Vec3; fov: number } {
  const n = data.frames.length;
  const f = Math.max(0, t * data.fps);
  const i = Math.min(Math.floor(f), n - 1);
  const a = data.frames[i];
  const b = data.frames[Math.min(i + 1, n - 1)];
  const k = Math.min(Math.max(f - i, 0), 1);
  if (i + 1 >= n || a.shot !== b.shot || a.fx?.kind === "whip_pan" || b.fx?.kind === "whip_pan") {
    return { frame: a, index: i, position: a.position, target: a.target, fov: a.fov_deg };
  }
  return { frame: a, index: i, position: lerp3(a.position, b.position, k), target: lerp3(a.target, b.target, k), fov: lerp(a.fov_deg, b.fov_deg, k) };
}

/** What lies over a frame's picture while a transition runs: the held frame's opacity, a color and its opacity, the plan's opacity, the smear. */
export interface FxLayers {
  held: number;
  color: "black" | "white" | null;
  colorAlpha: number;
  plan: number;
  blur: number;
}

/** The overlay of a frame (tours.rs `Fx`), the same in playback and in the export. */
export function fxLayers(fx: TourFx | undefined): FxLayers {
  const none: FxLayers = { held: 0, color: null, colorAlpha: 0, plan: 0, blur: 0 };
  if (!fx) return none;
  const m = Math.min(Math.max(fx.mix, 0), 1);
  switch (fx.kind) {
    case "dissolve":
      // From the held frame, or from black at the very start.
      return fx.from_frame !== undefined ? { ...none, held: 1 - m } : { ...none, color: "black", colorAlpha: 1 - m };
    case "dip_black":
    case "dip_white": {
      const color = fx.kind === "dip_black" ? "black" : "white";
      if (m < 0.5) return fx.from_frame !== undefined ? { ...none, held: 1, color, colorAlpha: m * 2 } : { ...none, color, colorAlpha: 1 };
      return { ...none, color, colorAlpha: (1 - m) * 2 };
    }
    case "whip_pan":
      return { ...none, blur: fx.blur ?? 0 };
    case "plan_lift":
      // The plan fades out over the first 60 percent of the lift.
      return { ...none, plan: Math.max(0, 1 - m / 0.6) };
  }
}

/** Frames that a later dissolve or dip holds, so the export keeps their pictures. */
export function heldFrames(data: TourFrames): Set<number> {
  const out = new Set<number>();
  for (const f of data.frames) if (f.fx?.from_frame !== undefined) out.add(f.fx.from_frame);
  return out;
}
