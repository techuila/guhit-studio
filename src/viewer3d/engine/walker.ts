// First person walk and fly for the live 3D view (viewerStore.nav).
//
// `WalkState` is the walker itself: a plan position in millimeters, an eye
// height, a heading and a pitch, and a velocity that eases toward what the
// keys ask for (under 100 ms, the keyboard guard rail of DECISIONS D11). It is
// pure, so the movement and the collision can be tested without a browser.
// Walk mode moves through `moveWithCollision` and holds the eye at the eye
// height above what the walker stands on; fly mode moves freely, up and down
// included. Eye height and speed are the walk settings (viewerStore `walk`).
//
// With a `WalkNav` (walk/worlds.ts) the walker knows every level and stair of
// the document: walking onto a flight climbs it as a ramp, leaving it at the
// head switches to the level at its top, leaving it at the foot switches back
// (walk/stairs.ts). Fly mode ignores stairs; landing picks the floor below.
//
// Besides the keys, a scroll or a swipe glides the walker a short way (a
// nudge), and a glide carries it to a picked spot (a minimap click, a double
// click on a floor) through anything, landing at eye height. Any movement
// input takes over from a glide wherever it got to.
//
// `WalkControls` is the input: keys on the window while walking (the 3D view
// owns every key without MOD then, docs/CONTRACT.md), drag to look, the wheel,
// the on-screen arrows for touch screens, and an optional pointer lock where
// the browser allows it. It never renders; it wakes the engine, whose single
// frame loop does the stepping.

import { ease, motionOK } from "../../ui/motion";
import { EYE_HEIGHT_MM } from "../geom/cameraMath";
import { EMPTY_WORLD, moveWithCollision, pushOut, WALKER_RADIUS_MM, type CollisionWorld } from "../geom/collision";
import type { Pt } from "../geom/coords";
import { flightLevelId, rampHeight, stairLocal, type StairInfo } from "../walk/stairs";

export type WalkMode = "walk" | "fly";

/** Default walking speed. The walk settings change it (`speedMmS`). */
export const WALK_SPEED_MM_S = 1400;
export const FLY_SPEED_MM_S = 2600;
/** Fly is this much faster than walking at the same speed setting. */
export const FLY_FACTOR = FLY_SPEED_MM_S / WALK_SPEED_MM_S;
/** Shift. */
export const RUN_FACTOR = 2.3;
/** Walk settings limits (viewerStore `walk`). */
export const SPEED_MIN_MM_S = 300;
export const SPEED_MAX_MM_S = 6000;
export const EYE_MIN_MM = 800;
export const EYE_MAX_MM = 2500;
/** Drag and pointer lock: radians per CSS pixel. The view follows the pointer 1:1. */
export const LOOK_RAD_PER_PX = 0.0042;
export const PITCH_LIMIT = 1.35;
/** Vertical field of view while walking or flying. */
export const WALK_FOV = 72;
/** The longest frame movement integrates. A longer stall moves as if it were this long. */
export const MAX_DT_S = 0.1;
/** Scroll and swipe: millimeters moved per CSS pixel of wheel at the default speed. */
export const NUDGE_MM_PER_PX = 5;
/** Wheel speed change: the speed is multiplied by exp(-px * this). One notch is about 12 percent. */
export const SPEED_PER_WHEEL_PX = 0.0012;
/** Velocity reaches ~63 percent of the target in this long. */
const VELOCITY_TAU_S = 0.07;
/** Eye height settles (fly to walk, a level change, a new eye height, a stair) this fast. */
const HEIGHT_TAU_S = 0.12;
/** A scroll step glides off this fast. */
const NUDGE_TAU_S = 0.09;
/** What is left of a scroll step below this is covered in one frame. */
const NUDGE_REST_MM = 2;
/** Scroll distance waiting to be covered never exceeds this. */
const NUDGE_MAX_MM = 20000;
/** Below this speed with no key held the walker stands still. */
const STOP_MM_S = 15;
/** How far outside the model the walker may go, mm. */
const ROAM_MM = 60000;

export interface WalkInput {
  /** W minus S. */
  forward: number;
  /** D minus A. */
  strafe: number;
  /** Fly only: E or Space minus Q or C. */
  up: number;
  run: boolean;
}

export const NO_INPUT: WalkInput = { forward: 0, strafe: 0, up: 0, run: false };

export interface Box2 {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

/** Every level and stair of a document, as the walker needs them. walk/worlds.ts builds one. */
export interface WalkNav {
  /** Floor height of a level, mm. */
  elevation(levelId: string): number;
  /** What blocks a walker standing on a level: walls, columns, objects and the strips around its stairs. */
  levelWorld(levelId: string): CollisionWorld;
  /** What blocks a walker on a flight: its sides, and the walls of its lower level (or of its upper level on the upper half). */
  flightWorld(stair: StairInfo, high: boolean): CollisionWorld;
  /** The flight under `p` that a walker on this level can be on (entered at its foot or at its head). */
  stairAt(levelId: string, p: Pt): StairInfo | null;
  /** Height above the level floor near the foot of a flight, where its walking line starts to rise. 0 elsewhere. */
  approach(levelId: string, p: Pt): number;
  /** Where fly mode lands at `p` with the feet at `feetZ`. */
  landing(p: Pt, feetZ: number): { levelId: string; stair: StairInfo | null } | null;
}

/** Where a glide ends: plan point, the level and flight there, and the height of the floor. */
export interface GlideTarget {
  x: number;
  y: number;
  levelId: string | null;
  stair: StairInfo | null;
  floorZ: number;
}

interface Glide {
  fromX: number;
  fromY: number;
  fromZ: number;
  toX: number;
  toY: number;
  /** Seconds done and seconds in all. */
  t: number;
  duration: number;
}

/** The walker. Plan millimeters, heights absolute (level elevation included). */
export class WalkState {
  mode: WalkMode = "walk";
  x = 0;
  y = 0;
  /** Eye height, mm above the project zero. */
  z = EYE_HEIGHT_MM;
  /** What the walker stands on, mm above the project zero: its level's floor, or the walking line of a flight. */
  floorZ = 0;
  /** Plan heading of the view: radians counter-clockwise from east. */
  yaw = Math.PI / 2;
  pitch = 0;
  vx = 0;
  vy = 0;
  vz = 0;
  /** Eye height above what the walker stands on (walk settings). */
  eyeMm = EYE_HEIGHT_MM;
  /** Walking speed (walk settings). Fly is FLY_FACTOR faster, Shift RUN_FACTOR. */
  speedMmS = WALK_SPEED_MM_S;
  /** What blocks the walker off any flight. With a `nav` it is the world of `levelId`. */
  world: CollisionWorld = EMPTY_WORLD;
  /** The walker stays within this box grown by 60 m. Null: anywhere. */
  area: Box2 | null = null;
  /** Fly limits for the eye, mm. */
  minZ = -Infinity;
  maxZ = Infinity;
  /** Levels and stairs. Null: one flat world (`world` and `floorZ` set from outside). */
  nav: WalkNav | null = null;
  /** The level walked on. On a flight it stays the level the flight was entered from until the walker leaves it. */
  levelId: string | null = null;
  /** The flight the walker is on. */
  stair: StairInfo | null = null;
  /** Scroll distance still to cover, mm: forward and to the right. */
  private nudgeF = 0;
  private nudgeS = 0;
  private glide: Glide | null = null;

  eyeHeight(): number {
    return this.floorZ + this.eyeMm;
  }

  /** Levels and stairs, and the level to stand on. The walker stays where it is. */
  setNav(nav: WalkNav | null, levelId: string | null): void {
    this.nav = nav;
    this.stair = null;
    this.setLevel(levelId);
    if (this.mode === "walk") this.updateSupport();
  }

  /** Stands on another level, off any flight. */
  setLevel(levelId: string | null): void {
    this.levelId = levelId;
    this.stair = null;
    if (this.nav && levelId !== null) {
      this.world = this.nav.levelWorld(levelId);
      this.floorZ = this.nav.elevation(levelId);
    }
  }

  /** Places the walker, standing (walk) or where it is told (fly). */
  place(x: number, y: number, yaw: number, pitch: number): void {
    this.x = x;
    this.y = y;
    this.yaw = wrapAngle(yaw);
    this.pitch = clamp(pitch, -PITCH_LIMIT, PITCH_LIMIT);
    this.vx = this.vy = this.vz = 0;
    this.nudgeF = this.nudgeS = 0;
    this.glide = null;
    if (this.mode === "walk") this.settle();
  }

  /** Walk mode: out of anything the walker stands in, and onto the floor or flight under it. */
  settle(): void {
    const p = pushOut({ x: this.x, y: this.y }, WALKER_RADIUS_MM, this.collisionWorld());
    this.x = p.x;
    this.y = p.y;
    this.updateSupport();
  }

  /** Fly to walk: onto the floor under the walker (a flight, or the highest level below the feet), then out of anything there. */
  land(): void {
    this.glide = null;
    const landing = this.nav?.landing(this, this.z - this.eyeMm);
    if (landing) {
      this.setLevel(landing.levelId);
      this.stair = landing.stair;
    }
    this.settle();
  }

  look(dxPx: number, dyPx: number): void {
    // Drag right turns right, drag down looks down, like the concept page.
    this.yaw = wrapAngle(this.yaw - dxPx * LOOK_RAD_PER_PX);
    this.pitch = clamp(this.pitch - dyPx * LOOK_RAD_PER_PX, -PITCH_LIMIT, PITCH_LIMIT);
  }

  /** Unit view direction in plan (x east, y north) and up. */
  forward(): { x: number; y: number; z: number } {
    const c = Math.cos(this.pitch);
    return { x: Math.cos(this.yaw) * c, y: Math.sin(this.yaw) * c, z: Math.sin(this.pitch) };
  }

  /**
   * A scroll or swipe: glide this many CSS pixels of wheel forward and to the
   * right, scaled by the speed setting. The distance adds up and is covered
   * over the next frames.
   */
  nudgePx(forwardPx: number, rightPx: number): void {
    const k = NUDGE_MM_PER_PX * (this.speedMmS / WALK_SPEED_MM_S);
    this.nudgeF = clamp(this.nudgeF + forwardPx * k, -NUDGE_MAX_MM, NUDGE_MAX_MM);
    this.nudgeS = clamp(this.nudgeS + rightPx * k, -NUDGE_MAX_MM, NUDGE_MAX_MM);
  }

  /**
   * The wheel while moving: a notch away from you (negative) is about 12
   * percent faster, toward you slower. Continuous, so the many small deltas
   * of a trackpad add up the same way. Returns the new speed, mm/s.
   */
  wheelSpeed(deltaPx: number): number {
    this.speedMmS = clamp(this.speedMmS * Math.exp(-deltaPx * SPEED_PER_WHEEL_PX), SPEED_MIN_MM_S, SPEED_MAX_MM_S);
    return this.speedMmS;
  }

  gliding(): boolean {
    return this.glide !== null;
  }

  /** Where the running glide ends, or null. */
  glideTarget(): Pt | null {
    return this.glide ? { x: this.glide.toX, y: this.glide.toY } : null;
  }

  /**
   * Carries the walker to `target` in `durationMs`, through anything, landing
   * at eye height on the floor there. The level changes at the start, so the
   * minimap and the doors follow the destination. 0 ms jumps.
   */
  startGlide(target: GlideTarget, durationMs: number): void {
    this.vx = this.vy = this.vz = 0;
    this.nudgeF = this.nudgeS = 0;
    if (this.nav && target.levelId !== null) this.setLevel(target.levelId);
    this.stair = target.stair;
    this.floorZ = target.floorZ;
    const duration = Math.max(durationMs, 0) / 1000;
    if (!(duration > 0)) {
      this.glide = null;
      this.x = target.x;
      this.y = target.y;
      this.z = this.eyeHeight();
      return;
    }
    this.glide = { fromX: this.x, fromY: this.y, fromZ: this.z, toX: target.x, toY: target.y, t: 0, duration };
  }

  /** Ends a glide where it got to. Walking, the walker steps out of anything it glided into. */
  stopGlide(): void {
    if (!this.glide) return;
    this.glide = null;
    if (this.mode === "walk") this.settle();
  }

  /**
   * Advances `dt` seconds (clamped to 100 ms). Returns true while the walker
   * still moves or a key is held, so the engine keeps exactly one frame
   * scheduled; false when it has come to rest, and the loop stops.
   */
  step(dtIn: number, input: WalkInput): boolean {
    const dt = clamp(Number.isFinite(dtIn) ? dtIn : 0, 0, MAX_DT_S);
    const fly = this.mode === "fly";
    if (this.glide) {
      const takeOver = input.forward !== 0 || input.strafe !== 0 || (fly && input.up !== 0) || this.nudgeF !== 0 || this.nudgeS !== 0;
      if (!takeOver) return this.stepGlide(dt);
      this.stopGlide();
    }
    const speed = this.speedMmS * (fly ? FLY_FACTOR : 1) * (input.run ? RUN_FACTOR : 1);
    // Forward and right in plan.
    const fx = Math.cos(this.yaw);
    const fy = Math.sin(this.yaw);
    let mx = fx * input.forward + fy * input.strafe;
    let my = fy * input.forward - fx * input.strafe;
    const ml = Math.hypot(mx, my);
    if (ml > 1e-9) {
      mx /= ml;
      my /= ml;
    }
    const tvx = ml > 1e-9 ? mx * speed : 0;
    const tvy = ml > 1e-9 ? my * speed : 0;
    const tvz = fly ? clamp(input.up, -1, 1) * speed : 0;
    const k = motionOK() ? 1 - Math.exp(-dt / VELOCITY_TAU_S) : 1;
    this.vx += (tvx - this.vx) * k;
    this.vy += (tvy - this.vy) * k;
    this.vz += (tvz - this.vz) * k;
    const held = ml > 1e-9 || (fly && input.up !== 0);
    if (!held && Math.hypot(this.vx, this.vy, this.vz) < STOP_MM_S) this.vx = this.vy = this.vz = 0;

    // Scroll: a share of the distance left is covered every frame, so a
    // wheel notch glides instead of jumping.
    let nf = 0;
    let ns = 0;
    if (this.nudgeF !== 0 || this.nudgeS !== 0) {
      const kn = motionOK() ? 1 - Math.exp(-dt / NUDGE_TAU_S) : 1;
      nf = this.nudgeF * kn;
      ns = this.nudgeS * kn;
      this.nudgeF -= nf;
      this.nudgeS -= ns;
      if (Math.hypot(this.nudgeF, this.nudgeS) < NUDGE_REST_MM) {
        nf += this.nudgeF;
        ns += this.nudgeS;
        this.nudgeF = this.nudgeS = 0;
      }
    }
    const nudging = nf !== 0 || ns !== 0;

    const dx = this.vx * dt + fx * nf + fy * ns;
    const dy = this.vy * dt + fy * nf - fx * ns;
    if (dx !== 0 || dy !== 0) {
      let p = { x: this.x + dx, y: this.y + dy };
      if (!fly) p = moveWithCollision({ x: this.x, y: this.y }, { x: dx, y: dy }, WALKER_RADIUS_MM, this.collisionWorld());
      if (this.area) {
        p.x = clamp(p.x, this.area.minX - ROAM_MM, this.area.maxX + ROAM_MM);
        p.y = clamp(p.y, this.area.minY - ROAM_MM, this.area.maxY + ROAM_MM);
      }
      this.x = p.x;
      this.y = p.y;
      if (!fly) this.updateSupport();
    }

    let settling = false;
    if (fly) {
      this.z = clamp(this.z + this.vz * dt, this.minZ, this.maxZ);
    } else {
      const target = this.eyeHeight();
      const kz = motionOK() ? 1 - Math.exp(-dt / HEIGHT_TAU_S) : 1;
      this.z += (target - this.z) * kz;
      if (Math.abs(target - this.z) < 1) this.z = target;
      settling = this.z !== target;
    }
    return held || this.vx !== 0 || this.vy !== 0 || this.vz !== 0 || settling || nudging;
  }

  private stepGlide(dt: number): boolean {
    const g = this.glide as Glide;
    g.t = Math.min(g.t + dt, g.duration);
    const k = ease.inOut(g.t / g.duration);
    this.x = g.fromX + (g.toX - g.fromX) * k;
    this.y = g.fromY + (g.toY - g.fromY) * k;
    this.z = g.fromZ + (this.eyeHeight() - g.fromZ) * k;
    if (g.t >= g.duration) this.glide = null;
    return true;
  }

  /** What blocks the walker where it is: the flight it is on (lower or upper half), or its level. */
  private collisionWorld(): CollisionWorld {
    const s = this.stair;
    if (!s || !this.nav) return this.world;
    return this.nav.flightWorld(s, rampHeight(s, stairLocal(s, this).u) >= s.rise / 2);
  }

  /**
   * After a move: stays on a flight while over it, leaves it at the foot (its
   * own level) or at the head (the level at its top), steps onto a flight
   * from the level at either end, and sets the floor height under the walker.
   */
  private updateSupport(): void {
    const nav = this.nav;
    if (!nav || this.levelId === null) return;
    const s = this.stair;
    if (s) {
      const { u, v } = stairLocal(s, this);
      if (u >= 0 && u <= s.run && Math.abs(v) <= s.width / 2 + 1) {
        this.floorZ = s.bottomZ + rampHeight(s, u);
        return;
      }
      this.setLevel(u < 0 ? s.levelId : u > s.run ? (s.topLevelId ?? s.levelId) : flightLevelId(s, u));
    }
    const next = nav.stairAt(this.levelId, this);
    if (next) {
      this.stair = next;
      this.floorZ = next.bottomZ + rampHeight(next, stairLocal(next, this).u);
      return;
    }
    this.floorZ = nav.elevation(this.levelId) + nav.approach(this.levelId, this);
  }
}

export function clamp(v: number, lo: number, hi: number): number {
  return Math.min(Math.max(v, lo), hi);
}

export function wrapAngle(a: number): number {
  const t = Math.PI * 2;
  let r = a % t;
  if (r > Math.PI) r -= t;
  if (r < -Math.PI) r += t;
  return r;
}

// ------------------------------------------------------------------ input

const MOVE_KEYS: Record<string, keyof WalkInput | "back" | "left" | "down"> = {
  KeyW: "forward",
  ArrowUp: "forward",
  KeyS: "back",
  ArrowDown: "back",
  KeyD: "strafe",
  ArrowRight: "strafe",
  KeyA: "left",
  ArrowLeft: "left",
  KeyE: "up",
  Space: "up",
  KeyQ: "down",
  KeyC: "down",
  ShiftLeft: "run",
  ShiftRight: "run",
};

/** The on-screen arrows for touch screens. */
export type TouchMove = "forward" | "back" | "left" | "right";
const TOUCH_CODES: Record<TouchMove, string> = { forward: "TouchForward", back: "TouchBack", left: "TouchLeft", right: "TouchRight" };

/** Wheel deltas in lines and pages, in CSS pixels. */
const WHEEL_LINE_PX = 40;
const WHEEL_PAGE_PX = 400;
/** One wheel event never counts for more than this. */
const WHEEL_MAX_PX = 300;

export interface WalkControlCallbacks {
  /** A key went down or the view turned: schedule a frame. */
  wake(): void;
  /** The view turned by this many CSS pixels of pointer movement. */
  look(dxPx: number, dyPx: number): void;
  /** Escape. */
  exit(): void;
  /** F. */
  toggleFly(): void;
  /** X. */
  cycleShell(): void;
  /** True while keys belong to someone else (a dialog, the palette). */
  blocked?(): boolean;
  lockChange?(locked: boolean): void;
  lockError?(): void;
  /** A scroll or swipe: move forward and to the right by this many CSS pixels of wheel. */
  nudge?(forwardPx: number, rightPx: number): void;
  /** The wheel while keys or a drag move the walker, or with Alt: pixels of wheel, negative is faster. */
  speedWheel?(deltaPx: number): void;
  /** A double click at this client point: glide there. */
  dblclick?(clientX: number, clientY: number): void;
}

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || !el.tagName) return false;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable;
}

/**
 * True where the Pointer Lock API can work. WKWebView may expose the call and
 * still refuse it, so a refusal (`pointerlockerror` or a rejected promise) is
 * handled as well: the caller hides the button for the rest of the session.
 */
export function pointerLockSupported(): boolean {
  if (typeof document === "undefined" || typeof HTMLCanvasElement === "undefined") return false;
  if (typeof HTMLCanvasElement.prototype.requestPointerLock !== "function") return false;
  if (typeof document.exitPointerLock !== "function" || !("pointerLockElement" in document)) return false;
  if (typeof matchMedia === "function" && matchMedia("(pointer: coarse)").matches) return false;
  return true;
}

/**
 * What a wheel event asks for while walking. With keys or a drag moving the
 * walker (or with Alt) the wheel sets the pace; otherwise a scroll or a
 * two-finger swipe moves forward and back, Shift+scroll or a sideways swipe
 * moves sideways. Pinch zoom on a trackpad arrives as a wheel with Ctrl and
 * moves forward and back too.
 */
export function wheelIntent(
  e: Pick<WheelEvent, "deltaX" | "deltaY" | "deltaMode" | "shiftKey" | "altKey">,
  moving: boolean,
): { kind: "speed"; deltaPx: number } | { kind: "move"; forwardPx: number; rightPx: number } | null {
  const unit = e.deltaMode === 1 ? WHEEL_LINE_PX : e.deltaMode === 2 ? WHEEL_PAGE_PX : 1;
  let dy = clamp(e.deltaY * unit, -WHEEL_MAX_PX, WHEEL_MAX_PX);
  let dx = clamp(e.deltaX * unit, -WHEEL_MAX_PX, WHEEL_MAX_PX);
  if (moving || e.altKey) {
    const d = Math.abs(dy) >= Math.abs(dx) ? dy : dx;
    return d !== 0 ? { kind: "speed", deltaPx: d } : null;
  }
  // Some browsers already turn Shift+wheel into a sideways delta.
  if (e.shiftKey && dx === 0) {
    dx = dy;
    dy = 0;
  }
  if (dx === 0 && dy === 0) return null;
  // Scrolling the way that zooms in (orbit) moves forward.
  return { kind: "move", forwardPx: -dy, rightPx: dx };
}

/** Keys, drag to look, wheel, touch arrows and pointer lock, attached only while walking or flying. */
export class WalkControls {
  private keys = new Set<string>();
  private drag: { id: number; x: number; y: number } | null = null;
  private attached = false;
  private unlockedAt = 0;

  constructor(
    private canvas: HTMLCanvasElement,
    private cb: WalkControlCallbacks,
  ) {}

  get isAttached(): boolean {
    return this.attached;
  }

  input(): WalkInput {
    const k = this.keys;
    const any = (...codes: string[]) => codes.some((c) => k.has(c));
    return {
      forward: (any("KeyW", "ArrowUp", "TouchForward") ? 1 : 0) - (any("KeyS", "ArrowDown", "TouchBack") ? 1 : 0),
      strafe: (any("KeyD", "ArrowRight", "TouchRight") ? 1 : 0) - (any("KeyA", "ArrowLeft", "TouchLeft") ? 1 : 0),
      up: (any("KeyE", "Space") ? 1 : 0) - (any("KeyQ", "KeyC") ? 1 : 0),
      run: any("ShiftLeft", "ShiftRight"),
    };
  }

  held(): boolean {
    return this.keys.size > 0;
  }

  /** True while a key or an on-screen arrow moves the walker (Shift alone does not). */
  moving(): boolean {
    for (const code of this.keys) if (code !== "ShiftLeft" && code !== "ShiftRight") return true;
    return false;
  }

  /** An on-screen arrow went down or up. */
  press(move: TouchMove, down: boolean): void {
    const code = TOUCH_CODES[move];
    if (down) {
      if (this.keys.has(code)) return;
      this.keys.add(code);
      this.cb.wake();
    } else if (this.keys.delete(code)) {
      this.cb.wake();
    }
  }

  get locked(): boolean {
    return typeof document !== "undefined" && document.pointerLockElement === this.canvas;
  }

  attach(): void {
    if (this.attached) return;
    this.attached = true;
    // Capture phase: the walker sees a key before anything else on the page.
    window.addEventListener("keydown", this.onKeyDown, true);
    window.addEventListener("keyup", this.onKeyUp, true);
    window.addEventListener("blur", this.release);
    document.addEventListener("visibilitychange", this.onVisibility);
    this.canvas.addEventListener("pointerdown", this.onPointerDown);
    this.canvas.addEventListener("pointermove", this.onPointerMove);
    this.canvas.addEventListener("pointerup", this.onPointerUp);
    this.canvas.addEventListener("pointercancel", this.onPointerUp);
    // Not passive: a walk scroll must not scroll or zoom the page.
    this.canvas.addEventListener("wheel", this.onWheel, { passive: false });
    this.canvas.addEventListener("dblclick", this.onDblClick);
    document.addEventListener("mousemove", this.onLockedMove);
    document.addEventListener("pointerlockchange", this.onLockChange);
    document.addEventListener("pointerlockerror", this.onLockError);
    this.canvas.style.cursor = "grab";
  }

  detach(): void {
    if (!this.attached) return;
    this.attached = false;
    window.removeEventListener("keydown", this.onKeyDown, true);
    window.removeEventListener("keyup", this.onKeyUp, true);
    window.removeEventListener("blur", this.release);
    document.removeEventListener("visibilitychange", this.onVisibility);
    this.canvas.removeEventListener("pointerdown", this.onPointerDown);
    this.canvas.removeEventListener("pointermove", this.onPointerMove);
    this.canvas.removeEventListener("pointerup", this.onPointerUp);
    this.canvas.removeEventListener("pointercancel", this.onPointerUp);
    this.canvas.removeEventListener("wheel", this.onWheel);
    this.canvas.removeEventListener("dblclick", this.onDblClick);
    document.removeEventListener("mousemove", this.onLockedMove);
    document.removeEventListener("pointerlockchange", this.onLockChange);
    document.removeEventListener("pointerlockerror", this.onLockError);
    this.release();
    this.endDrag();
    if (this.locked) {
      try {
        document.exitPointerLock();
      } catch {
        // Nothing to release.
      }
    }
    this.canvas.style.cursor = "";
  }

  /** Lets go of every key: window blur, a hidden tab, leaving walk mode. */
  release = (): void => {
    this.keys.clear();
  };

  requestLock(): void {
    if (!pointerLockSupported()) {
      this.cb.lockError?.();
      return;
    }
    try {
      const r = this.canvas.requestPointerLock() as unknown;
      if (r && typeof (r as Promise<void>).then === "function") (r as Promise<void>).catch(() => this.cb.lockError?.());
    } catch {
      this.cb.lockError?.();
    }
  }

  exitLock(): void {
    if (!this.locked) return;
    try {
      document.exitPointerLock();
    } catch {
      // Already gone.
    }
  }

  private onVisibility = (): void => {
    if (document.hidden) this.release();
  };

  private onKeyDown = (e: KeyboardEvent): void => {
    if (isTyping(e.target) || this.cb.blocked?.()) return;
    // MOD shortcuts (undo, the palette, export) stay with the app.
    if (e.metaKey || e.ctrlKey) return;
    const code = e.code;
    if (code === "Escape") {
      // The first Escape under pointer lock only unlocks the mouse, and an
      // open walk popover (the settings) closes before the walk ends.
      if (this.locked || performance.now() - this.unlockedAt < 250) return;
      if (typeof document !== "undefined" && document.querySelector('[data-walk-popover="open"]')) return;
      e.preventDefault();
      this.release();
      this.cb.exit();
      return;
    }
    if (code === "KeyF") {
      e.preventDefault();
      if (!e.repeat) this.cb.toggleFly();
      return;
    }
    if (code === "KeyX") {
      e.preventDefault();
      if (!e.repeat) this.cb.cycleShell();
      return;
    }
    if (MOVE_KEYS[code]) {
      e.preventDefault();
      if (!this.keys.has(code)) {
        this.keys.add(code);
        this.cb.wake();
      }
    }
  };

  private onKeyUp = (e: KeyboardEvent): void => {
    if (this.keys.delete(e.code)) this.cb.wake();
  };

  private onPointerDown = (e: PointerEvent): void => {
    if (this.locked || e.button !== 0) return;
    this.drag = { id: e.pointerId, x: e.clientX, y: e.clientY };
    try {
      this.canvas.setPointerCapture(e.pointerId);
    } catch {
      // Synthetic pointers cannot be captured; the drag still works inside the canvas.
    }
    this.canvas.style.cursor = "grabbing";
  };

  private onPointerMove = (e: PointerEvent): void => {
    const d = this.drag;
    if (!d || e.pointerId !== d.id) return;
    const dx = e.clientX - d.x;
    const dy = e.clientY - d.y;
    d.x = e.clientX;
    d.y = e.clientY;
    if (dx !== 0 || dy !== 0) this.cb.look(dx, dy);
  };

  private onPointerUp = (e: PointerEvent): void => {
    if (this.drag && e.pointerId === this.drag.id) this.endDrag();
  };

  private onWheel = (e: WheelEvent): void => {
    if (this.cb.blocked?.()) return;
    e.preventDefault();
    const intent = wheelIntent(e, this.moving() || this.drag !== null);
    if (!intent) return;
    if (intent.kind === "speed") this.cb.speedWheel?.(intent.deltaPx);
    else this.cb.nudge?.(intent.forwardPx, intent.rightPx);
  };

  private onDblClick = (e: MouseEvent): void => {
    if (this.cb.blocked?.()) return;
    // Under pointer lock the cursor is hidden: aim with the crosshair.
    if (this.locked) {
      const r = this.canvas.getBoundingClientRect();
      this.cb.dblclick?.(r.left + r.width / 2, r.top + r.height / 2);
      return;
    }
    this.cb.dblclick?.(e.clientX, e.clientY);
  };

  private endDrag(): void {
    const d = this.drag;
    this.drag = null;
    if (d) {
      try {
        if (this.canvas.hasPointerCapture(d.id)) this.canvas.releasePointerCapture(d.id);
      } catch {
        // Released already.
      }
    }
    if (this.attached) this.canvas.style.cursor = this.locked ? "none" : "grab";
  }

  private onLockedMove = (e: MouseEvent): void => {
    if (!this.locked) return;
    if (e.movementX !== 0 || e.movementY !== 0) this.cb.look(e.movementX, e.movementY);
  };

  private onLockChange = (): void => {
    const locked = this.locked;
    if (!locked) this.unlockedAt = performance.now();
    this.canvas.style.cursor = locked ? "none" : this.attached ? "grab" : "";
    this.cb.lockChange?.(locked);
  };

  private onLockError = (): void => {
    this.cb.lockError?.();
  };
}
