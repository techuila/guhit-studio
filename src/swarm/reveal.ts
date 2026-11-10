// The paced reveal of a swarm draft (DECISIONS D56, D57; plan.md section 4).
// Pure: no DOM, no store, no clock of its own. Every time is the caller's
// `performance.now()`.
//
// Steps land in the draft faster than anyone can follow. The reveal gives
// each part a cursor that glides to every element its step made and draws
// it in: walls from start to end, a door leaf swinging open, a room's fill
// fading in. Times are fixed when a step arrives, so sampling the same moment
// gives the same picture to the plan, the 3D view and the cursors.
//
// Timings follow the prototype, in mm, each divided by the speed:
//   - glide 160 ms plus 0.083 ms per mm, 160 to 700 ms;
//   - draw 0.114 ms per mm, 180 to 900 ms;
//   - a 120 ms gap after each element.
// Honesty rules:
//   - nothing starts drawing before what it stands on has finished (an
//     opening's wall, a room's walls);
//   - a part more than 8 s behind takes new elements at 3x, and the panel
//     offers "Skip to the end";
//   - counts and Accept use the draft, never the reveal;
//   - with reduced motion everything shows as it arrives.

import type { Point } from "../contract/bindings";
import type { CursorTarget, RevealItem, RevealSegment } from "./types";

export const GLIDE_BASE_MS = 160;
export const GLIDE_MS_PER_MM = 0.083;
export const GLIDE_MAX_MS = 700;
export const DRAW_MS_PER_MM = 0.114;
export const DRAW_MIN_MS = 180;
export const DRAW_MAX_MS = 900;
export const GAP_MS = 120;
/** A part this far behind what landed catches up. */
export const BEHIND_MS = 8000;
/** How much faster a part that is behind takes new elements. */
export const CATCH_UP = 3;
export const SPEED_MIN = 0.5;
export const SPEED_MAX = 3;
export const SPEED_DEFAULT = 1.25;

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

function dist(a: Point, b: Point): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

/** How long the cursor glides over `distMm`. */
export function glideMs(distMm: number, speed: number): number {
  return clamp(GLIDE_BASE_MS + GLIDE_MS_PER_MM * distMm, GLIDE_BASE_MS, GLIDE_MAX_MS) / speed;
}

/** How long drawing `lengthMm` takes. */
export function drawMs(lengthMm: number, speed: number): number {
  return clamp(DRAW_MS_PER_MM * lengthMm, DRAW_MIN_MS, DRAW_MAX_MS) / speed;
}

export function gapMs(speed: number): number {
  return GAP_MS / speed;
}

export function clampSpeed(speed: number): number {
  return Number.isFinite(speed) ? clamp(speed, SPEED_MIN, SPEED_MAX) : SPEED_DEFAULT;
}

interface Entry {
  item: RevealItem;
  /** Arrival order across every part, so a reschedule keeps it. */
  order: number;
  /** Where the cursor was when this entry began; null for a part's first: it appears in place. */
  start: Point | null;
  glideStart: number;
  glideEnd: number;
  drawStart: number;
  drawEnd: number;
  restEnd: number;
  /** The id came in before: it stays shown, the cursor only visits. */
  revisit: boolean;
  /** The step this entry belongs to (index into the lane's batches). */
  batch: number;
}

interface Lane {
  entries: Entry[];
  /** When the cursor is free for the next element. */
  free: number;
  /** Where the cursor rests after its last entry. */
  cursor: Point | null;
  label: string;
  levelId: string | null;
  /** Per step that arrived: when its last element finishes drawing. */
  batches: number[];
  /** Steps the window found already in the draft: shown as they are. */
  known: number;
}

export class Reveal {
  private lanes = new Map<string, Lane>();
  /** Each id's first entry: its draw is what shows the element. */
  private first = new Map<string, Entry>();
  /** Every id that ever came in, so a later step that touches it again does not hide it. */
  private seen = new Set<string>();
  private seq = 0;
  private speedValue = SPEED_DEFAULT;

  /** `reduced` is asked at each arrival: true shows elements as they arrive. */
  constructor(private readonly reduced: () => boolean = () => false) {}

  get speed(): number {
    return this.speedValue;
  }

  /** Forgets everything: a new swarm, or none. */
  reset(): void {
    this.lanes.clear();
    this.first.clear();
    this.seen.clear();
    this.seq = 0;
  }

  /** Part keys in the order they first came in. */
  parts(): string[] {
    return [...this.lanes.keys()];
  }

  /**
   * What was in the draft before the window saw it: shown at once, never
   * drawn in. `steps` counts toward the part's revealed steps.
   */
  known(part: string, steps: number, ids: readonly string[]): void {
    const lane = this.lane(part);
    lane.known += steps;
    for (const id of ids) this.seen.add(id);
  }

  /** One step of `part` landed: pace its elements. Elements of one step must all be `part`'s. */
  arrive(part: string, items: readonly RevealItem[], now: number): void {
    const lane = this.lane(part);
    const batch = lane.batches.length;
    lane.batches.push(now);
    for (const item of hostsFirst(items)) this.place(item, now, this.seq++, batch);
    let end = now;
    for (const e of lane.entries) if (e.batch === batch) end = Math.max(end, e.drawEnd);
    lane.batches[batch] = end;
  }

  /**
   * The reveal speed, 0.5 to 3. Elements already drawing keep their times;
   * the ones waiting are scheduled again from `now` at the new speed.
   */
  setSpeed(speed: number, now: number): void {
    const next = clampSpeed(speed);
    if (next === this.speedValue) return;
    this.speedValue = next;
    const waiting: Entry[] = [];
    for (const lane of this.lanes.values()) {
      const keep = lane.entries.filter((e) => e.glideStart <= now);
      const moved = lane.entries.filter((e) => e.glideStart > now);
      if (moved.length === 0) continue;
      waiting.push(...moved);
      lane.entries = keep;
      const last = keep[keep.length - 1];
      lane.free = last ? last.restEnd : now;
      lane.cursor = last ? last.item.to : moved[0].start;
      for (const e of moved) {
        if (this.first.get(e.item.id) === e) {
          this.first.delete(e.item.id);
          this.seen.delete(e.item.id);
        }
      }
    }
    waiting.sort((a, b) => a.order - b.order);
    for (const e of waiting) this.place(e.item, now, e.order, e.batch);
    for (const lane of this.lanes.values()) {
      for (let b = 0; b < lane.batches.length; b++) {
        let end = -Infinity;
        for (const e of lane.entries) if (e.batch === b) end = Math.max(end, e.drawEnd);
        if (end > -Infinity) lane.batches[b] = end;
      }
    }
  }

  /**
   * How far an element is drawn at `now`: 0 before it starts, 1 once drawn.
   * An element the reveal never paced is 1: it shows as it is.
   */
  progress(id: string, now: number): number {
    const e = this.first.get(id);
    if (!e || now >= e.drawEnd) return 1;
    if (now < e.drawStart) return 0;
    const d = e.drawEnd - e.drawStart;
    return d <= 0 ? 1 : (now - e.drawStart) / d;
  }

  /** True while any element is being drawn in: the plan needs frames. */
  busy(now: number): boolean {
    for (const lane of this.lanes.values()) {
      for (const e of lane.entries) if (e.drawStart <= now && now < e.drawEnd) return true;
    }
    return false;
  }

  /** True while any cursor still has work queued. */
  pending(now: number): boolean {
    for (const lane of this.lanes.values()) if (lane.free > now) return true;
    return false;
  }

  /** How far behind what landed a part is (or the furthest behind of all), ms. */
  behindMs(now: number, part?: string): number {
    let most = 0;
    for (const [key, lane] of this.lanes) {
      if (part !== undefined && key !== part) continue;
      most = Math.max(most, lane.free - now);
    }
    return most;
  }

  /** Steps of `part` whose elements are all drawn at `now`. */
  revealedSteps(part: string, now: number): number {
    const lane = this.lanes.get(part);
    if (!lane) return 0;
    let n = lane.known;
    for (const end of lane.batches) if (end <= now) n++;
    return n;
  }

  /** Everything still to come shows at `now`. */
  skip(now: number): void {
    for (const lane of this.lanes.values()) {
      for (const e of lane.entries) {
        if (e.restEnd <= now) continue;
        e.glideStart = Math.min(e.glideStart, now);
        e.glideEnd = Math.min(e.glideEnd, now);
        e.drawStart = Math.min(e.drawStart, now);
        e.drawEnd = Math.min(e.drawEnd, now);
        e.restEnd = now;
      }
      if (lane.free > now) lane.free = now;
      for (let b = 0; b < lane.batches.length; b++) if (lane.batches[b] > now) lane.batches[b] = now;
    }
  }

  /** The next moment any cursor changes what it does, or null when all rest. */
  nextBoundary(now: number): number | null {
    let next = Infinity;
    for (const lane of this.lanes.values()) {
      this.prune(lane, now);
      for (const e of lane.entries) {
        for (const t of [e.glideStart, e.glideEnd, e.drawStart, e.drawEnd, e.restEnd]) if (t > now && t < next) next = t;
      }
    }
    return next === Infinity ? null : next;
  }

  /** What `part`'s cursor does at `now`, or null before its first element. */
  segment(part: string, now: number): RevealSegment | null {
    const lane = this.lanes.get(part);
    if (!lane || lane.cursor === null) return null;
    let prev: Entry | null = null;
    for (const e of lane.entries) {
      if (now >= e.restEnd) {
        prev = e;
        continue;
      }
      const it = e.item;
      const seg = (kind: RevealSegment["kind"], start: number, end: number, from: Point, to: Point): RevealSegment => ({
        part,
        kind,
        id: it.id,
        start,
        end,
        from,
        to,
        label: it.label,
        levelId: it.levelId,
      });
      const at = e.start ?? it.from;
      if (now < e.glideStart) {
        // Idle until this step arrived: resting where the last one ended.
        return { part, kind: "rest", id: null, start: prev?.restEnd ?? -Infinity, end: e.glideStart, from: at, to: at, label: prev?.item.label ?? "", levelId: prev?.item.levelId ?? it.levelId };
      }
      if (now < e.glideEnd) return seg("glide", e.glideStart, e.glideEnd, at, it.from);
      if (now < e.drawStart) return seg("wait", e.glideEnd, e.drawStart, it.from, it.from);
      if (now < e.drawEnd) return seg("draw", e.drawStart, e.drawEnd, it.from, it.to);
      return seg("rest", e.drawEnd, e.restEnd, it.to, it.to);
    }
    return { part, kind: "rest", id: null, start: prev?.restEnd ?? -Infinity, end: Infinity, from: lane.cursor, to: lane.cursor, label: lane.label, levelId: lane.levelId };
  }

  /**
   * Where `part`'s cursor should head at `now`, for a CSS transition: the
   * segment's end point, over what is left of it. A glide eases in and out,
   * a draw is linear, a wait or a rest stays put.
   */
  target(part: string, now: number): CursorTarget | null {
    const seg = this.segment(part, now);
    if (!seg) return null;
    const moving = seg.kind === "glide" || seg.kind === "draw";
    return {
      part,
      at: seg.to,
      ms: moving ? Math.max(0, seg.end - now) : 0,
      easing: seg.kind === "glide" ? "glide" : "linear",
      label: seg.label,
      levelId: seg.levelId,
    };
  }

  // ------------------------------------------------------------ internals

  private lane(part: string): Lane {
    let lane = this.lanes.get(part);
    if (!lane) {
      lane = { entries: [], free: -Infinity, cursor: null, label: "", levelId: null, batches: [], known: 0 };
      this.lanes.set(part, lane);
    }
    return lane;
  }

  private place(item: RevealItem, now: number, order: number, batch: number): void {
    const lane = this.lane(item.part);
    const revisit = this.seen.has(item.id);
    let hostReady = -Infinity;
    for (const h of item.hosts) {
      const host = this.first.get(h);
      if (host) hostReady = Math.max(hostReady, host.drawEnd);
    }
    const start = lane.cursor;
    let entry: Entry;
    if (this.reduced()) {
      const t = Math.max(now, hostReady);
      entry = { item, order, start, glideStart: t, glideEnd: t, drawStart: t, drawEnd: t, restEnd: t, revisit, batch };
    } else {
      const base = Math.max(now, lane.free);
      const behind = lane.free - now > BEHIND_MS;
      const speed = this.speedValue * (behind ? CATCH_UP : 1);
      const g = start ? glideMs(dist(start, item.from), speed) : 0;
      const d = drawMs(revisit ? 0 : item.lengthMm, speed);
      const glideEnd = base + g;
      const drawStart = Math.max(glideEnd, hostReady);
      const drawEnd = drawStart + d;
      entry = { item, order, start, glideStart: base, glideEnd, drawStart, drawEnd, restEnd: drawEnd + gapMs(speed), revisit, batch };
    }
    lane.entries.push(entry);
    lane.free = entry.restEnd;
    lane.cursor = item.to;
    lane.label = item.label;
    lane.levelId = item.levelId;
    if (!revisit) {
      this.first.set(item.id, entry);
      this.seen.add(item.id);
    }
  }

  /** Drops entries long finished. Their ids read as drawn (progress 1) either way. */
  private prune(lane: Lane, now: number): void {
    let drop = 0;
    while (drop < lane.entries.length - 1 && lane.entries[drop].restEnd <= now) drop++;
    if (drop === 0) return;
    for (const e of lane.entries.slice(0, drop)) if (this.first.get(e.item.id) === e) this.first.delete(e.item.id);
    lane.entries = lane.entries.slice(drop);
  }
}

/**
 * Orders one step's elements so each comes after the ones it stands on that
 * the same step made (a wall before its door), keeping the step's order
 * otherwise.
 */
export function hostsFirst(items: readonly RevealItem[]): RevealItem[] {
  const ids = new Set(items.map((i) => i.id));
  const out: RevealItem[] = [];
  const placed = new Set<string>();
  const visiting = new Set<string>();
  const byId = new Map(items.map((i) => [i.id, i]));
  const visit = (it: RevealItem) => {
    if (placed.has(it.id) || visiting.has(it.id)) return;
    visiting.add(it.id);
    for (const h of it.hosts) {
      const host = ids.has(h) ? byId.get(h) : undefined;
      if (host) visit(host);
    }
    visiting.delete(it.id);
    placed.add(it.id);
    out.push(it);
  };
  for (const it of items) visit(it);
  return out;
}
