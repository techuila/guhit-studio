// This window's presence: what goes to `presence_set`, and how often.
// Pure, tested in presenceSync.test.ts.
//
// Selection, level and the AI scope are always sent when they change: the
// MCP tool `get_selection` reads them even without a live session
// (DECISIONS D30). The pointer and cursor chat typing only go out while a
// session runs, and so do who this window follows, its view mode and the 3D
// pointer. The plan area and the 3D camera go out only while someone follows
// this window, and only for a view on screen (DECISIONS D37). At most one
// send per interval (20 a second), with a trailing send so the last state
// always arrives, and nothing when nothing changed.

import type { LiveCamera, PlanView, Point, Presence, Vec3, ViewMode } from "../contract/bindings";
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
  /** A live session runs: pointer, typing, following, view mode and the 3D pointer go out. */
  live: boolean;
  /** The participant this window follows, null for no one. */
  following: string | null;
  viewMode: ViewMode;
  /** Someone in the session follows this window: the plan area and camera go out. */
  followed: boolean;
  /** The plan area on screen (`PlanController.planView`), null without a plan. */
  planView: PlanView | null;
  /** The 3D camera (`ViewerEngine.onCameraChange`), null without a 3D view. */
  camera: LiveCamera | null;
  /** The point under the pointer in 3D (`useLiveView.pointer3d`). */
  pointer3d: Vec3 | null;
}

/** Tenth of a millimeter: finer than any screen shows, and short in JSON. */
function round(n: number): number {
  return Math.round(n * 10) / 10;
}

function finite3(v: Vec3 | null | undefined): v is Vec3 {
  return !!v && Number.isFinite(v.x) && Number.isFinite(v.y) && Number.isFinite(v.z);
}

/** Whole millimeters, as the engine keeps them: a view a hair away is the same view. */
function mm3(v: Vec3): Vec3 {
  return { x: Math.round(v.x), y: Math.round(v.y), z: Math.round(v.z) };
}

function planArea(v: PlanView | null): PlanView | null {
  if (!v || !Number.isFinite(v.center.x) || !Number.isFinite(v.center.y) || !(v.width_mm > 0) || !(v.height_mm > 0) || !Number.isFinite(v.width_mm) || !Number.isFinite(v.height_mm)) return null;
  return { center: { x: Math.round(v.center.x), y: Math.round(v.center.y) }, width_mm: Math.max(1, Math.round(v.width_mm)), height_mm: Math.max(1, Math.round(v.height_mm)) };
}

function liveCamera(c: LiveCamera | null): LiveCamera | null {
  if (!c || !finite3(c.position) || !finite3(c.target) || !Number.isFinite(c.fov_deg)) return null;
  return { position: mm3(c.position), target: mm3(c.target), fov_deg: Math.min(110, Math.max(5, Math.round(c.fov_deg * 10) / 10)), nav: c.nav };
}

export function buildPresence(input: PresenceInput): Presence {
  const live = input.live;
  const cursor = live && input.cursor && Number.isFinite(input.cursor.x) && Number.isFinite(input.cursor.y) ? { x: round(input.cursor.x), y: round(input.cursor.y) } : null;
  const typing = live && input.typing !== null ? [...input.typing].slice(0, CURSOR_CHAT_MAX).join("") : null;
  const plan = input.viewMode !== "3d";
  const model = input.viewMode !== "2d";
  const shared = live && input.followed;
  return {
    cursor,
    level_id: input.levelId,
    selection: input.selection.length > SELECTION_MAX ? input.selection.slice(0, SELECTION_MAX) : [...input.selection],
    typing,
    ai_scope: input.aiScope,
    following: live ? input.following : null,
    view_mode: live ? input.viewMode : null,
    plan_view: shared && plan ? planArea(input.planView) : null,
    camera: shared && model ? liveCamera(input.camera) : null,
    pointer_3d: live && model && finite3(input.pointer3d) ? mm3(input.pointer3d) : null,
  };
}

function sameVec(a: Vec3 | null | undefined, b: Vec3 | null | undefined): boolean {
  if (!a || !b) return !a === !b;
  return a.x === b.x && a.y === b.y && a.z === b.z;
}

function samePlanView(a: PlanView | null | undefined, b: PlanView | null | undefined): boolean {
  if (!a || !b) return !a === !b;
  return a.center.x === b.center.x && a.center.y === b.center.y && a.width_mm === b.width_mm && a.height_mm === b.height_mm;
}

function sameCamera(a: LiveCamera | null | undefined, b: LiveCamera | null | undefined): boolean {
  if (!a || !b) return !a === !b;
  return a.nav === b.nav && a.fov_deg === b.fov_deg && sameVec(a.position, b.position) && sameVec(a.target, b.target);
}

/**
 * Every field compared, the follow fields too: a camera-only update must not
 * be dropped. A field an older version leaves out counts as null.
 */
export function samePresence(a: Presence, b: Presence): boolean {
  if (a.level_id !== b.level_id || a.typing !== b.typing || a.ai_scope !== b.ai_scope) return false;
  if ((a.following ?? null) !== (b.following ?? null) || (a.view_mode ?? null) !== (b.view_mode ?? null)) return false;
  if ((a.cursor === null) !== (b.cursor === null)) return false;
  if (a.cursor && b.cursor && (a.cursor.x !== b.cursor.x || a.cursor.y !== b.cursor.y)) return false;
  if (!samePlanView(a.plan_view, b.plan_view) || !sameCamera(a.camera, b.camera) || !sameVec(a.pointer_3d, b.pointer_3d)) return false;
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
