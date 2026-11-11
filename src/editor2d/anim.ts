// Animated values for the plan canvas. Pure and time driven: nothing here
// reads the clock, the DOM or the document, so any state can be sampled at any
// time in a test. The controller ticks it once per frame and samples while it
// draws. Tested in anim.test.ts.
//
// Contract with callers:
//   - `value(key, rest)` returns `rest` when the key has no track.
//   - pass `drop: true` only when the target equals that same rest value, so
//     forgetting the finished track cannot change what is drawn.
//   - a duration of 0 (what `dur()` returns under reduced motion) lands on the
//     target immediately and keeps it there.

import type { P } from "./geom";

export type Easing = (x: number) => number;

export interface Track {
  from: number;
  to: number;
  /** Clock value the track starts at. A start in the future is a delay. */
  start: number;
  /** Milliseconds. 0 means the value is already at `to`. */
  duration: number;
  easing: Easing;
  /** Forgotten once finished, so the key falls back to its rest value. */
  drop: boolean;
}

export function sampleTrack(t: Track, now: number): number {
  const x = t.duration > 0 ? (now - t.start) / t.duration : 1;
  if (x <= 0) return t.from;
  if (x >= 1) return t.to;
  return t.from + (t.to - t.from) * t.easing(x);
}

export function trackDone(t: Track, now: number): boolean {
  return now >= t.start + t.duration;
}

export interface AnimSpec {
  easing?: Easing;
  /** Value to start from when the key is not animating yet. Default: the target. */
  from?: number;
  /** Milliseconds to wait before moving. Used for staggers. */
  delayMs?: number;
  /** Forget the track once it finishes. Only when the target is the rest value. */
  drop?: boolean;
}

const linear: Easing = (x) => x;

/** A registry of named animated numbers. One instance lives on the controller. */
export class Anim {
  private tracks = new Map<string, Track>();
  private clock = 0;

  /** Current clock value, in the same units as the times passed to `tick`. */
  get now(): number {
    return this.clock;
  }

  /** Moves the clock forward without touching tracks. Call before starting one from an event. */
  setClock(now: number): void {
    if (now > this.clock) this.clock = now;
  }

  /** Advances the clock, forgets finished throwaway tracks, returns how many still run. */
  tick(now: number): number {
    this.setClock(now);
    let running = 0;
    for (const [key, t] of this.tracks) {
      if (!trackDone(t, this.clock)) running++;
      else if (t.drop) this.tracks.delete(key);
    }
    return running;
  }

  /** How many tracks have not finished at the current clock. */
  running(): number {
    let n = 0;
    for (const t of this.tracks.values()) if (!trackDone(t, this.clock)) n++;
    return n;
  }

  /** Total tracks held, finished ones included. */
  size(): number {
    return this.tracks.size;
  }

  has(key: string): boolean {
    return this.tracks.has(key);
  }

  /** Value of `key` at the current clock, or `rest` when there is no track. */
  value(key: string, rest = 0): number {
    const t = this.tracks.get(key);
    return t ? sampleTrack(t, this.clock) : rest;
  }

  /**
   * Starts `key` moving to `to`, or retargets it from wherever it is now.
   * Calling it again with the same target is a no op, so it is safe to call
   * every frame or on every pointer move.
   */
  to(key: string, to: number, ms: number, spec: AnimSpec = {}): void {
    const cur = this.tracks.get(key);
    let from: number;
    if (cur) {
      if (cur.to === to) return;
      from = sampleTrack(cur, this.clock);
    } else {
      from = spec.from ?? to;
      if (from === to) {
        // Nothing to move. Park the key at rest so a later change to another
        // target still has somewhere to animate from (a door ghost flipping).
        this.set(key, to);
        return;
      }
    }
    const duration = Math.max(0, ms);
    this.tracks.set(key, {
      from,
      to,
      start: this.clock + (duration > 0 ? Math.max(0, spec.delayMs ?? 0) : 0),
      duration,
      easing: spec.easing ?? linear,
      drop: spec.drop ?? false,
    });
  }

  /** Jumps `key` straight to `v` with no motion. */
  set(key: string, v: number): void {
    this.tracks.set(key, { from: v, to: v, start: this.clock, duration: 0, easing: linear, drop: false });
  }

  clear(key: string): void {
    this.tracks.delete(key);
  }

  clearPrefix(prefix: string): void {
    for (const key of this.tracks.keys()) if (key.startsWith(prefix)) this.tracks.delete(key);
  }

  clearAll(): void {
    this.tracks.clear();
  }

  /** Visits every key that starts with `prefix`, with the key tail and its value. */
  each(prefix: string, fn: (id: string, value: number) => void): void {
    for (const [key, t] of this.tracks) {
      if (!key.startsWith(prefix)) continue;
      fn(key.slice(prefix.length), sampleTrack(t, this.clock));
    }
  }

  /** Dev and test helper: every key with its current value. */
  snapshot(): Record<string, number> {
    const out: Record<string, number> = {};
    for (const [key, t] of this.tracks) out[key] = sampleTrack(t, this.clock);
    return out;
  }
}

/**
 * The AI proposal ghost's alpha (docs/MOTION.md rule 11), the same curve as
 * the 3D view's (`src/viewer3d/engine/breath.ts` `breathCurve`), so Split
 * view breathes in step: it starts at `lo`, takes `breaths` cosine breaths of
 * `periodMs` up to `hi` and back, and the last half breath settles on `hold`
 * instead of falling back, so nothing jumps when it ends. Then a steady
 * `hold`, so the render loop can stop. A steady `hold` when motion is off.
 */
export function ghostAlpha(now: number, since: number, periodMs: number, lo: number, hi: number, hold: number, breaths = 2, animated = true): number {
  if (!animated || periodMs <= 0 || breaths <= 0) return hold;
  const t = now - since;
  // The first frame may sample a clock just short of the arrival: it starts low too.
  if (t <= 0) return lo;
  const x = t / (breaths * periodMs);
  if (x >= 1) return hold;
  // On the scale where 0 is `lo` and 1 is `hold`: the peak, and where the last half breath starts.
  const peak = (hi - lo) / (hold - lo);
  const last = 1 - 1 / (2 * breaths);
  const y =
    x < last ? peak * (0.5 - 0.5 * Math.cos(x * 2 * breaths * Math.PI)) : 1 + (peak - 1) * (0.5 + 0.5 * Math.cos(((x - last) / (1 - last)) * Math.PI));
  return lo + (hold - lo) * y;
}

/**
 * True while `ghostAlpha` still moves, so the frame loop runs only that long.
 * A clock still short of `since` (the first frame after the proposal) counts:
 * the loop must stay awake for the breaths to start.
 */
export function ghostBreathing(now: number, since: number, periodMs: number, breaths = 2, animated = true): boolean {
  return animated && periodMs > 0 && now - since < breaths * periodMs;
}

/**
 * Render on demand, one frame at a time: draw while the canvas is dirty,
 * something moves or loops, and once more on the frame where the last track
 * lands or is dropped (`wasRunning`), so the plan always rests on the end
 * state and never on a frame from the middle of a fade. Ask for another frame
 * only while something still moves.
 */
export function frameStep(s: { dirty: boolean; running: number; looping: boolean; wasRunning: boolean }): { draw: boolean; again: boolean } {
  const moving = s.running > 0 || s.looping;
  return { draw: s.dirty || moving || s.wasRunning, again: moving };
}

export function mix(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

export function mixP(a: P, b: P, t: number): P {
  return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
}
