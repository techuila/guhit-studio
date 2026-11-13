// The 3D height handle of a selected pipe (DECISIONS D39, the SketchUp part).
//
// A selected run, or the segment or point picked on it (editor2d/pipePick.ts),
// shows a vertical double arrow in 3D. Dragging it moves that part up or down
// and tracks the pointer 1:1: the pointer ray meets a vertical plane through
// the handle that faces the camera. While dragging:
//
// - snaps: 10 mm steps, the level floor, the level height, and the heights of
//   other runs nearby (and of the run's own points that stay put);
// - typing a number sets the exact distance, Enter applies it;
// - arrow keys lock the axis the SketchUp way: Up or Down vertical, Right
//   plan x, Left plan y (the same key again goes back to vertical);
// - Alt breaks the joins: other runs resting on the moved points stay put;
// - Esc cancels and the handle eases back.
//
// A press and release without moving picks the handle up instead: it follows
// the pointer until the next click, which is how typing a distance reads best.
//
// The preview is a `MovePipePoints` through `ipc.docPreview`, shown with
// `setPreview` (the same ghost a keyboard nudge and an AI proposal use), and
// the move is dispatched once on release. Frames: every one goes through the
// engine's `invalidate` (ViewerEngine.schedule); the drag is read once per
// drawn frame in `beforeRender`. Motion: the handle grows in, grows a little
// on hover, eases back on cancel or a refused move (docs/MOTION.md).

import * as THREE from "three";
import type { ApplyResult, Command, DisplayUnit, DocState, Level, Pipe, Vec3 } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";
import { usePipePick, type PipePick } from "../../editor2d/pipePick";
import { emptyTyped, formatLength, typedKey, typedValues, type TypedState } from "../../editor2d/typed";
import { useApp } from "../../state/store";
import { dur, ease } from "../../ui/motion";
import { cutRun } from "../scene/pipes";
import type { Animator } from "./animator";
import styles from "./pipeHandle.module.css";

// ------------------------------------------------------------------- math

/** The axis a move runs along: height, plan x (east) or plan y (north). */
export type HandleAxis = "z" | "x" | "y";

/** Plan axis to world direction: plan x is world +x, plan y (north) is world -z, height is world +y. */
export function axisWorld(axis: HandleAxis, out = new THREE.Vector3()): THREE.Vector3 {
  if (axis === "z") return out.set(0, 1, 0);
  if (axis === "x") return out.set(1, 0, 0);
  return out.set(0, 0, -1);
}

/** A move of `mm` along an axis, as a plan delta (x, y, height). */
export function axisDelta(axis: HandleAxis, mm: number): Vec3 {
  return { x: axis === "x" ? mm : 0, y: axis === "y" ? mm : 0, z: axis === "z" ? mm : 0 };
}

/** Below this the ray runs too flat to the plane for a stable hit. */
const MIN_FACING = 0.02;

/**
 * Where a pointer ray points along an axis through `origin`, in meters from
 * it. The ray meets the plane that holds the axis and faces the eye, and the
 * hit is read along the axis: for the vertical axis that plane is the
 * vertical plane through the handle facing the camera. Null when the eye
 * looks along the axis, or the ray runs nearly flat to the plane or away.
 */
export function axisParam(ray: THREE.Ray, origin: THREE.Vector3, axis: THREE.Vector3, eye: THREE.Vector3): number | null {
  const n = new THREE.Vector3().subVectors(eye, origin);
  n.addScaledVector(axis, -n.dot(axis));
  if (n.lengthSq() < 1e-12) return null;
  n.normalize();
  const facing = ray.direction.dot(n);
  if (Math.abs(facing) < MIN_FACING) return null;
  const t = new THREE.Vector3().subVectors(origin, ray.origin).dot(n) / facing;
  if (!(t > 0)) return null;
  return ray.at(t, new THREE.Vector3()).sub(origin).dot(axis);
}

/** What a move caught: a height, a 10 mm step, or a typed distance. */
export type SnapKind = "floor" | "level" | "run" | "step" | "typed";

/** Moves go in steps of this much, mm, unless a height catches them. */
export const STEP_MM = 10;

export interface SnapInput {
  axis: HandleAxis;
  /** The pointer's move along the axis, mm. */
  raw: number;
  /** Height of the handle's point above its level floor before the move, mm. */
  fromZ: number;
  /** The level's floor-to-floor height, mm. */
  levelHeight: number;
  /** Heights of runs nearby, mm above the same floor. */
  heights: readonly number[];
  /** How close a height has to come to catch the move, mm. */
  tolerance: number;
}

/**
 * Snaps a move. Upward or downward, the moved point catches the level floor
 * (z 0), the level height and the heights of runs nearby when it comes within
 * `tolerance`, the nearest first; otherwise, and along the plan axes, the
 * move goes in 10 mm steps.
 */
export function snapMove(s: SnapInput): { mm: number; snap: SnapKind } {
  if (s.axis === "z") {
    const to = s.fromZ + s.raw;
    const candidates: [number, SnapKind][] = [
      [0, "floor"],
      [s.levelHeight, "level"],
      ...s.heights.map((h): [number, SnapKind] => [h, "run"]),
    ];
    let best: [number, SnapKind] | null = null;
    let bestD = Infinity;
    for (const c of candidates) {
      const d = Math.abs(to - c[0]);
      if (d <= s.tolerance && d < bestD) {
        best = c;
        bestD = d;
      }
    }
    // Exact to the caught height, without float noise in the model.
    if (best) return { mm: Math.round((best[0] - s.fromZ) * 1000) / 1000 + 0, snap: best[1] };
  }
  return { mm: Math.round(s.raw / STEP_MM) * STEP_MM + 0, snap: "step" };
}

/** The axis an arrow key locks the move to, SketchUp style. Null for any other key. */
export function nextAxis(current: HandleAxis, key: string): HandleAxis | null {
  if (key === "ArrowUp" || key === "ArrowDown") return "z";
  if (key === "ArrowRight") return current === "x" ? "z" : "x";
  if (key === "ArrowLeft") return current === "y" ? "z" : "y";
  return null;
}

/** Point indices a pick moves: one point, a segment's two, or the whole run (empty). */
export function pickIndices(pipe: Pipe, pick: PipePick | null): number[] {
  if (!pick || pick.pipeId !== pipe.id) return [];
  const n = pipe.points.length;
  if (pick.kind === "point" && pick.index >= 0 && pick.index < n) return [pick.index];
  if (pick.kind === "segment" && pick.index >= 0 && pick.index + 1 < n) return [pick.index, pick.index + 1];
  return [];
}

const midpoint = (p: Vec3, q: Vec3): Vec3 => ({ x: (p.x + q.x) / 2, y: (p.y + q.y) / 2, z: (p.z + q.z) / 2 });

/** The middle of the longest segment of some polylines, or null when there is none. */
function longestMiddle(lines: readonly (readonly Vec3[])[]): Vec3 | null {
  let best = -1;
  let at: Vec3 | null = null;
  for (const pts of lines) {
    for (let i = 0; i + 1 < pts.length; i++) {
      const l = Math.hypot(pts[i + 1].x - pts[i].x, pts[i + 1].y - pts[i].y, pts[i + 1].z - pts[i].z);
      if (l > best) {
        best = l;
        at = midpoint(pts[i], pts[i + 1]);
      }
    }
  }
  return at;
}

/**
 * Where the handle stands, plan mm with z above the level floor: on a point,
 * at the middle of a segment, or for a whole run at the middle of its longest
 * segment (a real straight stretch, never a corner). With `cutMm` (the
 * cutaway's height above this floor) it stands on what the cut left: the
 * part of a segment below the cut, the longest segment of the run's parts
 * below it, and nothing for a point above it or a run the cut took away.
 */
export function handleAnchor(pipe: Pipe, indices: readonly number[], cutMm: number | null = null): Vec3 | null {
  const pts = pipe.points;
  if (pts.length === 0) return null;
  if (indices.length === 1) {
    const p = pts[indices[0]];
    return cutMm !== null && p.z > cutMm ? null : { ...p };
  }
  if (indices.length === 2) {
    const seg = [pts[indices[0]], pts[indices[1]]];
    if (cutMm === null) return midpoint(seg[0], seg[1]);
    return longestMiddle(cutRun(seg, cutMm)) ?? (seg[0].z <= cutMm && seg[1].z <= cutMm ? midpoint(seg[0], seg[1]) : null);
  }
  if (pts.length === 1) return { ...pts[0] };
  return longestMiddle(cutMm === null ? [pts] : cutRun(pts, cutMm));
}

/** Plan distance from a point to a segment, mm. */
function planDistance(p: Vec3, a: Vec3, b: Vec3): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const l2 = dx * dx + dy * dy;
  const t = l2 > 1e-9 ? Math.min(Math.max(((p.x - a.x) * dx + (p.y - a.y) * dy) / l2, 0), 1) : 0;
  return Math.hypot(p.x - (a.x + dx * t), p.y - (a.y + dy * t));
}

/** Runs this close in plan count as nearby, mm. */
export const NEAR_MM = 3000;

/**
 * Heights a move can catch, mm above the run's floor: the run's own points
 * that stay put, and the points of other runs on the same level that pass
 * within `radius` of the handle in plan.
 */
export function nearbyHeights(doc: DocState, pipe: Pipe, indices: readonly number[], at: Vec3, radius = NEAR_MM): number[] {
  const out = new Set<number>();
  if (indices.length > 0) {
    const moved = new Set(indices);
    pipe.points.forEach((p, i) => {
      if (!moved.has(i)) out.add(p.z);
    });
  }
  for (const e of doc.project.elements) {
    if (e.kind !== "pipe" || e.id === pipe.id || e.level_id !== pipe.level_id) continue;
    for (let i = 0; i < e.points.length; i++) {
      const p = e.points[i];
      const q = e.points[i + 1] ?? p;
      if (planDistance(at, p, q) > radius) continue;
      out.add(p.z);
      out.add(q.z);
    }
  }
  return [...out];
}

/** The command a handle move sends: a point, a segment or a whole run, by `delta`. */
export function moveCommand(pipeId: string, indices: readonly number[], delta: Vec3, keepJoins: boolean): Command {
  return { type: "move_pipe_points", targets: [{ pipe_id: pipeId, indices: [...indices] }], delta, keep_joins: keepJoins };
}

/**
 * A typed distance, mm. The number says how far, the pointer says which way
 * (SketchUp): typed while dragging down, it moves down. A typed minus turns it
 * around. Null while nothing usable is typed.
 */
export function typedMove(t: TypedState | null, unit: DisplayUnit, pointerMm: number): number | null {
  if (!t) return null;
  const v = typedValues(t, unit).a;
  if (v === null) return null;
  return (pointerMm < 0 ? -v : v) + 0;
}

const tmpDepth = new THREE.Vector3();

/** World meters per CSS pixel at `p` through a perspective camera. */
export function metersPerPixel(camera: THREE.PerspectiveCamera, p: THREE.Vector3, viewHeightPx: number): number {
  const depth = Math.max(-tmpDepth.copy(p).applyMatrix4(camera.matrixWorldInverse).z, camera.near);
  return (2 * depth * Math.tan((camera.fov * Math.PI) / 360)) / Math.max(viewHeightPx, 1);
}

/** Distance from a point to a 2D segment, pixels. */
export function segmentDistancePx(px: number, py: number, ax: number, ay: number, bx: number, by: number): number {
  const dx = bx - ax;
  const dy = by - ay;
  const l2 = dx * dx + dy * dy;
  const t = l2 > 1e-9 ? Math.min(Math.max(((px - ax) * dx + (py - ay) * dy) / l2, 0), 1) : 0;
  return Math.hypot(px - (ax + dx * t), py - (ay + dy * t));
}

// ---------------------------------------------------------------- the handle

/** What the engine lends the handle. */
export interface HandleHost {
  scene: THREE.Scene;
  camera: THREE.PerspectiveCamera;
  anim: Animator;
  /** The canvas's parent: the pointer listeners and the readout live on it. */
  container: HTMLElement;
  canvas: HTMLCanvasElement;
  /** Turns the orbit controls off while dragging, back on after. */
  setOrbitEnabled(on: boolean): void;
  /** Asks for a frame through the engine's single frame loop. */
  invalidate(): void;
  /** The document on screen (an active preview's state while there is one). */
  doc(): DocState | null;
  selection(): readonly string[];
  /** True in orbit. Walking and flying show no handle. */
  orbiting(): boolean;
  /** A pipe's solo in the current build, when the build shows that pipe. */
  solo(id: string): THREE.Object3D | undefined;
  /** World height, meters, the build cuts pipes at (cutaway in a solid shell), or null when runs are whole. */
  pipeCutY(): number | null;
  /**
   * A handle drag started: the user is using the view. Stops a camera glide
   * and counts as a hand on the view, which stops following someone (D37).
   */
  grab(): void;
}

/** The app side: preview, commit, the pick and the display unit. Tests pass their own. */
export interface HandleActions {
  preview(cmd: Command): Promise<ApplyResult>;
  showPreview(result: ApplyResult | null): void;
  currentPreview(): ApplyResult | null;
  /** Dispatches once. Null when the engine refused the move. */
  commit(cmd: Command): Promise<ApplyResult | null>;
  pick(): PipePick | null;
  onPick(fn: () => void): () => void;
  unit(): DisplayUnit;
}

const storeActions: HandleActions = {
  preview: (cmd) => ipc.docPreview(cmd),
  showPreview: (result) => useApp.getState().setPreview(result),
  currentPreview: () => useApp.getState().preview,
  commit: (cmd) => useApp.getState().dispatch(cmd),
  pick: () => usePipePick.getState().pick,
  onPick: (fn) => usePipePick.subscribe(fn),
  unit: () => useApp.getState().doc?.project.settings.display_unit ?? "mm",
};

/** Colors (tokens.css): the selection teal at rest, its strong shade under the pointer, SketchUp's axis colors when locked. */
const COLOR_REST = new THREE.Color("#0e8a8f"); // --accent
const COLOR_ACTIVE = new THREE.Color("#0a6d71"); // --accent-strong
const AXIS_COLOR: Record<HandleAxis, THREE.Color> = {
  z: new THREE.Color("#1f5f99"), // --blueprint
  x: new THREE.Color("#c0392b"), // --danger, red like SketchUp's x
  y: new THREE.Color("#2e8b57"), // --ok, green like SketchUp's y
};

/** Handle size in CSS pixels: center dot, gap, shaft end, arrow tip. */
const DOT_PX = 4;
const GAP_PX = 9;
const SHAFT_PX = 28;
const TIP_PX = 41;
/** How near the pointer has to be to grab it, and how far it travels before a press is a drag. */
const HIT_PX = 11;
const DRAG_PX = 4;
/** A height catches the move within this many pixels. */
const SNAP_PX = 8;
const HOVER_GROW = 0.15;
/** A committed move whose document never shows up stops holding the handle after this long. */
const HOLD_MAX_MS = 2000;

interface Target {
  pipe: Pipe;
  level: Level;
  indices: number[];
  /** Plan mm, z above the level floor. */
  anchor: Vec3;
  key: string;
}

interface Drag {
  target: Target;
  /** World point the handle started at. */
  origin: THREE.Vector3;
  axis: HandleAxis;
  /** The ray at the press: each axis reads the move from it, so a lock never jumps. */
  downRay: THREE.Ray;
  t0: number | null;
  client: { x: number; y: number };
  down: { x: number; y: number };
  pointerId: number;
  moved: boolean;
  /** Picked up with a click: follows the pointer until the next one. */
  sticky: boolean;
  /** The pointer's move along the axis before snapping, and the move it makes, mm. */
  raw: number;
  mm: number;
  snap: SnapKind;
  typed: TypedState | null;
  alt: boolean;
  heights: number[];
  previewFailed: boolean;
  /** What the last preview asked for. */
  asked: string;
}

/** Where a finished or cancelled move leaves the handle for a moment. */
interface Offset {
  origin: THREE.Vector3;
  axis: HandleAxis;
  mm: number;
}

/** True for a form control or editable text: keys there belong to it. */
export function editable(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.tagName !== "string") return false;
  const tag = el.tagName.toLowerCase();
  return tag === "input" || tag === "textarea" || tag === "select" || el.isContentEditable === true;
}

const tmpV = new THREE.Vector3();
const tmpDir = new THREE.Vector3();
const tmpNdc = new THREE.Vector2();
const UP = new THREE.Vector3(0, 1, 0);

export class PipeHandle {
  private group = new THREE.Group();
  private arrow = new THREE.Group();
  private guide: THREE.Mesh;
  private start: THREE.Mesh;
  private colorMat: THREE.MeshBasicMaterial;
  private haloMat: THREE.MeshBasicMaterial;
  private guideMat: THREE.MeshBasicMaterial;
  private geometries: THREE.BufferGeometry[] = [];
  private label: HTMLDivElement;
  private labelMain: HTMLSpanElement;
  private labelSub: HTMLSpanElement;
  private labelText = "";

  private target: Target | null = null;
  private shown = false;
  private hovered = false;
  private drag: Drag | null = null;
  private hold: (Offset & { state: DocState | null; until: number }) | null = null;
  private back: Offset | null = null;
  private ownPreview: ApplyResult | null = null;
  /** Bumped when a drag ends: a preview that comes back later is dropped. */
  private session = 0;
  private inFlight = false;
  private askAgain = false;
  private captureHidden = false;
  private wasOrbiting = true;
  private lastCut: number | null = null;
  private disposed = false;
  private raycaster = new THREE.Raycaster();
  private offPick: () => void;

  constructor(
    private host: HandleHost,
    private actions: HandleActions = storeActions,
  ) {
    this.colorMat = this.basic(COLOR_REST, 1);
    this.haloMat = this.basic(new THREE.Color("#ffffff"), 0.9);
    this.guideMat = this.basic(AXIS_COLOR.z, 0.9);
    this.buildArrow();
    const shaft = this.track(new THREE.CylinderGeometry(1, 1, 1, 10));
    this.guide = new THREE.Mesh(shaft, this.guideMat);
    this.start = new THREE.Mesh(this.track(new THREE.SphereGeometry(1, 12, 8)), this.guideMat);
    for (const m of [this.guide, this.start]) {
      m.renderOrder = 29;
      m.visible = false;
      m.frustumCulled = false;
      m.raycast = () => {};
    }
    this.group.name = "pipe-handle";
    this.group.add(this.arrow);
    this.group.visible = false;
    host.scene.add(this.group, this.guide, this.start);

    this.label = document.createElement("div");
    this.label.className = styles.label;
    this.label.dataset.testid = "pipe-handle-label";
    const inner = document.createElement("div");
    inner.className = styles.inner;
    this.labelMain = document.createElement("span");
    this.labelMain.className = styles.main;
    this.labelSub = document.createElement("span");
    this.labelSub.className = styles.sub;
    inner.append(this.labelMain, this.labelSub);
    this.label.append(inner);
    host.container.append(this.label);

    host.container.addEventListener("pointerdown", this.onDown, { capture: true });
    host.container.addEventListener("pointermove", this.onMove, { capture: true });
    host.container.addEventListener("pointerup", this.onUp, { capture: true });
    host.container.addEventListener("pointercancel", this.onCancel, { capture: true });
    host.container.addEventListener("pointerleave", this.onLeave);
    this.offPick = actions.onPick(() => this.sync());
  }

  private basic(color: THREE.Color, opacity: number): THREE.MeshBasicMaterial {
    return new THREE.MeshBasicMaterial({ color, transparent: true, opacity, depthTest: false, depthWrite: false, toneMapped: false, fog: false });
  }

  private track<T extends THREE.BufferGeometry>(g: T): T {
    this.geometries.push(g);
    return g;
  }

  /** A double arrow along +y in pixel units, white halo behind the teal. */
  private buildArrow(): void {
    const sphere = this.track(new THREE.SphereGeometry(1, 16, 10));
    const shaft = this.track(new THREE.CylinderGeometry(1, 1, 1, 12));
    const cone = this.track(new THREE.ConeGeometry(1, 1, 18));
    const add = (geo: THREE.BufferGeometry, halo: boolean, y: number, sx: number, sy: number, flip: boolean) => {
      const m = new THREE.Mesh(geo, halo ? this.haloMat : this.colorMat);
      m.position.y = y;
      m.scale.set(sx, sy, sx);
      if (flip) m.rotation.x = Math.PI;
      m.renderOrder = halo ? 30 : 31;
      m.frustumCulled = false;
      m.raycast = () => {};
      this.arrow.add(m);
    };
    const shaftMid = (GAP_PX + SHAFT_PX) / 2;
    const shaftLen = SHAFT_PX - GAP_PX;
    const coneMid = (SHAFT_PX + TIP_PX) / 2;
    const coneLen = TIP_PX - SHAFT_PX;
    for (const halo of [true, false]) {
      const k = halo ? 1 : 0;
      add(sphere, halo, 0, DOT_PX + 2 * k, DOT_PX + 2 * k, false);
      for (const s of [1, -1]) {
        add(shaft, halo, s * shaftMid, 1.6 + 1.6 * k, shaftLen + 2 * k, false);
        add(cone, halo, s * (coneMid + k * 0.5), 6 + 2.5 * k, coneLen + 3 * k, s < 0);
      }
    }
  }

  // ------------------------------------------------------------- target

  /**
   * Re-reads what the handle stands on: the selection, the pick, the
   * document, walking or not. Cheap; the engine calls it on every document,
   * selection and navigation change.
   */
  sync(): void {
    if (this.disposed) return;
    const doc = this.host.doc();
    if (this.drag) {
      // The run went away or the selection moved on mid-drag: let go.
      const id = this.drag.target.pipe.id;
      const sel = this.host.selection();
      if (!doc || sel.length !== 1 || sel[0] !== id || !doc.project.elements.some((e) => e.id === id)) this.cancel();
      return;
    }
    if (this.hold && doc && (doc === this.hold.state || (this.hold.state && doc.revision > this.hold.state.revision) || performance.now() > this.hold.until)) {
      this.hold = null;
    }
    const prevKey = this.target?.key ?? null;
    const next = this.findTarget(doc);
    this.target = next;
    const now = performance.now();
    if (next && (!this.shown || next.key !== prevKey)) {
      // A new handle, or the same one on another part: it grows in where it stands.
      this.shown = true;
      this.host.anim.set("handle", 0);
      this.host.anim.to("handle", 1, now, { duration: dur("base"), easing: ease.spring });
      this.host.invalidate();
    } else if (!next && this.shown) {
      this.shown = false;
      this.setHover(false);
      this.host.anim.to("handle", 0, now, { duration: dur("base") * 0.7, easing: ease.in, onDone: () => this.host.invalidate() });
      this.host.invalidate();
    } else if (next) {
      this.host.invalidate();
    }
  }

  private findTarget(doc: DocState | null): Target | null {
    if (!doc || !this.host.orbiting()) return null;
    const sel = this.host.selection();
    if (sel.length !== 1) return null;
    // Someone else's preview is on screen (an AI proposal, a nudge): it owns the ghost.
    const preview = this.actions.currentPreview();
    if (preview && preview !== this.ownPreview) return null;
    const e = doc.project.elements.find((x) => x.id === sel[0]);
    if (!e || e.kind !== "pipe" || e.points.length < 2) return null;
    const solo = this.host.solo(e.id);
    // Nothing drawn (the cutaway took the whole run away): nothing to grab.
    if (!solo || solo.userData.locked || !solo.children.some((c) => (c as THREE.Mesh).isMesh)) return null;
    const levels = doc.project.levels;
    const level = levels.find((l) => l.id === e.level_id) ?? [...levels].sort((a, b) => a.elevation_mm - b.elevation_mm)[0];
    if (!level) return null;
    const indices = pickIndices(e, this.actions.pick());
    const cutY = this.host.pipeCutY();
    const anchor = handleAnchor(e, indices, cutY === null ? null : Math.round(cutY * 1000) - level.elevation_mm);
    if (!anchor) return null;
    return { pipe: e, level, indices, anchor, key: `${e.id}|${indices.join(",")}` };
  }

  private worldOf(t: Target, out = new THREE.Vector3()): THREE.Vector3 {
    return out.set(t.anchor.x / 1000, (t.level.elevation_mm + t.anchor.z) / 1000, -t.anchor.y / 1000);
  }

  /** Where the handle is drawn now, world meters. */
  private position(out: THREE.Vector3): THREE.Vector3 | null {
    const d = this.drag;
    if (d) return out.copy(d.origin).addScaledVector(axisWorld(d.axis, tmpDir), d.mm / 1000);
    if (this.hold) return out.copy(this.hold.origin).addScaledVector(axisWorld(this.hold.axis, tmpDir), this.hold.mm / 1000);
    if (this.back) {
      const k = this.host.anim.value("handle-back", 0);
      return out.copy(this.back.origin).addScaledVector(axisWorld(this.back.axis, tmpDir), (this.back.mm * k) / 1000);
    }
    return this.target ? this.worldOf(this.target, out) : null;
  }

  private axisNow(): HandleAxis {
    return this.drag?.axis ?? this.hold?.axis ?? this.back?.axis ?? "z";
  }

  // ------------------------------------------------------------- pointer

  /** True when a client point is on the drawn handle. */
  private hit(clientX: number, clientY: number): boolean {
    if (!this.target || !this.shown || this.hold || this.captureHidden) return false;
    const rect = this.host.canvas.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return false;
    const cam = this.host.camera;
    cam.updateMatrixWorld();
    const at = this.position(new THREE.Vector3());
    if (!at) return false;
    const mpp = metersPerPixel(cam, at, rect.height) * this.scaleNow();
    const dir = axisWorld(this.axisNow(), new THREE.Vector3());
    const a = at.clone().addScaledVector(dir, TIP_PX * mpp).project(cam);
    const b = at.clone().addScaledVector(dir, -TIP_PX * mpp).project(cam);
    if (a.z > 1 || b.z > 1) return false;
    const sx = (v: THREE.Vector3) => rect.left + ((v.x + 1) / 2) * rect.width;
    const sy = (v: THREE.Vector3) => rect.top + ((1 - v.y) / 2) * rect.height;
    return segmentDistancePx(clientX, clientY, sx(a), sy(a), sx(b), sy(b)) <= HIT_PX;
  }

  private rayAt(clientX: number, clientY: number, out: THREE.Ray): boolean {
    const rect = this.host.canvas.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return false;
    tmpNdc.set(((clientX - rect.left) / rect.width) * 2 - 1, -((clientY - rect.top) / rect.height) * 2 + 1);
    this.host.camera.updateMatrixWorld();
    this.raycaster.setFromCamera(tmpNdc, this.host.camera);
    out.copy(this.raycaster.ray);
    return true;
  }

  private onDown = (e: PointerEvent): void => {
    const d = this.drag;
    if (d?.sticky) {
      // The click that puts a picked-up handle down. Any other button drops it.
      e.stopPropagation();
      e.preventDefault();
      if (e.button === 0) {
        d.client = { x: e.clientX, y: e.clientY };
        d.alt = e.altKey;
        void this.commit();
      } else this.cancel();
      return;
    }
    if (d || e.button !== 0 || !this.hit(e.clientX, e.clientY)) return;
    // Ahead of the canvas: neither the orbit controls nor the picker see this press.
    e.stopPropagation();
    e.preventDefault();
    this.begin(e);
  };

  private onMove = (e: PointerEvent): void => {
    const d = this.drag;
    if (d) {
      e.stopPropagation();
      d.client = { x: e.clientX, y: e.clientY };
      d.alt = e.altKey;
      if (!d.moved && Math.hypot(e.clientX - d.down.x, e.clientY - d.down.y) > DRAG_PX) d.moved = true;
      this.host.invalidate();
      return;
    }
    if (e.buttons !== 0) return;
    this.setHover(this.hit(e.clientX, e.clientY));
  };

  private onUp = (e: PointerEvent): void => {
    const d = this.drag;
    if (!d || d.sticky || e.pointerId !== d.pointerId) return;
    e.stopPropagation();
    this.releaseCapture(d);
    if (!d.moved) {
      // A click: picked up. It follows the pointer until the next click.
      d.sticky = true;
      this.host.invalidate();
      return;
    }
    d.client = { x: e.clientX, y: e.clientY };
    d.alt = e.altKey;
    void this.commit();
  };

  private onCancel = (e: PointerEvent): void => {
    if (this.drag && !this.drag.sticky && e.pointerId === this.drag.pointerId) this.cancel();
  };

  private onLeave = (): void => {
    if (!this.drag) this.setHover(false);
  };

  private releaseCapture(d: Drag): void {
    try {
      if (this.host.container.hasPointerCapture(d.pointerId)) this.host.container.releasePointerCapture(d.pointerId);
    } catch {
      // the pointer is already gone
    }
  }

  private setHover(on: boolean): void {
    if (on === this.hovered) return;
    this.hovered = on;
    this.host.anim.to("handle-hover", on ? 1 : 0, performance.now(), { duration: dur("hover"), easing: on ? ease.out : ease.in });
    this.setCursor();
    this.host.invalidate();
  }

  private setCursor(): void {
    this.host.container.style.cursor = this.drag ? "grabbing" : this.hovered ? "grab" : "";
  }

  // ------------------------------------------------------------- the drag

  private begin(e: PointerEvent): void {
    const t = this.target;
    const doc = this.host.doc();
    if (!t || !doc) return;
    const downRay = new THREE.Ray();
    if (!this.rayAt(e.clientX, e.clientY, downRay)) return;
    const origin = this.worldOf(t);
    // A hand on the view: a camera glide stops where it is, following someone stops.
    this.host.grab();
    this.back = null;
    this.host.anim.remove("handle-back");
    this.drag = {
      target: t,
      origin,
      axis: "z",
      downRay,
      t0: axisParam(downRay, origin, UP, this.host.camera.position),
      client: { x: e.clientX, y: e.clientY },
      down: { x: e.clientX, y: e.clientY },
      pointerId: e.pointerId,
      moved: false,
      sticky: false,
      raw: 0,
      mm: 0,
      snap: "step",
      typed: null,
      alt: e.altKey,
      heights: nearbyHeights(doc, t.pipe, t.indices, t.anchor),
      previewFailed: false,
      asked: "",
    };
    this.session++;
    try {
      this.host.container.setPointerCapture(e.pointerId);
    } catch {
      // a synthetic pointer: the window listeners below still follow it
    }
    this.host.setOrbitEnabled(false);
    window.addEventListener("keydown", this.onKey, { capture: true });
    window.addEventListener("keyup", this.onKeyUp, { capture: true });
    window.addEventListener("pointerdown", this.onOutsideDown, { capture: true });
    window.addEventListener("blur", this.onBlur);
    this.hovered = true;
    this.host.anim.to("handle-hover", 1, performance.now(), { duration: dur("press"), easing: ease.out });
    this.setCursor();
    this.host.invalidate();
  }

  /** Reads the move from the latest pointer: once per drawn frame, and once more on release. */
  private update(d: Drag): void {
    const ray = new THREE.Ray();
    const cam = this.host.camera;
    if (this.rayAt(d.client.x, d.client.y, ray)) {
      const t = axisParam(ray, d.origin, axisWorld(d.axis, tmpDir), cam.position);
      if (t !== null) {
        if (d.t0 === null) d.t0 = t;
        d.raw = (t - d.t0) * 1000;
      }
    }
    const typed = typedMove(d.typed, this.actions.unit(), d.raw);
    if (typed !== null) {
      d.mm = typed;
      d.snap = "typed";
      return;
    }
    const rect = this.host.canvas.getBoundingClientRect();
    const tolerance = SNAP_PX * metersPerPixel(cam, tmpV.copy(d.origin), rect.height) * 1000;
    const s = snapMove({
      axis: d.axis,
      raw: d.raw,
      fromZ: d.target.anchor.z,
      levelHeight: d.target.level.height_mm,
      heights: d.heights,
      tolerance,
    });
    d.mm = s.mm;
    d.snap = s.snap;
  }

  private command(d: Drag, mm = d.mm): Command {
    return moveCommand(d.target.pipe.id, d.target.indices, axisDelta(d.axis, mm), !d.alt);
  }

  private setAxis(d: Drag, axis: HandleAxis): void {
    if (axis === d.axis) return;
    d.axis = axis;
    // Read along the new axis from the press, the way SketchUp's lock does: no jump.
    d.t0 = axisParam(d.downRay, d.origin, axisWorld(axis, tmpDir), this.host.camera.position);
    d.raw = 0;
    this.host.invalidate();
  }

  /** One preview in flight at a time; the latest move goes next. */
  private askPreview(d: Drag): void {
    const key = `${d.axis}|${d.mm}|${d.alt}`;
    if (key === d.asked) return;
    if (this.inFlight) {
      this.askAgain = true;
      return;
    }
    d.asked = key;
    if (d.mm === 0) {
      this.dropPreview();
      return;
    }
    const session = this.session;
    this.inFlight = true;
    this.actions
      .preview(this.command(d))
      .then((result) => {
        if (session !== this.session || this.drag !== d) return;
        d.previewFailed = false;
        this.ownPreview = result;
        this.actions.showPreview(result);
      })
      .catch(() => {
        if (session !== this.session || this.drag !== d) return;
        // The handle still moves; the run follows once the engine can preview it.
        d.previewFailed = true;
        this.dropPreview();
        this.host.invalidate();
      })
      .finally(() => {
        this.inFlight = false;
        if (this.askAgain) {
          this.askAgain = false;
          if (this.drag) this.askPreview(this.drag);
        }
      });
  }

  /** Clears the preview when it is still this handle's. */
  private dropPreview(): void {
    const own = this.ownPreview;
    this.ownPreview = null;
    if (own && this.actions.currentPreview() === own) this.actions.showPreview(null);
  }

  private endDrag(): void {
    const d = this.drag;
    if (!d) return;
    this.drag = null;
    this.session++;
    this.askAgain = false;
    this.releaseCapture(d);
    window.removeEventListener("keydown", this.onKey, { capture: true });
    window.removeEventListener("keyup", this.onKeyUp, { capture: true });
    window.removeEventListener("pointerdown", this.onOutsideDown, { capture: true });
    window.removeEventListener("blur", this.onBlur);
    this.host.setOrbitEnabled(true);
    this.hovered = false;
    this.host.anim.to("handle-hover", 0, performance.now(), { duration: dur("hover"), easing: ease.in });
    this.setCursor();
  }

  /** Release, a click or Enter: dispatches the move once, keeping joins unless Alt is down. */
  private async commit(): Promise<void> {
    const d = this.drag;
    if (!d) return;
    this.update(d);
    this.endDrag();
    if (d.mm === 0) {
      this.dropPreview();
      this.sync();
      this.host.invalidate();
      return;
    }
    // The handle stays where it was let go until the moved run is on screen.
    const hold = { origin: d.origin, axis: d.axis, mm: d.mm, state: null as DocState | null, until: Number.POSITIVE_INFINITY };
    this.hold = hold;
    this.host.invalidate();
    const result = await this.actions.commit(this.command(d));
    // Before anything else, even when the view closed meanwhile: a preview
    // left behind would stand over every later edit on the plan.
    this.dropPreview();
    if (this.disposed) return;
    if (this.hold === hold) {
      if (result) {
        hold.state = result.state;
        hold.until = performance.now() + HOLD_MAX_MS;
      } else this.hold = null;
    }
    if (!result) this.easeBack(d);
    this.sync();
    this.host.invalidate();
  }

  /** Esc, a lost pointer or a focus change: the preview goes and the handle eases back. */
  private cancel(): void {
    const d = this.drag;
    if (!d) return;
    this.endDrag();
    this.dropPreview();
    this.easeBack(d);
    this.sync();
  }

  /** A cancelled or refused move: the handle eases back to where it started. */
  private easeBack(d: Drag): void {
    this.host.invalidate();
    if (d.mm === 0) return;
    const back: Offset = { origin: d.origin, axis: d.axis, mm: d.mm };
    this.back = back;
    this.host.anim.set("handle-back", 1);
    this.host.anim.to("handle-back", 0, performance.now(), {
      duration: dur("base"),
      easing: ease.out,
      onDone: () => {
        if (this.back === back) this.back = null;
        this.host.invalidate();
      },
    });
  }

  private onKey = (e: KeyboardEvent): void => {
    const d = this.drag;
    // Typing into a field is never a distance.
    if (!d || editable(e.target)) return;
    if (e.key === "Alt") {
      d.alt = true;
      this.host.invalidate();
      return;
    }
    if (e.key === "Shift" || e.key === "Meta" || e.key === "Control") return;
    // A shortcut with a modifier: the move gives way to it.
    if (e.metaKey || e.ctrlKey) {
      this.cancel();
      return;
    }
    // Everything else belongs to the move while it lasts, the way SketchUp's
    // measurements box takes the keys: a digit is a distance, not a view.
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      this.cancel();
      return;
    }
    if (e.key === "Enter") {
      void this.commit();
      return;
    }
    const axis = nextAxis(d.axis, e.key);
    if (axis) {
      this.setAxis(d, axis);
      return;
    }
    if (/^[0-9]$/.test(e.key) || e.key === "." || e.key === "-" || e.key === "Backspace") {
      d.typed = typedKey(d.typed ?? emptyTyped("height"), e.key);
      this.host.invalidate();
    }
  };

  private onKeyUp = (e: KeyboardEvent): void => {
    if (this.drag && e.key === "Alt") {
      this.drag.alt = false;
      this.host.invalidate();
    }
  };

  private onBlur = (): void => this.cancel();

  /** A press anywhere outside the 3D view (a field, the inspector, the plan) drops a move in progress. */
  private onOutsideDown = (e: PointerEvent): void => {
    const target = e.target as Node | null;
    if (this.drag && !(target && this.host.container.contains(target))) this.cancel();
  };

  // ------------------------------------------------------------- drawing

  private scaleNow(): number {
    return this.host.anim.value("handle", this.shown ? 1 : 0) * (1 + HOVER_GROW * this.host.anim.value("handle-hover", 0));
  }

  /**
   * Right before every live render: reads the drag, places the handle at a
   * constant size on screen, the guide along the move and the readout.
   */
  beforeRender(width: number, height: number): void {
    if (this.disposed) return;
    // Walking and flying have no handle: it goes when a walk starts and comes back after.
    const orbit = this.host.orbiting();
    if (orbit !== this.wasOrbiting) {
      this.wasOrbiting = orbit;
      if (!orbit) this.cancel();
      this.sync();
    }
    // The cut came or went (the cutaway landed, the shell changed): the handle moves onto what is drawn.
    const cut = this.host.pipeCutY();
    if (cut !== this.lastCut) {
      this.lastCut = cut;
      this.sync();
    }
    const d = this.drag;
    if (d) {
      this.update(d);
      this.askPreview(d);
    }
    const cam = this.host.camera;
    cam.updateMatrixWorld();
    const at = this.position(tmpV);
    const grow = this.scaleNow();
    const visible = !!at && grow > 0.001 && !this.captureHidden;
    this.group.visible = visible;
    if (!visible || !at) {
      this.guide.visible = this.start.visible = false;
      this.setLabel(null, width, height);
      return;
    }
    const mpp = metersPerPixel(cam, at, height);
    const axis = this.axisNow();
    const dir = axisWorld(axis, tmpDir);
    this.group.position.copy(at);
    this.group.quaternion.setFromUnitVectors(UP, dir);
    this.group.scale.setScalar(Math.max(mpp * grow, 1e-6));
    const active = !!d || this.hovered;
    this.colorMat.color.copy(axis !== "z" ? AXIS_COLOR[axis] : active ? COLOR_ACTIVE : COLOR_REST);

    // The guide: from where the move started to where the handle is, in the axis color.
    const from = d?.origin ?? this.hold?.origin ?? this.back?.origin ?? null;
    const len = from ? at.distanceTo(from) : 0;
    if (from && len > 1e-4) {
      this.guideMat.color.copy(AXIS_COLOR[axis]);
      this.guide.visible = this.start.visible = true;
      this.guide.position.copy(from).add(at).multiplyScalar(0.5);
      this.guide.quaternion.setFromUnitVectors(UP, dir);
      this.guide.scale.set(mpp * 1.2, len, mpp * 1.2);
      this.start.position.copy(from);
      this.start.scale.setScalar(mpp * 3);
    } else {
      this.guide.visible = this.start.visible = false;
    }
    this.setLabel(at, width, height);
  }

  /** The readout: the hint on hover, the move while dragging. Text changes only when it differs. */
  private setLabel(at: THREE.Vector3 | null, width: number, height: number): void {
    const d = this.drag;
    const lines = at && !this.captureHidden ? this.labelLines(d) : null;
    const on = lines !== null;
    if (this.label.dataset.on !== String(on)) this.label.dataset.on = String(on);
    if (!lines || !at) return;
    const text = `${lines.main}\n${lines.sub}\n${lines.typed}`;
    if (text !== this.labelText) {
      this.labelText = text;
      this.labelMain.textContent = lines.main;
      this.labelMain.dataset.typed = String(lines.typed);
      this.labelSub.textContent = lines.sub;
    }
    const v = tmpDepth.copy(at).project(this.host.camera);
    if (v.z > 1) {
      this.label.dataset.on = "false";
      return;
    }
    const x = ((v.x + 1) / 2) * width + 26;
    const y = ((1 - v.y) / 2) * height;
    this.label.style.transform = `translate(${Math.round(x)}px, ${Math.round(y)}px) translateY(-50%)`;
  }

  private labelLines(d: Drag | null): { main: string; sub: string; typed: boolean } | null {
    const unit = this.actions.unit();
    if (!d) {
      if (!this.hovered || this.hold || this.back || !this.target) return null;
      const what = this.target.indices.length === 1 ? "point" : this.target.indices.length === 2 ? "segment" : "run";
      return { main: `Drag to move the ${what} up or down`, sub: "Or click it, then type a distance", typed: false };
    }
    const typedText = d.typed && d.typed.fields[0] !== "" ? d.typed.fields[0] : null;
    const dist = formatLength(Math.abs(d.mm), unit, true);
    const word = d.axis === "z" ? (d.mm < 0 ? "Down" : "Up") : d.axis === "x" ? (d.mm < 0 ? "West" : "East") : d.mm < 0 ? "South" : "North";
    const main = typedText !== null ? `${word} ${typedText} ${unit}` : `${word} ${dist}`;
    const notes: string[] = [];
    if (d.axis === "z") notes.push(`Height ${formatLength(d.target.anchor.z + d.mm, unit, true)}`);
    else notes.push(`Locked to plan ${d.axis}`);
    if (d.snap === "floor") notes.push("at the floor");
    else if (d.snap === "level") notes.push("at the level height");
    else if (d.snap === "run") notes.push("level with a run");
    if (d.alt) notes.push("joins break");
    if (d.previewFailed) notes.push("no preview yet");
    return { main, sub: notes.join(", "), typed: typedText !== null };
  }

  /** Hidden for a capture: a picture of the model never shows the handle. */
  setCaptureHidden(hidden: boolean): void {
    this.captureHidden = hidden;
    if (hidden) {
      this.group.visible = this.guide.visible = this.start.visible = false;
    }
  }

  /** True while a drag owns the pointer. */
  dragging(): boolean {
    return this.drag !== null;
  }

  stats(): { shown: boolean; target: string | null; dragging: boolean; axis: HandleAxis | null; mm: number | null } {
    return {
      shown: this.shown,
      target: this.target?.key ?? null,
      dragging: !!this.drag,
      axis: this.drag?.axis ?? null,
      mm: this.drag?.mm ?? null,
    };
  }

  dispose(): void {
    if (this.disposed) return;
    this.endDrag();
    // Mid-drag, or a released move still being applied: the preview is this handle's to clear.
    this.dropPreview();
    this.disposed = true;
    this.offPick();
    const c = this.host.container;
    c.removeEventListener("pointerdown", this.onDown, { capture: true });
    c.removeEventListener("pointermove", this.onMove, { capture: true });
    c.removeEventListener("pointerup", this.onUp, { capture: true });
    c.removeEventListener("pointercancel", this.onCancel, { capture: true });
    c.removeEventListener("pointerleave", this.onLeave);
    c.style.cursor = "";
    this.label.remove();
    this.host.scene.remove(this.group, this.guide, this.start);
    for (const g of this.geometries) g.dispose();
    this.colorMat.dispose();
    this.haloMat.dispose();
    this.guideMat.dispose();
    for (const key of ["handle", "handle-hover", "handle-back"]) this.host.anim.remove(key);
  }
}
