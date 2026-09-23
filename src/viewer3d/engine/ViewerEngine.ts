// The three.js side of the viewer: renderer, scene, camera, controls,
// picking, highlights, motion and capture. No React in here. One instance per
// mounted Viewer3D; `dispose` releases every GPU resource it created.
//
// Motion (docs/MOTION.md, rows "3D view"): every animated value lives in
// `Animator` (engine/animator.ts), durations come from src/ui/motion.ts and
// the frame loop runs only while something is animating. The engine renders
// on demand; it never holds a permanent requestAnimationFrame.
//
// Navigation: `orbit` is OrbitControls. `walk` and `fly` hand the camera to the
// walker (engine/walker.ts), which steps inside the same single frame loop:
// a frame is scheduled while a key is held or the walker still moves, and
// none at all while it stands still. The shell modes (engine/shell.ts) fade
// the building around the pipes (scene/pipes.ts).

import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { LineSegmentsGeometry } from "three/examples/jsm/lines/LineSegmentsGeometry.js";
import type { Camera, DocState, Element, Vec3 } from "../../contract/bindings";
import { dur, ease, motionOK } from "../../ui/motion";
import {
  axonometric,
  exteriorCorner,
  eyeLevel,
  fitDistance,
  fitFromDirection,
  roomInterior,
  topView,
  type ModelBounds,
  type PosePreset,
} from "../geom/cameraMath";
import { buildCollisionWorld } from "../geom/collision";
import { cameraToPose, worldToVec3, type WorldPose } from "../geom/coords";
import { signedArea } from "../geom/polygon";
import { hiddenAt, pipeMostlyHidden, roomsOn, walkStartPose, walkToPose, type HiddenReason, type WalkPose } from "../geom/walkStart";
import { buildExportGroup, exportDAE, exportGLB, exportOBJ } from "../scene/exportScene";
import { BuildCache } from "../scene/buildCache";
import { buildScene, CUTAWAY_MM, levelFilter, type BuiltScene } from "../scene/buildScene";
import { MaterialLibrary } from "../scene/materials";
import { loadPackManifest, modelPack } from "../scene/pack";
import { PIPE_PICK_LAYER, pipeShown } from "../scene/pipes";
import { Minimap, type MinimapScene } from "../walk/minimap";
import type { NavMode, ShellMode } from "../viewerStore";
import { Animator } from "./animator";
import { Environment } from "./environment";
import { Highlight, spec, type HighlightState } from "./highlights";
import { MeshFade, meshesUnder } from "./meshFade";
import { ReferenceModelStore, type ReferenceModelPlacement } from "./referenceModels";
import { ShellView } from "./shell";
import { pointerLockSupported, WALK_FOV, WalkControls, WalkState, type WalkMode } from "./walker";

export type PresetKind = "eye_level" | "exterior_corner" | "top" | "axonometric" | "room_interior" | "fit";

export interface EngineCallbacks {
  onPick: (id: string | null, additive: boolean) => void;
  onHover: (id: string | null) => void;
  /** The user grabbed the camera. */
  onUserOrbit: () => void;
  onContextLost: (lost: boolean) => void;
  /** The first frame is on screen. The pane fades the canvas in on this. */
  onFirstRender?: () => void;
  /** Reads a stored reference model file (glTF/GLB/OBJ) as a data URL. */
  fetchReferenceModel: (fileName: string) => Promise<string>;
  /** A reference model was kept as a placeholder (missing file or over the triangle cap). */
  onReferenceModelWarning?: (message: string) => void;
  /**
   * The engine changed the navigation mode itself: Escape or F while walking,
   * or a camera request (a preset, fit, focus) that leaves walk mode.
   */
  onNavChange?: (nav: NavMode) => void;
  /** X while walking or flying: the global shortcut handler is quiet then. */
  onCycleShell?: () => void;
  /** True while the keyboard belongs to something else (a dialog). */
  keysBlocked?: () => boolean;
  /** Pointer lock came or went. */
  onPointerLock?: (locked: boolean) => void;
  /** The browser refused pointer lock (WKWebView may). */
  onPointerLockError?: () => void;
  /**
   * A `walk_to` put the walker in front of its finding. `hidden` says why a
   * solid shell would hide the finding (in a wall, under the floor), or null.
   */
  onWalkTo?: (hidden: HiddenReason | null) => void;
}

export interface Highlights {
  selection: string[];
  hoverId: string | null;
  /** Elements added or modified by an AI proposal. */
  previewIds: string[];
}

export interface DocOptions {
  cutaway: boolean;
  roofVisible: boolean;
  activeLevelId: string | null;
  /** Elements an in-flight AI proposal would remove, drawn as ghosts. */
  ghostsRemoved?: { project: DocState["project"]; ids: string[] } | null;
}

interface CameraTween {
  fromPos: THREE.Vector3;
  toPos: THREE.Vector3;
  fromTarget: THREE.Vector3;
  toTarget: THREE.Vector3;
  fromFov: number;
  toFov: number;
}

interface FadeJob {
  key: string;
  fade: MeshFade;
  /** Meters below the resting height at opacity 0. Negative lifts instead. */
  lift: number;
}

/** Breathing AI preview: opacity 0.55 to 0.85 over 1.6 s (docs/MOTION.md). */
const BREATH_LOW = 0.55;
const BREATH_HIGH = 0.85;
const BREATH_MS = 1600;
/** Resting opacity of a preview tint when it is not breathing. */
const PREVIEW_OPACITY = 0.78;
/** How far a new element rises into place, and a removed one sinks. */
const ENTER_LIFT_M = 0.06;
const EXIT_SINK_M = 0.08;
/** How far the roof lifts while it fades out. */
const ROOF_LIFT_M = 0.4;
/** First load: one bucket every this many ms, 5 buckets, total under 500 ms. */
const STAGGER_MS = 60;
/** Full resolution comes back this long after the last camera input. */
const INTERACTION_TAIL_MS = 150;
/** A frame costing more than this drops the interaction to one device pixel. */
const FRAME_BUDGET_MS = 8;
/** Damping is cut off after this long so the frame loop never hangs on it. */
const DAMPING_MAX_MS = 300;
/** Pack models that land inside this window share one rebuild. */
const PACK_COALESCE_MS = 32;
/**
 * Tracks that change nothing the sun sees: the camera, walking, the shell
 * fade, shadow strength, the sky blend and highlight tints. Their frames do
 * not redraw the shadow map; only meshes that move, fade or get cut do.
 */
const VIEW_ONLY_TRACKS = new Set(["camera", "walk", "shell", "shadow", "breath", "env"]);
const isViewOnlyTrack = (key: string) => VIEW_ONLY_TRACKS.has(key) || key.startsWith("hl:");
/** How far back and up the camera settles when a walk ends, mm, and how close it may come to anything. */
const EXIT_BACK_MM = 1200;
const EXIT_UP_MM = 400;
const CLEARANCE_M = 0.35;

const tmpPos = new THREE.Vector3();
const tmpQuat = new THREE.Quaternion();
const tmpEuler = new THREE.Euler(0, 0, 0, "YXZ");

let liveEngines = 0;
/** Number of engines that were created and not disposed. For the leak check. */
export function liveEngineCount(): number {
  return liveEngines;
}

function elementSignature(e: Element): string {
  return JSON.stringify(e);
}

export class ViewerEngine {
  readonly renderer: THREE.WebGLRenderer;
  readonly scene = new THREE.Scene();
  readonly camera = new THREE.PerspectiveCamera(45, 1, 0.1, 3000);
  readonly controls: OrbitControls;
  readonly anim = new Animator();
  private env: Environment;
  private lib = new MaterialLibrary();
  /** Built forms of elements that survive a rebuild untouched. */
  private cache = new BuildCache();
  private refs: ReferenceModelStore;
  private built: BuiltScene | null = null;
  private doc: DocState | null = null;
  private docKey = "";
  private buildKey = "";
  private framed = false;

  private raf = 0;
  private needsRender = true;
  private tween: CameraTween | null = null;
  private disposed = false;
  private contextLost = false;
  private firstRendered = false;
  private width = 0;
  private height = 0;
  private resizeObserver: ResizeObserver;

  private highlights: Highlights = { selection: [], hoverId: null, previewIds: [] };
  private live = new Map<string, Highlight>();
  private breathRunning = false;
  private edgeCache = new Map<string, LineSegmentsGeometry>();
  private fades: FadeJob[] = [];
  private fadeSeq = 0;

  /** Model bookkeeping for the enter/exit diff. */
  private signatures = new Map<string, string>();
  private opts: DocOptions = { cutaway: false, roofVisible: true, activeLevelId: null, ghostsRemoved: null };
  /** Meshes kept alive past their rebuild so a removal can animate out. */
  private leaving: { group: THREE.Group; kit: BuiltScene["kit"] } | null = null;

  /** What the user asked for, and what the geometry is currently built with. */
  private cutWanted = false;
  private cutBaked = false;
  private clip: THREE.Plane | null = null;
  private clipFrom = 0;
  private clipTo = 0;
  private clipped = new Set<THREE.Material>();
  private roofShown = true;
  private shadowsOn = true;

  /** Dev toggles for the CC0 pack: GLB furniture and PBR maps, and the HDRI. */
  private packAssets = true;
  private packOff: (() => void) | null = null;
  /** Catalog keys whose model arrived and have not been built in yet. */
  private packArrived = new Set<string>();
  private packTimer: ReturnType<typeof setTimeout> | 0 = 0;

  private raycaster = new THREE.Raycaster();
  private pointerDown: { x: number; y: number } | null = null;
  private hoverQueued: { x: number; y: number } | null = null;
  private lastHover: string | null = null;
  /** Flat list of everything a ray can hit. Rebuilt with the scene. */
  private pickables: THREE.Object3D[] = [];

  /** True between a controls "start" and the tail after its "end". */
  private interacting = false;
  private interactionTail = 0;
  /** Device pixels per CSS pixel while interacting. Drops to 1 when frames run long. */
  private interactionRatio = 0;
  private appliedRatio = 0;
  /** True between a controls "start" and its "end": the pointer is driving. */
  private pointerActive = false;
  /** Damping may keep the loop alive until this timestamp, no longer. */
  private dampingUntil = 0;
  /** Consecutive over-budget frames during the current interaction. */
  private overBudget = 0;
  private lastFrameAt = 0;

  /** How the camera moves. Walk and fly hand it to the walker. */
  private nav: NavMode = "orbit";
  /** Walk or fly asked for before there was a document to stand in. */
  private navPending: WalkMode | null = null;
  /** A `walk_to` that arrived before the document. */
  private walkToPending: { ids: string[]; location: Vec3 | null } | null = null;
  private walker = new WalkState();
  private walkControls: WalkControls;
  /** Level the walker is on. Follows the active level when that changes. */
  private walkLevelId: string | null = null;
  /** The active level last seen, so only a change of it moves the walker. */
  private seenActiveLevel: string | null = null;
  /** The document and level the collision world was built from. */
  private walkWorldDoc: DocState | null = null;
  private walkWorldLevel: string | null = null;
  /** The camera pose a walk started from: the entrance blends out of it. */
  private walkBlend: { pos: THREE.Vector3; quat: THREE.Quaternion; fov: number } | null = null;
  /** Time of the last moving walk frame, 0 when the walker stood still. */
  private lastWalkAt = 0;
  /** Orbit limits relaxed while a walk hands the camera back. */
  private orbitMaxPolar = Math.PI * 0.5 + 0.12;

  private shell = new ShellView();
  private minimap: Minimap | null = null;
  private minimapScene: MinimapScene | null = null;
  private minimapVersion = 0;
  private minimapDirty = false;

  constructor(
    private container: HTMLElement,
    private cb: EngineCallbacks,
  ) {
    liveEngines++;
    this.renderer = new THREE.WebGLRenderer({ antialias: true, powerPreference: "high-performance" });
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.renderer.toneMappingExposure = 0.95;
    this.renderer.shadowMap.enabled = true;
    this.renderer.shadowMap.type = THREE.PCFShadowMap;
    // The sun and the model stand still while the camera moves, so the shadow
    // map is redrawn only when something that casts or receives one actually
    // changes (`shadowDirty`). Orbiting never pays for a second scene pass.
    this.renderer.shadowMap.autoUpdate = false;
    this.renderer.shadowMap.needsUpdate = true;
    // The cutaway animates a clipping plane over the model only, so the sky
    // and ground keep their own materials and are never cut.
    this.renderer.localClippingEnabled = true;
    this.applyPixelRatio();
    this.lib.setAnisotropy(this.renderer.capabilities.getMaxAnisotropy());
    this.refs = new ReferenceModelStore(this.lib, {
      fetchModel: (name) => this.cb.fetchReferenceModel(name),
      onSwap: (id, mount) => this.enterReferenceModel(id, mount),
      invalidate: () => {
        this.shadowDirty();
        this.invalidate();
      },
      warn: (message) => this.cb.onReferenceModelWarning?.(message),
    });
    this.scene.add(this.refs.root);

    const canvas = this.renderer.domElement;
    canvas.style.cssText = "display:block;width:100%;height:100%;outline:none;touch-action:none";
    container.appendChild(canvas);

    this.env = new Environment(this.scene);
    this.env.bakeEnvironment(this.renderer);
    this.anim.set("shadow", 1);
    this.anim.set("breath", PREVIEW_OPACITY);
    // 1 is "the HDRI look, fully applied". It only takes effect once the HDRI
    // is actually on the scene; until then `applyBlend` holds the sky values.
    this.anim.set("env", 1);
    this.env.applyBlend(1, this.renderer);
    // The CC0 pack (scene/pack.ts). Every part of it is optional and async:
    // the sky above, a texture per preset, a GLB per catalog key. Nothing here
    // blocks the first frame.
    void loadPackManifest();
    this.env.loadHdri(this.renderer, { onHdriReady: () => this.onHdriReady() });
    this.lib.onPackTexture = () => {
      // A new map changes what the sun sees through alpha and what the walls
      // reflect: redraw the shadow map and ask for exactly one frame.
      this.shadowDirty();
      this.invalidate();
    };
    this.packOff = modelPack.onLoad((key) => this.onPackModel(key));

    this.controls = new OrbitControls(this.camera, canvas);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.12;
    this.controls.zoomToCursor = true;
    this.controls.minDistance = 0.3;
    this.controls.maxPolarAngle = this.orbitMaxPolar;
    // Pipe solos rest on their own layer: not drawn, still clickable.
    this.raycaster.layers.enable(PIPE_PICK_LAYER);
    this.walkControls = new WalkControls(canvas, {
      wake: () => this.invalidate(),
      look: (dx, dy) => {
        this.walker.look(dx, dy);
        this.applyWalkerCamera();
        this.minimapDirty = true;
        this.invalidate();
      },
      exit: () => this.requestNav("orbit"),
      toggleFly: () => this.requestNav(this.nav === "fly" ? "walk" : "fly"),
      cycleShell: () => this.cb.onCycleShell?.(),
      blocked: () => this.cb.keysBlocked?.() ?? false,
      lockChange: (locked) => this.cb.onPointerLock?.(locked),
      lockError: () => this.cb.onPointerLockError?.(),
    });
    this.controls.addEventListener("change", this.onControlChange);
    this.controls.addEventListener("start", this.onControlStart);
    this.controls.addEventListener("end", this.onControlEnd);

    canvas.addEventListener("pointerdown", this.onPointerDown);
    canvas.addEventListener("pointerup", this.onPointerUp);
    canvas.addEventListener("pointermove", this.onPointerMove);
    canvas.addEventListener("pointerleave", this.onPointerLeave);
    // OrbitControls raises "start" for wheel zoom too, but a bare listener
    // means a fly is dropped even if the wheel is ignored for some reason.
    canvas.addEventListener("wheel", this.onWheel, { passive: true });
    canvas.addEventListener("webglcontextlost", this.onContextLost);
    canvas.addEventListener("webglcontextrestored", this.onContextRestored);
    document.addEventListener("visibilitychange", this.onVisibility);

    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(container);
    this.resize();

    this.applyPose(cameraToPose(exteriorCorner(this.bounds(), this.aspect())));
    this.env.fit(this.bounds(), -0.15, 0, null);
  }

  // ------------------------------------------------------------------ model

  setDoc(doc: DocState | null, opts: DocOptions): void {
    if (this.disposed) return;
    const now = performance.now();
    const prevDoc = this.doc;
    this.doc = doc;
    this.opts = opts;
    if (opts.cutaway !== this.cutWanted) {
      this.cutWanted = opts.cutaway;
      this.startCutaway(opts.cutaway, doc, now);
    }
    this.requestPackModels();
    if (this.currentKey() !== this.buildKey || doc !== prevDoc) this.rebuild();
    this.setRoofShown(opts.roofVisible && !opts.cutaway, now);
    this.shadowDirty();
    this.syncReferenceModels(doc, opts);
    this.syncWalk();
    this.invalidate();
  }

  // -------------------------------------------------------------- CC0 pack

  /** Starts the GLB load for every catalog key on screen. Already-known keys are free. */
  private requestPackModels(): void {
    if (!this.packAssets) return;
    for (const e of this.doc?.project.elements ?? []) {
      if (e.kind === "asset") modelPack.request(e.catalog_key);
    }
  }

  /**
   * A catalog model finished parsing. Several land within a few milliseconds
   * of each other, so the rebuild is coalesced: one pass swaps every form that
   * is ready, and the swapped elements play the standard entrance motion.
   */
  private onPackModel(key: string): void {
    if (this.disposed || !this.packAssets) return;
    this.packArrived.add(key);
    if (this.packTimer) return;
    this.packTimer = setTimeout(() => {
      this.packTimer = 0;
      this.swapPackModels();
    }, PACK_COALESCE_MS);
  }

  private swapPackModels(): void {
    if (this.disposed || !this.doc) return;
    const keys = this.packArrived;
    this.packArrived = new Set();
    const ids: string[] = [];
    for (const e of this.doc.project.elements) {
      if (e.kind === "asset" && keys.has(e.catalog_key)) ids.push(e.id);
    }
    if (ids.length === 0) return;
    // The document did not change, so the diff would see nothing to do: force
    // the build through and animate the swapped elements by hand.
    this.buildKey = "";
    this.rebuild();
    if (motionOK()) this.enterElements(ids, performance.now());
    this.shadowDirty();
    this.invalidate();
  }

  /**
   * Dev harness toggles. `assets` covers the GLB furniture and the PBR maps,
   * `hdri` the sky and its light, so both looks can be compared side by side.
   */
  setPackOptions(opts: { assets?: boolean; hdri?: boolean }): void {
    if (this.disposed) return;
    if (opts.assets !== undefined && opts.assets !== this.packAssets) {
      this.packAssets = opts.assets;
      this.lib.setPackEnabled(opts.assets);
      this.requestPackModels();
      this.buildKey = "";
      this.rebuild();
      this.shadowDirty();
    }
    if (opts.hdri !== undefined) {
      this.env.setHdriEnabled(opts.hdri);
      this.anim.set("env", 1);
      this.applyAnimated();
      this.shadowDirty();
    }
    this.invalidate();
  }

  /** The HDRI is on the scene: settle into its exposure instead of popping. */
  private onHdriReady(): void {
    if (this.disposed) return;
    this.shadowDirty();
    if (!motionOK()) {
      this.anim.set("env", 1);
      this.applyAnimated();
      this.invalidate();
      return;
    }
    this.anim.set("env", 0);
    this.anim.to("env", 1, performance.now(), { duration: dur("panel"), easing: ease.out });
    this.applyAnimated();
    this.invalidate();
  }

  /**
   * Reference models are context, not part of the diffed model: they keep
   * their own small store (engine/referenceModels.ts) so a placeholder in
   * flight is never torn down by an unrelated rebuild. Placement mirrors the
   * same layer and level rules as the rest of the model; they are never cut
   * by the cutaway clip because they live outside `built.root` entirely.
   */
  private syncReferenceModels(doc: DocState | null, opts: DocOptions): void {
    if (!doc) {
      this.refs.sync([], false, false);
      return;
    }
    const layer = doc.project.layers?.find((l) => l.key === "underlays");
    const layerVisible = layer?.visible !== false;
    const layerLocked = layer?.locked === true;
    const levels = new Map(doc.project.levels.map((l) => [l.id, l]));
    const sorted = [...doc.project.levels].sort((a, b) => a.elevation_mm - b.elevation_mm);
    const lowest = sorted[0] ?? null;
    const activeLevel = (opts.activeLevelId && levels.get(opts.activeLevelId)) || lowest;
    const shown = (levelId: string): boolean => {
      const level = levels.get(levelId) ?? lowest;
      if (!level) return true;
      return !opts.cutaway || !activeLevel || level.elevation_mm <= activeLevel.elevation_mm + 1;
    };
    const items: ReferenceModelPlacement[] = [];
    for (const e of doc.project.elements) {
      if (e.kind !== "reference_model" || !shown(e.level_id)) continue;
      const level = levels.get(e.level_id) ?? lowest;
      items.push({
        id: e.id,
        fileName: e.file_name,
        position: e.position,
        elevationMm: (level?.elevation_mm ?? 0) + e.elevation_mm,
        rotationDeg: e.rotation_deg,
        scaleToMm: e.scale_to_mm,
      });
    }
    this.refs.sync(items, layerVisible, layerLocked);
  }

  /** A reference model finished loading: rise into place like any other new element. */
  private enterReferenceModel(_id: string, mount: THREE.Group): void {
    const meshes = meshesUnder(mount);
    if (meshes.length === 0) return;
    const key = `enter:${this.fadeSeq++}`;
    this.pushFade(key, new MeshFade(meshes), ENTER_LIFT_M, 0, 1, dur("base"), ease.spring);
  }

  /** What the built scene depends on. The cutaway part is what is baked in,
   *  not what the user asked for: the cut height animates before the rebuild. */
  private currentKey(): string {
    const doc = this.doc;
    if (!doc) return "";
    const ghostKey = this.opts.ghostsRemoved?.ids.join(",") ?? "";
    return [
      doc.project.id,
      doc.revision,
      doc.project.updated_at,
      doc.project.elements.length,
      this.cutBaked,
      this.opts.activeLevelId,
      ghostKey,
    ].join("|");
  }

  private rebuild(): void {
    const opts = this.opts;
    const prevBuilt = this.built;
    this.buildKey = this.currentKey();
    const prevSignatures = this.signatures;
    const nextSignatures = new Map<string, string>();
    for (const e of this.doc?.project.elements ?? []) nextSignatures.set(e.id, elementSignature(e));

    const firstOfProject = (this.doc?.project.id ?? "") !== this.docKey;
    const animate = motionOK() && !firstOfProject && prevBuilt !== null;
    const added: string[] = [];
    const removed: string[] = [];
    const modified: string[] = [];
    if (animate) {
      for (const [id, sig] of nextSignatures) {
        const before = prevSignatures.get(id);
        if (before === undefined) added.push(id);
        else if (before !== sig) modified.push(id);
      }
      for (const id of prevSignatures.keys()) if (!nextSignatures.has(id)) removed.push(id);
      // A pipe layer switched off (or a level cut away) takes its runs out the
      // same way a delete does, and switching it back brings them in.
      const shownBefore = new Set(prevBuilt?.pipes.solos.keys() ?? []);
      const shownAfter = this.shownPipeIds();
      for (const id of shownBefore) if (!shownAfter.has(id) && nextSignatures.has(id)) removed.push(id);
      for (const id of shownAfter) {
        if (shownBefore.has(id) || !prevSignatures.has(id)) continue;
        added.push(id);
        const m = modified.indexOf(id);
        if (m >= 0) modified.splice(m, 1);
      }
    }
    // The red removal ghosts of an AI preview leave the same way elements do.
    const hadGhosts = prevBuilt?.root.getObjectByName("ghosts") ?? null;
    const keepsGhosts = (opts.ghostsRemoved?.ids.length ?? 0) > 0;

    this.finishFades();
    this.releaseHighlights();
    this.detachLeaving(prevBuilt, removed, animate && !keepsGhosts ? hadGhosts : null);
    this.disposeBuilt();
    this.signatures = nextSignatures;
    for (const key of this.anim.keys()) {
      if (key.startsWith("hl:") && !nextSignatures.has(key.slice(3))) this.anim.remove(key);
    }

    if (!this.doc) {
      this.framed = false;
      this.docKey = "";
      this.refreshPickables();
      return;
    }
    try {
      this.built = buildScene(this.doc, this.lib, { ...opts, cache: this.cache, packModels: this.packAssets });
    } catch (e) {
      console.error("viewer3d: scene build failed", e);
      this.built = null;
      this.refreshPickables();
      return;
    }
    this.scene.add(this.built.root);
    this.refreshPickables();
    // The shell look goes onto any material this build created, and the
    // outlines follow the new geometry, before any entrance lifts a mesh.
    this.shell.invalidate();
    this.shell.ensureOutlines(this.built, this.doc, () => undefined, (g) => this.built?.kit.track(g));
    this.applyShell();
    // A new model: pay for full resolution again before deciding it is heavy.
    this.interactionRatio = this.fullRatio();
    this.shadowDirty();
    const north = this.doc.project.settings?.north_angle_deg ?? 0;
    this.env.fit(this.built.bounds, this.built.groundY, Number.isFinite(north) ? north : 0, this.built.contact);
    const radius = this.radius();
    this.controls.maxDistance = Math.max(radius * 30, 60);
    if (this.clip) this.applyClipToModel(this.clip);

    // Frame a project the first time it has something to show.
    const docKey = this.doc.project.id;
    const isNewProject = docKey !== this.docKey;
    if (isNewProject) {
      this.docKey = docKey;
      this.framed = false;
    }
    if (!this.framed && !this.built.empty) {
      this.framed = true;
      this.applyPose(cameraToPose(exteriorCorner(this.built.bounds, this.aspect())));
    }
    // The roof group is new: put it straight into the state it should be in.
    this.built.roofGroup.visible = this.roofShown;

    const now = performance.now();
    if (isNewProject && motionOK() && !this.built.empty) this.staggerIn(now);
    else if (animate) {
      if (added.length > 0) this.enterElements(added, now);
      for (const id of modified) this.flashElement(id, now);
    }
    // Cached forms nothing holds any more can go. Anything still playing its
    // exit fade is parented into `leaving` and is swept when that lands.
    this.cache.sweep();
    this.syncHighlights();
  }

  private disposeBuilt(): void {
    this.pickables = [];
    if (!this.built) return;
    this.scene.remove(this.built.root);
    // Empty the discarded root: anything the build cache still owns has to end
    // up unparented, otherwise `BuildCache.sweep` can never let go of it.
    for (const o of [...this.built.root.children]) o.removeFromParent();
    // The kit is disposed by the exit animation when it borrowed meshes.
    if (this.leaving?.kit !== this.built.kit) this.built.kit.dispose();
    this.built = null;
    for (const g of this.edgeCache.values()) g.dispose();
    this.edgeCache.clear();
  }

  // ------------------------------------------------------------ model motion

  /** Top level object under the model root that carries this element. */
  private topFor(built: BuiltScene, id: string): THREE.Object3D[] {
    const out = new Set<THREE.Object3D>();
    for (const mesh of built.byElement.get(id) ?? []) {
      let o: THREE.Object3D = mesh;
      while (o.parent && o.parent !== built.root) o = o.parent;
      if (o.parent === built.root) out.add(o);
    }
    return [...out];
  }

  /**
   * Keeps the meshes of removed elements (and a retired ghost group) in the
   * scene past the rebuild so they can sink and fade before they go. The old
   * kit goes with them and is disposed when the exit lands, so geometry
   * counts return to their steady values.
   */
  private detachLeaving(prev: BuiltScene | null, removed: string[], ghosts: THREE.Object3D | null): void {
    if (!prev) return;
    // Only one exit at a time: land the previous one first.
    if (this.leaving) this.anim.finish("leave");
    const tops: THREE.Object3D[] = [];
    for (const id of removed) tops.push(...this.topFor(prev, id));
    if (ghosts) tops.push(ghosts);
    if (tops.length === 0) return;
    const group = new THREE.Group();
    group.name = "leaving";
    for (const o of tops) group.add(o);
    // A leaving pipe has no batch any more: its own form is what fades out.
    group.traverse((o) => {
      if (o.userData.pipe) o.layers.set(0);
    });
    this.scene.add(group);
    const kit = prev.kit;
    this.leaving = { group, kit };
    const fade = new MeshFade(meshesUnder(group));
    this.pushFade("leave", fade, EXIT_SINK_M, 1, 0, dur("base") * 0.7, ease.in, () => {
      this.scene.remove(group);
      for (const o of [...group.children]) o.removeFromParent();
      kit.dispose();
      this.leaving = null;
      this.cache.sweep();
    });
  }

  /** New elements rise a few centimeters into place and fade in. */
  private enterElements(ids: string[], now: number): void {
    const built = this.built;
    if (!built) return;
    const meshes: THREE.Mesh[] = [];
    for (const id of ids) for (const top of this.topFor(built, id)) meshes.push(...meshesUnder(top));
    if (meshes.length === 0) return;
    const key = `enter:${this.fadeSeq++}`;
    this.pushFade(key, new MeshFade(meshes), ENTER_LIFT_M, 0, 1, dur("base"), ease.spring, undefined, now);
  }

  /**
   * First load of a project: a gentle rise by kind, slab and floors first,
   * assets last. Total stays under 500 ms and is skipped under reduced motion.
   */
  private staggerIn(now: number): void {
    const built = this.built;
    if (!built) return;
    const kindOf = new Map<string, Element["kind"]>();
    for (const e of this.doc?.project.elements ?? []) kindOf.set(e.id, e.kind);
    const bucketOf = (kind: Element["kind"] | undefined): number => {
      switch (kind) {
        case "wall":
          return 1;
        case "opening":
        case "column":
        case "stair":
        case "pipe":
          return 2;
        case "asset":
          return 4;
        default:
          return 0; // rooms, slabs and anything unlabelled: the floor
      }
    };
    const buckets: THREE.Mesh[][] = [[], [], [], [], []];
    const roof = new Set(meshesUnder(built.roofGroup));
    for (const mesh of meshesUnder(built.root)) {
      // Pipe batches draw nothing of a pipe that is fading: its solo does.
      if (mesh.userData.batch || mesh.userData.outline) continue;
      if (roof.has(mesh)) {
        buckets[3].push(mesh);
        continue;
      }
      buckets[bucketOf(kindOf.get(mesh.userData.elementId as string))].push(mesh);
    }
    buckets.forEach((meshes, i) => {
      if (meshes.length === 0) return;
      const key = `enter:${this.fadeSeq++}`;
      this.pushFade(key, new MeshFade(meshes), ENTER_LIFT_M, 0, 1, dur("base"), ease.spring, undefined, now, i * STAGGER_MS);
    });
  }

  /** Lands every running enter or roof fade, before their meshes go away. */
  private finishFades(): void {
    for (const job of [...this.fades]) this.anim.finish(job.key);
  }

  private fadeOwns(mesh: THREE.Mesh): boolean {
    for (const job of this.fades) if (job.fade.owns(mesh)) return true;
    return false;
  }

  private pushFade(
    key: string,
    fade: MeshFade,
    lift: number,
    from: number,
    to: number,
    duration: number,
    easing: (x: number) => number,
    onLand?: () => void,
    now = performance.now(),
    delay = 0,
  ): void {
    if (fade.empty) {
      onLand?.();
      return;
    }
    const job: FadeJob = { key, fade, lift };
    this.fades.push(job);
    this.anim.set(key, from);
    fade.setOpacity(from);
    if (lift !== 0) fade.setLift((from - 1) * lift);
    this.anim.to(key, to, now, {
      duration,
      easing,
      delay,
      onDone: () => {
        fade.release();
        this.fades = this.fades.filter((f) => f !== job);
        this.anim.remove(key);
        onLand?.();
        // Anything that was waiting for this fade (a hover or preview tint on
        // an element that was still rising) can take its material now.
        this.syncHighlights();
        this.invalidate();
      },
    });
    this.invalidate();
  }

  // -------------------------------------------------------------- highlights

  setHighlights(h: Highlights): void {
    this.highlights = h;
    this.syncHighlights();
  }

  private wantedStates(): Map<string, HighlightState> {
    const { selection, hoverId, previewIds } = this.highlights;
    const selected = new Set(selection);
    const preview = new Set(previewIds);
    const wanted = new Map<string, HighlightState>();
    for (const id of preview) wanted.set(id, selected.has(id) ? "preview-selected" : "preview");
    for (const id of selected) if (!preview.has(id)) wanted.set(id, "selected");
    if (hoverId && !wanted.has(hoverId)) wanted.set(hoverId, "hover");
    return wanted;
  }

  private syncHighlights(): void {
    const built = this.built;
    const now = performance.now();
    const wanted = built ? this.wantedStates() : new Map<string, HighlightState>();

    for (const [id, hl] of this.live) {
      if (wanted.has(id) || hl.state === "flash") continue;
      const drop = () => {
        this.live.get(id)?.release();
        this.live.delete(id);
        this.anim.remove(`hl:${id}`);
        // The last breathing tint just left: stop the breath, or the frame
        // loop would run on forever for nothing.
        this.syncBreath(performance.now());
        this.invalidate();
      };
      // Retargets instead of queueing: rapid hover across meshes never stacks.
      if (this.anim.value(`hl:${id}`, 0) <= 0) drop();
      else this.anim.to(`hl:${id}`, 0, now, { duration: dur("hover") * 0.7, easing: ease.in, onDone: drop });
    }

    for (const [id, state] of wanted) {
      const meshes = this.allMeshesFor(id);
      if (meshes.length === 0) continue;
      // An element that is still rising into place is driven by its fade.
      // The tint is applied when that lands, so the two never fight over
      // the same material slot.
      if (meshes.some((m) => this.fadeOwns(m))) continue;
      // A room floor keeps its material and takes a light wash instead.
      const soft = state === "selected" && meshes.every((m) => m.userData.soft === true);
      const want: HighlightState = soft ? "selected-soft" : state;
      let hl = this.live.get(id);
      if (!hl) {
        hl = new Highlight(id, want, meshes);
        if (hl.empty) continue;
        this.live.set(id, hl);
        // A rebuild keeps the value, so a still-selected element does not
        // fade in again: only a genuinely new highlight starts at 0.
        if (!this.anim.has(`hl:${id}`)) this.anim.set(`hl:${id}`, 0);
      } else {
        hl.retarget(want);
      }
      if (spec(want).outline && !hl.hasOutline()) {
        for (const mesh of meshes) {
          // A pipe is too thin and round for an edge outline: it gets a silhouette band.
          if (mesh.userData.pipe) hl.addHullOutline(mesh, this.outlineResolution());
          else hl.addOutline(mesh, this.edgesFor(mesh), this.outlineResolution());
        }
      }
      if (this.anim.value(`hl:${id}`, 0) < 1) {
        this.anim.to(`hl:${id}`, 1, now, { duration: dur("hover"), easing: ease.out });
      }
    }

    this.syncBreath(now);
    this.applyAnimated();
    this.invalidate();
  }

  /** Every mesh for an element, live model or reference model, for highlighting and focus. */
  private allMeshesFor(id: string): THREE.Mesh[] {
    const out = [...(this.built?.byElement.get(id) ?? [])];
    const mount = this.refs.getMount(id);
    if (mount) out.push(...meshesUnder(mount));
    return out;
  }

  /** A short emissive flash: a modified element swaps in place, it never re-enters. */
  private flashElement(id: string, now: number): void {
    const meshes = this.allMeshesFor(id);
    if (meshes.length === 0 || this.live.has(id)) return;
    const hl = new Highlight(id, "flash", meshes);
    if (hl.empty) return;
    this.live.set(id, hl);
    this.anim.set(`hl:${id}`, 0);
    this.anim.to(`hl:${id}`, 1, now, {
      duration: dur("press"),
      easing: ease.out,
      onDone: () => {
        this.anim.to(`hl:${id}`, 0, performance.now(), {
          duration: dur("hover"),
          easing: ease.in,
          onDone: () => {
            this.live.get(id)?.release();
            this.live.delete(id);
            this.anim.remove(`hl:${id}`);
            this.invalidate();
          },
        });
        this.invalidate();
      },
    });
    this.invalidate();
  }

  /**
   * The purple AI preview is the only looping motion in 3D. It starts when a
   * preview tint is on screen and stops the moment it clears, so the frame
   * loop stops with it. Reduced motion holds a steady tint.
   */
  private syncBreath(now: number): void {
    let breathing = false;
    for (const hl of this.live.values()) if (spec(hl.state).breathes) breathing = true;
    if (!breathing) {
      // Stops the only looping animation in 3D, so the frame loop stops too.
      this.breathRunning = false;
      this.anim.set("breath", PREVIEW_OPACITY);
      return;
    }
    if (this.breathRunning) return;
    if (!motionOK()) {
      this.anim.set("breath", BREATH_HIGH);
      return;
    }
    this.breathRunning = true;
    this.anim.set("breath", BREATH_LOW);
    this.anim.to("breath", BREATH_HIGH, now, { duration: BREATH_MS / 2, easing: ease.inOut, loop: true });
  }

  private edgesFor(mesh: THREE.Mesh): LineSegmentsGeometry {
    const key = mesh.geometry.uuid;
    let edges = this.edgeCache.get(key);
    if (!edges) {
      const thin = new THREE.EdgesGeometry(mesh.geometry, 25);
      edges = new LineSegmentsGeometry().fromEdgesGeometry(thin);
      thin.dispose();
      this.edgeCache.set(key, edges);
    }
    return edges;
  }

  private outlineResolution(): THREE.Vector2 {
    return new THREE.Vector2(Math.max(this.width, 1), Math.max(this.height, 1));
  }

  private releaseHighlights(): void {
    for (const hl of this.live.values()) hl.release();
    this.live.clear();
    this.breathRunning = false;
    if (this.anim.has("breath")) this.anim.set("breath", PREVIEW_OPACITY);
  }

  // ------------------------------------------------------------ roof, cut, sun

  private setRoofShown(on: boolean, now: number): void {
    if (on === this.roofShown) return;
    this.roofShown = on;
    const group = this.built?.roofGroup;
    if (!group) return;
    this.shadowDirty();
    if (!motionOK()) {
      group.visible = on;
      return;
    }
    group.visible = true;
    const meshes = meshesUnder(group);
    if (meshes.length === 0) {
      group.visible = on;
      return;
    }
    // A hidden roof sits ROOF_LIFT_M higher, so a negative lift takes it up.
    this.pushFade(
      `roof:${this.fadeSeq++}`,
      new MeshFade(meshes),
      -ROOF_LIFT_M,
      on ? 0 : 1,
      on ? 1 : 0,
      dur("panel"),
      on ? ease.out : ease.in,
      () => {
        group.visible = this.roofShown;
      },
      now,
    );
  }

  /** World height in meters of the cutaway plane for the active level. */
  private cutHeightY(doc: DocState | null): number {
    const levels = doc?.project.levels ?? [];
    const sorted = [...levels].sort((a, b) => a.elevation_mm - b.elevation_mm);
    const active = (this.opts.activeLevelId && levels.find((l) => l.id === this.opts.activeLevelId)) || sorted[0];
    return ((active?.elevation_mm ?? 0) + CUTAWAY_MM) / 1000;
  }

  /**
   * The cut height animates with a clipping plane over the model, because the
   * baked cutaway geometry cannot show what it does not contain: turning the
   * cutaway on clips the full model down to 1200 mm and rebuilds at the end,
   * turning it off rebuilds first and lets the plane rise back to the top.
   */
  /** World height in meters of the tallest wall top. Where the cut starts. */
  private wallTopY(doc: DocState | null): number {
    let top = 0;
    for (const l of doc?.project.levels ?? []) top = Math.max(top, l.elevation_mm + l.height_mm);
    return (top > 0 ? top : this.bounds().maxZ) / 1000;
  }

  private startCutaway(on: boolean, doc: DocState | null, now: number): void {
    const cutY = this.cutHeightY(doc);
    const topY = Math.max(this.wallTopY(doc), cutY + 0.1);
    this.shadowDirty();
    if (!motionOK()) {
      this.cutBaked = on;
      this.clearClip();
      return;
    }
    if (!on) this.cutBaked = false; // the uncut geometry is rebuilt right away
    // Toggling again mid-flight picks up the plane where it is: no jump.
    this.clipFrom = this.clip ? this.clip.constant : on ? topY : cutY;
    this.clipTo = on ? cutY : topY;
    this.ensureClip();
    this.anim.set("cut", 0);
    this.anim.to("cut", 1, now, {
      duration: dur("panel"),
      easing: ease.inOut,
      onDone: () => {
        if (on && !this.cutBaked) {
          this.cutBaked = true;
          this.rebuild();
        }
        this.clearClip();
        this.invalidate();
      },
    });
  }

  private ensureClip(): void {
    this.clip ??= new THREE.Plane(new THREE.Vector3(0, -1, 0), this.clipFrom);
    this.clip.constant = this.clipFrom;
    this.applyClipToModel(this.clip);
  }

  private applyClipToModel(plane: THREE.Plane): void {
    const built = this.built;
    if (!built) return;
    built.root.traverse((o) => {
      // The roof is never cut: it fades out over the same beat instead, so
      // the plane can start at the wall tops and sweep down from there.
      for (let p: THREE.Object3D | null = o; p; p = p.parent) if (p === built.roofGroup) return;
      const mat = (o as THREE.Mesh).material;
      if (!mat || Array.isArray(mat)) return;
      // Pipes on the active level stay whole: the cutaway is for seeing them.
      if (mat.userData.shellCat === "pipe") return;
      if (mat.clippingPlanes && mat.clippingPlanes[0] === plane) return;
      mat.clippingPlanes = [plane];
      mat.needsUpdate = true;
      this.clipped.add(mat);
    });
  }

  private clearClip(): void {
    for (const mat of this.clipped) {
      mat.clippingPlanes = null;
      mat.needsUpdate = true;
    }
    this.clipped.clear();
    this.clip = null;
    this.anim.remove("cut");
    this.shadowDirty();
  }

  bounds(): ModelBounds {
    return this.built?.bounds ?? { minX: -4000, minY: -4000, maxX: 4000, maxY: 4000, minZ: -150, maxZ: 3000 };
  }

  private radius(): number {
    const b = this.bounds();
    return Math.max(Math.hypot(b.maxX - b.minX, b.maxY - b.minY, b.maxZ - b.minZ) / 2000, 2);
  }

  isEmpty(): boolean {
    return !this.built || this.built.empty;
  }

  /** Sun shadows fade their strength in and out instead of popping. */
  setShadows(on: boolean): void {
    if (on === this.shadowsOn) return;
    this.shadowsOn = on;
    const now = performance.now();
    if (on) this.env.setShadows(true);
    this.shadowDirty();
    this.anim.to("shadow", on ? 1 : 0, now, {
      duration: dur("panel"),
      easing: on ? ease.out : ease.in,
      onDone: () => {
        if (!this.shadowsOn) this.env.setShadows(false);
        this.shadowDirty();
        this.invalidate();
      },
    });
    this.applyAnimated();
    this.invalidate();
  }

  // ----------------------------------------------------------------- camera

  aspect(): number {
    return this.width > 0 && this.height > 0 ? this.width / this.height : 16 / 9;
  }

  private applyPose(pose: WorldPose): void {
    this.tween = null;
    this.anim.remove("camera");
    this.camera.position.set(...pose.position);
    this.controls.target.set(...pose.target);
    this.camera.fov = pose.fovDeg;
    this.camera.updateProjectionMatrix();
    this.controls.update();
    this.invalidate();
  }

  /**
   * Eases to a pose with the shared `--dur-scene` easing. Interruptible at any
   * moment: any orbit, pan or zoom drops the fly where it is, with no jump.
   */
  flyTo(pose: WorldPose, duration = dur("scene"), onDone?: () => void): void {
    if (this.disposed) return;
    const fov = Math.min(Math.max(pose.fovDeg || 45, 5), 110);
    this.tween = {
      fromPos: this.camera.position.clone(),
      toPos: new THREE.Vector3(...pose.position),
      fromTarget: this.controls.target.clone(),
      toTarget: new THREE.Vector3(...pose.target),
      fromFov: this.camera.fov,
      toFov: fov,
    };
    this.anim.set("camera", 0);
    this.anim.to("camera", 1, performance.now(), {
      duration,
      easing: ease.inOut,
      onDone: () => {
        this.applyCameraTween(1);
        this.tween = null;
        this.anim.remove("camera");
        onDone?.();
        this.invalidate();
      },
    });
    this.invalidate();
  }

  private applyCameraTween(k: number): void {
    const tw = this.tween;
    if (!tw) return;
    this.camera.position.lerpVectors(tw.fromPos, tw.toPos, k);
    this.controls.target.lerpVectors(tw.fromTarget, tw.toTarget, k);
    this.camera.fov = tw.fromFov + (tw.toFov - tw.fromFov) * k;
    this.camera.updateProjectionMatrix();
  }

  flyToCamera(camera: Pick<Camera, "position" | "target" | "fov_deg">, duration?: number): void {
    const ok = [camera.position, camera.target].every((v) => [v.x, v.y, v.z].every(Number.isFinite));
    if (!ok) return;
    this.leaveWalkFor();
    this.flyTo(cameraToPose(camera), duration);
  }

  /** Returns false when the preset cannot be computed (no room to stand in). */
  goPreset(kind: PresetKind, selection: string[], duration?: number): PosePreset | null {
    const pose = this.presetPose(kind, selection);
    if (pose) {
      this.leaveWalkFor();
      this.flyTo(cameraToPose(pose), duration);
    }
    return pose;
  }

  presetPose(kind: PresetKind, selection: string[]): PosePreset | null {
    const b = this.bounds();
    const a = this.aspect();
    switch (kind) {
      case "eye_level":
        return eyeLevel(b, a);
      case "exterior_corner":
        return exteriorCorner(b, a);
      case "top":
        return topView(b, a);
      case "axonometric":
        return axonometric(b, a);
      case "fit": {
        const dir = this.camera.position.clone().sub(this.controls.target);
        if (dir.lengthSq() < 1e-9) dir.set(-1, 1, 1);
        const d = worldToVec3(dir.x, dir.y, dir.z);
        return fitFromDirection(b, a, d, this.camera.fov);
      }
      case "room_interior":
        return this.roomPose(selection);
    }
  }

  private roomPose(selection: string[]): PosePreset | null {
    const doc = this.doc;
    if (!doc) return null;
    const rooms = doc.derived.rooms ?? [];
    const usable = rooms.filter((r) => r.polygon.length >= 3);
    if (usable.length === 0) return null;
    let geo = usable.find((r) => selection.includes(r.room_id));
    if (!geo) {
      // a selected wall: the first room it bounds
      geo = usable.find((r) => r.wall_ids.some((w) => selection.includes(w)));
    }
    if (!geo) geo = [...usable].sort((p, q) => Math.abs(signedArea(q.polygon)) - Math.abs(signedArea(p.polygon)))[0];
    const room = doc.project.elements.find((e) => e.id === geo.room_id);
    const levelId = room && room.kind === "room" ? room.level_id : null;
    const level = doc.project.levels.find((l) => l.id === levelId) ?? doc.project.levels[0];
    const name = room && room.kind === "room" ? `${room.name} interior` : "Room interior";
    return roomInterior(geo.polygon, geo.label_point, level?.elevation_mm ?? 0, name);
  }

  /**
   * Frames the elements. Returns true when one of them is a pipe that a solid
   * shell would mostly hide (in a wall, under the floor), so the caller can
   * switch to X-ray and say so.
   */
  focusElements(ids: string[]): boolean {
    const doc = this.doc;
    let hiddenPipe = false;
    if (doc) {
      for (const id of ids) {
        const e = doc.project.elements.find((x) => x.id === id);
        if (e?.kind === "pipe" && pipeMostlyHidden(doc, e)) hiddenPipe = true;
      }
    }
    const box = new THREE.Box3();
    for (const id of ids) for (const mesh of this.allMeshesFor(id)) box.expandByObject(mesh);
    if (box.isEmpty()) return hiddenPipe;
    this.leaveWalkFor();
    const sphere = box.getBoundingSphere(new THREE.Sphere());
    const dir = this.camera.position.clone().sub(this.controls.target).normalize();
    if (dir.lengthSq() < 0.5) dir.set(-0.5, 0.5, 0.7).normalize();
    const fov = Math.max(this.camera.fov, 30);
    const dist = fitDistance(Math.max(sphere.radius, 0.8), fov, this.aspect()) * 1.15;
    const pos = sphere.center.clone().addScaledVector(dir, dist);
    this.flyTo({ position: [pos.x, pos.y, pos.z], target: [sphere.center.x, sphere.center.y, sphere.center.z], fovDeg: fov });
    return hiddenPipe;
  }

  /** Current pose as a contract camera: world mm, x east, y north, z up. */
  currentCamera(name = "View"): Camera {
    const p = this.camera.position;
    // Walking, the target is a point straight ahead of the eyes.
    const t =
      this.nav === "orbit" ? this.controls.target : p.clone().add(new THREE.Vector3(0, 0, -3).applyQuaternion(this.camera.quaternion));
    const round = (v: { x: number; y: number; z: number }) => ({
      x: Math.round(v.x * 10) / 10,
      y: Math.round(v.y * 10) / 10,
      z: Math.round(v.z * 10) / 10,
    });
    return {
      id: "",
      name,
      preset: "custom",
      position: round(worldToVec3(p.x, p.y, p.z)),
      target: round(worldToVec3(t.x, t.y, t.z)),
      fov_deg: Math.round(this.camera.fov * 100) / 100,
    };
  }


  // ------------------------------------------------------------ walk and fly

  /** Current navigation mode. */
  navMode(): NavMode {
    return this.nav;
  }

  /**
   * Orbit, walk or fly. Entering walk or fly tweens the camera to eye height
   * (1600 mm above the level floor): inside the room under the orbit target
   * when there is one, else just outside the first exterior door. Leaving
   * tweens to an orbit pose that looks where the walker looked.
   */
  setNav(next: NavMode): void {
    if (this.disposed) return;
    if (next === this.nav) {
      if (next === "orbit") this.navPending = null;
      return;
    }
    if (next === "orbit") {
      this.navPending = null;
      this.leaveWalk(true);
      return;
    }
    if (!this.doc) {
      this.navPending = next;
      return;
    }
    if (this.nav === "orbit") this.enterWalk(next, null, null);
    else this.switchWalkMode(next);
  }

  /** Applies a change the engine decided on and tells the app. */
  private requestNav(next: NavMode): void {
    this.setNav(next);
    this.cb.onNavChange?.(this.nav);
  }

  /** A camera request while walking hands the camera back to orbit first, without a tween of its own. */
  private leaveWalkFor(): void {
    if (this.nav === "orbit") {
      if (this.navPending) {
        this.navPending = null;
        this.cb.onNavChange?.("orbit");
      }
      return;
    }
    this.leaveWalk(false);
    this.cb.onNavChange?.("orbit");
  }

  /** The level the walker stands on: the active level, or the lowest. */
  private activeLevelIdOrLowest(): string | null {
    const levels = this.doc?.project.levels ?? [];
    if (this.opts.activeLevelId && levels.some((l) => l.id === this.opts.activeLevelId)) return this.opts.activeLevelId;
    return [...levels].sort((a, b) => a.elevation_mm - b.elevation_mm)[0]?.id ?? null;
  }

  private enterWalk(mode: WalkMode, pose: WalkPose | null, levelId: string | null): void {
    const doc = this.doc;
    if (!doc) {
      this.navPending = mode;
      return;
    }
    this.navPending = null;
    this.seenActiveLevel = this.opts.activeLevelId;
    // An orbit fly in progress is dropped where it is: the walk takes over from there.
    if (this.tween) {
      this.anim.remove("camera");
      this.tween = null;
    }
    this.walkLevelId = levelId ?? this.activeLevelIdOrLowest();
    this.refreshWalkWorld();
    const w = this.walker;
    w.mode = mode;
    const start = pose ?? this.defaultWalkStart();
    w.place(start.x, start.y, start.yaw, start.pitch);
    w.z = w.eyeHeight();
    this.beginWalkBlend();
    this.nav = mode;
    this.controls.enabled = false;
    this.walkControls.attach();
    this.lastWalkAt = 0;
    this.minimapDirty = true;
    this.applyWalkerCamera();
    this.invalidate();
  }

  /** The camera eases from where it is into the walker's eyes, following the walker if it moves meanwhile. */
  private beginWalkBlend(): void {
    this.walkBlend = { pos: this.camera.position.clone(), quat: this.camera.quaternion.clone(), fov: this.camera.fov };
    this.anim.set("walk", 0);
    this.anim.to("walk", 1, performance.now(), {
      duration: dur("scene"),
      easing: ease.inOut,
      onDone: () => {
        this.walkBlend = null;
        this.anim.remove("walk");
        this.invalidate();
      },
    });
  }

  private switchWalkMode(mode: WalkMode): void {
    const w = this.walker;
    w.mode = mode;
    w.vz = 0;
    // Back on foot: step out of anything flown into; the eye settles to 1600 mm.
    if (mode === "walk") w.settle();
    this.nav = mode;
    this.lastWalkAt = 0;
    this.minimapDirty = true;
    this.invalidate();
  }

  /** Where a walk starts when nothing asked for a place. */
  private defaultWalkStart(): WalkPose {
    const doc = this.doc;
    const levelId = this.walkLevelId;
    const t = this.controls.target;
    const target = { x: t.x * 1000, y: -t.z * 1000 };
    // Keep looking the way the orbit camera looked.
    const d = t.clone().sub(this.camera.position);
    const heading = Math.hypot(d.x, d.z) > 1e-6 ? Math.atan2(-d.z, d.x) : null;
    if (!doc || !levelId) return { x: target.x, y: target.y, yaw: heading ?? Math.PI / 2, pitch: -0.05 };
    return walkStartPose(doc, levelId, this.walker.world, target, heading);
  }

  private leaveWalk(tween: boolean): void {
    if (this.nav === "orbit") return;
    const eye = this.camera.position.clone();
    const fwd = new THREE.Vector3(0, 0, -1).applyQuaternion(this.camera.quaternion).normalize();
    this.walkControls.detach();
    this.nav = "orbit";
    this.walkBlend = null;
    this.anim.remove("walk");
    this.lastWalkAt = 0;
    this.controls.enabled = true;
    // The orbit target is what the walker looked at, so the first orbit frame
    // looks exactly where the walk left off.
    const dist = this.lookDistance(eye, fwd);
    const target = eye.clone().addScaledVector(fwd, dist);
    this.controls.target.copy(target);
    if (!tween || !motionOK()) {
      this.controls.update();
      this.invalidate();
      return;
    }
    // Settle back and up a little so the orbit has room to turn, stopping
    // short of whatever stands behind (a wall, a door leaf, a wardrobe).
    const back = new THREE.Vector3(-fwd.x, 0, -fwd.z);
    if (back.lengthSq() < 1e-6) back.set(0, 0, 1);
    back.normalize();
    const room = this.clearDistance(eye, back, EXIT_BACK_MM / 1000 + CLEARANCE_M) - CLEARANCE_M;
    const end = eye.clone().addScaledVector(back, Math.max(room, 0));
    end.y += Math.max(this.clearDistance(end, new THREE.Vector3(0, 1, 0), EXIT_UP_MM / 1000 + CLEARANCE_M) - CLEARANCE_M, 0);
    // OrbitControls keeps the camera above the horizon of its target: raise
    // the end pose if the walker looked up, instead of letting it snap there.
    const flat = Math.hypot(end.x - target.x, end.z - target.z);
    const minY = target.y - Math.tan(0.1) * flat;
    if (end.y < minY) end.y = minY;
    this.controls.maxPolarAngle = Math.PI - 0.01;
    this.flyTo({ position: [end.x, end.y, end.z], target: [target.x, target.y, target.z], fovDeg: this.camera.fov }, dur("scene"), () => {
      this.controls.maxPolarAngle = this.orbitMaxPolar;
    });
  }

  /** How far a ray from `from` goes before it meets something drawn, up to `max` meters. */
  private clearDistance(from: THREE.Vector3, dir: THREE.Vector3, max: number): number {
    this.raycaster.set(from, dir);
    this.raycaster.far = max;
    const hits = this.raycaster.intersectObjects(this.pickables, false);
    this.raycaster.far = Infinity;
    for (const h of hits) {
      const mat = (h.object as THREE.Mesh).material;
      if (mat && !Array.isArray(mat) && mat.visible === false) continue;
      return h.distance;
    }
    return max;
  }

  /** Distance to what the eye looks at, 1.5 to 12 m, or 5 m when nothing is there. */
  private lookDistance(eye: THREE.Vector3, dir: THREE.Vector3): number {
    this.raycaster.set(eye, dir);
    this.raycaster.far = 12;
    const hits = this.raycaster.intersectObjects(this.pickables, false);
    this.raycaster.far = Infinity;
    for (const h of hits) {
      const mat = (h.object as THREE.Mesh).material;
      if (mat && !Array.isArray(mat) && mat.visible === false) continue;
      return Math.min(Math.max(h.distance, 1.5), 12);
    }
    return 5;
  }

  /**
   * `walk_to`: stands about 1.5 m from the finding on the room side, at eye
   * height, facing it. `location` is plan mm with z above the floor of the
   * first element's level. Returns why a solid shell would hide the finding.
   */
  walkTo(ids: string[], location: Vec3 | null): HiddenReason | null {
    const doc = this.doc;
    if (this.disposed) return null;
    if (!doc) {
      this.walkToPending = { ids, location };
      return null;
    }
    const first = ids.map((id) => doc.project.elements.find((e) => e.id === id)).find((e) => e);
    let levelId = this.activeLevelIdOrLowest();
    if (first) {
      if ("level_id" in first && typeof first.level_id === "string") levelId = first.level_id;
      else if (first.kind === "opening") {
        const wall = doc.project.elements.find((e) => e.id === first.wall_id);
        if (wall?.kind === "wall") levelId = wall.level_id;
      }
    }
    const loc = location ?? this.findingPoint(ids, levelId);
    if (!loc || !levelId) {
      this.setNav("walk");
      this.cb.onWalkTo?.(null);
      return null;
    }
    if (this.nav === "orbit") {
      this.walkLevelId = levelId;
      this.refreshWalkWorld();
      this.enterWalk("walk", walkToPose(doc, levelId, this.walker.world, loc), levelId);
    } else {
      this.walkLevelId = levelId;
      this.refreshWalkWorld();
      const pose = walkToPose(doc, levelId, this.walker.world, loc);
      this.beginWalkBlend();
      const wasFly = this.nav === "fly";
      this.walker.mode = "walk";
      this.nav = "walk";
      this.walker.place(pose.x, pose.y, pose.yaw, pose.pitch);
      this.walker.z = this.walker.eyeHeight();
      this.minimapDirty = true;
      this.applyWalkerCamera();
      this.invalidate();
      if (wasFly) this.cb.onNavChange?.("walk");
    }
    const hidden = hiddenAt(doc, levelId, loc);
    this.cb.onWalkTo?.(hidden);
    return hidden;
  }

  /** A point to look at when a finding has no location: the middle of its first element. */
  private findingPoint(ids: string[], levelId: string | null): Vec3 | null {
    const doc = this.doc;
    if (!doc) return null;
    const e = doc.project.elements.find((x) => ids.includes(x.id));
    if (e?.kind === "pipe" && e.points.length > 0) return e.points[Math.floor((e.points.length - 1) / 2)];
    const box = new THREE.Box3();
    for (const id of ids) for (const mesh of this.allMeshesFor(id)) box.expandByObject(mesh);
    if (box.isEmpty()) return null;
    const c = box.getCenter(new THREE.Vector3());
    const level = doc.project.levels.find((l) => l.id === levelId);
    return { x: c.x * 1000, y: -c.z * 1000, z: c.y * 1000 - (level?.elevation_mm ?? 0) };
  }

  /** Keeps walk mode in step with a new document, level or pending request. */
  private syncWalk(): void {
    if (!this.doc) {
      if (this.nav !== "orbit") this.requestNav("orbit");
      return;
    }
    if (this.walkToPending) {
      const req = this.walkToPending;
      this.walkToPending = null;
      this.walkTo(req.ids, req.location);
      return;
    }
    if (this.navPending) {
      this.enterWalk(this.navPending, null, null);
      return;
    }
    if (this.nav === "orbit") return;
    // Picking another level in the app takes the walker there. A walk_to on
    // another level stays put until the active level changes.
    if (this.opts.activeLevelId !== this.seenActiveLevel) {
      this.seenActiveLevel = this.opts.activeLevelId;
      const active = this.activeLevelIdOrLowest();
      if (active) this.walkLevelId = active;
    }
    this.refreshWalkWorld();
  }

  /** Rebuilds what blocks the walker and what the minimap shows, when the document or level changed. */
  private refreshWalkWorld(): void {
    const doc = this.doc;
    const levelId = this.walkLevelId ?? this.activeLevelIdOrLowest();
    if (doc === this.walkWorldDoc && levelId === this.walkWorldLevel) return;
    this.walkWorldDoc = doc;
    this.walkWorldLevel = levelId;
    const world = buildCollisionWorld(doc, levelId);
    const w = this.walker;
    w.world = world;
    const level = doc?.project.levels.find((l) => l.id === levelId);
    w.floorZ = level?.elevation_mm ?? 0;
    const b = this.bounds();
    w.area = { minX: b.minX, minY: b.minY, maxX: b.maxX, maxY: b.maxY };
    w.minZ = (this.built?.groundY ?? -0.15) * 1000 - 2500;
    w.maxZ = b.maxZ + 20000;
    const layerOn = (key: string) => doc?.project.layers?.find((l) => l.key === key)?.visible !== false;
    this.minimapScene = {
      version: ++this.minimapVersion,
      world,
      rooms: doc && levelId ? roomsOn(doc, levelId).map((r) => r.polygon) : [],
      pipes: (doc?.project.elements ?? []).flatMap((e) =>
        e.kind === "pipe" && layerOn(e.system) && (world.levelId === null || this.levelIdOf(e.level_id) === world.levelId)
          ? [{ system: e.system, points: e.points, diameterMm: e.diameter_mm }]
          : [],
      ),
    };
    this.minimapDirty = true;
    if (this.nav === "walk") w.settle();
  }

  private levelIdOf(levelId: string): string | null {
    const levels = this.doc?.project.levels ?? [];
    if (levels.some((l) => l.id === levelId)) return levelId;
    return [...levels].sort((a, b) => a.elevation_mm - b.elevation_mm)[0]?.id ?? null;
  }

  /** Puts the walker's eyes on the camera, blended with the entrance while it runs. */
  private applyWalkerCamera(): void {
    if (this.nav === "orbit") return;
    const w = this.walker;
    tmpPos.set(w.x / 1000, w.z / 1000, -w.y / 1000);
    tmpEuler.set(w.pitch, w.yaw - Math.PI / 2, 0, "YXZ");
    tmpQuat.setFromEuler(tmpEuler);
    const b = this.walkBlend;
    let fov = WALK_FOV;
    if (b) {
      const k = this.anim.value("walk", 1);
      this.camera.position.lerpVectors(b.pos, tmpPos, k);
      this.camera.quaternion.slerpQuaternions(b.quat, tmpQuat, k);
      fov = b.fov + (WALK_FOV - b.fov) * k;
    } else {
      this.camera.position.copy(tmpPos);
      this.camera.quaternion.copy(tmpQuat);
    }
    if (this.camera.fov !== fov) {
      this.camera.fov = fov;
      this.camera.updateProjectionMatrix();
    }
  }

  /** The walker as the overlay needs it, in plan mm. */
  walkerPose(): { x: number; y: number; z: number; yaw: number; pitch: number; mode: WalkMode } {
    const w = this.walker;
    return { x: w.x, y: w.y, z: w.z, yaw: w.yaw, pitch: w.pitch, mode: w.mode };
  }

  /** The overlay's minimap canvas, or null when it unmounts. */
  setMinimap(canvas: HTMLCanvasElement | null): void {
    this.minimap = canvas ? new Minimap(canvas) : null;
    if (canvas) this.drawMinimap();
  }

  private drawMinimap(): void {
    this.minimapDirty = false;
    if (!this.minimap || this.nav === "orbit" || !this.minimapScene) return;
    const w = this.walker;
    this.minimap.draw(this.minimapScene, { x: w.x, y: w.y, yaw: w.yaw });
  }

  pointerLockAvailable(): boolean {
    return pointerLockSupported();
  }

  requestPointerLock(): void {
    if (this.nav !== "orbit") this.walkControls.requestLock();
  }

  exitPointerLock(): void {
    this.walkControls.exitLock();
  }

  // ------------------------------------------------------------ shell modes

  shellMode(): ShellMode {
    return this.shell.mode;
  }

  /** Solid, X-ray or hidden, animated over `--dur-panel`. Pipes stay solid in all three. */
  setShell(mode: ShellMode, animate = true): void {
    if (this.disposed || mode === this.shell.mode) return;
    this.shell.begin(mode);
    this.shell.ensureOutlines(this.built, this.doc, (m) => this.restYOf(m), (g) => this.built?.kit.track(g));
    if (!animate || !motionOK()) {
      this.shell.jump(mode);
      this.anim.remove("shell");
      this.applyShell();
      this.applyAnimated();
      this.invalidate();
      return;
    }
    this.anim.set("shell", 0);
    this.anim.to("shell", 1, performance.now(), {
      duration: dur("panel"),
      easing: ease.inOut,
      onDone: () => {
        this.anim.remove("shell");
        this.shell.sample(1);
        this.applyShell();
        this.invalidate();
      },
    });
    this.applyAnimated();
    this.invalidate();
  }

  /** The resting height of a mesh an entrance fade still lifts. */
  private restYOf(mesh: THREE.Mesh): number | undefined {
    for (const job of this.fades) {
      const y = job.fade.restY(mesh);
      if (y !== undefined) return y;
    }
    return undefined;
  }

  /**
   * Puts the current shell look on the materials, the outlines and the ground.
   * The sun shadow fades with the shell, so the shadow map is only redrawn
   * when a part of the building appears or disappears: once per change.
   */
  private applyShell(): void {
    if (this.shell.applyMaterials(this.lib.all())) this.shadowDirty();
    this.shell.applyOutlines();
    this.env.setGroundOpacity(this.shell.look.ground);
  }

  /** Pipe ids the current options would build: visible layer, shown level. */
  private shownPipeIds(): Set<string> {
    const out = new Set<string>();
    const doc = this.doc;
    if (!doc) return out;
    const levels = new Map(doc.project.levels.map((l) => [l.id, l]));
    const lowest = [...levels.values()].sort((a, b) => a.elevation_mm - b.elevation_mm)[0] ?? null;
    const levelOf = (id: string) => levels.get(id) ?? lowest;
    const shown = levelFilter(doc.project, this.opts);
    const layerOn = (key: string) => doc.project.layers?.find((l) => l.key === key)?.visible !== false;
    for (const e of doc.project.elements) if (e.kind === "pipe" && pipeShown(e, levelOf, shown, layerOn)) out.add(e.id);
    return out;
  }

  /**
   * A pipe draws from its solo while it is highlighted or fading, and from its
   * system's batch otherwise. Checked right before every render.
   */
  private syncPipes(): void {
    const pipes = this.built?.pipes;
    if (!pipes || pipes.size === 0) return;
    const want: string[] = [];
    for (const id of pipes.solos.keys()) {
      if (this.live.has(id)) want.push(id);
      else if (this.fades.length > 0 && pipes.meshesOf(id).some((m) => this.fadeOwns(m))) want.push(id);
    }
    pipes.promote(want);
  }

  // ---------------------------------------------------------------- capture

  /**
   * Renders the model without highlights at the given size. PNG data URL.
   * Every running animation lands first, so a capture never catches a frame
   * halfway through a rise, a fade or a cutaway.
   */
  capture(width = 1920, height = 1080): string {
    if (this.disposed || this.contextLost) throw new Error("The 3D view is not available right now.");
    this.anim.finishAll();
    this.applyAnimated();
    this.releaseHighlights();
    this.syncPipes();
    const prevRatio = this.renderer.getPixelRatio();
    const prevAspect = this.camera.aspect;
    try {
      this.renderer.setPixelRatio(1);
      this.renderer.setSize(width, height, false);
      this.camera.aspect = width / height;
      this.camera.updateProjectionMatrix();
      this.env.followCamera(this.camera);
      this.shadowDirty();
      this.renderer.render(this.scene, this.camera);
      return this.renderer.domElement.toDataURL("image/png");
    } finally {
      this.renderer.setPixelRatio(prevRatio);
      this.renderer.setSize(Math.max(this.width, 1), Math.max(this.height, 1), false);
      this.shadowDirty();
      this.camera.aspect = prevAspect;
      this.camera.updateProjectionMatrix();
      this.syncHighlights();
      this.renderNow();
    }
  }

  // ----------------------------------------------------------------- export

  /**
   * Serializes the current model (no helpers, lights, grid, ground, ghosts,
   * highlights or camera) to one of the interop formats. Builds its own
   * throwaway scene graph and material set from `buildScene`, so it never
   * touches what is on screen. Reference models are included from whatever
   * this engine currently has mounted (loaded geometry, or the placeholder
   * box while a file is still loading or missing).
   */
  async exportScene(format: "glb" | "obj" | "dae"): Promise<{ data: string; extension: string }> {
    if (this.disposed) throw new Error("The 3D view is not available right now.");
    if (!this.doc) throw new Error("Nothing to export yet.");
    const { group, dispose } = buildExportGroup(this.doc, this.refs.exportSnapshot(), { packModels: this.packAssets });
    try {
      if (format === "glb") return await exportGLB(group);
      if (format === "obj") return exportOBJ(group);
      return exportDAE(group);
    } finally {
      dispose();
    }
  }

  // ------------------------------------------------------------ render loop

  invalidate = (): void => {
    this.needsRender = true;
    this.schedule();
  };

  /**
   * One pending frame at a time. `invalidate` fires from inside the frame
   * callback (OrbitControls raises "change" from `controls.update()`), so a
   * second unconditional `requestAnimationFrame` at the end of the frame used
   * to leave two callbacks queued where one was expected. That doubles every
   * tick: a fast orbit reached 50 renders in a single 16 ms frame and starved
   * pointer input. Everything schedules through here now.
   */
  private schedule(): void {
    if (this.raf || this.disposed || this.paused()) return;
    this.raf = requestAnimationFrame(this.frame);
  }

  private paused(): boolean {
    return this.contextLost || document.hidden || this.width < 2 || this.height < 2;
  }

  // ------------------------------------------------------- interaction scale

  /** Device pixels per CSS pixel at rest. Retina is capped at 2. */
  private fullRatio(): number {
    return Math.min(window.devicePixelRatio || 1, 2);
  }

  /**
   * While the camera is moving the view renders at the interaction ratio,
   * which starts at the full ratio and drops to one device pixel as soon as a
   * frame runs over budget. Full resolution comes back 150 ms after the last
   * input, with one clean frame. Nothing else about the look changes, so there
   * is no flash: only the sampling rate of the same image.
   */
  private applyPixelRatio(): void {
    const full = this.fullRatio();
    if (this.interactionRatio <= 0) this.interactionRatio = full;
    const want = this.interacting ? Math.min(this.interactionRatio, full) : full;
    if (want === this.appliedRatio) return;
    this.appliedRatio = want;
    // setPixelRatio re-sizes the drawing buffer from the size three already has.
    this.renderer.setPixelRatio(want);
  }

  private beginInteraction(now: number): void {
    this.interactionTail = now + INTERACTION_TAIL_MS;
    this.dampingUntil = now + DAMPING_MAX_MS;
    if (this.interacting) return;
    this.interacting = true;
    this.overBudget = 0;
    this.applyPixelRatio();
  }

  /** Applies whatever damping still owes in one step and stops it. */
  private landDamping(): void {
    if (!this.controls.enableDamping) return;
    this.controls.enableDamping = false;
    this.controls.update();
    this.controls.enableDamping = true;
  }

  private endInteraction(): void {
    if (!this.interacting) return;
    this.interacting = false;
    this.overBudget = 0;
    this.applyPixelRatio();
    // One clean frame at full resolution.
    this.needsRender = true;
  }

  /** A frame that ran long drops the interaction to one device pixel. */
  private noteFrameCost(costMs: number, intervalMs: number): void {
    if (!this.interacting || this.interactionRatio <= 1) return;
    const slow = costMs > FRAME_BUDGET_MS || intervalMs > 20;
    this.overBudget = slow ? this.overBudget + 1 : 0;
    if (this.overBudget >= 2) {
      this.interactionRatio = 1;
      this.applyPixelRatio();
    }
  }

  /** The shadow map is redrawn on the next frame. */
  private shadowDirty(): void {
    this.renderer.shadowMap.needsUpdate = true;
  }

  /** Writes every animated value onto the scene. Called once per frame. */
  private applyAnimated(): void {
    if (this.tween) this.applyCameraTween(this.anim.value("camera", 1));
    if (this.anim.has("shell")) {
      this.shell.sample(this.anim.value("shell", 1));
      this.applyShell();
    }
    if (this.walkBlend) this.applyWalkerCamera();
    for (const job of this.fades) {
      const v = this.anim.value(job.key, 1);
      job.fade.setOpacity(v);
      if (job.lift !== 0) job.fade.setLift((v - 1) * job.lift);
    }
    if (this.live.size > 0) {
      const breath = this.anim.value("breath", PREVIEW_OPACITY);
      for (const [id, hl] of this.live) hl.apply(this.anim.value(`hl:${id}`, 1), breath);
    }
    // X-ray and hidden fade the sun shadow instead of redrawing the shadow map.
    this.env.sun.shadow.intensity = this.anim.value("shadow", 1) * this.shell.look.shadow;
    // Exposure, ambient fill and sun strength between the procedural sky and
    // the HDRI. A no-op until the HDRI is actually on the scene.
    this.env.applyBlend(this.anim.value("env", 1), this.renderer);
    if (this.clip) this.clip.constant = this.clipFrom + (this.clipTo - this.clipFrom) * this.anim.value("cut", 1);
  }

  private frame = (now: number): void => {
    this.raf = 0;
    if (this.disposed || this.paused()) return;
    const started = performance.now();
    const interval = this.lastFrameAt > 0 ? now - this.lastFrameAt : 0;
    this.lastFrameAt = now;
    let active = false;
    if (this.anim.sample(now)) {
      this.applyAnimated();
      this.needsRender = true;
      // Meshes moved, faded or got clipped: the shadow map has to follow. A
      // camera, walk or shell move changes nothing the sun sees.
      if (this.anim.moved().some((k) => !isViewOnlyTrack(k))) this.shadowDirty();
    }
    if (this.anim.animating()) active = true;
    if (this.nav !== "orbit") {
      // Frame-rate independent: the step uses the real gap (clamped inside),
      // and the first frame after standing still counts as one 60 Hz frame.
      const dt = this.lastWalkAt > 0 ? (now - this.lastWalkAt) / 1000 : 1 / 60;
      const moving = this.walker.step(dt, this.walkControls.input());
      this.lastWalkAt = moving ? now : 0;
      if (moving) {
        active = true;
        this.needsRender = true;
        this.minimapDirty = true;
        this.beginInteraction(now);
      }
      this.applyWalkerCamera();
    }
    // No hover raycast while the camera is being driven or a button is down:
    // the pointer is orbiting, not pointing at anything.
    if (this.hoverQueued) {
      if (this.interacting || this.pointerDown) {
        this.hoverQueued = null;
      } else {
        const p = this.hoverQueued;
        this.hoverQueued = null;
        const id = this.pick(p.x, p.y);
        if (id !== this.lastHover) {
          this.lastHover = id;
          this.cb.onHover(id);
        }
      }
    }
    if (this.nav === "orbit") {
      // Damping keeps reporting change long after the pointer is released. It
      // is cut off at DAMPING_MAX_MS: the camera lands on its damped target in
      // one step and the loop stops instead of trickling towards an epsilon.
      if (this.controls.update()) {
        if (this.pointerActive || now < this.dampingUntil) active = true;
        else this.landDamping();
      }
      // Stay above the ground.
      const floor = (this.built?.groundY ?? -0.15) + 0.12;
      if (this.camera.position.y < floor) this.camera.position.y = floor;
    }
    if (this.interacting && !active && now >= this.interactionTail) this.endInteraction();
    if (this.needsRender || active) this.renderNow();
    if (this.minimapDirty) this.drawMinimap();
    this.noteFrameCost(performance.now() - started, interval);
    // The interaction tail needs frames of its own to land the clean one.
    if (active || (this.interacting && now < this.interactionTail)) this.schedule();
  };

  private renderNow(): void {
    if (this.disposed || this.contextLost) return;
    this.needsRender = false;
    this.syncPipes();
    this.env.followCamera(this.camera);
    this.renderer.render(this.scene, this.camera);
    if (!this.firstRendered) {
      this.firstRendered = true;
      this.cb.onFirstRender?.();
    }
  }

  private resize(): void {
    const w = this.container.clientWidth;
    const h = this.container.clientHeight;
    if (w === this.width && h === this.height) return;
    this.width = w;
    this.height = h;
    if (w < 2 || h < 2) return;
    this.appliedRatio = 0;
    this.applyPixelRatio();
    this.renderer.setSize(w, h, false);
    for (const hl of this.live.values()) hl.setResolution(w, h);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    // Render in the same task so the resized canvas never shows a blank frame.
    this.renderNow();
    this.invalidate();
  }

  // ---------------------------------------------------------------- picking

  /**
   * Flat list of every mesh a ray can hit, rebuilt with the scene. Raycasting
   * a list instead of the two roots skips the per-pick traversal of the whole
   * graph; three still rejects each mesh by its bounding sphere first.
   */
  private refreshPickables(): void {
    const out: THREE.Object3D[] = [];
    if (this.built) for (const m of meshesUnder(this.built.root)) if (!m.userData.batch) out.push(m);
    this.pickables = out;
  }

  private pick(clientX: number, clientY: number): string | null {
    const targets: THREE.Object3D[] = this.pickables;
    const refs = this.refs.root.children.length > 0 ? [this.refs.root] : [];
    if (targets.length === 0 && refs.length === 0) return null;
    const rect = this.renderer.domElement.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return null;
    const ndc = new THREE.Vector2(
      ((clientX - rect.left) / rect.width) * 2 - 1,
      -((clientY - rect.top) / rect.height) * 2 + 1,
    );
    this.raycaster.setFromCamera(ndc, this.camera);
    const hits = this.raycaster.intersectObjects(targets, false);
    if (refs.length > 0) {
      hits.push(...this.raycaster.intersectObjects(refs, true));
      hits.sort((a, b) => a.distance - b.distance);
    }
    // Through an X-ray or hidden shell the pipes are what you point at: a
    // pipe anywhere along the ray wins over the faint building in front of it.
    const preferPipes = this.shell.mode !== "solid";
    let fallback: THREE.Intersection | null = null;
    for (const hit of hits) {
      let visible = true;
      for (let o: THREE.Object3D | null = hit.object; o; o = o.parent) {
        if (!o.visible) {
          visible = false;
          break;
        }
      }
      if (!visible) continue;
      const mat = (hit.object as THREE.Mesh).material;
      if (mat && !Array.isArray(mat) && mat.visible === false) continue;
      if (preferPipes && !hit.object.userData.pipe) {
        fallback ??= hit;
        continue;
      }
      if (hit.object.userData.locked) return null;
      return (hit.object.userData.elementId as string | undefined) ?? null;
    }
    if (fallback) {
      if (fallback.object.userData.locked) return null;
      return (fallback.object.userData.elementId as string | undefined) ?? null;
    }
    return null;
  }

  private onPointerDown = (e: PointerEvent): void => {
    if (e.button === 0) this.pointerDown = { x: e.clientX, y: e.clientY };
  };

  private onPointerUp = (e: PointerEvent): void => {
    const down = this.pointerDown;
    this.pointerDown = null;
    if (!down || e.button !== 0) return;
    if (Math.hypot(e.clientX - down.x, e.clientY - down.y) > 4) return;
    this.cb.onPick(this.pick(e.clientX, e.clientY), e.shiftKey);
  };

  private onPointerMove = (e: PointerEvent): void => {
    // No hover work while a button is down or the camera is being driven.
    if (e.buttons !== 0 || this.pointerActive || this.interacting) return;
    // One raycast per frame at most: the frame loop drains this.
    this.hoverQueued = { x: e.clientX, y: e.clientY };
    this.schedule();
  };

  private onPointerLeave = (): void => {
    this.hoverQueued = null;
    if (this.lastHover !== null) {
      this.lastHover = null;
      this.cb.onHover(null);
    }
  };

  /** Any user camera input drops a fly where it is. No jump, no queue. */
  private onControlStart = (): void => {
    this.dropFly();
    this.pointerActive = true;
    this.beginInteraction(performance.now());
    this.hoverQueued = null;
  };

  /** The wheel, as a safety net: OrbitControls raises start and end itself. */
  private onWheel = (): void => {
    this.dropFly();
    this.beginInteraction(performance.now());
  };

  private dropFly(): void {
    if (this.tween) {
      this.anim.remove("camera");
      this.tween = null;
    }
    this.cb.onUserOrbit();
  }

  private onControlEnd = (): void => {
    this.pointerActive = false;
    const now = performance.now();
    this.interactionTail = now + INTERACTION_TAIL_MS;
    this.dampingUntil = now + DAMPING_MAX_MS;
    this.invalidate();
  };

  /** The camera moved: render, and hold the interaction window open. */
  private onControlChange = (): void => {
    if (this.pointerActive) this.interactionTail = performance.now() + INTERACTION_TAIL_MS;
    this.invalidate();
  };

  // -------------------------------------------------------------- lifecycle

  private onVisibility = (): void => {
    if (!document.hidden) this.invalidate();
  };

  private onContextLost = (e: Event): void => {
    e.preventDefault();
    this.contextLost = true;
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
    this.cb.onContextLost(true);
  };

  private onContextRestored = (): void => {
    this.contextLost = false;
    // Render targets lost their contents: bake the sky light again, and the
    // HDRI's PMREM with it.
    this.env.bakeEnvironment(this.renderer);
    this.env.reloadHdri(this.renderer);
    this.shadowDirty();
    this.cb.onContextLost(false);
    this.invalidate();
  };

  /**
   * Holds one animated value at `k` and renders that exact frame. The motion
   * checks use it to photograph a real mid-animation state instead of racing
   * a screenshot against a 240 ms animation. It stops that track where it is,
   * like any other interruption.
   */
  freezeAt(key: string, k: number): void {
    if (!this.anim.has(key)) return;
    this.anim.hold(key, k);
    this.applyAnimated();
    this.renderNow();
  }

  /** Resource counters for the leak check in the dev harness. */
  stats() {
    return {
      geometries: this.renderer.info.memory.geometries,
      textures: this.renderer.info.memory.textures,
      programs: this.renderer.info.programs?.length ?? 0,
      triangles: this.renderer.info.render.triangles,
      library: this.lib.stats(),
      contextLost: this.contextLost,
      /** Animation bookkeeping, for the motion checks. */
      animations: this.anim.active(),
      animKeys: this.anim.keys(),
      /** True while a frame loop is scheduled. Must return to false when idle. */
      looping: this.raf !== 0,
      fades: this.fades.length,
      highlights: this.live.size,
      cached: this.cache.size(),
      nav: this.nav,
      shell: this.shell.mode,
      walker: this.nav === "orbit" ? null : this.walkerPose(),
      pipes: {
        solos: this.built?.pipes.size ?? 0,
        batches: this.built?.pipes.batches.length ?? 0,
        promoted: [...(this.built?.pipes.promotedIds() ?? [])],
      },
      referenceModels: this.refs.stats(),
      /** CC0 pack: how much of it is on screen, for the dev readout. */
      pack: {
        assets: this.packAssets,
        models: modelPack.stats(),
        textures: this.lib.packStats(),
        hdri: this.env.hdriActive,
      },
    };
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    liveEngines--;
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
    if (this.packTimer) clearTimeout(this.packTimer);
    this.packTimer = 0;
    this.packOff?.();
    this.packOff = null;
    this.resizeObserver.disconnect();
    const canvas = this.renderer.domElement;
    canvas.removeEventListener("pointerdown", this.onPointerDown);
    canvas.removeEventListener("pointerup", this.onPointerUp);
    canvas.removeEventListener("pointermove", this.onPointerMove);
    canvas.removeEventListener("pointerleave", this.onPointerLeave);
    canvas.removeEventListener("wheel", this.onWheel);
    canvas.removeEventListener("webglcontextlost", this.onContextLost);
    canvas.removeEventListener("webglcontextrestored", this.onContextRestored);
    document.removeEventListener("visibilitychange", this.onVisibility);
    this.controls.removeEventListener("change", this.onControlChange);
    this.controls.removeEventListener("start", this.onControlStart);
    this.controls.removeEventListener("end", this.onControlEnd);
    this.walkControls.detach();
    this.minimap = null;
    this.controls.dispose();
    this.anim.clear();
    for (const job of this.fades) job.fade.release();
    this.fades.length = 0;
    this.releaseHighlights();
    this.clearClip();
    if (this.leaving) {
      this.scene.remove(this.leaving.group);
      this.leaving.kit.dispose();
      this.leaving = null;
    }
    this.disposeBuilt();
    this.shell.dispose();
    this.cache.dispose();
    this.refs.dispose();
    this.scene.remove(this.refs.root);
    this.lib.dispose();
    this.env.dispose();
    this.renderer.dispose();
    this.renderer.forceContextLoss();
    canvas.remove();
  }
}
