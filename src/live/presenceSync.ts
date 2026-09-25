// This window's presence: what goes to `presence_set`, and how often.
// Pure, tested in presenceSync.test.ts.
//
// Selection, level and the AI scope are always sent when they change: the
// MCP tool `get_selection` reads them even without a live session
// (DECISIONS D30). The pointer and cursor chat typing only go out while a
// session runs. At most one send per interval (20 a second), with a trailing
// send so the last state always arrives, and nothing when nothing changed.

import type { Point, Presence } from "../contract/bindings";
import { CURSOR_CHAT_MAX } from "./format";

/** 20 sends a second, the rate the host forwards presence at. */
export const PRESENCE_INTERVAL_MS = 50;

/** The engine cuts a selection to this many ids (docs/CONTRACT.md, "Live sessions"). */
export const SELECTION_MAX = 2000;

export interface PresenceInput {
  selection: readonly string[];
  levelId: string | null;
  aiScope: boolean;
  /** Plan pointer in mm, null while it is off the plan. */
  cursor: Point | null;
  /** Cursor chat text while the bubble is open, null when closed. */
  typing: string | null;
  /** A live session runs: pointer and typing go out. */
  live: boolean;
}

/** Tenth of a millimeter: finer than any screen shows, and short in JSON. */
function round(n: number): number {
  return Math.round(n * 10) / 10;
}

export function buildPresence(input: PresenceInput): Presence {
  const cursor = input.live && input.cursor && Number.isFinite(input.cursor.x) && Number.isFinite(input.cursor.y) ? { x: round(input.cursor.x), y: round(input.cursor.y) } : null;
  const typing = input.live && input.typing !== null ? [...input.typing].slice(0, CURSOR_CHAT_MAX).join("") : null;
  return {
    cursor,
    level_id: input.levelId,
    selection: input.selection.length > SELECTION_MAX ? input.selection.slice(0, SELECTION_MAX) : [...input.selection],
    typing,
    ai_scope: input.aiScope,
  };
}

export function samePresence(a: Presence, b: Presence): boolean {
  if (a.level_id !== b.level_id || a.typing !== b.typing || a.ai_scope !== b.ai_scope) return false;
  if ((a.cursor === null) !== (b.cursor === null)) return false;
  if (a.cursor && b.cursor && (a.cursor.x !== b.cursor.x || a.cursor.y !== b.cursor.y)) return false;
  if (a.selection.length !== b.selection.length) return false;
  for (let i = 0; i < a.selection.length; i++) if (a.selection[i] !== b.selection[i]) return false;
  return true;
}

export interface ThrottleOptions {
  intervalMs: number;
  send: (presence: Presence) => void;
  now: () => number;
  setTimer: (fn: () => void, ms: number) => unknown;
  clearTimer: (id: unknown) => void;
}

/**
 * Leading and trailing throttle with a diff. `update` is called on every
 * change (every pointer move); a send goes out right away when the last one
 * is at least an interval old, else once when the interval ends, with the
 * latest state. A state equal to the last one sent is never sent again.
 */
export class PresenceThrottle {
  private last: Presence | null = null;
  private lastAt = Number.NEGATIVE_INFINITY;
  private pending: Presence | null = null;
  private timer: unknown = null;
  private readonly opts: ThrottleOptions;

  constructor(opts: ThrottleOptions) {
    this.opts = opts;
  }

  update(presence: Presence): void {
    if (this.timer !== null) {
      // A trailing send is on its way: it takes whatever is latest.
      this.pending = presence;
      return;
    }
    if (this.last && samePresence(this.last, presence)) return;
    const wait = this.lastAt + this.opts.intervalMs - this.opts.now();
    if (wait <= 0) {
      this.send(presence);
      return;
    }
    this.pending = presence;
    this.timer = this.opts.setTimer(() => this.trailing(), wait);
  }

  /** Forget what was sent, so the next update goes out even when equal (a new session). */
  reset(): void {
    this.last = null;
  }

  /** The last presence sent, for tests and dev checks. */
  sent(): Presence | null {
    return this.last;
  }

  dispose(): void {
    if (this.timer !== null) this.opts.clearTimer(this.timer);
    this.timer = null;
    this.pending = null;
  }

  private trailing(): void {
    this.timer = null;
    const p = this.pending;
    this.pending = null;
    if (p && !(this.last && samePresence(this.last, p))) this.send(p);
  }

  private send(presence: Presence): void {
    this.last = presence;
    this.lastAt = this.opts.now();
    this.opts.send(presence);
  }
}
