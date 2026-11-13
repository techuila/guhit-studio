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
  PipeEnd,
  PipeSystem,
  PlanView,
  Room,
  Stair,
  Vec3,
  Wall,
} from "../contract/bindings";
import { ipc } from "../contract/ipc";
import { PIPE_LAYER, fixtureConnectionZ, pipeStartHeightMm } from "../contract/pipes";
import { bus } from "../state/bus";
import { useApp, type AppState, type Tool } from "../state/store";
import { dur, ease, motionOK } from "../ui/motion";
import { lengthToInput, parseLength, parseNumber } from "../ui/units";
import { useLive } from "../live/liveStore";
import { isModalOpen } from "../ui/Dialog";
import { useViewer } from "../viewer3d/viewerStore";
import { Anim, breathe, mix, mixP } from "./anim";
import type { Grip } from "./edit";
import { gripsFor, hitGrip, jointInset, normalDelta, rotationFromGrip } from "./edit";
import type { P, Rect, Seg } from "./geom";
import { add, angleDeg, cross, dirDeg, dist, dot, mul, normDeg, projectParam, rectFromPoints, rectIsEmpty, sub, unit } from "./geom";
import { hitTest, hitsElement, marqueeSelect, roomFloorAt } from "./hit";
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
import type { FaceSnap, OpeningFit, OpeningPlacement, OpeningSpan } from "./place";
import { doorSwingSide, findHostWall, fitOnWall, openingFitNote, openingMinWidth, placeOnWall, roomSideOfWall, snapToFace } from "./place";
import type { FixturePoint, PipeEl, PipeSnapScene, PipeSpec } from "./pipe";
import {
  PIPE_HEIGHT_STEP,
  PIPE_HEIGHT_STEP_FINE,
  PIPE_SYSTEM_LABEL,
  addRunPoint,
  cleanRun,
  finishRun,
  fixturePoints,
  isServiceFixture,
  pipeBandHalfPx,
  pipeNodes,
  pipeSpec,
  planOf,
  popRunPoint,
  segmentFallPct,
  snapToPipes,
  toolFallPct,
  withPendingRiser,
} from "./pipe";
import type { ElementStyle, Palette, RenderContext } from "./render";
import { DEFAULT_PALETTE, drawElement, drawFlash, drawGrid, drawHighlight, drawModel, drawOrigin, drawPeerOutline, labelHeightMm, readPalette } from "./render";
import type { PipePick } from "./pipePick";
import { usePipePick } from "./pipePick";
import {
  CONTINUE_HANDLE_PX,
  continuedPoints,
  endDirection,
  followers,
  heightSetCommand,
  heightStepCommand,
  lengthCommand,
  moveCommand,
  movedPoints,
  nodeAt,
  nodeOf,
  normalOnly,
  openEnds,
  planIndices,
  pointHeightCommand,
  segmentAt,
  segmentLength,
  segmentMid,
  slopeCommand,
  typedDragDelta,
  validPick,
} from "./pipeEdit";
import { isTypingTarget, planKeyBlocked, selectionKey } from "./keys";
import { decideResize } from "./resizePolicy";
import type { SnapResult, SnapScene } from "./snap";
import { computeIntersections, emptyScene, snap } from "./snap";
import type { RoomOutline } from "./slab";
import { SLAB_KIND_LABEL, canCloseSlab, cleanOutline, insertVertex, moveVertex, newSlab, offersRoom, popSlabPoint, removeVertex, roomOutlineAt, sameOutline, slabClick } from "./slab";
import { assetSymbolAnchor } from "./symbols";
import { useUnderlay } from "./underlay";
import type { Box } from "./tags";
import type { TypedState } from "./typed";
import { emptyTyped, formatArea, formatLength, isTypedKey, typedIsEmpty, typedKey, typedValues } from "./typed";
import type { View } from "./view";
import { clampScale, fitRect, panBy, snapStep, toScreen, toWorld, zoomAt } from "./view";

export const SNAP_PX = 10;
const DRAG_PX = 4;
const GRIP_PX = 9;
/** Radius of a link's flip handle, CSS pixels. */
export const HANDLE_PX = 5;

/** Layer names as the layer panel and the engine's messages give them (`layer_name`). */
const LAYER_LABEL: Record<LayerKey, string> = {
  slabs: "Slabs",
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
  site: "Site",
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
  /** The door or window ghost's width easing to a narrower fit and back. */
  ghostWidth: "ghost.w",
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
  /** The picked segment or point of the selected run fading in, and the one before it fading out (DECISIONS D39). */
  pipePick: "pipe.pick",
  pipePickOut: "pipe.pickx",
  /** `ptag:<id>`: a click to type pipe tag under the pointer. */
  tagHover: "ptag:",
  /** Runs that follow a pipe drag: their ghost fades in on pick up. */
  pipeFollow: "pipe.follow",
  /** The room outline the slab tool would take fading in, and a refused outline fading out. */
  slabRoom: "slab.room",
  slabRefused: "slab.no",
  /** The picked corner of the selected slab filling in. */
  slabVertex: "slab.v",
  slabVertexOut: "slab.vx",
  /** How long the slab tool's refusal pill stays before it fades. */
  slabNoticeHold: "slab.notice",
} as const;

/** How long the slab tool's refusal pill stays, with or without motion. */
export const SLAB_NOTICE_MS = 1440;

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
  | {
      kind: "press";
      downScreen: P;
      downWorld: P;
      hitId: string | null;
      additive: boolean;
      alt: boolean;
      pipe?: PipePress;
      /** The room whose empty floor was pressed: a click selects it, a drag still draws a marquee. */
      floorId?: string | null;
    }
  | { kind: "marquee"; start: P; current: P; additive: boolean }
  | { kind: "move"; ids: string[]; ref: P; delta: P; duplicate: boolean; committing: boolean }
  | { kind: "slide"; opening: OpeningEl; host: Wall; placement: OpeningPlacement; committing: boolean }
  | { kind: "grip"; grip: Grip; element: Element; current: P; z: number | null; committing: boolean }
  | { kind: "wall"; points: P[]; typed: TypedState | null; committing: boolean }
  /** Slab tool: the outline so far. `closed` is the outline on its way to the engine. */
  | { kind: "slab"; points: P[]; typed: TypedState | null; committing: boolean; closed: P[] | null }
  /** `extend`: continuing that run from one of its ends (its "+" handle) instead of drawing a new one. */
  | { kind: "pipe"; points: Vec3[]; penZ: number; typed: TypedState | null; committing: boolean; extend?: PipeExtend }
  | { kind: "rect"; origin: P; downScreen: P; typed: TypedState | null; committing: boolean }
  | { kind: "dimension"; a: P; b: P | null; committing: boolean }
  | { kind: "camera"; position: P; committing: boolean }
  /** Link tool with a device picked: each click on a load toggles its link. */
  | { kind: "link"; sourceId: string; committing: boolean }
  /**
   * A point or segment of the selected run being dragged (DECISIONS D39).
   * `delta` is the move so far, `ref` the plan point it is measured from,
   * `grab` the pointer's offset from `ref` at pick up.
   */
  | {
      kind: "pipe_drag";
      pipe: PipeEl;
      pick: PipePick;
      indices: number[];
      ref: P;
      grab: P;
      delta: Vec3;
      typed: TypedState | null;
      keepJoins: boolean;
      /** The segment's middle handle: it moves sideways only, its neighbours stretch. */
      sideways: boolean;
      committing: boolean;
    };

/**
 * Continuing a run from one end with its "+" handle: the run's own system,
 * material and size draw on, and the tool options stay the user's.
 */
interface PipeExtend {
  pipeId: string;
  end: PipeEnd;
  spec: PipeSpec;
}

/** A press on the selected run: what a click there picks, and whether a drag moves that part (else the run). */
interface PipePress {
  pick: PipePick | null;
  drag: boolean;
  /** Pressed on the picked segment's middle handle. */
  sideways?: boolean;
}

/** A pipe tag that opens a typed value on click: a point height, a fall or the picked segment's length. */
export interface PipeTagTarget {
  pipeId: string;
  field: "height" | "fall" | "length";
  /** Point index (height) or segment index (fall, length). */
  index: number;
  /** Height only: move that one point, not its whole node (one end of a riser). */
  only: boolean;
}

export interface InlineEditor {
  mode: "room" | "annotation" | "new_text" | "pipe";
  id: string | null;
  world: P;
  value: string;
  fontPx: number;
  /** mode "pipe": the value a pipe tag edits. */
  pipe?: PipeTagTarget;
  /** Why the typed value was not taken, and a count that shakes the box once per refusal. */
  error?: string | null;
  errorSeq?: number;
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
  /** True when the tool's width did not fit and the ghost is narrower. */
  narrowed: boolean;
  /** Said beside the cursor: the width it narrowed to, or why it cannot go here and what would. */
  note: string | null;
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
  /** Who the height typed after `h` is for: the pipe tool, or the selected run (DECISIONS D39). */
  private heightFor: "tool" | "selection" = "tool";
  /** A height key edit of the selected run in flight, and the steps pressed meanwhile (sent as one). */
  private pipeKeyBusy = false;
  private pipeKeyPending = 0;
  /** The click to type pipe tag under the pointer. */
  hoverTag: string | null = null;
  /** The pick before the current one, drawn while its highlight fades out. */
  lastPick: PipePick | null = null;
  /** Alt is held: a pipe drag breaks its joins. */
  alt = false;
  /** A press on a click to type pipe tag, opened on release. */
  private tagPress: { id: string; box: Box; screen: P } | null = null;
  /** Index into `gripList()` of the grip under the pointer. */
  hoverGrip: number | null = null;
  openingGhost: OpeningGhost | null = null;
  placementGhost: PlacementGhost | null = null;
  /** Slab tool before its first point: the room whose outline a click takes. */
  slabRoom: RoomOutline | null = null;
  lastSlabRoom: RoomOutline | null = null;
  /** An outline the engine refused, fading out in the danger color. */
  slabRefused: P[] | null = null;
  /** The corner of the selected slab picked with a click: Delete removes it. */
  slabVertexPick: { slabId: string; index: number } | null = null;
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
  /** The door or window ghost's last note, to bump it once when it changes. */
  private ghostNote: string | null = null;
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
      this.alt = false;
    }, false);

    this.cleanups.push(bus.on("zoom_to_fit", () => this.zoomToFit()));
    this.cleanups.push(bus.on("focus_elements", (ids) => this.focusElements(ids)));
    this.cleanups.push(bus.on("focus_point", ({ point, level_id }) => this.focusPoint(point, level_id)));
    this.cleanups.push(
      usePipePick.subscribe((st, pv) => {
        if (st.pick !== pv.pick) this.onPickChange(pv.pick, st.pick);
      }),
    );

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
    this.cleanups.push(
      useUnderlay.subscribe((st, pv) => {
        if (st.on === pv.on) return;
        this.refreshToolGhost();
        this.invalidate();
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
    window.clearTimeout(this.noticeTimer);
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
    if ((op.kind !== "wall" && op.kind !== "rect" && op.kind !== "pipe" && op.kind !== "pipe_drag" && op.kind !== "slab") || !op.typed || !this.cursorScreen) return null;
    const labels: [string, string] =
      op.kind === "rect" ? [`Width ${unitName}`, `Depth ${unitName}`] : op.kind === "pipe_drag" ? [`Distance ${unitName}`, "Angle"] : [`Length ${unitName}`, "Angle"];
    return { x: this.cursorScreen.x, y: this.cursorScreen.y, state: op.typed, labels };
  }

  private cssCursor(): string {
    if (this.op.kind === "pan") return "grabbing";
    const tool = useApp.getState().tool;
    if (this.space || tool === "pan") return "grab";
    if (tool === "select") {
      if (this.op.kind === "move" || this.op.kind === "grip" || this.op.kind === "slide" || this.op.kind === "pipe_drag") return "grabbing";
      if (this.hoverTag) return "text";
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
      case "slab":
        return this.slabHint();
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
      case "select":
        return this.pipeHint();
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

  private slabHint(): string {
    const op = this.op;
    const kind = SLAB_KIND_LABEL[useApp.getState().toolOptions.slabKind].toLowerCase();
    const layer = this.slabLayer();
    if (layer.locked) return "The Slabs layer is locked. Unlock it to draw slabs";
    if (op.kind === "slab") {
      return op.points.length >= 3
        ? "Click the next corner, or click the first point, double click or press Enter to close. Backspace takes the last point back"
        : "Click the next corner, or type a length and press Enter";
    }
    const room = this.slabRoom;
    if (room) return `Click to make this ${room.below ? "room below" : "room"} a ${kind}, or hold Alt to draw one inside it`;
    return layer.hidden ? `Click the corners of the ${kind}. The Slabs layer is hidden, new slabs show when it is on` : `Click the corners of the ${kind}, or click inside a room to take its outline`;
  }

  /** The Slabs layer: locked refuses new slabs, hidden hides them. */
  slabLayer(): { locked: boolean; hidden: boolean } {
    const l = this.index?.doc.project.layers.find((x) => x.key === "slabs");
    return { locked: !!l?.locked, hidden: !!l && !l.visible };
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
    const before = this.index;
    this.index = visible ? buildIndex(visible, s.activeLevelId) : null;
    this.realIndex = s.preview && s.doc ? buildIndex(s.doc, s.activeLevelId) : null;
    this.sceneCache = null;
    const docId = visible?.project.id ?? null;
    useUnderlay.getState().sync();
    if (docId !== this.lastDocId) {
      this.lastDocId = docId;
      this.slabVertexPick = null;
      this.slabRefused = null;
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
    this.syncPick(before);
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
    bus.emit("user_view", { view: "plan" });
    const at = this.cursorScreen && this.pointerInside ? this.cursorScreen : { x: this.width / 2, y: this.height / 2 };
    this.setView(zoomAt(this.view, at, inOut > 0 ? 1.25 : 1 / 1.25), dur("press"));
  }

  /**
   * Moves the view. `this.view` is always the final transform, so hit testing
   * and the scripted checks never wait: only what is drawn eases there.
   */
  private setView(next: View, ms: number, easing: (t: number) => number = ease.inOut): void {
    if (ms > 0 && motionOK() && this.width > 0) {
      // Clock first: the ease starts from where the view is now, not from the
      // last drawn frame, so steady updates (follow mode) never lag behind.
      this.anim.setClock(performance.now());
      this.viewFrom = this.drawView();
      this.anim.clear(K.view);
      this.anim.to(K.view, 1, ms, { from: 0, easing, drop: true });
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

  /**
   * Follow mode (DECISIONS D37): shows the plan area someone else sees,
   * centered and scaled to fit this canvas. `linear` for the steady updates
   * while following, so one glide runs into the next; the first move eases.
   * `ms` 0 jumps. Never emits `user_view`: it is not this user's input.
   */
  followView(area: PlanView, ms: number, linear = false): void {
    if (this.width === 0 || !Number.isFinite(area.center.x) || !Number.isFinite(area.center.y)) return;
    this.autoFit = false;
    const scale = clampScale(Math.min(this.width / Math.max(area.width_mm, 1), this.height / Math.max(area.height_mm, 1)));
    const next = { scale, ox: this.width / 2 - area.center.x * scale, oy: this.height / 2 + area.center.y * scale };
    this.setView(next, ms, linear ? (t) => t : ease.inOut);
  }

  /** The plan area this canvas shows (the view it is easing to), for presence while someone follows. */
  planView(): PlanView | null {
    if (this.width === 0 || this.height === 0) return null;
    const c = toWorld(this.view, { x: this.width / 2, y: this.height / 2 });
    return { center: { x: c.x, y: c.y }, width_mm: this.width / this.view.scale, height_mm: this.height / this.view.scale };
  }

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
      underlay: useUnderlay.getState().on,
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
      if (this.slabVertexPick && !next.selection.includes(this.slabVertexPick.slabId)) this.setSlabVertexPick(null);
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
      // Narrowed to fit: the ghost eases to the new width and its note bumps once.
      if (fresh) this.anim.set(K.ghostWidth, og.opening.width_mm);
      else this.animate(K.ghostWidth, og.opening.width_mm, dur("hover"), { easing: ease.out });
      if (og.note !== this.ghostNote) {
        if (og.note && !fresh) {
          this.anim.clear(K.placeRefused);
          this.animate(K.placeRefused, 0, dur("base"), { from: 1, easing: ease.out, drop: true });
        }
        this.ghostNote = og.note;
      }
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
    if (this.slabRoom) {
      // Moving to another room swaps the outline at full strength: only the first one fades in.
      this.lastSlabRoom = this.slabRoom;
      this.animate(K.slabRoom, 1, dur("hover"), { from: 0 });
    } else {
      this.animate(K.slabRoom, 0, exitDur("hover"), { drop: true });
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
    if (op.kind !== "move" && op.kind !== "slide" && op.kind !== "grip" && op.kind !== "pipe_drag") return;
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
      case "pipe_drag":
        return { ...op, delta: { x: op.delta.x * v, y: op.delta.y * v, z: op.delta.z * v } };
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
    const grips = gripsFor(el, 1 / this.view.scale);
    if (el.kind === "pipe") grips.push(...this.pipeExtraGrips(el));
    return grips;
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
    // Pressing the plan stops following someone (DECISIONS D37).
    bus.emit("user_view", { view: "plan" });
    const screen = this.eventPoint(e);
    const world = toWorld(this.view, screen);
    this.cursorScreen = screen;
    this.cursorWorld = world;
    this.shift = e.shiftKey;
    this.alt = e.altKey;
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
      case "slab":
        this.slabToolClick(presses);
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
    this.alt = e.altKey;
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
        bus.emit("user_view", { view: "plan" });
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
      case "pipe_drag":
        if (!op.committing) this.updatePipeDrag(op, world);
        break;
      default:
        this.updateToolHover(world);
    }
    if (this.op.kind === "idle" && s.tool === "select") {
      // Handles win over the tags that overlap them, as a press does.
      const grips = this.gripList();
      const grip = grips.length > 0 && !this.hoverHandle ? hitGrip(grips, world, GRIP_PX / this.view.scale) : null;
      this.setHoverGrip(grip ? grips.indexOf(grip) : null);
      const tag = this.hoverHandle || grip || !this.pointerInside ? null : this.pipeTagAt(screen);
      this.setHoverTag(tag?.id ?? null);
      const id = this.hoverHandle || tag ? null : hitTest(world, this.index, this.hitOptions());
      if (id !== s.hoverId) s.setHover(id);
    } else {
      this.setHoverGrip(null);
      this.setHoverTag(null);
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
    this.slabRoom = null;
    switch (tool) {
      case "slab":
        this.updateSlabHover(world);
        break;
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
    return k === "pan" || k === "press" || k === "marquee" || k === "move" || k === "slide" || k === "grip" || k === "pipe_drag";
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
    const tagPress = this.tagPress;
    this.tagPress = null;
    if (tagPress && op.kind === "idle" && dist(this.eventPoint(e), tagPress.screen) <= DRAG_PX) this.openPipeTag(tagPress.id, tagPress.box, tagPress.screen);
    switch (op.kind) {
      case "pan":
        this.op = this.panReturn;
        this.panReturn = { kind: "idle" };
        break;
      case "press":
        // A click on the selected run picks the part under it; the selection stays.
        if (op.pipe) {
          if (op.pipe.pick) usePipePick.getState().setPick(op.pipe.pick);
        } else if (op.hitId) s.select([op.hitId], op.additive);
        else if (op.floorId) s.select([op.floorId], op.additive);
        else if (!op.additive) s.select([]);
        this.op = { kind: "idle" };
        break;
      case "pipe_drag":
        this.commitPipeDrag(op);
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
    this.slabRoom = null;
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
    bus.emit("user_view", { view: "plan" });
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
    // The selected run (DECISIONS D39): its handles first (an open end's "+",
    // the picked segment's middle, the points), then a click to type tag,
    // then the run itself. Tags sit close enough to overlap the handles.
    const cont = e.shiftKey ? null : this.continueAt(world);
    if (cont) {
      this.startContinue(cont.pipe, cont.end);
      return;
    }
    const handlePress = e.shiftKey ? null : this.pipeHandlePressAt(world);
    const tag = e.shiftKey || handlePress ? null : this.pipeTagAt(screen);
    if (tag) {
      // It opens on release: the browser's focus handling for this press would blur it right away.
      this.tagPress = { id: tag.id, box: tag.box, screen };
      return;
    }
    const pipePress = handlePress ?? (e.shiftKey ? null : this.pipeBodyPressAt(world));
    if (pipePress) {
      const pipe = this.selectedPipe();
      this.op = { kind: "press", downScreen: screen, downWorld: world, hitId: pipe?.id ?? null, additive: false, alt: e.altKey, pipe: pipePress };
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
    // Any press that is not on a grip lets a picked slab corner go, so
    // Delete afterwards removes what is selected, never a corner from before.
    this.setSlabVertexPick(null);
    let hitId = hitTest(world, this.index, this.hitOptions());
    // Nothing but the floor (or a traced image under it): a click selects the room.
    const under = hitId ? this.index.byId.get(hitId) : null;
    const floorId = !hitId || under?.kind === "underlay" ? roomFloorAt(world, this.index) : null;
    if (floorId) hitId = null;
    this.op = { kind: "press", downScreen: screen, downWorld: world, hitId, additive: e.shiftKey, alt: e.altKey, floorId };
  }

  private beginDrag(op: Extract<Op, { kind: "press" }>, alt: boolean): void {
    const s = useApp.getState();
    const index = this.index;
    if (!index) return;
    if (!op.hitId) {
      this.op = { kind: "marquee", start: op.downWorld, current: this.cursorWorld ?? op.downWorld, additive: op.additive };
      return;
    }
    if (op.pipe?.drag && op.pipe.pick && this.beginPipeDrag(op, op.pipe.pick, !!op.pipe.sideways)) return;
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

  /** `placementFor` that narrows the width to fit, down to `min` (place.ts `fitOnWall`). */
  private fitFor(world: P, host: Wall, width: number, min: number): OpeningFit {
    const index = this.index;
    const insetS = index ? jointInset(index, host, "start") : 0;
    const insetE = index ? jointInset(index, host, "end") : 0;
    const others: OpeningSpan[] = [];
    if (index) {
      for (const e of index.byId.values()) {
        if (e.kind === "opening" && e.wall_id === host.id) others.push({ offset: e.offset_mm, width: e.width_mm });
      }
    }
    return fitOnWall(world, host, width, min, insetS, insetE, this.gridStep(), SNAP_PX / this.view.scale, useApp.getState().snapEnabled, others);
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
      case "slab_vertex":
      case "slab_mid": {
        if (el.kind !== "slab") return;
        const i = op.grip.index ?? 0;
        const n = el.polygon.length;
        const moving = op.grip.kind === "slab_vertex";
        // The other corners snap, and the edge before keeps ortho and angle locks.
        const others = el.polygon.filter((_, k) => !(moving && k === i));
        const anchor = moving ? el.polygon[(i - 1 + n) % n] : el.polygon[i];
        const r = this.snapAt(world, anchor, { exclude: [el.id], extra: others, useFaces: true });
        this.snapResult = r;
        op.current = r.point;
        break;
      }
      // Pipe point handles drag as a `pipe_drag` (pipe editing below), never as a grip.
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
      case "slab_vertex":
        return el.kind === "slab" ? { ...el, polygon: moveVertex(el.polygon, op.grip.index ?? 0, op.current) } : null;
      case "slab_mid":
        return el.kind === "slab" ? { ...el, polygon: insertVertex(el.polygon, op.grip.index ?? 0, op.current) } : null;
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
      // A click on a slab corner picks it: Delete then removes it.
      if (el.kind === "slab" && op.grip.kind === "slab_vertex") this.setSlabVertexPick({ slabId: el.id, index: op.grip.index ?? 0 });
      this.op = { kind: "idle" };
      return;
    }
    // Corners shift when one is added or moved: the pick goes.
    if (el.kind === "slab") this.setSlabVertexPick(null);
    op.committing = true;
    void this.run(command);
  }

  /** Picks a corner of the selected slab, or clears the pick. The fill grows in and fades out. */
  setSlabVertexPick(pick: { slabId: string; index: number } | null): void {
    const cur = this.slabVertexPick;
    if (cur === pick || (cur && pick && cur.slabId === pick.slabId && cur.index === pick.index)) return;
    if (cur) {
      this.lastSlabVertexPick = cur;
      this.anim.clear(K.slabVertexOut);
      this.animate(K.slabVertexOut, 0, exitDur("hover"), { from: 1, easing: ease.in, drop: true });
    }
    this.slabVertexPick = pick;
    if (pick) {
      this.anim.clear(K.slabVertex);
      this.animate(K.slabVertex, 0, dur("hover"), { from: 1, easing: ease.out, drop: true });
    }
    this.invalidate();
  }

  /** The corner picked before the current one, drawn while its fill fades out. */
  lastSlabVertexPick: { slabId: string; index: number } | null = null;

  /** Delete on a picked corner: it goes when more than three remain. */
  private removePickedVertex(): void {
    const pick = this.slabVertexPick;
    const el = pick ? this.index?.byId.get(pick.slabId) : null;
    if (!pick || !el || el.kind !== "slab") {
      this.setSlabVertexPick(null);
      return;
    }
    const polygon = removeVertex(el.polygon, pick.index);
    if (!polygon) {
      useApp.getState().toast("info", "A slab needs at least three corners");
      return;
    }
    this.setSlabVertexPick(null);
    void this.run({ type: "update_element", element: { ...el, polygon } });
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
    const { wallThicknessMm, wallRole } = useApp.getState().toolOptions;
    void this.run({
      type: "add_wall_chain",
      points: op.points.map((q) => ({ x: q.x, y: q.y })),
      closed,
      thickness_mm: wallThicknessMm,
      level_id: this.levelId(),
      // A fence or boundary wall: on the site layer, 1800 mm, never a room.
      ...(wallRole === "site" ? { role: "site" as const } : {}),
    });
  }

  // ------------------------------------------------------------ pipe tool

  /** System, material, size and start height the pipe tool draws with. */
  pipeSpec(): PipeSpec {
    if (this.op.kind === "pipe" && this.op.extend) return this.op.extend.spec;
    const o = useApp.getState().toolOptions;
    const spec = pipeSpec(o);
    if (o.pipeElevationMm !== null) return spec;
    // No height chosen: the level's own start height, so conduit starts in the
    // slab above or above the top level's ceiling (DECISIONS D40).
    const levels = this.index?.doc.project.levels ?? [];
    const level = levels.find((l) => l.id === this.levelId());
    return { ...spec, startHeightMm: pipeStartHeightMm(spec.system, level, levels) };
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
        else if (el.kind === "asset" && isServiceFixture(el, system, deviceKindOf(el, catalog))) {
          // A run ends at the fixture's connection height with a riser (DECISIONS D40).
          const z = fixtureConnectionZ(el, system);
          for (const f of fixturePoints(el)) fixtures.push({ ...f, z });
        }
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

  /** Plan point and joined height of the next click: the typed length, else the snapped cursor. `fixture`: the height is a fixture connection. */
  private pipeTarget(op: Extract<Op, { kind: "pipe" }>): { point: P; z: number | null; fixture?: boolean } | null {
    if (op.typed) {
      const p = this.typedPipePoint(op);
      if (p) return { point: p, z: null };
    }
    const r = this.snapResult;
    if (r) return { point: r.point, z: r.z ?? null, fixture: r.type === "fixture" };
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
    const next = addRunPoint(op, target.point, { fallPct: this.pipeFallPct(op), snapZ: target.z, fixture: target.fixture });
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
      const draft = addRunPoint({ points: [], penZ: spec.startHeightMm }, r.point, { fallPct: null, snapZ: r.z ?? null, fixture: r.type === "fixture" });
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

  private pushPipePoint(op: Extract<Op, { kind: "pipe" }>, target: { point: P; z: number | null; fixture?: boolean }): void {
    const before = op.points.length;
    const next = addRunPoint(op, target.point, { fallPct: this.pipeFallPct(op), snapZ: target.z, fixture: target.fixture });
    op.typed = null;
    op.points = next.points;
    op.penZ = next.penZ;
    if (op.points.length > before) this.pulsePipePoint(target.point, this.pipeSpec().system);
  }

  private finishPipe(op: Extract<Op, { kind: "pipe" }>): void {
    if (op.committing) return;
    if (op.extend) {
      this.finishContinue(op, op.extend);
      return;
    }
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

  /** A continued run: the drawn points join the run at its end, as one step, then back to Select with it selected. */
  private finishContinue(op: Extract<Op, { kind: "pipe" }>, extend: PipeExtend): void {
    const drawn = finishRun(op);
    const run = this.index?.byId.get(extend.pipeId);
    if (!drawn || !run || run.kind !== "pipe") {
      this.resetOp();
      useApp.getState().setTool("select");
      return;
    }
    const points = cleanRun(continuedPoints(run.points, extend.end, drawn));
    op.committing = true;
    op.typed = null;
    this.heightEntry = null;
    void this.run({ type: "update_element", element: { ...run, points } }, () => {
      const s = useApp.getState();
      s.setTool("select");
      s.select([run.id]);
    });
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
    // Continuing a run keeps the user's own start height option.
    if (!(this.op.kind === "pipe" && this.op.extend)) useApp.getState().setTool("pipe", { pipeElevationMm: v });
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
      if (v !== null) {
        if (this.heightFor === "selection") this.setSelectionHeight(v);
        else this.setPipeHeight(v);
      }
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
      if (!back) {
        // Backing out of continuing a run returns to Select with it.
        const cont = op.extend;
        this.resetOp();
        if (cont) useApp.getState().setTool("select");
      } else {
        op.points = back.points;
        op.penZ = back.penZ;
      }
    } else if (e.key === "PageUp" || e.key === "PageDown") {
      const step = e.shiftKey ? PIPE_HEIGHT_STEP_FINE : PIPE_HEIGHT_STEP;
      this.setPipeHeight(op.penZ + (e.key === "PageUp" ? step : -step));
    } else if (k === "h" && !e.altKey) {
      op.typed = null;
      this.startHeightEntry("tool");
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

  // ------------------------------------------------------------ pipe editing (DECISIONS D39)

  /** The one selected run, when the select tool can edit it: visible and not on a locked layer. */
  selectedPipe(): PipeEl | null {
    const s = useApp.getState();
    if (s.tool !== "select" || s.selection.length !== 1 || !this.index) return null;
    const el = this.index.byId.get(s.selection[0]);
    if (!el || el.kind !== "pipe" || !this.index.visibleIds.has(el.id) || isLocked(el, this.index)) return null;
    return el;
  }

  /** The picked segment or point of the selected run, or null for the whole run. */
  pipePick(): PipePick | null {
    const pipe = this.selectedPipe();
    const pick = usePipePick.getState().pick;
    return pipe && pick && pick.pipeId === pipe.id ? validPick(pipe.points, pick) : null;
  }

  /**
   * After a document change (an undo, a remote edit). A drag of a run that
   * changed under it is cancelled: its copy is stale. A pick is dropped when
   * its run is gone or gained or lost points, since its index would name
   * another part; otherwise it is kept where it still fits.
   */
  private syncPick(before: DocIndex | null): void {
    const op = this.op;
    if (op.kind === "pipe_drag" && !op.committing) {
      const now = this.index?.byId.get(op.pipe.id);
      if (!now || now.kind !== "pipe" || !samePoints(now.points, op.pipe.points)) {
        this.op = { kind: "idle" };
        this.snapResult = null;
        this.setLift(false);
      }
    }
    const st = usePipePick.getState();
    const pick = st.pick;
    if (!pick) return;
    const el = this.index?.byId.get(pick.pipeId);
    const old = before?.byId.get(pick.pipeId);
    if (!el || el.kind !== "pipe" || (old?.kind === "pipe" && old.points.length !== el.points.length)) {
      st.setPick(null);
      return;
    }
    const next = validPick(el.points, pick);
    if (!next) st.setPick(null);
    else if (next.index !== pick.index) st.setPick(next);
  }

  /** The picked part's highlight fades in; the one it replaces fades out. */
  private onPickChange(prev: PipePick | null, next: PipePick | null): void {
    if (prev) {
      this.lastPick = prev;
      this.anim.clear(K.pipePickOut);
      this.animate(K.pipePickOut, 0, exitDur("hover"), { from: 1, easing: ease.in, drop: true });
    }
    this.anim.clear(K.pipePick);
    if (next) this.animate(K.pipePick, 0, dur("hover"), { from: 1, easing: ease.out, drop: true });
    this.invalidate();
    this.updateUi();
  }

  /** Pick radius of a run's centerline in mm: half its drawn band plus the usual pick tolerance. */
  private pipeTol(pipe: PipeEl): number {
    return pipeBandHalfPx(pipe, this.view.scale) / this.view.scale + this.hitOptions().tol * 0.5;
  }

  private openEndCache: { index: DocIndex; pipe: PipeEl; ends: PipeEnd[] } | null = null;

  /** The ends of the run that join nothing, cached per document and run. */
  private openEndsOf(pipe: PipeEl): PipeEnd[] {
    const index = this.index;
    if (!index) return [];
    const c = this.openEndCache;
    if (c && c.index === index && c.pipe === pipe) return c.ends;
    const catalog = this.catalogMap();
    const others: PipeEl[] = [];
    const fixtures: P[] = [];
    for (const el of index.visible) {
      if (el.kind === "pipe" && el.id !== pipe.id) others.push(el);
      else if (el.kind === "asset" && isServiceFixture(el, pipe.system, deviceKindOf(el, catalog))) fixtures.push(...fixturePoints(el).map((f) => f.point));
    }
    const ends = openEnds(pipe, others, fixtures);
    this.openEndCache = { index, pipe, ends };
    return ends;
  }

  /**
   * Handles the selected run has beyond its point handles: the picked
   * segment's middle (drag it sideways) and a "+" past each open end
   * (continue the run from there).
   */
  private pipeExtraGrips(pipe: PipeEl): Grip[] {
    const out: Grip[] = [];
    const pick = this.pipePick();
    if (pick?.kind === "segment") {
      const m = segmentMid(pipe.points, pick.index);
      if (m) out.push({ elementId: pipe.id, kind: "pipe_mid", pos: m, index: pick.index });
    }
    const n = pipe.points.length;
    for (const end of this.openEndsOf(pipe)) {
      const p = planOf(end === "start" ? pipe.points[0] : pipe.points[n - 1]);
      const pos = add(p, mul(endDirection(pipe.points, end), CONTINUE_HANDLE_PX / this.view.scale));
      out.push({ elementId: pipe.id, kind: "pipe_continue", pos, index: end === "start" ? 0 : 1 });
    }
    return out;
  }

  /** The "+" handle of the selected run under a plan point. */
  private continueAt(world: P): { pipe: PipeEl; end: PipeEnd } | null {
    const pipe = this.selectedPipe();
    if (!pipe || this.pipeLayerLocked(pipe)) return null;
    const g = hitGrip(this.pipeExtraGrips(pipe).filter((x) => x.kind === "pipe_continue"), world, GRIP_PX / this.view.scale);
    return g ? { pipe, end: g.index === 0 ? "start" : "end" } : null;
  }

  private pipeLayerLocked(pipe: PipeEl): boolean {
    return !!this.index?.doc.project.layers.find((l) => l.key === PIPE_LAYER[pipe.system])?.locked;
  }

  /**
   * The "+" of an open end: the pipe tool takes over with the run's system,
   * size and that end's height, drawing on from the end. Finishing writes
   * the new points into the same run (one step) and comes back to Select.
   */
  private startContinue(pipe: PipeEl, end: PipeEnd): void {
    const p = end === "start" ? pipe.points[0] : pipe.points[pipe.points.length - 1];
    // The tool options are left alone: the run's spec rides on the operation.
    useApp.getState().setTool("pipe");
    const spec: PipeSpec = { system: pipe.system, material: pipe.material, diameterMm: pipe.diameter_mm, startHeightMm: p.z };
    this.op = { kind: "pipe", points: [{ x: p.x, y: p.y, z: p.z }], penZ: p.z, typed: null, committing: false, extend: { pipeId: pipe.id, end, spec } };
    this.pulsePipePoint(planOf(p), pipe.system);
    this.refreshToolGhost();
    this.updateUi();
    this.invalidate();
  }

  /** Fall of the next segment while drawing: the tool's, turned into a rise when continuing a run back from its start. */
  private pipeFallPct(op: Extract<Op, { kind: "pipe" }>): number | null {
    const f = toolFallPct(this.pipeSpec());
    return f !== null && op.extend?.end === "start" ? -f : f;
  }

  /**
   * A press on the selected run: a point handle picks that point and drags
   * it; the run itself picks the segment under the cursor, and drags that
   * segment once a part is picked (with no part picked a drag moves the run).
   */
  private pipeHandlePressAt(world: P): PipePress | null {
    const pipe = this.selectedPipe();
    if (!pipe || !this.index) return null;
    const handleTol = GRIP_PX / this.view.scale;
    const mid = hitGrip(this.pipeExtraGrips(pipe).filter((g) => g.kind === "pipe_mid"), world, handleTol);
    if (mid && mid.index !== undefined) return { pick: { pipeId: pipe.id, kind: "segment", index: mid.index }, drag: true, sideways: true };
    const node = nodeAt(pipe.points, world, handleTol);
    return node === null ? null : { pick: { pipeId: pipe.id, kind: "point", index: node }, drag: true };
  }

  private pipeBodyPressAt(world: P): PipePress | null {
    const pipe = this.selectedPipe();
    if (!pipe || !this.index) return null;
    const seg = segmentAt(pipe.points, world, this.pipeTol(pipe));
    const pick: PipePick | null = seg === null ? null : { pipeId: pipe.id, kind: "segment", index: seg };
    if (!pick && !hitsElement(world, pipe, this.index, this.hitOptions())) return null;
    return { pick, drag: !!pick && this.pipePick() !== null };
  }

  private beginPipeDrag(op: Extract<Op, { kind: "press" }>, pick: PipePick, sideways = false): boolean {
    const pipe = this.selectedPipe();
    if (!pipe) return false;
    const pts = pipe.points;
    let ref: P;
    if (pick.kind === "point") {
      const node = pipeNodes(pts).find((n) => n.index === nodeOf(pts, pick.index).index);
      if (!node) return false;
      ref = node.point;
    } else {
      const a = planOf(pts[pick.index]);
      const b = planOf(pts[pick.index + 1]);
      const t = sideways ? 0.5 : Math.max(0, Math.min(1, projectParam(op.downWorld, a, b)));
      ref = { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
    }
    usePipePick.getState().setPick(pick);
    this.op = {
      kind: "pipe_drag",
      pipe,
      pick,
      indices: planIndices(pts, pick),
      ref,
      grab: sub(op.downWorld, ref),
      delta: { x: 0, y: 0, z: 0 },
      typed: null,
      keepJoins: !(op.alt || this.alt),
      sideways: sideways && pick.kind === "segment",
      committing: false,
    };
    this.setHoverGrip(null);
    this.setLift(true);
    this.anim.clear(K.pipeFollow);
    this.animate(K.pipeFollow, 0, dur("hover"), { from: 1, easing: ease.out, drop: true });
    if (this.cursorWorld) this.updatePipeDrag(this.op, this.cursorWorld);
    return true;
  }

  /**
   * Tracks the pointer 1:1. A point snaps like the pipe tool (and joins a
   * pipe it lands on at that pipe's height); a segment moves in grid steps,
   * ortho with Shift. A typed distance replaces the length along the drag.
   */
  private updatePipeDrag(op: Extract<Op, { kind: "pipe_drag" }>, world: P): void {
    const s = useApp.getState();
    const pts = op.pipe.points;
    op.keepJoins = !this.alt;
    const target = sub(world, op.grab);
    let d: P;
    let dz = 0;
    if (op.pick.kind === "point") {
      const nodes = pipeNodes(pts);
      const node = nodeOf(pts, op.pick.index);
      const i = nodes.findIndex((n) => n.index === node.index);
      const anchor = nodes[i - 1]?.point ?? nodes[i + 1]?.point ?? null;
      const r = this.snapPipeAt(target, anchor, { system: op.pipe.system, penZ: nodes[i]?.zOut ?? 0, exclude: [op.pipe.id] });
      this.snapResult = r;
      d = sub(r.point, op.ref);
      // A single point joining another run takes its height; a fixture keeps the drag flat.
      if (node.count === 1 && r.type !== "fixture" && r.z !== undefined && r.z !== null) dz = r.z - pts[node.index].z;
    } else if (op.sideways) {
      // Along the segment's normal only, in grid steps.
      const a = pts[op.pick.index];
      const b = pts[op.pick.index + 1];
      d = normalOnly(sub(target, op.ref), a, b);
      const step = this.gridStep();
      const len = Math.hypot(d.x, d.y);
      if (step > 0 && len > 1e-9) d = mul(d, (Math.round(len / step) * step) / len);
    } else {
      d = sub(target, op.ref);
      if (s.orthoEnabled || this.shift) d = Math.abs(d.x) >= Math.abs(d.y) ? { x: d.x, y: 0 } : { x: 0, y: d.y };
      const step = this.gridStep();
      if (step > 0) d = { x: Math.round(d.x / step) * step, y: Math.round(d.y / step) * step };
    }
    if (op.typed && this.index) {
      // Along the pointer's own direction, not the snapped one (sideways: along the normal).
      let raw = sub(target, op.ref);
      if (op.sideways) {
        const a = pts[op.pick.index];
        const b = pts[op.pick.index + 1];
        raw = normalOnly(raw, a, b);
        // Nothing moved yet: the left of the segment.
        if (Math.hypot(raw.x, raw.y) < 1e-6) raw = normalOnly({ x: -(b.y - a.y), y: b.x - a.x }, a, b);
      }
      const typed = typedDragDelta(raw, op.typed, this.index.doc.project.settings.display_unit, !op.sideways);
      if (typed) {
        d = typed;
        dz = 0;
        this.snapResult = null;
      }
    }
    op.delta = { x: d.x, y: d.y, z: dz };
  }

  /** The dragged run as it will be, and the runs that follow it (with keep_joins). */
  pipeDragPreview(op: Extract<Op, { kind: "pipe_drag" }>): { pipe: PipeEl; followers: PipeEl[] } {
    const pipe = { ...op.pipe, points: movedPoints(op.pipe.points, op.indices, op.delta) };
    if (!op.keepJoins || !this.index) return { pipe, followers: [] };
    // Every run in the project, with absolute heights: joins reach across levels as the engine's do.
    const project = this.index.doc.project;
    const others = project.elements.filter((e): e is PipeEl => e.kind === "pipe" && e.id !== op.pipe.id);
    const floors = new Map(project.levels.map((l) => [l.id, l.elevation_mm] as const));
    const floorOf = (id: string | undefined): number => (id ? (floors.get(id) ?? 0) : 0);
    return { pipe, followers: followers(op.pipe, op.indices, op.delta, others, floorOf) as PipeEl[] };
  }

  private commitPipeDrag(op: Extract<Op, { kind: "pipe_drag" }>): void {
    if (op.committing) return;
    const d = op.delta;
    if (Math.hypot(d.x, d.y, d.z) < 0.5) {
      this.op = { kind: "idle" };
      return;
    }
    op.committing = true;
    op.typed = null;
    void this.run(moveCommand(op.pipe.id, op.indices, d, op.keepJoins));
  }

  /** Keys while dragging a pipe part: a typed distance, Enter applies it, Escape clears it or cancels. */
  private pipeDragKey(e: KeyboardEvent, op: Extract<Op, { kind: "pipe_drag" }>): void {
    if (e.key === "Escape") {
      if (op.typed) op.typed = null;
      else {
        this.easeBack(op);
        this.op = { kind: "idle" };
        this.snapResult = null;
        this.setLift(false);
      }
    } else if (isTypedKey(e.key) || (op.typed && (e.key === "Tab" || e.key === "Backspace"))) {
      op.typed = typedKey(op.typed ?? emptyTyped("polar"), e.key);
    } else if (e.key === "Enter" && !typedIsEmpty(op.typed)) {
      if (this.cursorWorld) this.updatePipeDrag(op, this.cursorWorld);
      this.commitPipeDrag(op);
    } else if (e.key === "Alt") {
      this.alt = true;
    }
    this.handleMove(this.alt);
    this.updateUi();
  }

  /** PageUp and PageDown on the selected run: the picked part moves up or down. Steps pressed during a send go out as one. */
  private pipeHeightStep(dz: number): void {
    if (this.pipeKeyBusy) {
      this.pipeKeyPending += dz;
      return;
    }
    const pipe = this.selectedPipe();
    const command = pipe ? heightStepCommand(pipe, this.pipePick(), dz) : null;
    if (command) void this.sendPipeKeyEdit(command);
  }

  /** `h` then a number on the selected run: the picked part moves so its height reads the value. */
  private setSelectionHeight(z: number): void {
    const pipe = this.selectedPipe();
    const command = pipe ? heightSetCommand(pipe, this.pipePick(), z) : null;
    if (command) void this.sendPipeKeyEdit(command);
  }

  private async sendPipeKeyEdit(command: Command): Promise<void> {
    this.pipeKeyBusy = true;
    this.pulsePipeHeight();
    try {
      await useApp.getState().dispatch(command);
    } catch {
      // dispatch already reports errors.
    } finally {
      this.pipeKeyBusy = false;
      const more = this.pipeKeyPending;
      this.pipeKeyPending = 0;
      if (Math.abs(more) > 1e-6) this.pipeHeightStep(more);
      this.invalidate();
    }
  }

  private startHeightEntry(target: "tool" | "selection"): void {
    this.heightFor = target;
    this.heightEntry = emptyTyped("height");
    this.updateUi();
  }

  /** The click to type tag of the selected run under a screen point, from the last drawn layout. */
  pipeTagAt(screen: P): { id: string; box: Box } | null {
    const pipe = this.selectedPipe();
    const layout = this.tagLayout;
    if (!pipe || !layout || !this.pointerInside) return null;
    const prefix = `${pipe.id}:`;
    for (let i = layout.placed.length - 1; i >= 0; i--) {
      const t = layout.placed[i];
      if (!t.id.startsWith(prefix)) continue;
      const b = t.box;
      if (screen.x >= b.x - 1 && screen.x <= b.x + b.w + 1 && screen.y >= b.y - 1 && screen.y <= b.y + b.h + 1) return t;
    }
    return null;
  }

  private setHoverTag(id: string | null): void {
    if (id === this.hoverTag) return;
    if (this.hoverTag) this.animate(`${K.tagHover}${this.hoverTag}`, 0, exitDur("hover"), { drop: true });
    if (id) this.animate(`${K.tagHover}${id}`, 1, dur("hover"), { from: 0 });
    this.hoverTag = id;
    this.updateUi();
  }

  /**
   * Opens the typed value of a tag of the selected run, where the tag is:
   * a point height (one end of a riser for its half of the tag), a fall or
   * the picked segment's length. The part it belongs to becomes the pick.
   */
  private openPipeTag(tagId: string, box: Box, screen: P): void {
    const pipe = this.selectedPipe();
    if (!pipe || !this.index) return;
    const [kind, n] = tagId.slice(pipe.id.length + 1).split(":");
    const index = Number(n);
    if (!Number.isInteger(index)) return;
    const unitName = this.index.doc.project.settings.display_unit;
    const pts = pipe.points;
    let target: PipeTagTarget;
    let value: string;
    if (kind === "h") {
      const node = nodeOf(pts, index);
      const zs = pts.slice(node.index, node.index + node.count).map((v) => v.z);
      const riser = node.count > 1 && Math.max(...zs) - Math.min(...zs) >= 1;
      const i = riser && screen.x > box.x + box.w / 2 ? node.index + node.count - 1 : node.index;
      target = { pipeId: pipe.id, field: "height", index: i, only: riser };
      value = lengthToInput(pts[i].z, unitName);
      usePipePick.getState().setPick({ pipeId: pipe.id, kind: "point", index: node.index });
    } else if (kind === "f" || kind === "l") {
      if (index < 0 || index + 1 >= pts.length) return;
      target = { pipeId: pipe.id, field: kind === "f" ? "fall" : "length", index, only: false };
      const pct = segmentFallPct(pts[index], pts[index + 1]) ?? 0;
      value = kind === "f" ? String(Math.round(pct * 100) / 100) : lengthToInput(segmentLength(pts, index), unitName);
      usePipePick.getState().setPick({ pipeId: pipe.id, kind: "segment", index });
    } else return;
    const center = { x: box.x + box.w / 2, y: box.y + box.h / 2 };
    this.setHoverTag(null);
    this.openEditor({ mode: "pipe", id: tagId, world: toWorld(this.view, center), value, fontPx: 11, pipe: target });
  }

  /**
   * The command a typed tag value sends, joins kept; a fall and a length hold
   * the end the inspector holds. A value out of the engine's range is still
   * sent, so its refusal says why. `error` when the text is not a value at all.
   */
  private pipeTagCommand(target: PipeTagTarget, text: string): { command: Command | null; error: string | null } {
    const el = this.index?.byId.get(target.pipeId);
    if (!el || el.kind !== "pipe" || !this.index) return { command: null, error: null };
    const unitName = this.index.doc.project.settings.display_unit;
    const hold: PipeEnd = usePipePick.getState().hold;
    if (target.field === "height") {
      const z = parseLength(text.replace(/^\+/, ""), unitName);
      return z === null ? { command: null, error: `Type a height in ${unitName}, such as ${unitName === "m" ? "1.2" : "1200"}` } : { command: pointHeightCommand(el, target.index, z, target.only), error: null };
    }
    if (target.field === "fall") {
      const pct = parseNumber(text.replace(/fall|%/gi, "").trim());
      return pct === null ? { command: null, error: "Type a fall in percent, such as 2" } : { command: slopeCommand(el, target.index, target.index, pct, hold), error: null };
    }
    const len = parseLength(text, unitName);
    return len === null ? { command: null, error: `Type a length in ${unitName}, such as ${unitName === "m" ? "1.5" : "1500"}` } : { command: lengthCommand(el, target.index, len, hold), error: null };
  }

  /** What the select tool can do with the selected run right now. */
  private pipeHint(): string | null {
    const op = this.op;
    if (op.kind === "pipe_drag") return op.keepJoins ? "Type a distance and press Enter. Joined runs follow, hold Alt to break the joins" : "Joins break on drop. Release Alt to keep them";
    if (op.kind !== "idle" || !this.selectedPipe()) return null;
    const pick = this.pipePick();
    if (!pick) return "Click a segment or a point to edit it. PageUp or PageDown moves the run, or type h and a height";
    const part = pick.kind === "point" ? "point" : "segment";
    return `Drag the ${part}, or PageUp, PageDown or h for its height. Click a tag to type a value. Escape picks the whole run`;
  }

  // ------------------------------------------------------------ slab tool

  private slabSnapCache: { index: DocIndex; underlay: boolean; points: P[] } | null = null;

  /** Extra snap points for slab outlines: the corners of this level's slabs and of the level below's walls. */
  private slabSnapPoints(): P[] {
    const index = this.index;
    if (!index) return [];
    const underlay = useUnderlay.getState().on;
    const cache = this.slabSnapCache;
    if (cache && cache.index === index && cache.underlay === underlay) return cache.points;
    const points: P[] = [];
    for (const e of index.visible) if (e.kind === "slab") points.push(...e.polygon);
    if (underlay) for (const o of index.below) points.push(...o);
    this.slabSnapCache = { index, underlay, points };
    return points;
  }

  private updateSlabHover(world: P): void {
    const op = this.op;
    const extra = this.slabSnapPoints();
    if (op.kind === "slab") {
      if (op.committing) return;
      const last = op.points[op.points.length - 1];
      // Closing the outline wins over every other snap.
      if (op.points.length >= 3 && dist(world, op.points[0]) <= SNAP_PX / this.view.scale && useApp.getState().snapEnabled) {
        this.snapResult = { point: op.points[0], type: "endpoint", guides: [], angleLocked: false, label: "Close" };
      } else {
        this.snapResult = this.snapAt(world, last, { extra: [...op.points.slice(0, -1), ...extra], useFaces: true });
      }
      return;
    }
    const r = this.snapAt(world, null, { extra, useFaces: true });
    // Away from every corner, face and line, a click inside a room takes its
    // outline; Alt held skips the offer so a free outline can start inside.
    let room = this.index && offersRoom(r.type, this.alt, this.slabLayer().locked) ? roomOutlineAt(this.index.doc, this.levelId(), world, useUnderlay.getState().on) : null;
    // A room that already has a slab of its own outline here is not offered again.
    if (room && this.index?.visible.some((e) => e.kind === "slab" && sameOutline(e.polygon, room!.polygon))) room = null;
    this.slabRoom = room;
    this.snapResult = room ? null : r;
  }

  private slabToolClick(detail: number): void {
    const world = this.cursorWorld;
    if (!world || !this.index || !this.levelId()) return;
    if (this.slabLayer().locked) {
      this.slabNotice = "The Slabs layer is locked";
      this.bumpRefusal();
      return;
    }
    const op = this.op;
    if (op.kind !== "slab") {
      // Alt pressed since the last move still means a free outline.
      const room = this.alt ? null : this.slabRoom;
      if (room) {
        void this.commitSlab(room.polygon, null);
        return;
      }
      const p = this.snapResult?.point ?? this.snapAt(world, null, { extra: this.slabSnapPoints(), useFaces: true }).point;
      this.op = { kind: "slab", points: [p], typed: null, committing: false, closed: null };
      this.syncGhostAnim();
      return;
    }
    if (op.committing) return;
    if (detail >= 2) {
      // Second click of a double click: the first one placed the last corner.
      this.closeSlab(op);
      return;
    }
    const last = op.points[op.points.length - 1];
    const snapped = this.snapResult?.point ?? world;
    const p = op.typed ? (this.typedEnd(last, op.typed) ?? snapped) : snapped;
    this.pushSlabPoint(op, p);
  }

  /** End of the edge being typed: exact length along the cursor or the typed angle. */
  typedSlabPoint(op: Extract<Op, { kind: "slab" }>): P | null {
    if (!op.typed || op.points.length === 0) return null;
    return this.typedEnd(op.points[op.points.length - 1], op.typed);
  }

  private pushSlabPoint(op: Extract<Op, { kind: "slab" }>, p: P): void {
    op.typed = null;
    const r = slabClick(op.points, p, Math.max(1, SNAP_PX / this.view.scale));
    if (r.kind === "add") op.points = r.points;
    else if (r.kind === "close") void this.commitSlab(r.points, op.points);
  }

  /** Enter, a double click or a right click: close the outline, or say why it cannot. */
  private closeSlab(op: Extract<Op, { kind: "slab" }>): void {
    if (op.committing) return;
    if (canCloseSlab(op.points)) {
      void this.commitSlab(cleanOutline(op.points), op.points);
      return;
    }
    this.slabNotice = "A slab needs three corners around some area";
    this.bumpRefusal();
  }

  /** Why the last slab click did nothing, shown in a pill that bumps once. */
  slabNotice: string | null = null;

  private bumpRefusal(): void {
    this.anim.clear(K.placeRefused);
    this.animate(K.placeRefused, 0, dur("base"), { from: 1, easing: ease.out, drop: true });
    this.anim.clear(K.slabNoticeHold);
    window.clearTimeout(this.noticeTimer);
    if (motionOK()) {
      this.animate(K.slabNoticeHold, 0, dur("panel"), { from: 1, easing: ease.in, drop: true, delayMs: SLAB_NOTICE_MS });
      return;
    }
    // Reduced motion: no fade, but the pill still stays long enough to read.
    this.anim.set(K.slabNoticeHold, 1);
    this.invalidate();
    this.noticeTimer = window.setTimeout(() => {
      this.anim.clear(K.slabNoticeHold);
      this.invalidate();
    }, SLAB_NOTICE_MS);
  }

  private noticeTimer = 0;

  /**
   * Adds the outline as a slab of the tool's kind. The closed outline stays
   * as a ghost until the engine answers. A refusal (an overlap, a void off
   * the floor) is toasted by the store; here the outline flashes in the
   * danger color and fades, and an outline drawn corner by corner stays open
   * so it can be fixed.
   */
  private async commitSlab(polygon: P[], draft: P[] | null): Promise<void> {
    const level = this.levelId();
    if (!level) return;
    const o = useApp.getState().toolOptions;
    const element = newSlab(level, o.slabKind, o.slabRailing, polygon);
    this.op = { kind: "slab", points: draft ?? [], typed: null, committing: true, closed: element.polygon };
    this.slabRoom = null;
    this.syncGhostAnim();
    this.invalidate();
    let ok = false;
    try {
      ok = !!(await useApp.getState().dispatch({ type: "add_element", element }));
    } catch {
      // dispatch already reports errors.
    }
    if (!ok) {
      this.slabRefused = element.polygon;
      this.anim.clear(K.slabRefused);
      this.animate(K.slabRefused, 0, dur("panel"), { from: 1, easing: ease.in, drop: true, delayMs: dur("scene") });
      const toasts = useApp.getState().toasts;
      const last = toasts.length > 0 ? toasts[toasts.length - 1] : null;
      this.slabNotice = last?.kind === "error" ? `Not added: ${last.message}` : "Not added";
      this.bumpRefusal();
    }
    const keep = !ok && !!draft && draft.length > 0 && useApp.getState().tool === "slab";
    this.op = keep && draft ? { kind: "slab", points: draft, typed: null, committing: false, closed: null } : { kind: "idle" };
    this.snapResult = null;
    this.refreshToolGhost();
    this.invalidate();
  }

  private slabOpKey(e: KeyboardEvent, op: Extract<Op, { kind: "slab" }>): void {
    if (e.key === "Escape") {
      // Escape first clears a typed value, then cancels the outline.
      if (op.typed) op.typed = null;
      else this.resetOp();
      return;
    }
    if (isTypedKey(e.key) || (op.typed && (e.key === "Tab" || e.key === "Backspace"))) {
      op.typed = typedKey(op.typed ?? emptyTyped("polar"), e.key);
      return;
    }
    if (e.key === "Backspace") {
      const next = popSlabPoint(op.points);
      if (next) op.points = next;
      else this.resetOp();
      this.handleMove();
      return;
    }
    if (e.key === "Enter") {
      if (!typedIsEmpty(op.typed)) {
        const p = this.typedSlabPoint(op);
        if (p) this.pushSlabPoint(op, p);
        else op.typed = null;
        this.handleMove();
      } else this.closeSlab(op);
    }
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
    // The tool keeps its style (CAD habit), but a width that does not fit the
    // wall under the pointer narrows to the widest that does.
    const fit = this.fitFor(world, host, d.width, openingMinWidth(type, d.style));
    const placement = fit.placement;
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
      width_mm: fit.width,
      height_mm: d.height,
      sill_mm: d.sill,
      flip_side: cursorFlip !== this.flipSide,
      flip_hinge: this.flipHinge,
      material_id: null,
    };
    const unit = index.doc.project.settings.display_unit;
    const note = openingFitNote(type, d.style, fit, (mm) => formatLength(mm, unit, true));
    return { opening, host, placement, narrowed: fit.narrowed, note };
  }

  private placeOpening(): void {
    const g = this.openingGhost;
    if (!g) return;
    if (!g.placement.valid) {
      // Nothing is placed: the reason near the cursor bumps once.
      this.anim.clear(K.placeRefused);
      this.animate(K.placeRefused, 0, dur("base"), { from: 1, easing: ease.out, drop: true });
      return;
    }
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
        run_mm: risers * 250, // n - 1 treads of run_mm / (n - 1) each (D44)
        riser_count: risers,
        // New stairs get a handrail (DECISIONS D50).
        railing: { height_mm: 900, style: "bars" },
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

  /**
   * Called by the inline input. `value` null cancels. False when it stays
   * open: a pipe value that is not a number, typed with Enter, keeps the box
   * with the reason and a shake; on blur it closes and the reason shows as a toast.
   */
  closeEditor(value: string | null, fromBlur = false): boolean {
    const ed = this.editor;
    if (!ed) return true;
    let pipeCommand: Command | null = null;
    if (ed.mode === "pipe" && ed.pipe && value !== null && value.trim() !== "" && value.trim() !== ed.value.trim()) {
      const r = this.pipeTagCommand(ed.pipe, value.trim());
      if (r.error && !fromBlur) {
        this.editor = { ...ed, error: r.error, errorSeq: (ed.errorSeq ?? 0) + 1 };
        this.updateUi();
        return false;
      }
      if (r.error) useApp.getState().toast("error", r.error);
      pipeCommand = r.command;
    }
    this.editor = null;
    this.invalidate();
    this.updateUi();
    const text = value?.trim() ?? "";
    if (value === null || text === "" || !this.index) return true;
    if (ed.mode === "pipe") {
      if (pipeCommand) {
        this.pulsePipeHeight();
        void this.run(pipeCommand);
      }
      return true;
    }
    if (ed.mode === "new_text") {
      const level = this.levelId();
      if (!level) return true;
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
      return true;
    }
    const el = ed.id ? this.index.byId.get(ed.id) : null;
    if (!el) return true;
    if (el.kind === "room" && text !== el.name) {
      const next: Room & { kind: "room" } = { ...el, name: text, auto_named: false };
      void this.run({ type: "update_element", element: next });
    } else if (el.kind === "annotation" && text !== el.text) {
      void this.run({ type: "update_element", element: { ...el, text } });
    }
    return true;
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
    this.slabRoom = null;
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
    else if (op.kind === "slab" && !op.committing) this.closeSlab(op);
    else if (!this.isBusy() && op.kind !== "idle") this.resetOp();
    this.invalidate();
  }

  /** True when a tool operation would consume Escape, Enter, Backspace or digits. */
  private inOperation(): boolean {
    const k = this.op.kind;
    return k !== "idle" && k !== "pan";
  }

  private onKeyDown = (e: KeyboardEvent): void => {
    const typing = isTypingTarget(e.target);
    if (e.key === "Shift") {
      this.shift = true;
      if (this.inOperation() || this.pointerInside) this.handleMove();
      return;
    }
    if (e.key === "Alt") this.alt = true;
    // Walking or flying in 3D: that view owns every key without MOD (docs/CONTRACT.md).
    if (useViewer.getState().nav !== "orbit") return;
    // A field, a dialog, or (Escape) following someone has the key first.
    if (planKeyBlocked(e.key, { defaultPrevented: e.defaultPrevented, typing, modal: isModalOpen(), following: !!useLive.getState().following })) return;
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

    // A height typed after `h` (the pipe tool, or the selected run) takes
    // digits, Enter and Escape first, so h1500 never reaches the 1, 2, 3 view shortcuts.
    if (this.heightEntry) {
      const live = this.heightFor === "selection" ? s.tool === "select" && !!this.selectedPipe() && op.kind === "idle" : s.tool === "pipe";
      if (!live) this.heightEntry = null;
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
      if (op.kind === "pipe_drag") {
        this.pipeDragKey(e, op);
        swallow();
        return;
      }
      if (op.kind === "slab") {
        this.slabOpKey(e, op);
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

    // Idle. A picked slab corner: Delete removes it, Escape lets it go.
    if (s.tool === "select" && this.slabVertexPick && !e.altKey) {
      if (e.key === "Delete" || e.key === "Backspace") {
        this.removePickedVertex();
        swallow();
        return;
      }
      if (e.key === "Escape") {
        this.setSlabVertexPick(null);
        swallow();
        return;
      }
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
        this.startHeightEntry("tool");
        swallow();
        return;
      }
    }
    // The selected run (DECISIONS D39): the same height keys as drawing move
    // the picked part, and Escape steps out of a picked segment or point.
    const selKey = s.tool === "select" && this.selectedPipe() ? selectionKey(e.key, { shift: e.shiftKey, alt: e.altKey }, { pointerInside: this.pointerInside, picked: !!this.pipePick() }) : null;
    if (selKey) {
      if (selKey.kind === "step") this.pipeHeightStep(selKey.dz);
      else if (selKey.kind === "height") this.startHeightEntry("selection");
      else usePipePick.getState().setPick(null);
      swallow();
      return;
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
    if (e.key === "Alt") {
      this.alt = false;
      if (this.op.kind === "pipe_drag") this.handleMove();
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

function samePoints(a: readonly Vec3[], b: readonly Vec3[]): boolean {
  return a.length === b.length && a.every((v, i) => v.x === b[i].x && v.y === b[i].y && v.z === b[i].z);
}

function sameIds(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

