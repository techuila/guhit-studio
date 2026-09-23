// First person walk and fly for the live 3D view (viewerStore.nav).
//
// `WalkState` is the walker itself: a plan position in millimeters, an eye
// height, a heading and a pitch, and a velocity that eases toward what the
// keys ask for (under 100 ms, the keyboard guard rail of DECISIONS D11). It is
// pure, so the movement and the collision can be tested without a browser.
// Walk mode moves through `moveWithCollision` and holds the eye 1600 mm above
// the level floor; fly mode moves freely, up and down included.
//
// `WalkControls` is the input: keys on the window while walking (the 3D view
// owns every key without MOD then, docs/CONTRACT.md), drag to look, and an
// optional pointer lock where the browser allows it. It never renders; it
// wakes the engine, whose single frame loop does the stepping.

import { motionOK } from "../../ui/motion";
import { EYE_HEIGHT_MM } from "../geom/cameraMath";
import { EMPTY_WORLD, moveWithCollision, pushOut, WALKER_RADIUS_MM, type CollisionWorld } from "../geom/collision";

export type WalkMode = "walk" | "fly";

export const WALK_SPEED_MM_S = 1400;
export const FLY_SPEED_MM_S = 2600;
/** Shift. */
export const RUN_FACTOR = 2.3;
/** Drag and pointer lock: radians per CSS pixel. The view follows the pointer 1:1. */
export const LOOK_RAD_PER_PX = 0.0042;
export const PITCH_LIMIT = 1.35;
/** Vertical field of view while walking or flying. */
export const WALK_FOV = 72;
/** The longest frame movement integrates. A longer stall moves as if it were this long. */
export const MAX_DT_S = 0.1;
/** Velocity reaches ~63 percent of the target in this long. */
const VELOCITY_TAU_S = 0.07;
/** Eye height settles back to 1600 mm (fly to walk, a level change) this fast. */
const HEIGHT_TAU_S = 0.12;
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

/** The walker. Plan millimeters, heights absolute (level elevation included). */
export class WalkState {
  mode: WalkMode = "walk";
  x = 0;
  y = 0;
  /** Eye height, mm above the project zero. */
  z = EYE_HEIGHT_MM;
  /** Elevation of the level walked on. */
  floorZ = 0;
  /** Plan heading of the view: radians counter-clockwise from east. */
  yaw = Math.PI / 2;
  pitch = 0;
  vx = 0;
  vy = 0;
  vz = 0;
  world: CollisionWorld = EMPTY_WORLD;
  /** The walker stays within this box grown by 60 m. Null: anywhere. */
  area: Box2 | null = null;
  /** Fly limits for the eye, mm. */
  minZ = -Infinity;
  maxZ = Infinity;

  eyeHeight(): number {
    return this.floorZ + EYE_HEIGHT_MM;
  }

  /** Places the walker, standing (walk) or where it is told (fly). */
  place(x: number, y: number, yaw: number, pitch: number): void {
    this.x = x;
    this.y = y;
    this.yaw = wrapAngle(yaw);
    this.pitch = clamp(pitch, -PITCH_LIMIT, PITCH_LIMIT);
    this.vx = this.vy = this.vz = 0;
    if (this.mode === "walk") this.settle();
  }

  /** Walk mode: out of anything the walker stands in. */
  settle(): void {
    const p = pushOut({ x: this.x, y: this.y }, WALKER_RADIUS_MM, this.world);
    this.x = p.x;
    this.y = p.y;
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
   * Advances `dt` seconds (clamped to 100 ms). Returns true while the walker
   * still moves or a key is held, so the engine keeps exactly one frame
   * scheduled; false when it has come to rest, and the loop stops.
   */
  step(dtIn: number, input: WalkInput): boolean {
    const dt = clamp(Number.isFinite(dtIn) ? dtIn : 0, 0, MAX_DT_S);
    const fly = this.mode === "fly";
    const speed = (fly ? FLY_SPEED_MM_S : WALK_SPEED_MM_S) * (input.run ? RUN_FACTOR : 1);
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

    const dx = this.vx * dt;
    const dy = this.vy * dt;
    if (dx !== 0 || dy !== 0) {
      let p = { x: this.x + dx, y: this.y + dy };
      if (!fly) p = moveWithCollision({ x: this.x, y: this.y }, { x: dx, y: dy }, WALKER_RADIUS_MM, this.world);
      if (this.area) {
        p.x = clamp(p.x, this.area.minX - ROAM_MM, this.area.maxX + ROAM_MM);
        p.y = clamp(p.y, this.area.minY - ROAM_MM, this.area.maxY + ROAM_MM);
      }
      this.x = p.x;
      this.y = p.y;
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
    return held || this.vx !== 0 || this.vy !== 0 || this.vz !== 0 || settling;
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

/** Keys, drag to look and pointer lock, attached only while walking or flying. */
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
      forward: (any("KeyW", "ArrowUp") ? 1 : 0) - (any("KeyS", "ArrowDown") ? 1 : 0),
      strafe: (any("KeyD", "ArrowRight") ? 1 : 0) - (any("KeyA", "ArrowLeft") ? 1 : 0),
      up: (any("KeyE", "Space") ? 1 : 0) - (any("KeyQ", "KeyC") ? 1 : 0),
      run: any("ShiftLeft", "ShiftRight"),
    };
  }

  held(): boolean {
    return this.keys.size > 0;
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
      // The first Escape under pointer lock only unlocks the mouse.
      if (this.locked || performance.now() - this.unlockedAt < 250) return;
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
