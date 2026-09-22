// Animation bookkeeping for the 3D view. Pure: no three.js, no DOM, no timers.
// One animated scalar per key, each with a start time, from, to, duration and
// easing. The engine retargets tracks mid-flight, samples them once per frame
// and stops its frame loop the moment `active()` reaches 0, so the viewer
// stays render on demand.
//
// Durations come from src/ui/motion.ts (`dur`), never from raw numbers, and
// `motion` is `motionOK` by default: with reduced motion every track jumps
// straight to its target and a looping track holds a steady value.

import { ease, motionOK } from "../../ui/motion";

export type Easing = (x: number) => number;

export interface TrackOptions {
  /** Milliseconds. 0 or less jumps. */
  duration: number;
  easing?: Easing;
  /** Wait this long before the value starts moving. For staggered entrances. */
  delay?: number;
  /**
   * Swings between `from` and `to` forever, one period being `duration * 2`.
   * Reserved for the AI preview breathing: the only looping motion in 3D.
   */
  loop?: boolean;
  /** Fires once when the track lands. Not called on a jump-free cancel. */
  onDone?: () => void;
}

interface Track {
  from: number;
  to: number;
  start: number;
  duration: number;
  delay: number;
  easing: Easing;
  loop: boolean;
  value: number;
  running: boolean;
  /** Parked at a value by `hold`. `finish` still lands it. */
  held: boolean;
  onDone?: () => void;
}

export class Animator {
  private tracks = new Map<string, Track>();
  private running = 0;

  constructor(private motion: () => boolean = motionOK) {}

  /** Current value of a key, or `fallback` when the key is unknown. */
  value(key: string, fallback = 0): number {
    return this.tracks.get(key)?.value ?? fallback;
  }

  has(key: string): boolean {
    return this.tracks.has(key);
  }

  /** Sets a value with no animation. Replaces any track on that key. */
  set(key: string, value: number): void {
    const t = this.tracks.get(key);
    if (t?.running) this.running--;
    this.tracks.set(key, {
      from: value,
      to: value,
      start: 0,
      duration: 0,
      delay: 0,
      easing: ease.out,
      loop: false,
      value,
      running: false,
      held: false,
    });
  }

  /**
   * Parks a track at `value` without ending it: the value stops moving but
   * `finish` still lands it and fires its `onDone`. The motion checks use it
   * through `ViewerEngine.freezeAt` to photograph a mid-animation frame.
   */
  hold(key: string, value: number): void {
    const t = this.tracks.get(key);
    if (!t) return;
    if (t.running) this.running--;
    t.running = false;
    t.held = true;
    t.value = value;
  }

  /**
   * Animates `key` to `target` from wherever it is now. Calling this again
   * mid-flight retargets from the current value instead of queueing. A key
   * that was never set starts at `target`, so callers that want an entrance
   * call `set(key, 0)` first.
   */
  to(key: string, target: number, now: number, opts: TrackOptions): void {
    const prev = this.tracks.get(key);
    const from = prev?.value ?? target;
    if (prev?.running) this.running--;
    const duration = Math.max(opts.duration, 0);
    const reduced = !this.motion();
    if (reduced || (duration <= 0 && !opts.loop)) {
      // Reduced motion: the end state right away. A looping track holds its
      // target so the tint is steady instead of absent.
      const value = target;
      this.tracks.set(key, {
        from: value,
        to: value,
        start: now,
        duration: 0,
        delay: 0,
        easing: opts.easing ?? ease.out,
        loop: false,
        value,
        running: false,
        held: false,
      });
      opts.onDone?.();
      return;
    }
    this.tracks.set(key, {
      from,
      to: target,
      start: now + Math.max(opts.delay ?? 0, 0),
      duration,
      delay: Math.max(opts.delay ?? 0, 0),
      easing: opts.easing ?? ease.out,
      loop: opts.loop === true,
      value: from,
      running: true,
      held: false,
      onDone: opts.onDone,
    });
    this.running++;
  }

  /**
   * Advances every track to `now`. Returns true when at least one value moved,
   * so the caller knows it has to render.
   */
  sample(now: number): boolean {
    let moved = false;
    const landed: Track[] = [];
    for (const t of this.tracks.values()) {
      if (!t.running) continue;
      const elapsed = now - t.start;
      if (elapsed < 0) continue; // still in its stagger delay
      const raw = t.duration <= 0 ? 1 : elapsed / t.duration;
      let next: number;
      if (t.loop) {
        // Ping-pong: 0 -> 1 -> 0, one full swing every `duration * 2`.
        const phase = raw % 2;
        const k = phase <= 1 ? phase : 2 - phase;
        next = t.from + (t.to - t.from) * t.easing(k);
      } else if (raw >= 1) {
        next = t.to;
        t.running = false;
        this.running--;
        landed.push(t);
      } else {
        next = t.from + (t.to - t.from) * t.easing(raw);
      }
      if (next !== t.value) moved = true;
      t.value = next;
    }
    for (const t of landed) t.onDone?.();
    return moved;
  }

  /** Tracks still moving. 0 means the engine can stop its frame loop. */
  active(): number {
    return this.running;
  }

  /** True when anything is still animating. */
  animating(): boolean {
    return this.running > 0;
  }

  /** Jumps one key to its end state and fires its `onDone`. */
  finish(key: string): void {
    const t = this.tracks.get(key);
    if (!t || (!t.running && !t.held)) return;
    if (t.running) this.running--;
    t.value = t.to;
    t.running = false;
    t.held = false;
    t.onDone?.();
  }

  /**
   * Jumps every track to its end state. Used before a capture so no frame is
   * ever grabbed halfway through an animation.
   */
  finishAll(): void {
    for (const key of [...this.tracks.keys()]) this.finish(key);
  }

  /** Drops a key without firing its `onDone`. */
  remove(key: string): void {
    const t = this.tracks.get(key);
    if (t?.running) this.running--;
    this.tracks.delete(key);
  }

  /** Drops every key without firing any `onDone`. */
  clear(): void {
    this.tracks.clear();
    this.running = 0;
  }

  /** Keys currently held, for the dev harness and tests. */
  keys(): string[] {
    return [...this.tracks.keys()];
  }
}
