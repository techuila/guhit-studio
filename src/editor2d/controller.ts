// Interaction controller for the plan canvas. Owns the view, the current
// operation and all pointer and keyboard handling. It never mutates the
// document: every change is one `dispatch(command)`.

import type {
  Annotation,
  Asset,
  Camera,
  CatalogItem,
  Column,
  Command,
  Dimension,
  Element,
  OpeningStyle,
  PipeSystem,
  Room,
  Stair,
  Vec3,
  Wall,
} from "../contract/bindings";
import { ipc } from "../contract/ipc";
import { bus } from "../state/bus";
import { useApp, type AppState, type Tool } from "../state/store";
import { dur, ease, motionOK } from "../ui/motion";
import { useViewer } from "../viewer3d/viewerStore";
import { Anim, breathe, mix, mixP } from "./anim";
import type { Grip } from "./edit";
import { gripsFor, hitGrip, jointInset, normalDelta, rotationFromGrip } from "./edit";
import type { P, Rect } from "./geom";
import { add, angleDeg, dirDeg, dist, dot, mul, normDeg, rectFromPoints, rectIsEmpty, sub, unit } from "./geom";
import { hitTest, marqueeSelect } from "./hit";
import type { DocIndex, OpeningEl, WallEl } from "./model";
import { boundsOfIds, buildIndex, isLocked, keyPoints, modelBounds, wallOutline } from "./model";
import { drawOverlay } from "./overlay";
import type { FaceSnap, OpeningPlacement, OpeningSpan } from "./place";
import { doorSwingSide, findHostWall, placeOnWall, roomSideOfWall, snapToFace } from "./place";
import type { FixturePoint, PipeEl, PipeSnapScene, PipeSpec } from "./pipe";
import {
  PIPE_HEIGHT_STEP,
  PIPE_HEIGHT_STEP_FINE,
  PIPE_SYSTEM_LABEL,
  addRunPoint,
  finishRun,
  fixturePoints,
  isPlumbingFixture,
  movePipeNode,
  pipeNodes,
  pipeSpec,
  planOf,
  popRunPoint,
  snapToPipes,
  toolFallPct,
  withPendingRiser,
} from "./pipe";
import type { ElementStyle, Palette, RenderContext } from "./render";
import { DEFAULT_PALETTE, drawElement, drawFlash, drawGrid, drawHighlight, drawModel, drawOrigin, labelHeightMm, readPalette } from "./render";
import { decideResize } from "./resizePolicy";
import type { SnapResult, SnapScene } from "./snap";
import { computeIntersections, emptyScene, snap } from "./snap";
import type { TypedState } from "./typed";
import { emptyTyped, formatArea, isTypedKey, typedIsEmpty, typedKey, typedValues } from "./typed";
import type { View } from "./view";
import { fitRect, panBy, snapStep, toScreen, toWorld, zoomAt } from "./view";

export const SNAP_PX = 10;
const DRAG_PX = 4;
const GRIP_PX = 9;
/** Two presses closer than this in time and place are a double click. */
const DOUBLE_CLICK_MS = 500;
const DOUBLE_CLICK_PX = 5;

/**
 * The controller currently mounted on the plan canvas, if any. Lets shell
 * code reach a couple of view-only actions (keyboard zoom step) that have no
 * document command and so cannot go through `dispatch` or the event bus.
 */
let activeController: PlanController | null = null;
export function getActiveController(): PlanController | null {
  return activeController;
}

/**
 * Animation keys. Everything per element runs 1 to 0 with 0 as its rest value,
 * so a finished track can be forgotten without changing what is drawn.
 * See docs/MOTION.md for which inventory row each one serves.
 */
export const K = {
  /** `hov:<id>`, `sel:<id>`: hover tint and selection outline, 0 to 1. */
  hover: "hov:",
  sel: "sel:",
  /** `grip:<index>`: grips scale in from 0, staggered. */
  grip: "grip:",
  snapAlpha: "snap.a",
  snapPop: "snap.pop",
  /** Drag pick up (shadow and tint) and the ease back after a rejected drop. */
  lift: "drag.lift",
  back: "drag.back",
  marquee: "marquee",
  ghostOpening: "ghost.o",
  ghostPlace: "ghost.p",
  swing: "ghost.swing",
  hinge: "ghost.hinge",
  /** `add:<id>` placed and settling, `fl:<id>` changed and flashing, `rm:<id>` removed and fading. */
  add: "add:",
  flash: "fl:",
  remove: "rm:",
  /** `area:<id>`: a room's area readout cross fading to its new value. */
  area: "area:",
  view: "view",
  /** `griph:<index>`: the grip under the pointer grows a little. */
  gripHover: "griph:",
  /** The ring where the pipe tool placed a point, and the height tag after a height change. */
  pipePulse: "pipe.pulse",
  pipeHeight: "pipe.h",
} as const;

/** Grips beyond this many share the last stagger step (docs/MOTION.md: at most 8). */
const GRIP_STAGGER = 8;

/** Leaving is faster than entering: about 70% of the enter duration. */
function exitDur(key: Parameters<typeof dur>[0]): number {
  return Math.round(dur(key) * 0.7);
}

/** Period of the AI preview breath, in ms. */
const BREATH_MS = 1600;

export type Op =
  | { kind: "idle" }
  | { kind: "pan"; last: P }
  | { kind: "press"; downScreen: P; downWorld: P; hitId: string | null; additive: boolean; alt: boolean }
  | { kind: "marquee"; start: P; current: P; additive: boolean }
  | { kind: "move"; ids: string[]; ref: P; delta: P; duplicate: boolean; committing: boolean }
  | { kind: "slide"; opening: OpeningEl; host: Wall; placement: OpeningPlacement; committing: boolean }
  | { kind: "grip"; grip: Grip; element: Element; current: P; z: number | null; committing: boolean }
  | { kind: "wall"; points: P[]; typed: TypedState | null; committing: boolean }
  | { kind: "pipe"; points: Vec3[]; penZ: number; typed: TypedState | null; committing: boolean }
  | { kind: "rect"; origin: P; downScreen: P; typed: TypedState | null; committing: boolean }
  | { kind: "dimension"; a: P; b: P | null; committing: boolean }
  | { kind: "camera"; position: P; committing: boolean };

export interface InlineEditor {
  mode: "room" | "annotation" | "new_text";
  id: string | null;
  world: P;
  value: string;
  fontPx: number;
}

export interface UiState {
  typed: { x: number; y: number; state: TypedState; labels: [string, string] } | null;
  editor: (InlineEditor & { x: number; y: number }) | null;
  cursor: string;
  hint: string | null;
}

/** Ghost of the door or window tool while it hovers a wall. */
export interface OpeningGhost {
  opening: OpeningEl;
  host: Wall;
  placement: OpeningPlacement;
}

export interface PlacementGhost {
  element: Element;
  faceSnap: FaceSnap | null;
}

const DOOR_STYLES: OpeningStyle[] = ["swing_single", "swing_double", "sliding"];
const WINDOW_STYLES: OpeningStyle[] = ["sliding", "fixed", "casement", "jalousie"];

export function openingDefaults(type: "door" | "window", style: OpeningStyle | null): {
  style: OpeningStyle;
  width: number;
  height: number;
  sill: number;
} {
  if (type === "door") {
    const s = style && DOOR_STYLES.includes(style) ? style : "swing_single";
    return { style: s, width: s === "swing_double" ? 1500 : s === "sliding" ? 1800 : 900, height: 2100, sill: 0 };
  }
  const s = style && WINDOW_STYLES.includes(style) ? style : "sliding";
  return { style: s, width: 1200, height: 1200, sill: 900 };
}

export class PlanController {
  view: View = { scale: 0.08, ox: 300, oy: 300 };
  width = 0;
  height = 0;
  palette: Palette = DEFAULT_PALETTE;
  index: DocIndex | null = null;
  /** Index of the committed document while an AI preview is showing. */
  realIndex: DocIndex | null = null;
  op: Op = { kind: "idle" };
  cursorScreen: P | null = null;
  cursorWorld: P | null = null;
  snapResult: SnapResult | null = null;
  shift = false;
  space = false;
  pointerInside = false;
  /** Door and window tool toggles (F and H). */
  flipSide = false;
  flipHinge = false;
  /** Quarter turns for the asset, stair and column tools (R). */
  turns = 0;
  /** Pipe tool: the height being typed after `h`, idle or while drawing. */
  heightEntry: TypedState | null = null;
  /** Where the pipe tool last placed a point, for its settle ring. */
  pipePulse: { at: P; system: PipeSystem } | null = null;
  /** Index into `gripList()` of the grip under the pointer. */
  hoverGrip: number | null = null;
  openingGhost: OpeningGhost | null = null;
  placementGhost: PlacementGhost | null = null;
  editor: InlineEditor | null = null;
  images = new Map<string, HTMLImageElement>();

  /** Every animated value on this canvas. Sampled while drawing, never in logic. */
  readonly anim = new Anim();
  /** Last ghosts and snap, kept so their exit can animate from where they were. */
  lastOpeningGhost: OpeningGhost | null = null;
  lastPlacementGhost: PlacementGhost | null = null;
  lastSnap: SnapResult | null = null;
  /** Marquee rectangle kept while it fades out after the release. */
  marqueeFade: { start: P; current: P } | null = null;
  /** Elements removed by a command, still drawn from their last geometry. */
  private fading: { el: Element; index: DocIndex; key: string }[] = [];
  /** Frozen operation of a rejected drag, easing back to where it started. */
  private returning: Op | null = null;
  /** Last area readout per room, and the one being cross faded away from. */
  private areaText = new Map<string, string>();
  private areaPrev = new Map<string, string>();
  /** Committed elements as of the last document change, for the change diff. */
  private prevCommitted: { id: string | null; json: Map<string, string>; index: DocIndex } | null = null;
  private snapOn = false;
  private swingTarget = 1;
  private hingeTarget = 0;
  /** View the running ease started from. Null when the view is settled. */
  private viewFrom: View | null = null;
  /** Frames drawn since construction. Dev checks read it to prove idleness. */
  frames = 0;
  /**
   * True while the view should track the model automatically (right after
   * zoom-to-fit, the first-load fit, or F). Any direct user zoom, pinch or
   * pan turns it off so a later resize never yanks their scale back.
   * Dev checks read it to prove the resize fix.
   */
  autoFit = true;

  private canvas: HTMLCanvasElement;
  private ctx: Ctx2D;
  private dpr = 1;
  private dirty = true;
  private raf = 0;
  private disposed = false;
  private fitted = false;
  /** Debounce id for the trailing-edge refit after a resize burst settles. */
  private resizeSettleTimer = 0;
  private listeners = new Set<() => void>();
  private ui: UiState = { typed: null, editor: null, cursor: "default", hint: null };
  private sceneCache: { index: DocIndex; key: string; scene: SnapScene } | null = null;
  private cleanups: (() => void)[] = [];
  private lastDocId: string | null = null;
  private requestedImages = new Set<string>();

  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("2D canvas is not available");
    this.ctx = ctx;
    this.palette = readPalette(canvas);

    const on = <K extends keyof HTMLElementEventMap>(type: K, fn: (e: HTMLElementEventMap[K]) => void, opts?: AddEventListenerOptions): void => {
      canvas.addEventListener(type, fn as EventListener, opts);
      this.cleanups.push(() => canvas.removeEventListener(type, fn as EventListener, opts));
    };
    on("pointerdown", this.onPointerDown);
    on("pointermove", this.onPointerMove);
    on("pointerup", this.onPointerUp);
    on("pointercancel", this.onPointerCancel);
    on("pointerleave", this.onPointerLeave);
    on("pointerenter", () => {
      this.pointerInside = true;
    });
    on("dblclick", this.onDoubleClick);
    on("click", this.onClick);
    on("wheel", this.onWheel, { passive: false });
    on("contextmenu", (e) => e.preventDefault());

    const onWin = <K extends keyof WindowEventMap>(type: K, fn: (e: WindowEventMap[K]) => void, capture: boolean): void => {
      window.addEventListener(type, fn as EventListener, capture);
      this.cleanups.push(() => window.removeEventListener(type, fn as EventListener, capture));
    };
    onWin("keydown", this.onKeyDown, true);
    onWin("keyup", this.onKeyUp, true);
    onWin("blur", () => {
      this.shift = false;
      this.space = false;
    }, false);

    this.cleanups.push(bus.on("zoom_to_fit", () => this.zoomToFit()));
    this.cleanups.push(bus.on("focus_elements", (ids) => this.focusElements(ids)));

    let prev = useApp.getState();
    this.cleanups.push(
      useApp.subscribe((s) => {
        const p = prev;
        prev = s;
        if (s.doc !== p.doc || s.preview !== p.preview || s.activeLevelId !== p.activeLevelId) this.syncDoc();
        if (s.tool !== p.tool) this.onToolChange(p.tool, s.tool);
        if (s.selection !== p.selection || s.hoverId !== p.hoverId || s.tool !== p.tool) this.syncHighlight(p, s);
        if (s.toolOptions.pipeElevationMm !== p.toolOptions.pipeElevationMm) this.onPipeHeightOption(s.toolOptions.pipeElevationMm);
        if (
          s.selection !== p.selection ||
          s.hoverId !== p.hoverId ||
          s.gridVisible !== p.gridVisible ||
          s.toolOptions !== p.toolOptions ||
          s.snapEnabled !== p.snapEnabled ||
          s.orthoEnabled !== p.orthoEnabled ||
          s.activeCameraId !== p.activeCameraId ||
          s.catalog !== p.catalog
        ) {
          this.refreshToolGhost();
          this.invalidate();
        }
      }),
    );
    useApp.getState().registerCapturePlan(() => this.capturePlan());
    // Dev only hook so scripted UI checks can map model points to the screen.
    if (import.meta.env.DEV) (window as unknown as { __planController?: PlanController }).__planController = this;
    activeController = this;
    this.anim.setClock(performance.now());
    this.syncDoc();
    this.updateUi();
    this.invalidate();
  }

  dispose(): void {
    this.disposed = true;
    cancelAnimationFrame(this.raf);
    this.raf = 0;
    window.clearTimeout(this.resizeSettleTimer);
    for (const c of this.cleanups) c();
    this.cleanups = [];
    const s = useApp.getState();
    s.registerCapturePlan(null);
    s.setCursor(null);
    if (activeController === this) activeController = null;
  }

  // ------------------------------------------------------------ ui state for React

  subscribe = (fn: () => void): (() => void) => {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  };

  getUi = (): UiState => this.ui;

  private updateUi(): void {
    const next: UiState = {
      typed: this.typedUi(),
      editor: this.editor ? { ...this.editor, ...toScreen(this.view, this.editor.world) } : null,
      cursor: this.cssCursor(),
      hint: this.hint(),
    };
    const a = this.ui;
    const same =
      JSON.stringify(a.typed) === JSON.stringify(next.typed) &&
      JSON.stringify(a.editor) === JSON.stringify(next.editor) &&
      a.cursor === next.cursor &&
      a.hint === next.hint;
    if (same) return;
    this.ui = next;
    this.listeners.forEach((l) => l());
  }

  private typedUi(): UiState["typed"] {
    const op = this.op;
    const unitName = this.index?.doc.project.settings.display_unit ?? "mm";
    if (this.heightEntry && this.cursorScreen) {
      return { x: this.cursorScreen.x, y: this.cursorScreen.y, state: this.heightEntry, labels: [`Height ${unitName}`, ""] };
    }
    if ((op.kind !== "wall" && op.kind !== "rect" && op.kind !== "pipe") || !op.typed || !this.cursorScreen) return null;
    const labels: [string, string] = op.kind === "rect" ? [`Width ${unitName}`, `Depth ${unitName}`] : [`Length ${unitName}`, "Angle"];
    return { x: this.cursorScreen.x, y: this.cursorScreen.y, state: op.typed, labels };
  }

  private cssCursor(): string {
    if (this.op.kind === "pan") return "grabbing";
    const tool = useApp.getState().tool;
    if (this.space || tool === "pan") return "grab";
    if (tool === "select") {
      if (this.op.kind === "move" || this.op.kind === "grip" || this.op.kind === "slide") return "grabbing";
      if (this.hoverGrip !== null) return "grab";
      return useApp.getState().hoverId ? "pointer" : "default";
    }
    if (tool === "text") return "text";
    return "crosshair";
  }

  private hint(): string | null {
    const tool = useApp.getState().tool;
    const op = this.op;
    switch (tool) {
      case "wall":
        if (op.kind !== "wall") return "Click to start a wall";
        return op.points.length >= 3
          ? "Click to continue, type a length, click the first point to close, Enter to finish"
          : "Click the next point, or type a length and press Enter. 3000<45 sets length and angle";
      case "rect_room":
        return op.kind === "rect" ? "Click the opposite corner, or type width,depth and press Enter" : "Click the first corner of the room";
      case "door":
      case "window":
        return this.openingGhost ? "Click to place. F flips the side, H flips the hinge" : `Hover a wall to place a ${tool}`;
      case "asset":
        return "Click to place. R rotates. Snaps to wall faces";
      case "column":
        return "Click to place a column. R rotates";
      case "stair":
        return "Click to place a stair. R rotates";
      case "dimension":
        if (op.kind !== "dimension") return "Click the first point to measure";
        return op.b ? "Move to set the offset, click to place" : "Click the second point";
      case "text":
        return "Click where the text goes";
      case "camera":
        return op.kind === "camera" ? "Click what the camera looks at" : "Click where the camera stands";
      case "pipe": {
        if (op.kind === "pipe") return "Click the next point. PageUp, PageDown or h adds a riser. Enter finishes, Escape steps back";
        const name = PIPE_SYSTEM_LABEL[this.pipeSpec().system];
        const layer = this.pipeLayer();
        if (layer.locked) return `The ${name} layer is locked. Unlock it to draw`;
        if (layer.hidden) return `The ${name} layer is hidden. New pipes show when it is on`;
        return `Click to start a ${name.toLowerCase()} run. PageUp or PageDown sets the height, or type h and a height`;
      }
      default:
        return null;
    }
  }

  // ------------------------------------------------------------ document sync

  private syncDoc(): void {
    const s = useApp.getState();
    const visible = s.preview?.state ?? s.doc;
    this.index = visible ? buildIndex(visible, s.activeLevelId) : null;
    this.realIndex = s.preview && s.doc ? buildIndex(s.doc, s.activeLevelId) : null;
    this.sceneCache = null;
    const docId = visible?.project.id ?? null;
    if (docId !== this.lastDocId) {
      this.lastDocId = docId;
      this.fitted = false;
      this.autoFit = true;
      this.resetOp();
      this.anim.clearAll();
      this.fading.length = 0;
      this.areaText.clear();
      this.areaPrev.clear();
      this.returning = null;
    }
    // Change motion comes from a diff of the committed document, so placing,
    // deleting, undo, redo and an accepted AI proposal all animate the same way.
    this.diffCommitted(this.realIndex ?? this.index);
    if (this.index && !this.fitted && this.width > 0) {
      this.zoomToFit(false);
      this.fitted = true;
    }
    this.loadUnderlays();
    this.refreshToolGhost();
    this.invalidate();
  }

  private diffCommitted(index: DocIndex | null): void {
    const prev = this.prevCommitted;
    const id = index?.doc.project.id ?? null;
    const json = index ? new Map(index.doc.project.elements.map((e) => [e.id, JSON.stringify(e)] as const)) : null;
    this.prevCommitted = index && json ? { id, json, index } : null;
    if (!prev || !index || !json || prev.id !== id) {
      if (index) this.recordAreas(index, false);
      return;
    }
    this.anim.setClock(performance.now());
    const enter = dur("base");
    const leave = exitDur("base");
    if (enter > 0) {
      for (const [elId, text] of json) {
        const before = prev.json.get(elId);
        if (before === undefined) this.anim.to(`${K.add}${elId}`, 0, enter, { from: 1, easing: ease.spring, drop: true });
        else if (before !== text) this.anim.to(`${K.flash}${elId}`, 0, enter, { from: 1, easing: ease.out, drop: true });
      }
    }
    if (leave > 0) {
      for (const elId of prev.json.keys()) {
        if (json.has(elId)) continue;
        const el = prev.index.byId.get(elId);
        if (!el || !prev.index.visibleIds.has(elId)) continue;
        const key = `${K.remove}${elId}`;
        this.fading.push({ el, index: prev.index, key });
        this.anim.to(key, 0, leave, { from: 1, easing: ease.in, drop: true });
      }
    }
    this.recordAreas(index, enter > 0);
  }

  /** Cross fades a room's area readout when the engine reports a new value. */
  private recordAreas(index: DocIndex, animated: boolean): void {
    const seen = new Set<string>();
    for (const g of index.doc.derived.rooms) {
      const text = formatArea(g.area_mm2);
      const before = this.areaText.get(g.room_id);
      seen.add(g.room_id);
      this.areaText.set(g.room_id, text);
      if (before === undefined || before === text || !animated) continue;
      this.areaPrev.set(g.room_id, before);
      this.anim.to(`${K.area}${g.room_id}`, 0, dur("base"), { from: 1, easing: ease.out, drop: true });
    }
    for (const id of [...this.areaText.keys()]) {
      if (seen.has(id)) continue;
      this.areaText.delete(id);
      this.areaPrev.delete(id);
    }
  }

  private loadUnderlays(): void {
    if (!this.index) return;
    for (const el of this.index.visible) {
      if (el.kind !== "underlay" || this.requestedImages.has(el.file_name)) continue;
      this.requestedImages.add(el.file_name);
      const name = el.file_name;
      ipc
        .underlayData(name)
        .then((data) => {
          const img = new Image();
          img.onload = () => this.invalidate();
          img.src = data;
          this.images.set(name, img);
        })
        .catch(() => {
          // The placeholder frame stays. Allow a retry on the next document change.
          this.requestedImages.delete(name);
        });
    }
  }

  // ------------------------------------------------------------ view

  /**
   * The shell animates pane size with CSS (--dur-panel) on a view mode
   * change, so this fires a burst of intermediate sizes, some very small
   * (a divider drag is the same kind of burst, just driven by the pointer
   * instead of a transition). Every tick keeps the model point that was at
   * the pane center still there (never changing scale); only the trailing
   * edge of the burst, once sizes stop changing for about one --dur-panel,
   * may refit - and only when the view is in auto fit. This is what keeps a
   * small intermediate frame from ever becoming the final, stuck scale.
   */
  resize(width: number, height: number, dpr: number): void {
    if (width === this.width && height === this.height && dpr === this.dpr) return;
    const first = this.width === 0;
    const prevWidth = this.width;
    const prevHeight = this.height;
    this.width = width;
    this.height = height;
    this.dpr = dpr;
    this.canvas.width = Math.max(1, Math.round(width * dpr));
    this.canvas.height = Math.max(1, Math.round(height * dpr));
    window.clearTimeout(this.resizeSettleTimer);
    if (this.index && !this.fitted && width > 0) {
      // First sizing: nothing was on screen to move from.
      this.zoomToFit(false);
      this.fitted = true;
      this.invalidate();
      return;
    }
    const live = decideResize({ autoFit: this.autoFit, settled: false, width, height });
    if (live === "recenter" && !first) {
      // Keep the model point that was at the old pane center still there.
      // Scale never changes here, so this is safe to run on every tick.
      this.view = panBy(this.view, (width - prevWidth) / 2, (height - prevHeight) / 2);
    }
    this.resizeSettleTimer = window.setTimeout(() => {
      this.resizeSettleTimer = 0;
      const settled = decideResize({ autoFit: this.autoFit, settled: true, width: this.width, height: this.height });
      if (settled === "fit") this.zoomToFit();
    }, dur("panel"));
    this.invalidate();
  }

  zoomToFit(animated = true): void {
    if (!this.index || this.width === 0) return;
    this.autoFit = true;
    this.setView(fitRect(modelBounds(this.index), this.width, this.height, 70), animated ? dur("scene") : 0);
  }

  focusElements(ids: string[], animated = true): void {
    if (!this.index || this.width === 0) return;
    const b = boundsOfIds(this.index, ids);
    if (rectIsEmpty(b)) return;
    // Focusing a specific selection is a deliberate, scoped zoom: a later
    // resize must not silently override it back to fitting the whole model.
    this.autoFit = false;
    const pad = 1200;
    const padded: Rect = { minX: b.minX - pad, minY: b.minY - pad, maxX: b.maxX + pad, maxY: b.maxY + pad };
    this.setView(fitRect(padded, this.width, this.height, 60), animated ? dur("scene") : 0);
  }

  /**
   * One keyboard zoom step about the cursor, or the middle of the canvas.
   * Short (--dur-press) because it repeats: holding the key must not feel slow.
   */
  zoomStep(inOut: 1 | -1): void {
    if (this.width === 0) return;
    this.autoFit = false;
    const at = this.cursorScreen && this.pointerInside ? this.cursorScreen : { x: this.width / 2, y: this.height / 2 };
    this.setView(zoomAt(this.view, at, inOut > 0 ? 1.25 : 1 / 1.25), dur("press"));
  }

  /**
   * Moves the view. `this.view` is always the final transform, so hit testing
   * and the scripted checks never wait: only what is drawn eases there.
   */
  private setView(next: View, ms: number): void {
    if (ms > 0 && motionOK() && this.width > 0) {
      this.viewFrom = this.drawView();
      this.anim.setClock(performance.now());
      this.anim.clear(K.view);
      this.anim.to(K.view, 1, ms, { from: 0, easing: ease.inOut, drop: true });
    } else {
      this.settleView();
    }
    this.view = next;
    this.invalidate();
  }

  /** Ends a running view ease at its target. Any direct input wins immediately. */
  private settleView(): void {
    if (!this.viewFrom) return;
    this.viewFrom = null;
    this.anim.clear(K.view);
  }

  /** The transform actually drawn. Equals `view` unless a fit or focus is easing. */
  drawView(): View {
    const from = this.viewFrom;
    if (!from) return this.view;
    const t = this.anim.value(K.view, 1);
    if (t >= 1) return this.view;
    // Zoom geometrically, pan by the world point at the center of the canvas,
    // so a fit reads as one movement instead of a slide plus a scale.
    const cx = this.width / 2;
    const cy = this.height / 2;
    const a = toWorld(from, { x: cx, y: cy });
    const b = toWorld(this.view, { x: cx, y: cy });
    const scale = Math.exp(mix(Math.log(from.scale), Math.log(this.view.scale), t));
    const w = mixP(a, b, t);
    return { scale, ox: cx - w.x * scale, oy: cy + w.y * scale };
  }

  invalidate(): void {
    this.dirty = true;
    this.schedule();
  }

  /** Starts or retargets one animation and keeps the loop awake for it. */
  private animate(key: string, to: number, ms: number, spec: Parameters<Anim["to"]>[3] = {}): void {
    this.anim.setClock(performance.now());
    this.anim.to(key, to, ms, spec);
    this.invalidate();
  }

  private schedule(): void {
    if (this.raf === 0 && !this.disposed) this.raf = requestAnimationFrame(this.loop);
  }

  /** The AI preview breath is the only looping animation, and only with motion on. */
  private breathing(): boolean {
    return !!useApp.getState().preview && motionOK();
  }

  /**
   * One requestAnimationFrame loop, render on demand: it only runs while the
   * canvas is dirty, something is animating, or an AI preview is breathing.
   */
  private loop = (now: number): void => {
    this.raf = 0;
    if (this.disposed) return;
    const running = this.anim.tick(now);
    const looping = this.breathing();
    if (this.returning && !this.anim.has(K.back)) this.returning = null;
    if (this.fading.length > 0) this.dropFinishedFades();
    if (this.dirty || running > 0 || looping) {
      this.dirty = false;
      this.frames++;
      this.draw();
      this.updateUi();
    }
    if (this.dirty || running > 0 || looping) this.schedule();
  };

  private dropFinishedFades(): void {
    let keep = 0;
    for (const f of this.fading) if (this.anim.has(f.key)) this.fading[keep++] = f;
    this.fading.length = keep;
  }

  // ------------------------------------------------------------ drawing

  private renderContext(): RenderContext | null {
    if (!this.index) return null;
    return {
      ctx: this.ctx,
      view: this.drawView(),
      width: this.width,
      height: this.height,
      palette: this.palette,
      index: this.index,
      unit: this.index.doc.project.settings.display_unit,
      images: this.images,
      uiScale: 1,
    };
  }

  private draw(): void {
    const { ctx } = this;
    const s = useApp.getState();
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    ctx.fillStyle = this.palette.paper;
    ctx.fillRect(0, 0, this.width, this.height);
    const rc = this.renderContext();
    if (!rc || !this.index) return;
    if (s.gridVisible) drawGrid(rc, this.index.doc.project.settings.grid_mm);
    drawOrigin(rc);

    const preview = s.preview;
    const tinted = new Set<string>(preview ? [...preview.diff.added, ...preview.diff.modified] : []);
    // The only looping animation in the app: the proposal ghost breathes.
    const breath = breathe(this.anim.now, BREATH_MS, 0.55, 0.85, motionOK());
    const previewStyle: ElementStyle = { color: this.palette.preview, alpha: breath };
    const hidden = new Set<string>();
    if (this.editor?.id) hidden.add(this.editor.id);
    const anim = this.anim;
    const effects = anim.size() > 0;
    drawModel(rc, {
      styleOf: (id) => {
        const tint = tinted.has(id) ? previewStyle : null;
        if (!effects) return tint;
        // A placed element fades up as it settles; a room's area cross fades.
        const grow = anim.value(`${K.add}${id}`, 0);
        const area = anim.value(`${K.area}${id}`, 0);
        if (grow <= 0 && area <= 0) return tint;
        const st: ElementStyle = tint ? { ...tint } : {};
        if (grow > 0) st.alpha = (st.alpha ?? 1) * Math.max(0, Math.min(1, 1 - grow));
        if (area > 0) {
          const prev = this.areaPrev.get(id);
          if (prev !== undefined) st.areaFade = { prev, p: Math.max(0, Math.min(1, 1 - area)) };
        }
        return st;
      },
      hidden: this.editor?.mode === "annotation" ? hidden : new Set(),
      activeCameraId: s.activeCameraId,
      showCameras: true,
    });
    if (preview && this.realIndex) {
      for (const id of preview.diff.removed) {
        const el = this.realIndex.byId.get(id);
        if (el) drawElement(rc, el, { color: this.palette.preview, alpha: breath, dashed: true, hollow: true }, this.realIndex);
      }
    }

    // Elements a command removed, still drawn from the geometry they had.
    for (const f of this.fading) {
      const v = anim.value(f.key, 0);
      if (v > 0.002) drawElement(rc, f.el, { alpha: v, hollow: f.el.kind === "room" }, f.index);
    }
    if (effects) {
      const index = this.index;
      anim.each(K.add, (id, v) => {
        const el = index.byId.get(id);
        if (v > 0.002 && el && index.visibleIds.has(id)) drawFlash(rc, el, this.palette.selection, v * 0.5, 1 + 0.06 * v);
      });
      anim.each(K.flash, (id, v) => {
        const el = index.byId.get(id);
        if (v > 0.002 && el && index.visibleIds.has(id)) drawFlash(rc, el, this.palette.selection, v * 0.45, 1);
      });
    }

    const selected = new Set(s.selection);
    if (this.op.kind === "idle") {
      anim.each(K.hover, (id, v) => {
        if (v <= 0.002 || selected.has(id) || id === s.hoverId) return;
        const el = this.index?.byId.get(id);
        if (el && this.index?.visibleIds.has(id)) drawHighlight(rc, el, "hover", v);
      });
      if (s.hoverId && !selected.has(s.hoverId)) {
        const el = this.index.byId.get(s.hoverId);
        if (el && this.index.visibleIds.has(el.id)) drawHighlight(rc, el, "hover", anim.value(`${K.hover}${s.hoverId}`, 1));
      }
    }
    anim.each(K.sel, (id, v) => {
      if (v <= 0.002 || selected.has(id)) return;
      const el = this.index?.byId.get(id);
      if (el && this.index?.visibleIds.has(id)) drawHighlight(rc, el, "selected", v);
    });
    for (const id of selected) {
      const el = this.index.byId.get(id);
      if (el && this.index.visibleIds.has(el.id)) drawHighlight(rc, el, "selected", anim.value(`${K.sel}${id}`, 1));
    }
    drawOverlay(rc, this);
  }

  /** Renders the plan without grid, handles or cameras on white. */
  async capturePlan(): Promise<string> {
    const s = useApp.getState();
    const doc = s.doc;
    const W = 2000;
    const H = 1400;
    const canvas = document.createElement("canvas");
    canvas.width = W;
    canvas.height = H;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("2D canvas is not available");
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, W, H);
    if (doc) {
      const index = buildIndex(doc, s.activeLevelId);
      const rc: RenderContext = {
        ctx,
        view: fitRect(modelBounds(index), W, H, 150),
        width: W,
        height: H,
        palette: { ...this.palette, paper: "#ffffff" },
        index,
        unit: doc.project.settings.display_unit,
        images: this.images,
        uiScale: 1.7,
        forExport: true,
      };
      drawModel(rc, { styleOf: () => null, hidden: new Set(), activeCameraId: null, showCameras: false });
    }
    return canvas.toDataURL("image/png");
  }

  // ------------------------------------------------------------ interaction motion

  /** Hover tint, selection outline and the grips that come with a selection. */
  private syncHighlight(prev: AppState, next: AppState): void {
    if (next.hoverId !== prev.hoverId) {
      if (prev.hoverId) this.animate(`${K.hover}${prev.hoverId}`, 0, exitDur("hover"), { drop: true });
      if (next.hoverId) this.animate(`${K.hover}${next.hoverId}`, 1, dur("hover"), { from: 0 });
    }
    // Another selection or tool has other grips: the hovered one is gone.
    if (next.selection !== prev.selection || next.tool !== prev.tool) this.setHoverGrip(null);
    if (next.selection !== prev.selection) {
      const after = new Set(next.selection);
      const before = new Set(prev.selection);
      for (const id of prev.selection) if (!after.has(id)) this.animate(`${K.sel}${id}`, 0, exitDur("hover"), { drop: true });
      for (const id of next.selection) if (!before.has(id)) this.animate(`${K.sel}${id}`, 1, dur("hover"), { from: 0 });
    }
    this.syncGrips(next);
  }

  /** Grips scale in from nothing, one after the other, and fade out together. */
  private syncGrips(s: AppState): void {
    const show = s.tool === "select" && s.selection.length === 1;
    const n = GRIP_STAGGER;
    for (let i = 0; i < n; i++) {
      const key = `${K.grip}${i}`;
      if (show) this.animate(key, 1, dur("base"), { from: 0, easing: ease.spring, delayMs: i * 30 });
      else this.animate(key, 0, exitDur("hover"), { drop: true });
    }
  }

  /**
   * The snap glyph pops when a snap first engages and fades when it releases.
   * Moving from one snap target to another only retargets: the glyph stays at
   * full size and slides, it does not pop again.
   */
  private syncSnapAnim(): void {
    const r = this.snapResult;
    const on = !!r && r.type !== "none" && this.pointerInside;
    if (on) {
      this.lastSnap = r;
      if (!this.snapOn) {
        this.snapOn = true;
        this.anim.clear(K.snapPop);
        this.animate(K.snapPop, 1, dur("base"), { from: 0.6, easing: ease.spring });
        this.animate(K.snapAlpha, 1, dur("hover"), { from: 0 });
      }
    } else if (this.snapOn) {
      this.snapOn = false;
      this.animate(K.snapAlpha, 0, exitDur("hover"), { drop: true });
    }
  }

  /** Tool ghosts fade in on a valid target and out when they lose one. */
  private syncGhostAnim(): void {
    const og = this.openingGhost;
    if (og) {
      const fresh = this.lastOpeningGhost === null || this.anim.value(K.ghostOpening, 0) <= 0;
      this.lastOpeningGhost = og;
      const swing = og.opening.flip_side ? -1 : 1;
      const hinge = og.opening.flip_hinge ? 1 : 0;
      if (fresh) {
        this.anim.set(K.swing, swing);
        this.anim.set(K.hinge, hinge);
      } else {
        // F and H flip the leaf: the swing sweeps through the wall plane.
        this.animate(K.swing, swing, dur("hover"), { from: this.swingTarget, easing: ease.inOut });
        this.animate(K.hinge, hinge, dur("hover"), { from: this.hingeTarget, easing: ease.inOut });
      }
      this.swingTarget = swing;
      this.hingeTarget = hinge;
      this.animate(K.ghostOpening, 1, dur("hover"), { from: 0 });
    } else {
      this.animate(K.ghostOpening, 0, exitDur("hover"), { drop: true });
    }
    if (this.placementGhost) {
      this.lastPlacementGhost = this.placementGhost;
      this.animate(K.ghostPlace, 1, dur("hover"), { from: 0 });
    } else {
      this.animate(K.ghostPlace, 0, exitDur("hover"), { drop: true });
    }
  }

  /** The grip under the pointer grows on hover and shrinks back when left. */
  private setHoverGrip(i: number | null): void {
    if (i === this.hoverGrip) return;
    if (this.hoverGrip !== null) this.animate(`${K.gripHover}${this.hoverGrip}`, 0, exitDur("hover"), { drop: true });
    if (i !== null) this.animate(`${K.gripHover}${i}`, 1, dur("hover"), { from: 0 });
    this.hoverGrip = i;
  }

  /** Pick up and drop. Only these animate: the drag itself tracks 1:1. */
  private setLift(on: boolean): void {
    if (on) this.animate(K.lift, 1, dur("hover"), { from: 0 });
    else this.animate(K.lift, 0, exitDur("hover"), { drop: true });
  }

  /** A rejected drag does not snap back: its ghost eases to where it started. */
  private easeBack(op: Op): void {
    if (op.kind !== "move" && op.kind !== "slide" && op.kind !== "grip") return;
    if (exitDur("base") <= 0) return;
    this.returning = op;
    this.animate(K.back, 0, exitDur("base"), { from: 1, easing: ease.inOut, drop: true });
  }

  /** The rejected drag's ghost, interpolated back toward its origin. */
  returningOp(): Op | null {
    const op = this.returning;
    if (!op) return null;
    const v = this.anim.value(K.back, 0);
    switch (op.kind) {
      case "move":
        return { ...op, delta: mul(op.delta, v) };
      case "slide":
        return { ...op, placement: { ...op.placement, offset: mix(op.opening.offset_mm, op.placement.offset, v) } };
      case "grip":
        return { ...op, current: mixP(op.grip.pos, op.current, v) };
      default:
        return null;
    }
  }

  // ------------------------------------------------------------ dev hooks

  /** Dev only: how many animations are still running. */
  animCount(): number {
    return this.anim.running();
  }

  /** Dev only: one sampled animation value. */
  animValue(key: string, rest = 0): number {
    return this.anim.value(key, rest);
  }

  /** Dev only: every animation key with its current value. */
  animSnapshot(): Record<string, number> {
    return this.anim.snapshot();
  }

  /** Dev only: true when no frame is scheduled, so the canvas is render on demand. */
  animIdle(): boolean {
    return this.raf === 0;
  }

  // ------------------------------------------------------------ snapping

  gripList(): Grip[] {
    const s = useApp.getState();
    if (s.tool !== "select" || s.selection.length !== 1 || !this.index) return [];
    const el = this.index.byId.get(s.selection[0]);
    if (!el || !this.index.visibleIds.has(el.id) || isLocked(el, this.index)) return [];
    return gripsFor(el, 1 / this.view.scale);
  }

  private scene(exclude: readonly string[] = [], extraPoints: readonly P[] = []): SnapScene {
    if (!this.index) return emptyScene();
    const key = exclude.join(",");
    let base = this.sceneCache;
    if (!base || base.index !== this.index || base.key !== key) {
      const scene = emptyScene();
      const skip = new Set(exclude);
      for (const el of this.index.visible) {
        if (skip.has(el.id)) continue;
        if (el.kind === "wall") {
          scene.centerlines.push({ a: el.start, b: el.end });
          scene.points.push(el.start, el.end);
          const o = wallOutline(el, this.index);
          for (let i = 0; i < o.length; i++) {
            const a = o[i];
            const b = o[(i + 1) % o.length];
            if (dist(a, b) > el.thickness_mm * 1.5 + 1) scene.faces.push({ a, b });
            scene.facePoints.push(a);
          }
        } else if (el.kind === "column") {
          scene.points.push(el.center);
        }
      }
      computeIntersections(scene);
      base = { index: this.index, key, scene };
      this.sceneCache = base;
    }
    if (extraPoints.length === 0) return base.scene;
    return { ...base.scene, points: [...base.scene.points, ...extraPoints] };
  }

  private snapAt(world: P, anchor: P | null, opts: { exclude?: string[]; extra?: P[]; useFaces?: boolean } = {}): SnapResult {
    const s = useApp.getState();
    const grid = this.index?.doc.project.settings.grid_mm ?? 100;
    return snap(world, this.scene(opts.exclude, opts.extra), {
      enabled: s.snapEnabled,
      ortho: s.orthoEnabled || this.shift,
      tolerance: SNAP_PX / this.view.scale,
      gridStep: snapStep(grid, this.view.scale),
      anchor,
      useFaces: opts.useFaces,
    });
  }

  private gridStep(): number {
    const s = useApp.getState();
    if (!s.snapEnabled) return 0;
    return snapStep(this.index?.doc.project.settings.grid_mm ?? 100, this.view.scale);
  }

  // ------------------------------------------------------------ pointer

  private eventPoint(e: MouseEvent): P {
    const r = this.canvas.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  private onPointerDown = (e: PointerEvent): void => {
    if (!this.index) return;
    if (this.editor) return; // the inline input commits on blur first
    // A press lands on what is on screen now, so a running view ease ends here.
    this.settleView();
    const screen = this.eventPoint(e);
    const world = toWorld(this.view, screen);
    this.cursorScreen = screen;
    this.cursorWorld = world;
    this.shift = e.shiftKey;
    this.pointerInside = true;
    const tool = useApp.getState().tool;
    try {
      this.canvas.setPointerCapture(e.pointerId);
    } catch {
      // Synthetic events may not have a capturable pointer.
    }

    if (e.button === 1 || (e.button === 0 && (this.space || tool === "pan"))) {
      e.preventDefault();
      if (this.isBusy()) return;
      // Panning pauses a tool operation. It resumes on release.
      this.panReturn = this.op.kind === "pan" ? this.panReturn : this.op;
      this.op = { kind: "pan", last: screen };
      this.invalidate();
      return;
    }
    if (e.button === 2) {
      this.finishOrCancel();
      return;
    }
    if (e.button !== 0) return;
    const presses = this.pressCount(e, screen);
    if (this.isBusy()) return;

    switch (tool) {
      case "select":
        this.selectDown(screen, world, e);
        break;
      case "wall":
        this.wallClick(presses);
        break;
      case "rect_room":
        this.rectDown(screen);
        break;
      case "door":
      case "window":
        this.placeOpening();
        break;
      case "column":
      case "stair":
      case "asset":
        this.placeElement();
        break;
      case "dimension":
        this.dimensionClick();
        break;
      case "text":
        // The inline input opens on click (after mouse up), so the browser's
        // own focus handling for this press cannot blur it right away.
        break;
      case "camera":
        this.cameraClick();
        break;
      case "pipe":
        this.pipeClick(presses);
        break;
      default:
        break;
    }
    this.invalidate();
  };

  private panReturn: Op = { kind: "idle" };

  /** The last primary press, for counting double clicks. */
  private lastPress: { t: number; at: P; count: number } | null = null;

  /**
   * How many presses in a row this one is. Chromium (so also WebView2) leaves
   * PointerEvent.detail at 0 on pointerdown, so presses close in time and
   * place are counted here. A browser that fills `detail` in still counts.
   */
  private pressCount(e: PointerEvent, screen: P): number {
    const last = this.lastPress;
    const count = last && e.timeStamp - last.t <= DOUBLE_CLICK_MS && dist(screen, last.at) <= DOUBLE_CLICK_PX ? last.count + 1 : 1;
    this.lastPress = { t: e.timeStamp, at: screen, count };
    return Math.max(count, e.detail);
  }

  private onPointerMove = (e: PointerEvent): void => {
    const screen = this.eventPoint(e);
    this.cursorScreen = screen;
    this.cursorWorld = toWorld(this.view, screen);
    this.shift = e.shiftKey;
    this.pointerInside = true;
    this.handleMove(e.altKey);
  };

  private handleMove(alt = false): void {
    const screen = this.cursorScreen;
    const world = this.cursorWorld;
    if (!screen || !world || !this.index) return;
    const s = useApp.getState();
    const op = this.op;
    this.snapResult = null;

    switch (op.kind) {
      case "pan":
        this.autoFit = false;
        this.view = panBy(this.view, screen.x - op.last.x, screen.y - op.last.y);
        op.last = screen;
        break;
      case "press":
        if (Math.hypot(screen.x - op.downScreen.x, screen.y - op.downScreen.y) > DRAG_PX) this.beginDrag(op, alt);
        break;
      case "marquee":
        op.current = world;
        break;
      case "move":
        if (!op.committing) this.updateMove(op, world);
        break;
      case "slide":
        if (!op.committing) op.placement = this.placementFor(world, op.host, op.opening.width_mm, op.opening.id);
        break;
      case "grip":
        if (!op.committing) this.updateGrip(op, world);
        break;
      default:
        this.updateToolHover(world);
    }
    if (this.op.kind === "idle" && s.tool === "select") {
      const grips = this.gripList();
      const grip = grips.length > 0 ? hitGrip(grips, world, GRIP_PX / this.view.scale) : null;
      this.setHoverGrip(grip ? grips.indexOf(grip) : null);
      const id = hitTest(world, this.index, this.hitOptions());
      if (id !== s.hoverId) s.setHover(id);
    } else {
      this.setHoverGrip(null);
    }
    const snapped = this.snapResult as SnapResult | null;
    const p = snapped?.point ?? world;
    s.setCursor({ x: p.x, y: p.y });
    this.syncSnapAnim();
    this.syncGhostAnim();
    this.invalidate();
  }

  private hitOptions(): { tol: number; labelHeightMm: number; pxMm: number } {
    return { tol: 6 / this.view.scale, labelHeightMm: labelHeightMm(this.view), pxMm: 1 / this.view.scale };
  }

  /** Snap and ghost updates for the drawing and placement tools. */
  private updateToolHover(world: P): void {
    const tool = useApp.getState().tool;
    const op = this.op;
    this.openingGhost = null;
    this.placementGhost = null;
    switch (tool) {
      case "wall": {
        if (op.kind === "wall") {
          const last = op.points[op.points.length - 1];
          const r = this.snapAt(world, last, { extra: op.points.slice(0, -1) });
          // Closing the chain wins over every other snap.
          if (op.points.length >= 3 && dist(world, op.points[0]) <= SNAP_PX / this.view.scale && useApp.getState().snapEnabled) {
            this.snapResult = { point: op.points[0], type: "endpoint", guides: [], angleLocked: false };
          } else this.snapResult = r;
        } else this.snapResult = this.snapAt(world, null);
        break;
      }
      case "rect_room":
      case "camera":
        this.snapResult = this.snapAt(world, null);
        break;
      case "dimension":
        if (op.kind === "dimension" && op.b) break;
        this.snapResult = this.snapAt(world, op.kind === "dimension" ? op.a : null, { useFaces: true });
        break;
      case "text":
        this.snapResult = this.snapAt(world, null);
        break;
      case "door":
      case "window":
        this.openingGhost = this.computeOpeningGhost(world, tool);
        break;
      case "column":
      case "stair":
      case "asset":
        this.placementGhost = this.computePlacementGhost(world, tool);
        break;
      case "pipe": {
        const spec = this.pipeSpec();
        if (op.kind === "pipe") {
          const last = op.points[op.points.length - 1];
          const onLast = !!this.cursorScreen && dist(toScreen(this.view, last), this.cursorScreen) <= SNAP_PX;
          if (onLast && useApp.getState().snapEnabled) {
            // On the last point a click adds nothing, so a double click there only finishes.
            this.snapResult = { point: planOf(last), type: "endpoint", guides: [], angleLocked: false, label: "Last point" };
          } else {
            const extra = op.points.slice(0, -1).map(planOf);
            this.snapResult = this.snapPipeAt(world, planOf(last), { system: spec.system, penZ: op.penZ, extra });
          }
        } else {
          this.snapResult = this.snapPipeAt(world, null, { system: spec.system, penZ: spec.startHeightMm });
        }
        break;
      }
      default:
        break;
    }
  }

  /** Recomputes the hover ghost after a key or option change. */
  refreshToolGhost(): void {
    if (this.cursorWorld && this.pointerInside && this.index && !this.isDragOp()) {
      this.snapResult = null;
      this.updateToolHover(this.cursorWorld);
    }
    this.syncSnapAnim();
    this.syncGhostAnim();
  }

  private isDragOp(): boolean {
    const k = this.op.kind;
    return k === "pan" || k === "press" || k === "marquee" || k === "move" || k === "slide" || k === "grip";
  }

  private isBusy(): boolean {
    const op = this.op;
    return "committing" in op && op.committing;
  }

  private onPointerUp = (e: PointerEvent): void => {
    const op = this.op;
    const s = useApp.getState();
    try {
      this.canvas.releasePointerCapture(e.pointerId);
    } catch {
      // Not captured.
    }
    switch (op.kind) {
      case "pan":
        this.op = this.panReturn;
        this.panReturn = { kind: "idle" };
        break;
      case "press":
        if (op.hitId) s.select([op.hitId], op.additive);
        else if (!op.additive) s.select([]);
        this.op = { kind: "idle" };
        break;
      case "marquee": {
        if (this.index) {
          const rect = rectFromPoints(op.start, op.current);
          const crossing = op.current.x < op.start.x;
          const ids = marqueeSelect(rect, crossing, this.index, this.hitOptions());
          s.select(op.additive ? [...new Set([...s.selection, ...ids])] : ids);
        }
        this.marqueeFade = { start: op.start, current: op.current };
        this.animate(K.marquee, 0, exitDur("base"), { from: 1, easing: ease.in, drop: true });
        this.op = { kind: "idle" };
        break;
      }
      case "move":
        this.commitMove(op);
        break;
      case "slide":
        this.commitSlide(op);
        break;
      case "grip":
        this.commitGrip(op);
        break;
      case "rect": {
        const screen = this.eventPoint(e);
        const moved = Math.hypot(screen.x - op.downScreen.x, screen.y - op.downScreen.y);
        if (moved > DRAG_PX * 2 && !op.committing) this.commitRect(op);
        break;
      }
      default:
        break;
    }
    this.setLift(false);
    this.invalidate();
  };

  private onPointerCancel = (): void => {
    if (this.isDragOp()) this.op = { kind: "idle" };
    this.setLift(false);
    this.invalidate();
  };

  private onPointerLeave = (): void => {
    this.pointerInside = false;
    if (this.isDragOp()) return;
    this.setHoverGrip(null);
    this.openingGhost = null;
    this.placementGhost = null;
    const s = useApp.getState();
    s.setCursor(null);
    if (s.hoverId) s.setHover(null);
    this.syncSnapAnim();
    this.syncGhostAnim();
    this.invalidate();
  };

  private onClick = (e: MouseEvent): void => {
    if (e.button !== 0 || !this.index || this.editor || this.isBusy()) return;
    if (useApp.getState().tool !== "text" || this.space) return;
    const world = toWorld(this.view, this.eventPoint(e));
    const snapped = this.snapAt(world, null).point;
    this.openEditor({ mode: "new_text", id: null, world: snapped, value: "", fontPx: Math.max(12, 250 * this.view.scale) });
  };

  private onDoubleClick = (e: MouseEvent): void => {
    if (!this.index) return;
    const s = useApp.getState();
    if (s.tool !== "select") return;
    const world = toWorld(this.view, this.eventPoint(e));
    const id = hitTest(world, this.index, this.hitOptions());
    const el = id ? this.index.byId.get(id) : null;
    if (!el) return;
    if (el.kind === "room") {
      const g = this.index.roomGeo.get(el.id);
      this.openEditor({ mode: "room", id: el.id, world: g ? g.label_point : el.seed, value: el.name, fontPx: 12 });
    } else if (el.kind === "annotation") {
      this.openEditor({ mode: "annotation", id: el.id, world: el.position, value: el.text, fontPx: Math.max(11, el.size_mm * this.view.scale) });
    }
  };

  private onWheel = (e: WheelEvent): void => {
    e.preventDefault();
    // Wheel, pinch and trackpad pan are 1:1 and always win over a view ease.
    this.settleView();
    this.autoFit = false;
    const screen = this.eventPoint(e);
    const legacy = (e as WheelEvent & { wheelDeltaY?: number }).wheelDeltaY;
    if (e.ctrlKey || e.metaKey) {
      // Trackpad pinch arrives as ctrl+wheel with small deltas.
      const factor = Math.exp(-Math.max(-60, Math.min(60, e.deltaY)) * 0.012);
      this.view = zoomAt(this.view, screen, factor);
    } else {
      const looksLikeTrackpad =
        e.deltaMode === 0 &&
        (typeof legacy === "number" && legacy !== 0 ? Math.abs(legacy + 3 * e.deltaY) < 1.5 && Math.abs(legacy) % 120 !== 0 : e.deltaX !== 0);
      if (looksLikeTrackpad) {
        this.view = panBy(this.view, -e.deltaX, -e.deltaY);
      } else {
        const notches = typeof legacy === "number" && legacy !== 0 ? legacy / 120 : -e.deltaY / (e.deltaMode === 1 ? 3 : 100);
        const factor = Math.pow(1.18, Math.max(-4, Math.min(4, notches)));
        this.view = zoomAt(this.view, screen, factor);
      }
    }
    this.cursorScreen = screen;
    this.cursorWorld = toWorld(this.view, screen);
    this.handleMove();
  };

  // ------------------------------------------------------------ select tool

  private selectDown(screen: P, world: P, e: PointerEvent): void {
    if (!this.index) return;
    const grip = hitGrip(this.gripList(), world, GRIP_PX / this.view.scale);
    if (grip) {
      const el = this.index.byId.get(grip.elementId);
      if (el) {
        this.op = { kind: "grip", grip, element: el, current: grip.pos, z: null, committing: false };
        this.setHoverGrip(null);
        this.setLift(true);
        return;
      }
    }
    const hitId = hitTest(world, this.index, this.hitOptions());
    this.op = { kind: "press", downScreen: screen, downWorld: world, hitId, additive: e.shiftKey, alt: e.altKey };
  }

  private beginDrag(op: Extract<Op, { kind: "press" }>, alt: boolean): void {
    const s = useApp.getState();
    const index = this.index;
    if (!index) return;
    if (!op.hitId) {
      this.op = { kind: "marquee", start: op.downWorld, current: this.cursorWorld ?? op.downWorld, additive: op.additive };
      return;
    }
    let ids = s.selection.includes(op.hitId) ? [...s.selection] : [op.hitId];
    if (!s.selection.includes(op.hitId)) s.select(ids);
    const hit = index.byId.get(op.hitId);
    if (!hit) {
      this.op = { kind: "idle" };
      return;
    }
    // A single opening slides along its host wall.
    if (hit.kind === "opening" && ids.length === 1) {
      const host = index.byId.get(hit.wall_id);
      if (host && host.kind === "wall" && !(op.alt || alt)) {
        this.op = {
          kind: "slide",
          opening: hit,
          host,
          placement: this.placementFor(this.cursorWorld ?? op.downWorld, host, hit.width_mm, hit.id),
          committing: false,
        };
        this.setLift(true);
        return;
      }
    }
    // Dragging a room moves its bounding walls.
    const expanded = new Set<string>();
    for (const id of ids) {
      const el = index.byId.get(id);
      if (!el || isLocked(el, index)) continue;
      expanded.add(id);
      if (el.kind === "room") for (const w of index.roomGeo.get(id)?.wall_ids ?? []) expanded.add(w);
    }
    // Openings cannot be translated freely. They follow their host wall.
    ids = [...expanded].filter((id) => {
      const el = index.byId.get(id);
      return el && (el.kind !== "opening" || (op.alt || alt));
    });
    if (ids.length === 0) {
      this.op = { kind: "idle" };
      return;
    }
    // Snap by the key point of the grabbed element nearest to the grab.
    let ref = op.downWorld;
    let best = Math.max(600, 24 / this.view.scale); // mm
    for (const k of keyPoints(hit, index)) {
      const d = dist(k, op.downWorld);
      if (d < best) {
        best = d;
        ref = k;
      }
    }
    const usesKey = ref !== op.downWorld;
    this.op = { kind: "move", ids, ref, delta: { x: 0, y: 0 }, duplicate: op.alt || alt, committing: false };
    this.moveGrab = usesKey ? sub(op.downWorld, ref) : { x: 0, y: 0 };
    this.moveUsesKey = usesKey;
    this.setLift(true);
  }

  private moveGrab: P = { x: 0, y: 0 };
  private moveUsesKey = false;

  private updateMove(op: Extract<Op, { kind: "move" }>, world: P): void {
    const target = sub(world, this.moveGrab);
    const s = useApp.getState();
    if (this.moveUsesKey) {
      const r = this.snapAt(target, op.ref, { exclude: op.duplicate ? [] : op.ids });
      this.snapResult = r;
      op.delta = sub(r.point, op.ref);
    } else {
      // No key point near the grab: move in whole grid steps, ortho when asked.
      let d = sub(target, op.ref);
      if (s.orthoEnabled || this.shift) d = Math.abs(d.x) >= Math.abs(d.y) ? { x: d.x, y: 0 } : { x: 0, y: d.y };
      const step = this.gridStep();
      if (step > 0) d = { x: Math.round(d.x / step) * step, y: Math.round(d.y / step) * step };
      op.delta = d;
    }
  }

  private commitMove(op: Extract<Op, { kind: "move" }>): void {
    if (op.committing) return;
    if (dist(op.delta, { x: 0, y: 0 }) < 0.5) {
      this.op = { kind: "idle" };
      return;
    }
    const command: Command = op.duplicate
      ? { type: "duplicate_elements", ids: op.ids, delta: op.delta }
      : { type: "move_elements", ids: op.ids, delta: op.delta, stretch_connected: true };
    op.committing = true;
    void this.run(command, (result) => {
      if (op.duplicate && result.added.length > 0) useApp.getState().select(result.added);
    });
  }

  private placementFor(world: P, host: Wall, width: number, selfId: string | null = null): OpeningPlacement {
    const index = this.index;
    const insetS = index ? jointInset(index, host, "start") : 0;
    const insetE = index ? jointInset(index, host, "end") : 0;
    const others: OpeningSpan[] = [];
    if (index) {
      for (const e of index.byId.values()) {
        if (e.kind === "opening" && e.wall_id === host.id && e.id !== selfId) others.push({ offset: e.offset_mm, width: e.width_mm });
      }
    }
    return placeOnWall(world, host, width, insetS, insetE, this.gridStep(), SNAP_PX / this.view.scale, useApp.getState().snapEnabled, others);
  }

  private commitSlide(op: Extract<Op, { kind: "slide" }>): void {
    if (op.committing) return;
    if (!op.placement.valid || Math.abs(op.placement.offset - op.opening.offset_mm) < 0.5) {
      this.op = { kind: "idle" };
      return;
    }
    op.committing = true;
    void this.run({ type: "update_element", element: { ...op.opening, offset_mm: op.placement.offset } });
  }

  private updateGrip(op: Extract<Op, { kind: "grip" }>, world: P): void {
    const el = op.element;
    const s = useApp.getState();
    switch (op.grip.kind) {
      case "wall_start":
      case "wall_end": {
        if (el.kind !== "wall") return;
        const anchor = op.grip.kind === "wall_start" ? el.end : el.start;
        const r = this.snapAt(world, anchor, { exclude: [el.id] });
        this.snapResult = r;
        op.current = r.point;
        break;
      }
      case "wall_mid": {
        if (el.kind !== "wall") return;
        op.current = add(op.grip.pos, normalDelta(el, world, this.gridStep()));
        break;
      }
      case "rotate":
        op.current = world;
        break;
      case "dim_offset":
        op.current = world;
        break;
      case "dim_a":
      case "dim_b": {
        const r = this.snapAt(world, null, { useFaces: true });
        this.snapResult = r;
        op.current = r.point;
        break;
      }
      case "cam_pos":
      case "cam_target": {
        const r = this.snapAt(world, null);
        this.snapResult = s.snapEnabled ? r : null;
        op.current = r.point;
        break;
      }
      case "pipe_node": {
        if (el.kind !== "pipe") return;
        // Ortho runs from the neighbouring node. A pipe never snaps to itself.
        const nodes = pipeNodes(el.points);
        const i = nodes.findIndex((n) => n.index === op.grip.index);
        const anchor = nodes[i - 1]?.point ?? nodes[i + 1]?.point ?? null;
        const r = this.snapPipeAt(world, anchor, { system: el.system, penZ: nodes[i]?.zOut ?? 0, exclude: [el.id] });
        this.snapResult = r;
        op.current = r.point;
        op.z = r.z ?? null;
        break;
      }
    }
  }

  /** The element as it will be after the grip drag, or null for wall grips. */
  gripResult(op: Extract<Op, { kind: "grip" }>): Element | null {
    const el = op.element;
    const snapOn = useApp.getState().snapEnabled;
    switch (op.grip.kind) {
      case "rotate": {
        const step = snapOn ? 15 : 0;
        if (el.kind === "asset") return { ...el, rotation_deg: rotationFromGrip(el.position, op.current, step) };
        if (el.kind === "column") return { ...el, rotation_deg: rotationFromGrip(el.center, op.current, step) };
        if (el.kind === "stair") return { ...el, rotation_deg: rotationFromGrip(el.origin, op.current, step) };
        if (el.kind === "reference_model") return { ...el, rotation_deg: rotationFromGrip(el.position, op.current, step) };
        return null;
      }
      case "dim_offset": {
        if (el.kind !== "dimension") return null;
        const d = unit(sub(el.b, el.a));
        let off = dot(sub(op.current, el.a), { x: -d.y, y: d.x });
        const step = this.gridStep();
        if (step > 0) off = Math.round(off / step) * step;
        return { ...el, offset_mm: off };
      }
      case "dim_a":
        return el.kind === "dimension" ? { ...el, a: op.current } : null;
      case "dim_b":
        return el.kind === "dimension" ? { ...el, b: op.current } : null;
      case "cam_pos":
        return el.kind === "camera" ? { ...el, position: { ...el.position, x: op.current.x, y: op.current.y } } : null;
      case "cam_target":
        return el.kind === "camera" ? { ...el, target: { ...el.target, x: op.current.x, y: op.current.y } } : null;
      case "pipe_node": {
        if (el.kind !== "pipe" || op.grip.index === undefined) return null;
        const points = movePipeNode(el.points, op.grip.index, op.grip.count ?? 1, op.current, op.z);
        return points ? { ...el, points } : null;
      }
      default:
        return null;
    }
  }

  private commitGrip(op: Extract<Op, { kind: "grip" }>): void {
    if (op.committing) return;
    const el = op.element;
    let command: Command | null = null;
    if (dist(op.current, op.grip.pos) >= 0.5) {
      if (el.kind === "wall" && op.grip.kind === "wall_start") {
        if (dist(op.current, el.end) > 1) command = { type: "set_wall_endpoints", wall_id: el.id, start: op.current, end: el.end };
      } else if (el.kind === "wall" && op.grip.kind === "wall_end") {
        if (dist(op.current, el.start) > 1) command = { type: "set_wall_endpoints", wall_id: el.id, start: el.start, end: op.current };
      } else if (el.kind === "wall" && op.grip.kind === "wall_mid") {
        command = { type: "move_elements", ids: [el.id], delta: sub(op.current, op.grip.pos), stretch_connected: true };
      } else {
        const next = this.gripResult(op);
        if (next && JSON.stringify(next) !== JSON.stringify(el)) command = { type: "update_element", element: next };
      }
    }
    if (!command) {
      // A pipe node dropped where the run would lose too many points eases back.
      if (op.grip.kind === "pipe_node" && dist(op.current, op.grip.pos) >= 0.5) this.easeBack(op);
      this.op = { kind: "idle" };
      return;
    }
    op.committing = true;
    void this.run(command);
  }

  // ------------------------------------------------------------ commands

  /**
   * Sends one command. The current operation stays visible as a ghost until
   * the engine answers, then the canvas always returns to idle, success or not.
   */
  private async run(command: Command, onDone?: (diff: { added: string[] }) => void, next: Op = { kind: "idle" }): Promise<void> {
    const from = this.op;
    let ok = false;
    try {
      const result = await useApp.getState().dispatch(command);
      if (result) {
        ok = true;
        if (onDone) onDone(result.diff);
      }
    } catch {
      // dispatch already reports errors. Nothing else to do.
    } finally {
      if (!ok) this.easeBack(from);
      this.op = next;
      this.snapResult = null;
      this.setLift(false);
      this.refreshToolGhost();
      this.invalidate();
    }
  }

  private levelId(): string | null {
    return this.index?.levelId ?? null;
  }

  // ------------------------------------------------------------ wall tool

  private wallClick(detail: number): void {
    const world = this.cursorWorld;
    if (!world) return;
    const p = this.snapResult?.point ?? this.snapAt(world, this.op.kind === "wall" ? this.op.points[this.op.points.length - 1] : null).point;
    if (this.op.kind !== "wall") {
      this.op = { kind: "wall", points: [p], typed: null, committing: false };
      return;
    }
    const op = this.op;
    if (detail >= 2) {
      // Second click of a double click: finish open.
      this.finishWall(op, false);
      return;
    }
    const target = op.typed ? this.typedWallPoint(op) ?? p : p;
    this.pushWallPoint(op, target);
  }

  private pushWallPoint(op: Extract<Op, { kind: "wall" }>, p: P): void {
    const last = op.points[op.points.length - 1];
    op.typed = null;
    if (dist(p, last) < 1) return;
    if (op.points.length >= 3 && dist(p, op.points[0]) < 1) {
      this.finishWall(op, true);
      return;
    }
    op.points.push(p);
  }

  /** End of the segment being typed: exact length along the current or typed direction. */
  typedWallPoint(op: Extract<Op, { kind: "wall" }>): P | null {
    if (!op.typed) return null;
    return this.typedEnd(op.points[op.points.length - 1], op.typed);
  }

  /** Typed length (and angle) from `last`, along the cursor direction when no angle is typed. */
  private typedEnd(last: P, typed: TypedState): P | null {
    if (!this.index) return null;
    const v = typedValues(typed, this.index.doc.project.settings.display_unit);
    const cursor = this.snapResult?.point ?? this.cursorWorld ?? add(last, { x: 1, y: 0 });
    const free = sub(cursor, last);
    const dir = v.b !== null ? dirDeg(v.b) : dist(cursor, last) < 1e-6 ? { x: 1, y: 0 } : unit(free);
    const length = v.a !== null ? v.a : Math.max(0, dot(free, dir));
    if (length <= 0) return null;
    // Round to a micron so typed geometry carries no float noise into the model.
    const end = add(last, mul(dir, length));
    return { x: Math.round(end.x * 1000) / 1000, y: Math.round(end.y * 1000) / 1000 };
  }

  private finishWall(op: Extract<Op, { kind: "wall" }>, closed: boolean): void {
    if (op.committing) return;
    if (op.points.length < 2) {
      this.op = { kind: "idle" };
      return;
    }
    op.committing = true;
    op.typed = null;
    void this.run({
      type: "add_wall_chain",
      points: op.points.map((q) => ({ x: q.x, y: q.y })),
      closed,
      thickness_mm: useApp.getState().toolOptions.wallThicknessMm,
      level_id: this.levelId(),
    });
  }

  // ------------------------------------------------------------ pipe tool

  /** System, material, size and start height the pipe tool draws with. */
  pipeSpec(): PipeSpec {
    return pipeSpec(useApp.getState().toolOptions);
  }

  /** The layer of the system being drawn. Locked stops the tool, hidden only warns. */
  pipeLayer(): { locked: boolean; hidden: boolean } {
    const key = this.pipeSpec().system;
    const l = this.index?.doc.project.layers.find((x) => x.key === key);
    return { locked: !!l?.locked, hidden: !!l && !l.visible };
  }

  private pipeSnapScene(exclude: readonly string[] = []): PipeSnapScene {
    const pipes: PipeEl[] = [];
    const fixtures: FixturePoint[] = [];
    if (this.index) {
      const skip = new Set(exclude);
      for (const el of this.index.visible) {
        if (skip.has(el.id)) continue;
        if (el.kind === "pipe") pipes.push(el);
        else if (el.kind === "asset" && isPlumbingFixture(el)) fixtures.push(...fixturePoints(el));
      }
    }
    return { pipes, fixtures };
  }

  /** Pipe snaps first (pipe points, fixtures, tees), then the plan snaps (grid, walls, angles). */
  private snapPipeAt(world: P, anchor: P | null, o: { system: PipeSystem; penZ: number; exclude?: string[]; extra?: P[] }): SnapResult {
    const s = useApp.getState();
    if (s.snapEnabled && this.index) {
      const ortho = (s.orthoEnabled || this.shift) && !!anchor;
      const hit = snapToPipes(world, this.pipeSnapScene(o.exclude), { tol: SNAP_PX / this.view.scale, system: o.system, penZ: o.penZ, anchor, ortho });
      if (hit) {
        return {
          point: hit.point,
          type: hit.kind,
          guides: ortho && anchor ? [{ from: anchor, to: hit.point, kind: "angle" }] : [],
          angleLocked: ortho,
          z: hit.z,
          label: hit.label,
        };
      }
    }
    return this.snapAt(world, anchor, { extra: o.extra });
  }

  /** Plan point and joined height of the next click: the typed length, else the snapped cursor. */
  private pipeTarget(op: Extract<Op, { kind: "pipe" }>): { point: P; z: number | null } | null {
    if (op.typed) {
      const p = this.typedPipePoint(op);
      if (p) return { point: p, z: null };
    }
    const r = this.snapResult;
    if (r) return { point: r.point, z: r.z ?? null };
    return this.cursorWorld ? { point: this.cursorWorld, z: null } : null;
  }

  typedPipePoint(op: Extract<Op, { kind: "pipe" }>): P | null {
    if (!op.typed || op.points.length === 0) return null;
    return this.typedEnd(planOf(op.points[op.points.length - 1]), op.typed);
  }

  /** The run so far with its pending riser, and what the next click adds (from the last point on). */
  pipePreview(op: Extract<Op, { kind: "pipe" }>): { placed: Vec3[]; band: Vec3[] } {
    const placed = withPendingRiser(op);
    const target = op.committing ? null : this.pipeTarget(op);
    if (!target) return { placed, band: [] };
    const next = addRunPoint(op, target.point, { fallPct: toolFallPct(this.pipeSpec()), snapZ: target.z });
    return { placed, band: next.points.slice(placed.length - 1) };
  }

  /** Height of the next point: the end of the rubber band while drawing, else the start height or a snapped pipe. */
  pipeNextZ(): number {
    const op = this.op;
    if (op.kind === "pipe") {
      const band = this.pipePreview(op).band;
      return band.length > 1 ? band[band.length - 1].z : op.penZ;
    }
    return this.snapResult?.z ?? this.pipeSpec().startHeightMm;
  }

  private pipeClick(detail: number): void {
    const world = this.cursorWorld;
    if (!world || !this.index) return;
    // A height being typed applies before the point goes in, as Enter would.
    if (this.heightEntry) this.applyHeightEntry();
    const spec = this.pipeSpec();
    if (this.op.kind !== "pipe") {
      if (this.pipeLayer().locked) return;
      const r = this.snapResult ?? this.snapPipeAt(world, null, { system: spec.system, penZ: spec.startHeightMm });
      const draft = addRunPoint({ points: [], penZ: spec.startHeightMm }, r.point, { fallPct: null, snapZ: r.z ?? null });
      this.op = { kind: "pipe", points: draft.points, penZ: draft.penZ, typed: null, committing: false };
      this.pulsePipePoint(r.point, spec.system);
      return;
    }
    const op = this.op;
    if (op.committing) return;
    if (detail >= 2) {
      // Second click of a double click: finish.
      this.finishPipe(op);
      return;
    }
    const target = this.pipeTarget(op);
    if (target) this.pushPipePoint(op, target);
  }

  private pushPipePoint(op: Extract<Op, { kind: "pipe" }>, target: { point: P; z: number | null }): void {
    const before = op.points.length;
    const next = addRunPoint(op, target.point, { fallPct: toolFallPct(this.pipeSpec()), snapZ: target.z });
    op.typed = null;
    op.points = next.points;
    op.penZ = next.penZ;
    if (op.points.length > before) this.pulsePipePoint(target.point, this.pipeSpec().system);
  }

  private finishPipe(op: Extract<Op, { kind: "pipe" }>): void {
    if (op.committing) return;
    const level = this.levelId();
    const points = finishRun(op);
    if (!points || !level) {
      // A run needs two points: one click and Enter only cancels.
      this.resetOp();
      return;
    }
    const spec = this.pipeSpec();
    const element: PipeEl = {
      kind: "pipe",
      id: "",
      level_id: level,
      system: spec.system,
      material: spec.material,
      diameter_mm: spec.diameterMm,
      points,
      name: "",
    };
    op.committing = true;
    op.typed = null;
    this.heightEntry = null;
    void this.run({ type: "add_element", element });
  }

  /**
   * Sets the height of the next point. While drawing, the run climbs or drops
   * there as a riser at its last point; before the first click it is the
   * start height. The tool option follows, so the options bar shows it too.
   */
  private setPipeHeight(z: number): void {
    if (!Number.isFinite(z)) return;
    const v = Math.round(z * 1000) / 1000;
    if (this.op.kind === "pipe") this.op.penZ = v;
    this.pulsePipeHeight();
    useApp.getState().setTool("pipe", { pipeElevationMm: v });
    this.refreshToolGhost();
    this.invalidate();
  }

  /** The options bar changed the height while drawing: the run takes it as a riser. */
  private onPipeHeightOption(z: number | null): void {
    const op = this.op;
    if (op.kind !== "pipe" || op.committing || z === null || Math.abs(z - op.penZ) < 0.5) return;
    op.penZ = z;
    this.pulsePipeHeight();
  }

  private applyHeightEntry(): void {
    const entry = this.heightEntry;
    this.heightEntry = null;
    if (entry && this.index) {
      const v = typedValues(entry, this.index.doc.project.settings.display_unit).a;
      if (v !== null) this.setPipeHeight(v);
    }
    this.updateUi();
  }

  private pulsePipePoint(at: P, system: PipeSystem): void {
    this.pipePulse = { at, system };
    this.anim.clear(K.pipePulse);
    this.animate(K.pipePulse, 0, dur("base"), { from: 1, easing: ease.out, drop: true });
  }

  /** Keyboard triggered, so it stays under 100 ms (docs/MOTION.md). */
  private pulsePipeHeight(): void {
    this.anim.clear(K.pipeHeight);
    this.animate(K.pipeHeight, 0, dur("press"), { from: 1, easing: ease.out, drop: true });
  }

  /** Keys of the height being typed after `h`. True when the key was used. */
  private heightKey(e: KeyboardEvent): boolean {
    const entry = this.heightEntry;
    if (!entry) return false;
    if (e.key === "Escape") {
      this.heightEntry = null;
    } else if (e.key === "Enter") {
      this.applyHeightEntry();
    } else if (/^[0-9]$/.test(e.key) || e.key === "." || e.key === "-" || e.key === "Backspace") {
      this.heightEntry = typedKey(entry, e.key);
    } else if (e.key.toLowerCase() !== "h") {
      return false;
    }
    this.updateUi();
    return true;
  }

  /** Keys while a pipe run is being drawn. Every key is swallowed by the caller. */
  private pipeOpKey(e: KeyboardEvent, op: Extract<Op, { kind: "pipe" }>): void {
    const k = e.key.toLowerCase();
    if (e.key === "Escape" && op.typed) {
      op.typed = null;
    } else if (e.key === "Escape" || (e.key === "Backspace" && !op.typed)) {
      // Escape steps back: the pending riser, then the last point, then the run.
      const back = popRunPoint(op);
      if (!back) this.resetOp();
      else {
        op.points = back.points;
        op.penZ = back.penZ;
      }
    } else if (e.key === "PageUp" || e.key === "PageDown") {
      const step = e.shiftKey ? PIPE_HEIGHT_STEP_FINE : PIPE_HEIGHT_STEP;
      this.setPipeHeight(op.penZ + (e.key === "PageUp" ? step : -step));
    } else if (k === "h" && !e.altKey) {
      op.typed = null;
      this.heightEntry = emptyTyped("height");
    } else if (isTypedKey(e.key) || (op.typed && (e.key === "Tab" || e.key === "Backspace"))) {
      op.typed = typedKey(op.typed ?? emptyTyped("polar"), e.key);
    } else if (e.key === "Enter") {
      if (!typedIsEmpty(op.typed)) {
        const p = this.typedPipePoint(op);
        if (p) this.pushPipePoint(op, { point: p, z: null });
        else op.typed = null;
      } else {
        this.finishPipe(op);
      }
    }
    this.handleMove();
    this.updateUi();
  }

  // ------------------------------------------------------------ rect room tool

  private rectDown(screen: P): void {
    const world = this.cursorWorld;
    if (!world) return;
    const p = this.snapResult?.point ?? this.snapAt(world, null).point;
    if (this.op.kind === "rect") {
      this.commitRect(this.op);
      return;
    }
    this.op = { kind: "rect", origin: p, downScreen: screen, typed: null, committing: false };
  }

  /** Opposite corner of the room being drawn, honoring typed width and depth. */
  rectCorner(op: Extract<Op, { kind: "rect" }>): P | null {
    const cursor = this.snapResult?.point ?? this.cursorWorld;
    if (!cursor || !this.index) return null;
    let dx = cursor.x - op.origin.x;
    let dy = cursor.y - op.origin.y;
    if (op.typed) {
      const v = typedValues(op.typed, this.index.doc.project.settings.display_unit);
      if (v.a !== null) dx = (dx < 0 ? -1 : 1) * v.a;
      if (v.b !== null) dy = (dy < 0 ? -1 : 1) * v.b;
    }
    return { x: op.origin.x + dx, y: op.origin.y + dy };
  }

  private commitRect(op: Extract<Op, { kind: "rect" }>): void {
    if (op.committing) return;
    const c = this.rectCorner(op);
    if (!c) return;
    const w = Math.abs(c.x - op.origin.x);
    const d = Math.abs(c.y - op.origin.y);
    if (w < 1 || d < 1) return;
    op.committing = true;
    op.typed = null;
    this.rectCommitCorner = c;
    void this.run({
      type: "add_rect_room",
      origin: { x: Math.min(c.x, op.origin.x), y: Math.min(c.y, op.origin.y) },
      width_mm: w,
      depth_mm: d,
      name: null,
      thickness_mm: useApp.getState().toolOptions.wallThicknessMm,
      level_id: this.levelId(),
    }).then(() => {
      this.rectCommitCorner = null;
    });
  }

  /** Corner frozen while the add_rect_room command is in flight. */
  rectCommitCorner: P | null = null;

  // ------------------------------------------------------------ openings

  private computeOpeningGhost(world: P, type: "door" | "window"): OpeningGhost | null {
    const index = this.index;
    if (!index) return null;
    const walls = index.visible.filter((e): e is WallEl => e.kind === "wall" && !isLocked(e, index));
    const host = findHostWall(world, walls, 14 / this.view.scale);
    if (!host) return null;
    const d = openingDefaults(type, useApp.getState().toolOptions.openingStyle);
    const placement = this.placementFor(world, host, d.width);
    let cursorFlip = false;
    if (type === "door") {
      const exterior = index.wallGeo.get(host.id)?.exterior ?? false;
      const roomSide = exterior ? roomSideOfWall(host, index.doc.derived.rooms) : null;
      cursorFlip = doorSwingSide(world, host, exterior, roomSide) === -1;
    }
    const opening: OpeningEl = {
      kind: "opening",
      id: "",
      wall_id: host.id,
      opening_type: type,
      style: d.style,
      offset_mm: placement.offset,
      width_mm: d.width,
      height_mm: d.height,
      sill_mm: d.sill,
      flip_side: cursorFlip !== this.flipSide,
      flip_hinge: this.flipHinge,
      material_id: null,
    };
    return { opening, host, placement };
  }

  private placeOpening(): void {
    const g = this.openingGhost;
    if (!g || !g.placement.valid) return;
    const o = g.opening;
    const command: Command = {
      type: "add_opening",
      wall_id: o.wall_id,
      opening_type: o.opening_type,
      offset_mm: o.offset_mm,
      width_mm: o.width_mm,
      height_mm: o.height_mm,
      sill_mm: o.sill_mm,
      style: o.style,
      flip_side: o.flip_side,
      flip_hinge: o.flip_hinge,
    };
    this.placing = true;
    void this.run(command).then(() => {
      this.placing = false;
    });
  }

  /** True while a click-to-place command is in flight (blocks double placement). */
  placing = false;

  // ------------------------------------------------------------ column, stair, asset

  private catalogItem(): CatalogItem | null {
    const s = useApp.getState();
    return s.catalog.find((c) => c.key === s.toolOptions.assetKey) ?? s.catalog[0] ?? null;
  }

  private computePlacementGhost(world: P, tool: Tool): PlacementGhost | null {
    const level = this.levelId();
    if (!level || !this.index) return null;
    const rot = normDeg(this.turns * 90);
    if (tool === "column") {
      const r = this.snapAt(world, null);
      this.snapResult = r;
      const el: Column & { kind: "column" } = {
        kind: "column",
        id: "",
        level_id: level,
        center: r.point,
        shape: "rect",
        width_mm: 300,
        depth_mm: 300,
        rotation_deg: rot,
        material_id: null,
      };
      return { element: el, faceSnap: null };
    }
    if (tool === "stair") {
      const r = this.snapAt(world, null);
      this.snapResult = r;
      const lv = this.index.doc.project.levels.find((l) => l.id === level);
      const risers = Math.max(3, Math.round((lv?.height_mm ?? 3000) / 180));
      const el: Stair & { kind: "stair" } = {
        kind: "stair",
        id: "",
        level_id: level,
        origin: r.point,
        rotation_deg: rot,
        width_mm: 1000,
        run_mm: risers * 250, // contract: going depth = run_mm / riser_count
        riser_count: risers,
      };
      return { element: el, faceSnap: null };
    }
    const item = this.catalogItem();
    if (!item) return null;
    const s = useApp.getState();
    let position = world;
    let rotation = rot;
    let faceSnap: FaceSnap | null = null;
    if (s.snapEnabled) {
      faceSnap = snapToFace(world, this.scene().faces, item.width_mm, item.depth_mm, this.turns, 28 / this.view.scale, this.gridStep());
      if (faceSnap) {
        position = faceSnap.position;
        rotation = faceSnap.rotationDeg;
      } else {
        const step = this.gridStep();
        if (step > 0) position = { x: Math.round(world.x / step) * step, y: Math.round(world.y / step) * step };
      }
    }
    const el: Asset & { kind: "asset" } = {
      kind: "asset",
      id: "",
      level_id: level,
      catalog_key: item.key,
      name: item.name,
      category: item.category,
      position,
      rotation_deg: rotation,
      width_mm: item.width_mm,
      depth_mm: item.depth_mm,
      height_mm: item.height_mm,
      elevation_mm: item.elevation_mm,
    };
    return { element: el, faceSnap };
  }

  private placeElement(): void {
    const g = this.placementGhost;
    if (!g || this.placing) return;
    this.placing = true;
    void this.run({ type: "add_element", element: g.element }).then(() => {
      this.placing = false;
    });
  }

  // ------------------------------------------------------------ dimension, camera, text

  /** Signed offset of the dimension being placed, from the cursor. */
  dimensionOffset(op: Extract<Op, { kind: "dimension" }>): number {
    if (!op.b || !this.cursorWorld) return 0;
    const d = unit(sub(op.b, op.a));
    let off = dot(sub(this.cursorWorld, op.a), { x: -d.y, y: d.x });
    const step = this.gridStep();
    if (step > 0) off = Math.round(off / step) * step;
    return off;
  }

  private dimensionClick(): void {
    const world = this.cursorWorld;
    const level = this.levelId();
    if (!world || !level) return;
    const op = this.op;
    if (op.kind !== "dimension") {
      const p = this.snapResult?.point ?? world;
      this.op = { kind: "dimension", a: p, b: null, committing: false };
      return;
    }
    if (op.committing) return;
    if (!op.b) {
      const p = this.snapResult?.point ?? world;
      if (dist(p, op.a) < 1) return;
      op.b = p;
      return;
    }
    const element: Dimension & { kind: "dimension" } = {
      kind: "dimension",
      id: "",
      level_id: level,
      a: op.a,
      b: op.b,
      offset_mm: this.dimensionOffset(op),
      text_override: null,
    };
    op.committing = true;
    void this.run({ type: "add_element", element });
  }

  private cameraClick(): void {
    const world = this.cursorWorld;
    if (!world || !this.index) return;
    const p = this.snapResult?.point ?? world;
    const op = this.op;
    if (op.kind !== "camera") {
      this.op = { kind: "camera", position: p, committing: false };
      return;
    }
    if (op.committing || dist(p, op.position) < 1) return;
    const count = this.index.doc.project.elements.filter((e) => e.kind === "camera").length;
    const element: Camera & { kind: "camera" } = {
      kind: "camera",
      id: "",
      name: `Camera ${count + 1}`,
      preset: "custom",
      position: { x: op.position.x, y: op.position.y, z: 1600 },
      target: { x: p.x, y: p.y, z: 1600 },
      fov_deg: 60,
    };
    op.committing = true;
    void this.run({ type: "add_element", element });
  }

  private openEditor(editor: InlineEditor): void {
    this.editor = editor;
    this.invalidate();
    this.updateUi();
  }

  /** Called by the inline input. `value` null cancels. */
  closeEditor(value: string | null): void {
    const ed = this.editor;
    if (!ed) return;
    this.editor = null;
    this.invalidate();
    this.updateUi();
    const text = value?.trim() ?? "";
    if (value === null || text === "" || !this.index) return;
    if (ed.mode === "new_text") {
      const level = this.levelId();
      if (!level) return;
      const element: Annotation & { kind: "annotation" } = {
        kind: "annotation",
        id: "",
        level_id: level,
        position: ed.world,
        text,
        size_mm: 250,
        rotation_deg: 0,
      };
      void this.run({ type: "add_element", element });
      return;
    }
    const el = ed.id ? this.index.byId.get(ed.id) : null;
    if (!el) return;
    if (el.kind === "room" && text !== el.name) {
      const next: Room & { kind: "room" } = { ...el, name: text, auto_named: false };
      void this.run({ type: "update_element", element: next });
    } else if (el.kind === "annotation" && text !== el.text) {
      void this.run({ type: "update_element", element: { ...el, text } });
    }
  }

  // ------------------------------------------------------------ keyboard

  private resetOp(): void {
    this.op = { kind: "idle" };
    this.heightEntry = null;
    this.panReturn = { kind: "idle" };
    this.snapResult = null;
    this.openingGhost = null;
    this.placementGhost = null;
    this.setLift(false);
    this.syncSnapAnim();
    this.syncGhostAnim();
  }

  private onToolChange(_from: Tool, _to: Tool): void {
    if (!this.isBusy()) this.resetOp();
    this.heightEntry = null;
    this.flipSide = false;
    this.flipHinge = false;
    this.turns = 0;
    const s = useApp.getState();
    if (_to === "asset" && s.catalog.length === 0) void s.loadCatalog();
    if (s.hoverId) s.setHover(null);
    this.syncGrips(s);
    this.refreshToolGhost();
    this.invalidate();
  }

  /** Right click and Enter: finish what can be finished, else cancel. */
  private finishOrCancel(): void {
    const op = this.op;
    if (op.kind === "wall") this.finishWall(op, false);
    else if (op.kind === "pipe") this.finishPipe(op);
    else if (!this.isBusy() && op.kind !== "idle") this.resetOp();
    this.invalidate();
  }

  /** True when a tool operation would consume Escape, Enter, Backspace or digits. */
  private inOperation(): boolean {
    const k = this.op.kind;
    return k !== "idle" && k !== "pan";
  }

  private onKeyDown = (e: KeyboardEvent): void => {
    const target = e.target as HTMLElement | null;
    const typing = !!target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable);
    if (e.key === "Shift") {
      this.shift = true;
      if (this.inOperation() || this.pointerInside) this.handleMove();
      return;
    }
    // Walking or flying in 3D: that view owns every key without MOD (docs/CONTRACT.md).
    if (useViewer.getState().nav !== "orbit") return;
    if (typing) return;
    if (e.key === " " || e.code === "Space") {
      if (this.pointerInside || this.op.kind === "pan") {
        this.space = true;
        e.preventDefault();
        this.updateUi();
      }
      return;
    }
    if (e.metaKey || e.ctrlKey) return;
    const s = useApp.getState();
    const op = this.op;
    const swallow = (): void => {
      e.preventDefault();
      e.stopPropagation();
      this.invalidate();
    };

    // A height typed after `h` (pipe tool) takes digits, Enter and Escape first,
    // so h1500 never reaches the 1, 2, 3 view shortcuts.
    if (this.heightEntry) {
      if (s.tool !== "pipe") this.heightEntry = null;
      else if (this.heightKey(e)) {
        swallow();
        return;
      }
    }

    if (this.inOperation()) {
      if (this.isBusy()) {
        swallow();
        return;
      }
      if (op.kind === "pipe") {
        this.pipeOpKey(e, op);
        swallow();
        return;
      }
      if (e.key === "Escape") {
        // Escape first clears a typed value, then cancels the operation.
        if ((op.kind === "wall" || op.kind === "rect") && op.typed) op.typed = null;
        else this.resetOp();
        swallow();
        return;
      }
      if (op.kind === "wall" || op.kind === "rect") {
        const mode = op.kind === "wall" ? "polar" : "rect";
        if (isTypedKey(e.key) || (op.typed && (e.key === "Tab" || e.key === "Backspace"))) {
          op.typed = typedKey(op.typed ?? emptyTyped(mode), e.key);
          swallow();
          return;
        }
        if (e.key === "Backspace" && op.kind === "wall") {
          if (op.points.length > 1) op.points.pop();
          else this.resetOp();
          this.handleMove();
          swallow();
          return;
        }
        if (e.key === "Enter") {
          if (op.kind === "wall") {
            if (!typedIsEmpty(op.typed)) {
              const p = this.typedWallPoint(op);
              if (p) this.pushWallPoint(op, p);
              else op.typed = null;
              this.handleMove();
            } else this.finishWall(op, false);
          } else this.commitRect(op);
          swallow();
          return;
        }
      }
      if (e.key === "Enter" || e.key === "Backspace") {
        swallow();
        return;
      }
      // Any other key (a tool letter, a nudge arrow...) must not reach the
      // shell's global shortcuts while a drawing operation is in progress:
      // for example W pressed mid wall-chain must not restart the tool.
      swallow();
      return;
    }

    // Idle. Only tool keys that act on a visible ghost are handled.
    if (e.key === "Escape" && s.tool !== "select") {
      // Idle: act, but let the key through so the shell can react too.
      s.setTool("select");
      return;
    }
    const k = e.key.toLowerCase();
    if (this.pointerInside && (k === "+" || k === "=" || k === "-" || k === "_")) {
      this.zoomStep(k === "+" || k === "=" ? 1 : -1);
      swallow();
      return;
    }
    if ((s.tool === "door" || s.tool === "window") && this.pointerInside && (k === "f" || k === "h")) {
      if (k === "f") this.flipSide = !this.flipSide;
      else this.flipHinge = !this.flipHinge;
      this.refreshToolGhost();
      swallow();
      return;
    }
    // Pipe tool over the canvas: PageUp and PageDown set the start height, h types it.
    // h is the Pan shortcut elsewhere, like the door tool's F and H.
    if (s.tool === "pipe" && this.pointerInside && !e.altKey) {
      if (e.key === "PageUp" || e.key === "PageDown") {
        const step = e.shiftKey ? PIPE_HEIGHT_STEP_FINE : PIPE_HEIGHT_STEP;
        this.setPipeHeight(this.pipeSpec().startHeightMm + (e.key === "PageUp" ? step : -step));
        swallow();
        return;
      }
      if (k === "h") {
        this.heightEntry = emptyTyped("height");
        this.updateUi();
        swallow();
        return;
      }
    }
    if ((s.tool === "asset" || s.tool === "stair" || s.tool === "column") && this.pointerInside && k === "r") {
      this.turns = (this.turns + 1) % 4;
      this.refreshToolGhost();
      swallow();
    }
  };

  private onKeyUp = (e: KeyboardEvent): void => {
    if (e.key === "Shift") {
      this.shift = false;
      if (this.inOperation() || this.pointerInside) this.handleMove();
    }
    if (e.key === " " || e.code === "Space") {
      this.space = false;
      this.updateUi();
    }
  };

  /** Angle of the segment being drawn, for the readout. */
  static angleOf(a: P, b: P): number {
    return dist(a, b) < 1e-9 ? 0 : angleDeg(sub(b, a));
  }
}

type Ctx2D = CanvasRenderingContext2D;
