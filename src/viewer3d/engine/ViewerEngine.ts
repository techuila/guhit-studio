// The three.js side of the viewer: renderer, scene, camera, controls,
// picking, highlights, motion and capture. No React in here. One instance per
// mounted Viewer3D; `dispose` releases every GPU resource it created.
//
// Motion (docs/MOTION.md, rows "3D view"): every animated value lives in
// `Animator` (engine/animator.ts), durations come from src/ui/motion.ts and
// the frame loop runs only while something is animating. The engine renders
// on demand; it never holds a permanent requestAnimationFrame.

import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { LineSegmentsGeometry } from "three/examples/jsm/lines/LineSegmentsGeometry.js";
import type { Camera, DocState, Element } from "../../contract/bindings";
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
import { cameraToPose, worldToVec3, type WorldPose } from "../geom/coords";
import { signedArea } from "../geom/polygon";
import { buildExportGroup, exportDAE, exportGLB, exportOBJ } from "../scene/exportScene";
import { BuildCache } from "../scene/buildCache";
import { buildScene, CUTAWAY_MM, type BuiltScene } from "../scene/buildScene";
import { MaterialLibrary } from "../scene/materials";
import { loadPackManifest, modelPack } from "../scene/pack";
import { Animator } from "./animator";
import { Environment } from "./environment";
import { Highlight, spec, type HighlightState } from "./highlights";
import { MeshFade, meshesUnder } from "./meshFade";
import { ReferenceModelStore, type ReferenceModelPlacement } from "./referenceModels";

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
    this.controls.maxPolarAngle = Math.PI * 0.5 + 0.12;
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
        for (const mesh of meshes) hl.addOutline(mesh, this.edgesFor(mesh), this.outlineResolution());
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
  flyTo(pose: WorldPose, duration = dur("scene")): void {
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
    this.flyTo(cameraToPose(camera), duration);
  }

  /** Returns false when the preset cannot be computed (no room to stand in). */
  goPreset(kind: PresetKind, selection: string[], duration?: number): PosePreset | null {
    const pose = this.presetPose(kind, selection);
    if (pose) this.flyTo(cameraToPose(pose), duration);
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

  focusElements(ids: string[]): void {
    const box = new THREE.Box3();
    for (const id of ids) for (const mesh of this.allMeshesFor(id)) box.expandByObject(mesh);
    if (box.isEmpty()) return;
    const sphere = box.getBoundingSphere(new THREE.Sphere());
    const dir = this.camera.position.clone().sub(this.controls.target).normalize();
    if (dir.lengthSq() < 0.5) dir.set(-0.5, 0.5, 0.7).normalize();
    const fov = Math.max(this.camera.fov, 30);
    const dist = fitDistance(Math.max(sphere.radius, 0.8), fov, this.aspect()) * 1.15;
    const pos = sphere.center.clone().addScaledVector(dir, dist);
    this.flyTo({ position: [pos.x, pos.y, pos.z], target: [sphere.center.x, sphere.center.y, sphere.center.z], fovDeg: fov });
  }

  /** Current pose as a contract camera: world mm, x east, y north, z up. */
  currentCamera(name = "View"): Camera {
    const p = this.camera.position;
    const t = this.controls.target;
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
    for (const job of this.fades) {
      const v = this.anim.value(job.key, 1);
      job.fade.setOpacity(v);
      if (job.lift !== 0) job.fade.setLift((v - 1) * job.lift);
    }
    if (this.live.size > 0) {
      const breath = this.anim.value("breath", PREVIEW_OPACITY);
      for (const [id, hl] of this.live) hl.apply(this.anim.value(`hl:${id}`, 1), breath);
    }
    this.env.sun.shadow.intensity = this.anim.value("shadow", 1);
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
      // Meshes moved, faded or got clipped: the shadow map has to follow.
      this.shadowDirty();
    }
    if (this.anim.animating()) active = true;
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
    if (this.interacting && !active && now >= this.interactionTail) this.endInteraction();
    if (this.needsRender || active) this.renderNow();
    this.noteFrameCost(performance.now() - started, interval);
    // The interaction tail needs frames of its own to land the clean one.
    if (active || (this.interacting && now < this.interactionTail)) this.schedule();
  };

  private renderNow(): void {
    if (this.disposed || this.contextLost) return;
    this.needsRender = false;
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
    if (this.built) for (const m of meshesUnder(this.built.root)) out.push(m);
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
    for (const hit of hits) {
      let visible = true;
      for (let o: THREE.Object3D | null = hit.object; o; o = o.parent) {
        if (!o.visible) {
          visible = false;
          break;
        }
      }
      if (!visible) continue;
      if (hit.object.userData.locked) return null;
      return (hit.object.userData.elementId as string | undefined) ?? null;
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
