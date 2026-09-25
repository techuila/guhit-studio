// Interaction controller for the plan canvas. Owns the view, the current
// operation and all pointer and keyboard handling. It never mutates the
// document: every change is one `dispatch(command)`.

import type {
  Annotation,
  Camera,
  CatalogItem,
  Column,
  Command,
  Dimension,
  Element,
  LayerKey,
  Mount,
  OpeningStyle,
  PipeSystem,
  Room,
  Stair,
  Vec3,
  Wall,
} from "../contract/bindings";
import { ipc } from "../contract/ipc";
import { PIPE_LAYER } from "../contract/pipes";
import { bus } from "../state/bus";
import { useApp, type AppState, type Tool } from "../state/store";
import { dur, ease, motionOK } from "../ui/motion";
import { useViewer } from "../viewer3d/viewerStore";
import { Anim, breathe, mix, mixP } from "./anim";
import type { Grip } from "./edit";
import { gripsFor, hitGrip, jointInset, normalDelta, rotationFromGrip } from "./edit";
import type { P, Rect, Seg } from "./geom";
import { add, angleDeg, cross, dirDeg, dist, dot, mul, normDeg, rectFromPoints, rectIsEmpty, sub, unit } from "./geom";
import { hitTest, hitsElement, marqueeSelect } from "./hit";
import type { AssetEl, Linkable, LinkPair } from "./links";
import { allLinks, defaultBow, deviceKindOf, linkArc, linkKey, linkables, linksOf, resolveLinkClick, threeWaySwitches, trimLink } from "./links";
import type { DocIndex, OpeningEl, WallEl } from "./model";
import { boundsOfIds, buildIndex, isLocked, keyPoints, layerOf, modelBounds, symbolMmOf, wallOutline } from "./model";
import type { LatchGuide, WallFace, WallMount, WindowHost } from "./mount";
import {
  ceilingElevation,
  ceilingHeightMm,
  facesOfWall,
  latchGuides,
  mountAtGuide,
  mountHeightLabel,
  mountInWindow,
  nearestGuide,
  nearestWindow,
  roomCenterSnap,
  snapToWallFace,
  usableGuides,
} from "./mount";
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
  isServiceFixture,
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
import { DEFAULT_PALETTE, drawElement, drawFlash, drawGrid, drawHighlight, drawModel, drawOrigin, drawPeerOutline, labelHeightMm, readPalette } from "./render";
import { decideResize } from "./resizePolicy";
import type { SnapResult, SnapScene } from "./snap";
import { computeIntersections, emptyScene, snap } from "./snap";
import { assetSymbolAnchor } from "./symbols";
import type { Box } from "./tags";
import type { TypedState } from "./typed";
import { emptyTyped, formatArea, isTypedKey, typedIsEmpty, typedKey, typedValues } from "./typed";
import type { View } from "./view";
import { fitRect, panBy, snapStep, toScreen, toWorld, zoomAt } from "./view";

export const SNAP_PX = 10;
const DRAG_PX = 4;
const GRIP_PX = 9;
/** Radius of a link's flip handle, CSS pixels. */
export const HANDLE_PX = 5;

/** Layer names as the layer panel and the engine's messages give them (`layer_name`). */
const LAYER_LABEL: Record<LayerKey, string> = {
  walls: "Walls",
  openings: "Openings",
  rooms: "Rooms",
  columns: "Columns",
  stairs: "Stairs",
  assets: "Assets",
  annotations: "Annotations",
  dimensions: "Dimensions",
  underlays: "Underlays",
  cold_water: "Cold water",
  hot_water: "Hot water",
  drainage: "Drainage",
  vent: "Vent",
  storm: "Storm drainage",
  electrical: "Electrical",
  aircon: "Aircon",
};
function layerLabel(key: LayerKey): string {
  return LAYER_LABEL[key] ?? key;
}
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
  /** A refused placement: the reason pill bumps once. */
  placeRefused: "place.no",
  /** `lnk:<key>` a new link drawing in, `lnx:<key>` a removed one fading, `bow:<key>` a flip sweeping. */
  linkGrow: "lnk:",
  linkGone: "lnx:",
  bow: "bow:",
  /** `bowh:<key>`: the flip handle under the pointer grows. */
  bowHover: "bowh:",
  /** `tagi:<id>` a fall or height tag fading in, `tago:<id>` one the layout dropped fading out. */
  tagIn: "tagi:",
  tagOut: "tago:",
  /** `psel:<n>`: another participant's selection outline fading in or out (live session). */
  peerSel: "psel:",
  /** The ring that marks where `focus_point` centered the view. */
  focusRing: "focus.ring",
} as const;

/** Another participant's selection as the plan outlines it. `color` is a CSS color. */
export interface PeerSelectionDraw {
  id: string;
  color: string;
  ids: readonly string[];
}

/** Told the drawn view transform and the canvas size whenever either changes. */
export type ViewListener = (view: View, width: number, height: number) => void;

/** A fall or height tag as drawn: its box in CSS pixels and how it looks. */
export interface TagDraw {
  id: string;
  box: Box;
  text: string;
  color: string;
  filled: boolean;
  alpha: number;
}

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
  | { kind: "camera"; position: P; committing: boolean }
  /** Link tool with a device picked: each click on a load toggles its link. */
  | { kind: "link"; sourceId: string; committing: boolean };

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
  /** How a mounted object sits (`CatalogItem::mount`). Null for columns and stairs. */
  mount: MountGhost | null;
}

export interface MountGhost {
  kind: Mount;
  /** False when a click would place nothing. `reason` says why. */
  valid: boolean;
  reason: string | null;
  /** The mounting height shown near the cursor: "Center +1200". */
  heightLabel: string | null;
  /** The wall face the back sits on. */
  face: Seg | null;
  /** The latch-side switch guide of the nearest door, and whether the switch snapped to it. */
  guide: (LatchGuide & { snapped: boolean }) | null;
  /** The window a window aircon goes into. */
  window: Seg | null;
}

/** A link as drawn this frame: both anchors, the bow (animated) and whether it takes a flip handle. */
export interface DrawnLink {
  key: string;
  controllerId: string;
  loadId: string;
  from: P;
  to: P;
  bow: number;
  /** Full strength (the selection's links), else a faint overview line. */
  strong: boolean;
  /** Where the flip handle sits: the middle of the curve. Null when it has none. */
  handle: P | null;
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
  /** Links whose bow the user flipped. View only: never saved, gone on reload. */
  private bowFlipped = new Set<string>();
  /** The side each link first bowed to, so a new load never flips the others. */
  private bowDefaults = new Map<string, 1 | -1>();
  /** Links a change removed, still drawn from where they were while they fade. */
  fadingLinks: { key: string; controllerId: string; loadId: string; from: P; to: P; bow: number }[] = [];
  /** The link flip handle under the pointer. */
  hoverHandle: string | null = null;
  /** Link tool: a refused click (a locked layer) shows its reason here. */
  linkNotice: string | null = null;
  /** Dev checks: the last tag layout, how many tags asked for a spot, where they went and what they kept clear of. */
  tagLayout: { asked: number; placed: { id: string; box: Box }[]; obstacles: Box[] } | null = null;
  /** Fall and height tags drawn last frame, and the ones the layout dropped, fading out. */
  private tagMemo = new Map<string, TagDraw>();
  private fadingTags = new Map<string, TagDraw>();
  /** Live session: other participants' selections, and ones fading out after a change. */
  private peerSel = new Map<string, { color: string; ids: string[]; key: string }>();
  private peerSelFading: { color: string; ids: string[]; key: string }[] = [];
  private peerSelSeq = 0;
  /** Where the last `focus_point` centered the view, marked by a ring once. */
  private focusMark: P | null = null;
  private viewListeners = new Set<ViewListener>();
  private notified: { view: View; width: number; height: number } | null = null;
  private catalogCache: { catalog: CatalogItem[]; map: Map<string, CatalogItem> } | null = null;
  private deviceCache: { elements: Element[]; catalog: Map<string, CatalogItem>; all: Map<string, Linkable>; threeWay: Set<string> } | null = null;
  private faceCache: { index: DocIndex; faces: WallFace[] } | null = null;

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
    this.cleanups.push(bus.on("focus_point", ({ point, level_id }) => this.focusPoint(point, level_id)));

    let prev = useApp.getState();
    this.cleanups.push(
      useApp.subscribe((s) => {
        const p = prev;
        prev = s;
        if (s.doc !== p.doc || s.preview !== p.preview || s.activeLevelId !== p.activeLevelId) this.syncDoc();
        if (s.tool !== p.tool) this.onToolChange(p.tool, s.tool);
        else if (s.tool === "link" && s.selection !== p.selection) this.followLinkSelection(s.selection);
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
    useApp.getState().registerCapturePlan((levelId) => this.capturePlan(levelId));
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
    this.viewListeners.clear();
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
      return useApp.getState().hoverId || this.hoverHandle ? "pointer" : "default";
    }
    if (tool === "link") return useApp.getState().hoverId || this.hoverHandle ? "pointer" : "crosshair";
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
        return this.assetHint();
      case "link":
        return this.linkHint();
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
        const system = this.pipeSpec().system;
        const name = PIPE_SYSTEM_LABEL[system];
        const layerName = layerLabel(PIPE_LAYER[system]);
        const layer = this.pipeLayer();
        if (layer.locked) return `The ${layerName} layer is locked. Unlock it to draw ${name.toLowerCase()} runs`;
        if (layer.hidden) return `The ${layerName} layer is hidden. New runs show when it is on`;
        return `Click to start a ${name.toLowerCase()} run. PageUp or PageDown sets the height, or type h and a height`;
      }
      default:
        return null;
    }
  }

  private assetHint(): string {
    const item = this.catalogItem();
    switch (item?.mount ?? "floor") {
      case "wall":
        return item && this.isSwitchItem(item)
          ? "Click to place on a wall face. Snaps 200 mm from the latch side of the nearest door"
          : "Click to place on a wall face";
      case "ceiling":
        return "Click to place on the ceiling. Snaps to the middle of the room";
      case "opening":
        return "Hover a window to set the unit into it, then click";
      default:
        return "Click to place. R rotates. Snaps to wall faces";
    }
  }

  private linkHint(): string {
    const source = this.linkSource();
    const role = source ? this.devices().all.get(source)?.role : null;
    switch (role) {
      case "switch":
        return "Click lights to link or unlink them. Esc ends";
      case "outlet":
        return "Click the aircon unit it feeds to link or unlink it. Esc ends";
      case "light":
        return "Click the switches that control this light. Esc ends";
      case "unit":
        return "Click the outlet that feeds this unit. Esc ends";
      default:
        return "Click a switch or an aircon outlet, then the lights or unit it controls";
    }
  }

  private isSwitchItem(item: CatalogItem): boolean {
    return (item.device ?? (item.key.startsWith("switch-") ? "switch" : null)) === "switch";
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
      this.fadingLinks = [];
      this.bowFlipped.clear();
      this.bowDefaults.clear();
      this.areaText.clear();
      this.areaPrev.clear();
      this.returning = null;
    }
    // The device being linked is gone (deleted, undone, another level): linking ends.
    const op = this.op;
    if (op.kind === "link" && !op.committing && !this.index?.visibleIds.has(op.sourceId)) this.op = { kind: "idle" };
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
    this.diffLinks(prev.index, index);
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
   * Centers the plan on a point at the same zoom, switching to its level
   * first when one is given (`focus_point`: a live session participant's
   * pointer, a cursor chat message). A ring marks the spot once the view
   * has eased there.
   */
  focusPoint(p: P, levelId: string | null = null): void {
    const s = useApp.getState();
    if (levelId && levelId !== s.activeLevelId && s.doc?.project.levels.some((l) => l.id === levelId)) s.setActiveLevel(levelId);
    if (this.width === 0 || !Number.isFinite(p.x) || !Number.isFinite(p.y)) return;
    this.autoFit = false;
    const scale = this.view.scale;
    this.setView({ scale, ox: this.width / 2 - p.x * scale, oy: this.height / 2 + p.y * scale }, dur("scene"));
    this.focusMark = { x: p.x, y: p.y };
    this.anim.clear(K.focusRing);
    this.animate(K.focusRing, 0, dur("scene"), { from: 1, easing: ease.out, drop: true, delayMs: dur("scene") });
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

  // ------------------------------------------------------------ live session

  /** Plan mm to canvas CSS pixels, with the transform on screen right now. */
  planToScreen(p: P): P {
    return toScreen(this.drawView(), p);
  }

  /**
   * Calls `fn` with the drawn view and the canvas size now, then whenever
   * either changes: pan, zoom, a fit or focus easing, a resize. It is told
   * from the draw pass, so following the view costs no frame of its own.
   */
  onViewChange(fn: ViewListener): () => void {
    this.viewListeners.add(fn);
    fn(this.drawView(), this.width, this.height);
    return () => {
      this.viewListeners.delete(fn);
    };
  }

  private notifyView(view: View): void {
    if (this.viewListeners.size === 0) return;
    const n = this.notified;
    if (n && n.width === this.width && n.height === this.height && n.view.scale === view.scale && n.view.ox === view.ox && n.view.oy === view.oy) return;
    this.notified = { view, width: this.width, height: this.height };
    for (const fn of this.viewListeners) fn(view, this.width, this.height);
  }

  /** Nothing in progress on the plan: no drawing, dragging, typed entry, inline text or held Space. */
  isIdle(): boolean {
    return this.op.kind === "idle" && !this.heightEntry && !this.editor && !this.space;
  }

  /**
   * Other participants' selections (live session), outlined thin in their
   * colors on the level on screen. Only a real change starts a fade and a
   * redraw; the same selections again cost nothing.
   */
  setPeerSelections(list: readonly PeerSelectionDraw[]): void {
    let changed = false;
    const seen = new Set<string>();
    for (const item of list) {
      seen.add(item.id);
      const cur = this.peerSel.get(item.id);
      if (cur && cur.color === item.color && sameIds(cur.ids, item.ids)) continue;
      if (cur) this.fadePeerSelection(cur);
      const key = `${K.peerSel}${++this.peerSelSeq}`;
      this.animate(key, 1, dur("hover"), { from: 0, drop: true });
      this.peerSel.set(item.id, { color: item.color, ids: [...item.ids], key });
      changed = true;
    }
    for (const [id, cur] of this.peerSel) {
      if (seen.has(id)) continue;
      this.peerSel.delete(id);
      this.fadePeerSelection(cur);
      changed = true;
    }
    if (changed) this.invalidate();
  }

  private fadePeerSelection(sel: { color: string; ids: string[]; key: string }): void {
    this.animate(sel.key, 0, exitDur("hover"), { from: 1, drop: true });
    this.peerSelFading.push(sel);
  }

  private drawPeerSelections(rc: RenderContext): void {
    const index = this.index;
    if (!index) return;
    const draw = (sel: { color: string; ids: string[] }, alpha: number): void => {
      if (alpha <= 0.002) return;
      for (const id of sel.ids) {
        const el = index.byId.get(id);
        if (el && index.visibleIds.has(id)) drawPeerOutline(rc, el, sel.color, alpha);
      }
    };
    for (const f of this.peerSelFading) draw(f, this.anim.value(f.key, 0));
    for (const sel of this.peerSel.values()) draw(sel, this.anim.value(sel.key, 1));
  }

  /** The ring where `focus_point` centered the view: it grows and fades once. */
  private drawFocusRing(rc: RenderContext): void {
    const at = this.focusMark;
    if (!at || !this.anim.has(K.focusRing)) return;
    const v = this.anim.value(K.focusRing, 0);
    if (v <= 0.002) return;
    const c = toScreen(rc.view, at);
    const { ctx } = this;
    ctx.save();
    ctx.strokeStyle = this.palette.selection;
    ctx.globalAlpha = 0.85 * v;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(c.x, c.y, 7 + (1 - v) * 22, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
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
    if (this.fadingLinks.length > 0) this.fadingLinks = this.fadingLinks.filter((f) => this.anim.has(`${K.linkGone}${f.key}`));
    if (this.peerSelFading.length > 0) this.peerSelFading = this.peerSelFading.filter((f) => this.anim.has(f.key));
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
      threeWay: this.devices().threeWay,
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
    this.notifyView(rc.view);
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
    // Other participants' selections sit under this window's own.
    if (this.peerSel.size > 0 || this.peerSelFading.length > 0) this.drawPeerSelections(rc);

    const selected = new Set(s.selection);
    if (this.op.kind === "idle" || this.op.kind === "link") {
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
    this.drawFocusRing(rc);
  }

  /**
   * Renders the plan without grid, handles or cameras on white: the level on
   * screen, or `levelId` when given (an MCP plan image of another level).
   */
  async capturePlan(levelId?: string | null): Promise<string> {
    const s = useApp.getState();
    const doc = s.doc;
    const level = levelId && doc?.project.levels.some((l) => l.id === levelId) ? levelId : s.activeLevelId;
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
      const index = buildIndex(doc, level);
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
        threeWay: threeWaySwitches([...linkables(doc.project.elements, this.catalogMap()).values()]),
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

  // ------------------------------------------------------------ devices and links

  /** The catalog by key, for device kinds. */
  catalogMap(): Map<string, CatalogItem> {
    const catalog = useApp.getState().catalog;
    if (this.catalogCache?.catalog !== catalog) this.catalogCache = { catalog, map: new Map(catalog.map((c) => [c.key, c] as const)) };
    return this.catalogCache.map;
  }

  /**
   * Every object that links, in the whole project (a stair light is switched
   * from both floors), and the switches that make a 3-way.
   */
  devices(): { all: Map<string, Linkable>; threeWay: Set<string> } {
    const elements = this.index?.doc.project.elements ?? [];
    const catalog = this.catalogMap();
    const c = this.deviceCache;
    if (c && c.elements === elements && c.catalog === catalog) return c;
    const all = linkables(elements, catalog);
    const next = { elements, catalog, all, threeWay: threeWaySwitches([...all.values()]) };
    this.deviceCache = next;
    return next;
  }

  /** The linkable object under a plan point, on the active level. Locked ones too: a load is never changed. */
  deviceAt(world: P): string | null {
    const index = this.index;
    if (!index) return null;
    const { all } = this.devices();
    const opt = { ...this.hitOptions(), includeLocked: true };
    for (let i = index.visible.length - 1; i >= 0; i--) {
      const el = index.visible[i];
      if (el.kind === "asset" && all.has(el.id) && hitsElement(world, el, index, opt)) return el.id;
    }
    return null;
  }

  /** Where links attach to an object's symbol, and the radius the symbol keeps clear. */
  anchorOf(el: AssetEl): P & { r: number } {
    return assetSymbolAnchor(el, { symbolMm: this.index ? symbolMmOf(this.index) : undefined });
  }

  /** The drawn ends of a link: trimmed to the symbols it joins. */
  linkEnds(controller: AssetEl, load: AssetEl): { from: P; to: P } | null {
    return trimLink(this.anchorOf(controller), this.anchorOf(load));
  }

  /** The device the link tool links from, when one is picked. */
  linkSource(): string | null {
    return this.op.kind === "link" ? this.op.sourceId : null;
  }

  /**
   * The links drawn this frame: the ones of the selection (or of the device
   * the link tool picked) at full strength with a flip handle, and while the
   * link tool is on, every other link faintly. Both ends must be visible, so
   * links follow the electrical and aircon layers.
   */
  linkDrawList(): DrawnLink[] {
    const index = this.index;
    if (!index) return [];
    const s = useApp.getState();
    const { all } = this.devices();
    const source = this.linkSource();
    const focus = s.tool === "link" ? (source ? [source] : []) : s.selection;
    const strong = linksOf(focus, all);
    const pairs: { pair: LinkPair; strong: boolean; handle: boolean }[] = strong.map((pair) => ({ pair, strong: true, handle: true }));
    const seen = new Set(strong.map((p) => p.key));
    // Links an AI proposal adds or changes show without a selection, in the preview color.
    if (s.preview) {
      for (const pair of linksOf([...s.preview.diff.added, ...s.preview.diff.modified], all)) {
        if (seen.has(pair.key)) continue;
        seen.add(pair.key);
        pairs.push({ pair, strong: true, handle: false });
      }
    }
    if (s.tool === "link") {
      for (const pair of allLinks(all)) if (!seen.has(pair.key)) pairs.push({ pair, strong: false, handle: false });
    }
    const out: DrawnLink[] = [];
    for (const { pair, strong: isStrong, handle } of pairs) {
      if (!index.visibleIds.has(pair.controllerId) || !index.visibleIds.has(pair.loadId)) continue;
      const c = all.get(pair.controllerId);
      const l = all.get(pair.loadId);
      if (!c || !l) continue;
      const ends = this.linkEnds(c.el, l.el);
      if (!ends) continue;
      const { from, to } = ends;
      const bow = this.anim.value(`${K.bow}${pair.key}`, this.bowTarget(pair, from, to, all));
      out.push({ ...pair, from, to, bow, strong: isStrong, handle: handle ? linkArc(from, to, bow).apex : null });
    }
    return out;
  }

  /** The side a link bows to: its first default, or the other side once flipped. */
  private bowTarget(pair: LinkPair, from: P, to: P, all: Map<string, Linkable>): 1 | -1 {
    let d = this.bowDefaults.get(pair.key);
    if (d === undefined) {
      const c = all.get(pair.controllerId)?.el;
      const others: P[] = [];
      for (const id of c?.links ?? []) {
        const o = id !== pair.loadId ? all.get(id)?.el : undefined;
        if (o) others.push(this.anchorOf(o));
      }
      d = defaultBow(from, to, others);
      this.bowDefaults.set(pair.key, d);
    }
    return this.bowFlipped.has(pair.key) ? (d === 1 ? -1 : 1) : d;
  }

  /** The bow a link between these two is drawn with: the one it keeps once it exists. */
  linkBow(controllerId: string, loadId: string, from: P, to: P): number {
    const pair = { key: linkKey(controllerId, loadId), controllerId, loadId };
    return this.anim.value(`${K.bow}${pair.key}`, this.bowTarget(pair, from, to, this.devices().all));
  }

  /** Flips one link to its other side: it sweeps through the straight line. View only. */
  flipBow(key: string): void {
    const d = this.bowDefaults.get(key) ?? 1;
    const before = this.bowFlipped.has(key) ? -d : d;
    if (this.bowFlipped.has(key)) this.bowFlipped.delete(key);
    else this.bowFlipped.add(key);
    this.animate(`${K.bow}${key}`, -before, dur("base"), { from: before, easing: ease.inOut, drop: true });
  }

  /** The flip handle under a screen point. */
  private handleAt(screen: P): string | null {
    let best: string | null = null;
    let bestD = HANDLE_PX + 3;
    for (const l of this.linkDrawList()) {
      if (!l.handle) continue;
      const d = dist(toScreen(this.view, l.handle), screen);
      if (d <= bestD) {
        bestD = d;
        best = l.key;
      }
    }
    return best;
  }

  /** The handle under the pointer grows on hover and shrinks back when left. */
  private setHoverHandle(key: string | null): void {
    if (key === this.hoverHandle) return;
    if (this.hoverHandle) this.animate(`${K.bowHover}${this.hoverHandle}`, 0, exitDur("hover"), { drop: true });
    if (key) this.animate(`${K.bowHover}${key}`, 1, dur("hover"), { from: 0 });
    this.hoverHandle = key;
  }

  /** New links draw in and removed ones fade, whatever changed them (link tool, undo, the copilot). */
  private diffLinks(before: DocIndex, after: DocIndex): void {
    const enter = dur("base");
    const leave = exitDur("base");
    const same = (a: readonly string[], b: readonly string[]): boolean => a.length === b.length && a.every((x, i) => x === b[i]);
    for (const el of after.doc.project.elements) {
      if (el.kind !== "asset") continue;
      const old = before.byId.get(el.id);
      const was = old && old.kind === "asset" ? old.links : [];
      if (same(was, el.links)) continue;
      if (enter > 0) {
        for (const id of el.links) if (!was.includes(id)) this.anim.to(`${K.linkGrow}${linkKey(el.id, id)}`, 1, enter, { from: 0, easing: ease.out, drop: true });
      }
      for (const id of was) if (!el.links.includes(id)) this.fadeLink(before, el.id, id, leave);
    }
    for (const old of before.doc.project.elements) {
      if (old.kind === "asset" && old.links.length > 0 && !after.byId.has(old.id)) for (const id of old.links) this.fadeLink(before, old.id, id, leave);
    }
  }

  private fadeLink(index: DocIndex, controllerId: string, loadId: string, ms: number): void {
    if (ms <= 0) return;
    const c = index.byId.get(controllerId);
    const l = index.byId.get(loadId);
    if (!c || c.kind !== "asset" || !l || l.kind !== "asset") return;
    const key = linkKey(controllerId, loadId);
    const ends = this.linkEnds(c, l);
    if (!ends) return;
    const d = this.bowDefaults.get(key) ?? 1;
    this.fadingLinks.push({ key, controllerId, loadId, ...ends, bow: this.bowFlipped.has(key) ? -d : d });
    this.anim.to(`${K.linkGone}${key}`, 0, ms, { from: 1, easing: ease.in, drop: true });
  }

  /**
   * Tags the layout placed this frame fade in the first time; the ones it
   * dropped fade out where they were. Returns those still fading out.
   */
  syncTags(placed: readonly TagDraw[]): TagDraw[] {
    const now = new Set(placed.map((t) => t.id));
    for (const t of placed) {
      if (!this.tagMemo.has(t.id)) {
        const from = this.anim.value(`${K.tagOut}${t.id}`, 0);
        this.anim.clear(`${K.tagOut}${t.id}`);
        this.fadingTags.delete(t.id);
        if (dur("hover") > 0) this.animate(`${K.tagIn}${t.id}`, 1, dur("hover"), { from, drop: true });
      }
      this.tagMemo.set(t.id, t);
    }
    for (const [id, t] of this.tagMemo) {
      if (now.has(id)) continue;
      this.tagMemo.delete(id);
      const from = this.anim.value(`${K.tagIn}${id}`, 1);
      this.anim.clear(`${K.tagIn}${id}`);
      if (exitDur("hover") > 0) {
        this.fadingTags.set(id, t);
        this.animate(`${K.tagOut}${id}`, 0, exitDur("hover"), { from, easing: ease.in, drop: true });
      }
    }
    for (const id of this.fadingTags.keys()) if (!this.anim.has(`${K.tagOut}${id}`)) this.fadingTags.delete(id);
    return [...this.fadingTags.values()];
  }

  /** One click of the link tool. */
  private linkClick(): void {
    const world = this.cursorWorld;
    const index = this.index;
    if (!world || !index) return;
    const op = this.op;
    if (op.kind === "link" && op.committing) return;
    const s = useApp.getState();
    const { all } = this.devices();
    const sourceId = this.linkSource();
    const r = resolveLinkClick(sourceId, this.deviceAt(world), (id) => all.get(id) ?? null);
    this.linkNotice = null;
    switch (r.kind) {
      case "none":
        return;
      case "clear":
        this.op = { kind: "idle" };
        s.select([]);
        return;
      case "pick":
        this.op = { kind: "link", sourceId: r.id, committing: false };
        s.select([r.id]);
        return;
      case "toggle": {
        const controller = all.get(r.controllerId)?.el;
        if (!controller || !sourceId) return;
        if (isLocked(controller, index)) {
          this.refuseLink(`The ${layerLabel(layerOf(controller))} layer is locked. Unlock it to link`);
          return;
        }
        const next: Op = { kind: "link", sourceId, committing: false };
        if (op.kind === "link") op.committing = true;
        void this.run(r.command, undefined, next);
        return;
      }
    }
  }

  /** A link click that cannot apply: say why near the cursor, with a bump. */
  private refuseLink(reason: string): void {
    this.linkNotice = reason;
    this.anim.clear(K.placeRefused);
    this.animate(K.placeRefused, 0, dur("base"), { from: 1, easing: ease.out, drop: true });
  }

  /**
   * Link tool: the selection picks the device being linked, so the
   * inspector's "link from here" works while the tool is already on.
   */
  private followLinkSelection(selection: readonly string[]): void {
    const op = this.op;
    if (op.kind === "link" && op.committing) return;
    if (selection.length === 0) {
      if (op.kind === "link") this.op = { kind: "idle" };
      return;
    }
    const id = selection.length === 1 ? selection[0] : null;
    if (!id || this.linkSource() === id) return;
    if (this.devices().all.has(id) && this.index?.visibleIds.has(id)) this.op = { kind: "link", sourceId: id, committing: false };
    this.invalidate();
  }

  /** Link tool: picks the selected device right away, so L with a switch selected starts linking it. */
  private startLinkTool(): void {
    const s = useApp.getState();
    if (s.selection.length !== 1) return;
    const id = s.selection[0];
    if (this.devices().all.has(id) && this.index?.visibleIds.has(id)) this.op = { kind: "link", sourceId: id, committing: false };
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
      case "link": {
        const handle = this.handleAt(screen);
        if (handle) this.flipBow(handle);
        else this.linkClick();
        break;
      }
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
    const handles = (s.tool === "select" && op.kind === "idle") || (s.tool === "link" && !this.isBusy());
    this.setHoverHandle(handles && this.pointerInside ? this.handleAt(screen) : null);

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
      const grip = grips.length > 0 && !this.hoverHandle ? hitGrip(grips, world, GRIP_PX / this.view.scale) : null;
      this.setHoverGrip(grip ? grips.indexOf(grip) : null);
      const id = this.hoverHandle ? null : hitTest(world, this.index, this.hitOptions());
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
      case "link": {
        const id = this.hoverHandle ? null : this.deviceAt(world);
        const s = useApp.getState();
        if (id !== s.hoverId) s.setHover(id);
        break;
      }
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
    this.setHoverHandle(null);
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
    // A link's flip handle, drawn over everything while its device is selected.
    const handle = this.handleAt(screen);
    if (handle) {
      this.flipBow(handle);
      return;
    }
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
    const key = PIPE_LAYER[this.pipeSpec().system];
    const l = this.index?.doc.project.layers.find((x) => x.key === key);
    return { locked: !!l?.locked, hidden: !!l && !l.visible };
  }

  /** Runs and the objects a run of `system` starts at: fixtures, devices, aircon units. */
  private pipeSnapScene(system: PipeSystem, exclude: readonly string[] = []): PipeSnapScene {
    const pipes: PipeEl[] = [];
    const fixtures: FixturePoint[] = [];
    if (this.index) {
      const skip = new Set(exclude);
      const catalog = this.catalogMap();
      for (const el of this.index.visible) {
        if (skip.has(el.id)) continue;
        if (el.kind === "pipe") pipes.push(el);
        else if (el.kind === "asset" && isServiceFixture(el, system, deviceKindOf(el, catalog))) fixtures.push(...fixturePoints(el));
      }
    }
    return { pipes, fixtures };
  }

  /** Pipe snaps first (pipe points, fixtures, tees), then the plan snaps (grid, walls, angles). */
  private snapPipeAt(world: P, anchor: P | null, o: { system: PipeSystem; penZ: number; exclude?: string[]; extra?: P[] }): SnapResult {
    const s = useApp.getState();
    if (s.snapEnabled && this.index) {
      const ortho = (s.orthoEnabled || this.shift) && !!anchor;
      const hit = snapToPipes(world, this.pipeSnapScene(o.system, o.exclude), { tol: SNAP_PX / this.view.scale, system: o.system, penZ: o.penZ, anchor, ortho });
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
      return { element: el, faceSnap: null, mount: null };
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
      return { element: el, faceSnap: null, mount: null };
    }
    const item = this.catalogItem();
    if (!item) return null;
    return this.assetGhost(world, item, level);
  }

  /**
   * The object the asset tool would place, by how it mounts
   * (`CatalogItem::mount`): wall objects with their back on the nearest wall
   * face at the catalog height (switches also at the latch side of the
   * nearest door), ceiling objects hanging from the level height, a window
   * aircon in a window, the rest on the floor against a face when near one.
   */
  private assetGhost(world: P, item: CatalogItem, level: string): PlacementGhost {
    const s = useApp.getState();
    const index = this.index;
    const el: AssetEl = {
      kind: "asset",
      id: "",
      level_id: level,
      catalog_key: item.key,
      name: item.name,
      category: item.category,
      position: world,
      rotation_deg: normDeg(this.turns * 90),
      width_mm: item.width_mm,
      depth_mm: item.depth_mm,
      height_mm: item.height_mm,
      elevation_mm: item.elevation_mm,
      light: item.light ?? null,
      links: [],
      circuit: "",
    };
    const kind: Mount = item.mount ?? "floor";
    const m: MountGhost = { kind, valid: true, reason: null, heightLabel: null, face: null, guide: null, window: null };
    const step = this.gridStep();
    const grid = (p: P): P => (step > 0 ? { x: Math.round(p.x / step) * step, y: Math.round(p.y / step) * step } : p);
    let faceSnap: FaceSnap | null = null;
    switch (kind) {
      case "wall": {
        // Generous: a wall object never goes anywhere but a wall face.
        const reach = Math.max(80 / this.view.scale, item.depth_mm / 2 + 900);
        const w = snapToWallFace(world, this.mountFaces(), item.width_mm, item.depth_mm, reach, step);
        if (!w) {
          el.position = grid(world);
          m.valid = false;
          m.reason = "Move onto a wall face";
        } else {
          el.position = w.position;
          el.rotation_deg = w.rotationDeg;
          m.face = { a: w.face.a, b: w.face.b };
          if (!w.valid) {
            m.valid = false;
            m.reason = w.reason === "short" ? "The wall is too short here" : "A door or window is in the way";
          }
        }
        if (this.isSwitchItem(item)) this.latchSnap(world, el, m, w);
        break;
      }
      case "ceiling": {
        const levels = index?.doc.project.levels ?? [];
        const lv = levels.find((l) => l.id === level);
        el.elevation_mm = ceilingElevation(ceilingHeightMm(lv, levels), item);
        const rooms = index ? index.doc.derived.rooms.filter((r) => index.visibleIds.has(r.room_id)) : [];
        const center = s.snapEnabled ? roomCenterSnap(world, rooms, Math.max((SNAP_PX * 2) / this.view.scale, 150)) : null;
        el.position = center ?? grid(world);
        if (center) this.snapResult = { point: center, type: "midpoint", guides: [], angleLocked: false, label: "Room center" };
        break;
      }
      case "opening": {
        const host = nearestWindow(world, this.windowHosts(), Math.max(40 / this.view.scale, 300));
        if (!host || !index) {
          el.position = grid(world);
          m.valid = false;
          m.reason = "Move onto a window";
          break;
        }
        // The back (the condenser) goes outside: away from the room, else away from the cursor.
        const exterior = index.wallGeo.get(host.wall.id)?.exterior ?? false;
        const roomSide = exterior ? roomSideOfWall(host.wall, index.doc.derived.rooms) : null;
        const cursorSide: 1 | -1 = cross(unit(sub(host.wall.end, host.wall.start)), sub(world, host.wall.start)) >= 0 ? 1 : -1;
        const outside: 1 | -1 = roomSide !== null ? (roomSide === 1 ? -1 : 1) : cursorSide === 1 ? -1 : 1;
        const size = { width: item.width_mm, depth: item.depth_mm, height: item.height_mm, elevation: item.elevation_mm };
        const w = mountInWindow(world, host, size, outside, (SNAP_PX * 2) / this.view.scale);
        el.position = w.position;
        el.rotation_deg = w.rotationDeg;
        el.elevation_mm = w.elevation;
        m.window = w.span;
        if (!w.fits) {
          m.valid = false;
          m.reason = "The unit does not fit this window";
        } else if (w.centered) {
          this.snapResult = { point: w.position, type: "midpoint", guides: [], angleLocked: false, label: "Centered" };
        }
        break;
      }
      default: {
        if (s.snapEnabled) {
          faceSnap = snapToFace(world, this.scene().faces, item.width_mm, item.depth_mm, this.turns, 28 / this.view.scale, step);
          if (faceSnap) {
            el.position = faceSnap.position;
            el.rotation_deg = faceSnap.rotationDeg;
          } else el.position = grid(world);
        }
      }
    }
    m.heightLabel = mountHeightLabel(kind, el, index?.doc.project.settings.display_unit ?? "mm");
    return { element: el, faceSnap, mount: m };
  }

  /**
   * Switches: the latch-side guide of the nearest door, on the cursor's side
   * of its wall. Near it, the switch snaps there: 200 mm from the latch jamb.
   */
  private latchSnap(world: P, el: AssetEl, m: MountGhost, onWall: WallMount | null): void {
    const found = this.nearestDoor(world);
    if (!found) return;
    const faces = this.mountFaces();
    const guides = usableGuides(latchGuides(found.door, found.wall), faces, el.width_mm);
    if (guides.length === 0) return;
    const dir = unit(sub(found.wall.end, found.wall.start));
    const side: 1 | -1 = cross(dir, sub(world, found.wall.start)) >= 0 ? 1 : -1;
    const onSide = guides.filter((g) => g.side === side);
    const near = nearestGuide(world, onSide.length > 0 ? onSide : guides);
    if (!near) return;
    // Near means: the switch slid along the guide's face to within reach of
    // it, or the cursor itself is that close to the guide point.
    const reach = Math.max((SNAP_PX * 2) / this.view.scale, 150);
    const sameFace = !!onWall && onWall.face.wallId === near.guide.wallId && onWall.face.side === near.guide.side;
    const along = sameFace && onWall ? Math.abs(dot(sub(onWall.position, near.guide.point), near.guide.away)) : Infinity;
    const snapped = useApp.getState().snapEnabled && (along <= reach || near.d <= reach);
    if (snapped) {
      const at = mountAtGuide(near.guide, el.depth_mm);
      el.position = at.position;
      el.rotation_deg = at.rotationDeg;
      const face = faces.find((f) => f.wallId === near.guide.wallId && f.side === near.guide.side);
      if (face) m.face = { a: face.a, b: face.b };
      m.valid = true;
      m.reason = null;
      this.snapResult = { point: near.guide.point, type: "face", guides: [], angleLocked: false, label: "Latch side" };
    }
    m.guide = { ...near.guide, snapped };
  }

  /** The door nearest a plan point, within reach, with its host wall. */
  private nearestDoor(world: P): { door: OpeningEl; wall: WallEl } | null {
    const index = this.index;
    if (!index) return null;
    let best: { door: OpeningEl; wall: WallEl } | null = null;
    let bestD = Math.max(2500, 80 / this.view.scale);
    for (const el of index.visible) {
      if (el.kind !== "opening" || el.opening_type !== "door") continue;
      const wall = index.byId.get(el.wall_id);
      if (!wall || wall.kind !== "wall" || !index.visibleIds.has(wall.id)) continue;
      const c = add(wall.start, mul(unit(sub(wall.end, wall.start)), el.offset_mm));
      const d = dist(world, c);
      if (d < bestD) {
        bestD = d;
        best = { door: el, wall };
      }
    }
    return best;
  }

  /** Faces of the visible walls, with their openings as gaps. */
  private mountFaces(): WallFace[] {
    const index = this.index;
    if (!index) return [];
    if (this.faceCache?.index === index) return this.faceCache.faces;
    const openings = new Map<string, OpeningEl[]>();
    for (const e of index.byId.values()) {
      if (e.kind !== "opening") continue;
      const list = openings.get(e.wall_id);
      if (list) list.push(e);
      else openings.set(e.wall_id, [e]);
    }
    const faces: WallFace[] = [];
    for (const el of index.visible) if (el.kind === "wall") faces.push(...facesOfWall(el, wallOutline(el, index), openings.get(el.id) ?? []));
    this.faceCache = { index, faces };
    return faces;
  }

  /** Visible windows with their walls, for the window aircon. */
  private windowHosts(): (WindowHost & { wall: WallEl })[] {
    const index = this.index;
    if (!index) return [];
    const out: (WindowHost & { wall: WallEl })[] = [];
    for (const el of index.visible) {
      if (el.kind !== "opening" || el.opening_type !== "window") continue;
      const wall = index.byId.get(el.wall_id);
      if (wall && wall.kind === "wall") out.push({ opening: el, wall });
    }
    return out;
  }

  private placeElement(): void {
    const g = this.placementGhost;
    if (!g || this.placing) return;
    if (g.mount && !g.mount.valid) {
      // Nothing is placed: the reason near the cursor bumps once.
      this.anim.clear(K.placeRefused);
      this.animate(K.placeRefused, 0, dur("base"), { from: 1, easing: ease.out, drop: true });
      return;
    }
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
      light: null,
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
    this.linkNotice = null;
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
    if ((_to === "asset" || _to === "link") && s.catalog.length === 0) void s.loadCatalog();
    this.linkNotice = null;
    if (_to === "link" && !this.isBusy()) this.startLinkTool();
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
      if (op.kind === "link") {
        // Escape or Enter ends linking. Tool letters and Delete still reach the app.
        if (e.key === "Escape" || e.key === "Enter") {
          this.resetOp();
          s.select([]);
          swallow();
        }
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

function sameIds(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

