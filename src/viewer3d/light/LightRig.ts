// The live view's light: sun, sky, lamps, exposure, the refine passes and
// the sun path overlay, driven by `useViewer().light` and the project's site.
// One per ViewerEngine; the engine calls three hooks and nothing else:
//
// - `beforeRender()` right before every live render: puts the light on the
//   scene (sky rebaked if the sun moved, intensities at the current exposure)
//   and throws away any refine in progress, because the picture changed.
// - `idleFrame(now)` from a frame in which nothing moved and nothing was
//   drawn: settles the exposure and takes refine samples. It returns true when
//   it wants another frame; the engine then asks for one through
//   `ViewerEngine.schedule()`. It never calls requestAnimationFrame itself.
// - `attach(built, doc)` after every rebuild: the fixtures' lamps, the ghost
//   lights and the site.
//
// Refine (docs/CONTRACT.md, "Sun and light"): when the camera rests, about 64
// frames with the camera shifted by a fraction of a pixel and the sun moved
// across a small disc are averaged into the canvas: clean edges and soft sun
// shadows. Then it stops, and a still, refined view draws nothing. Anything
// that draws the live view again starts it over.
//
// Exposure: auto in the live view, settling in about half a second. The
// target is the light's base EV (light/model.ts), corrected by a reading of
// the first refine sample (a log average, clamped to a stop or so, ignored
// when small, so the view does not pump). A light with `exposureEv` is locked.

import * as THREE from "three";
import type { DocState, Site, ViewLight } from "../../contract/bindings";
import { motionOK } from "../../ui/motion";
import type { Environment } from "../engine/environment";
import type { BuiltScene } from "../scene/buildScene";
import { useViewer, type LiveLight } from "../viewerStore";
import { LampRig, type LampState } from "./lamps";
import { clamp, currentYear, exposureScale, lightFrame, METER_KEY, sameLight, toViewLight, type LightFrame } from "./model";
import { Accumulator, canAccumulate, discPoint } from "./refine";
import { MANILA } from "./sun";
import { SunPathOverlay } from "./sunPath";

/** Samples averaged by a full refine. */
export const REFINE_SAMPLES = 64;
/** The refined picture replaces the live one from this many samples on, when it is already cleaner. */
const SHOW_AFTER = 6;
/** The sun disc the refine jitters across, degrees of radius. Wider than the real 0.27 for a softer edge. */
const SUN_DISC_DEG = 0.55;
/**
 * Exposure settles with this time constant, seconds: a stop lands in about a
 * third of a second, two stops in under half a second.
 */
const EXPOSURE_TAU = 0.09;
/** Closer than this, EV, the exposure snaps onto its target and the frames stop. */
const EXPOSURE_SNAP = 0.02;
/** Metered corrections smaller than this are ignored, EV, so the view does not pump. */
const METER_DEADBAND = 0.35;
/** CPU budget for refine samples in one frame, ms. */
const SAMPLE_BUDGET_MS = 6;

export interface LightHost {
  readonly renderer: THREE.WebGLRenderer;
  readonly scene: THREE.Scene;
  readonly camera: THREE.PerspectiveCamera;
  readonly env: Environment;
  /** A live render on the next frame. */
  invalidate(): void;
  /** Another frame, without a live render: refine and exposure settling. */
  requestFrame(): void;
  /** A live render now, from inside a frame. */
  renderLive(): void;
  /** The sun and lamp shadow maps are drawn again on the next render. */
  shadowDirty(): void;
  /** What to compile ahead when the lamps change: every lit object in the scene, no lights. */
  compileTargets(): THREE.Object3D[];
}

export interface LightStats {
  sky: string;
  ev: number;
  targetEv: number;
  baseEv: number;
  metered: number;
  lampsLit: boolean;
  /** The lamps are in the scene (after their shaders compiled). */
  lampsShown: boolean;
  /** A lamp set compiling in the background. */
  compiling: string | null;
  lights: number;
  refineSamples: number;
  refining: boolean;
  refineMs: number;
  sunAltitude: number;
  sunAzimuth: number;
}

export class LightRig {
  private lamps = new LampRig();
  private sunPath = new SunPathOverlay();
  private accum: Accumulator | null = null;
  private live: LiveLight;
  /** A light shown for a moment (the shadow study), instead of the live one. */
  private override: LiveLight | null = null;
  private site: Site = MANILA;
  private north = 0;
  private year = currentYear();
  private frame: LightFrame;
  private frameDirty = true;
  private overrides = new Map<string, boolean>();
  private refineOn: boolean;
  private sunPathOn: boolean;
  private unsubscribe: () => void;
  private disposed = false;

  /** The exposure on screen, where it is heading, and the metered correction. */
  private ev: number;
  private targetEv: number;
  private metered = 0;
  private lastT = 0;

  /** Refine: samples taken, the exposure scale they were taken at, whether it finished. */
  private samples = 0;
  private accScale = 1;
  private refineDone = false;
  private refineStart = 0;
  private refineMs = 0;
  private perFrame = 2;
  private lastRefineAt = 0;
  /** The sun's shadow map was left at a jittered sun and must be redrawn before a live frame. */
  private sunStale = false;
  /** Drawn, as far as the display goes: the exposure moved after the samples were taken. */
  private displayDirty = false;
  /** Metering run for this rest. */
  private metering = false;
  /** Lamp sets whose shaders are compiled (LampRig.configKey). The first frame compiles "off". */
  private compiled = new Set<string>(["off"]);
  /** A lamp set compiling in the background, by key. */
  private prewarming: string | null = null;
  private size = new THREE.Vector2();

  /** The GPU can hold the refine's half float targets (refine.ts, `canAccumulate`). */
  private refineCapable: boolean;

  constructor(private host: LightHost) {
    const v = useViewer.getState();
    this.live = v.light;
    this.refineCapable = canAccumulate(host.renderer);
    this.refineOn = v.refine && this.refineCapable;
    this.sunPathOn = v.sunPath;
    this.frame = lightFrame(this.live, this.site, this.north, this.year);
    this.ev = this.targetEv = this.autoTarget();
    host.scene.add(this.lamps.group, this.sunPath.group);
    this.unsubscribe = useViewer.subscribe((s, p) => {
      if (s.light !== p.light) this.setLive(s.light);
      if (s.refine !== p.refine) this.setRefine(s.refine);
      if (s.sunPath !== p.sunPath) this.setSunPath(s.sunPath);
    });
  }

  // ------------------------------------------------------------------ inputs

  private setLive(light: LiveLight): void {
    const lockChanged = (light.exposureEv ?? null) !== (this.live.exposureEv ?? null);
    const lightChanged = !sameLight(light, this.live);
    this.live = light;
    if (lightChanged) this.frameDirty = true;
    if (lightChanged || lockChanged) {
      // A new light is metered afresh; a locked one is never metered.
      if (lightChanged) this.metered = 0;
      this.retarget(!motionOK());
    }
    this.host.invalidate();
  }

  private setRefine(on: boolean): void {
    this.refineOn = on && this.refineCapable;
    if (!on && this.samples > 0) this.host.invalidate();
    else this.host.requestFrame();
  }

  private setSunPath(on: boolean): void {
    this.sunPathOn = on;
    this.frameDirty = true;
    this.host.invalidate();
  }

  /** The model was rebuilt: its lamps, ghost lights and site. */
  attach(built: BuiltScene | null, doc: DocState | null): void {
    const settings = doc?.project.settings;
    const site = settings?.site ?? MANILA;
    const north = Number.isFinite(settings?.north_angle_deg) ? (settings?.north_angle_deg as number) : 0;
    if (site.latitude_deg !== this.site.latitude_deg || site.longitude_deg !== this.site.longitude_deg || site.utc_offset_min !== this.site.utc_offset_min || north !== this.north) {
      this.site = site;
      this.north = north;
      this.frameDirty = true;
      this.retarget(true);
    }
    const glows = new Map<string, THREE.MeshStandardMaterial>();
    built?.root.traverse((o) => {
      const mesh = o as THREE.Mesh;
      const mat = mesh.isMesh ? (mesh.material as THREE.MeshStandardMaterial) : null;
      const id = mat?.userData?.lampGlow as string | undefined;
      if (mat && id) glows.set(id, mat);
    });
    this.lamps.build(built?.lamps ?? [], built?.lightRooms ?? [], glows);
    // A different set of lamps is a different set of shaders: out of the
    // scene until they are compiled, instead of a stall on the next frame.
    if (this.lamps.visible && !this.compiled.has(this.lamps.configKey(true))) this.lamps.setVisible(false);
    this.frameDirty = true;
    // The walk mode's switches only last while the fixtures they switch exist.
    const ids = new Set((built?.lamps ?? []).map((l) => l.id));
    for (const id of [...this.overrides.keys()]) if (!ids.has(id)) this.overrides.delete(id);
  }

  /**
   * Switches fixtures on or off in the view only, never in the model: walk
   * mode toggles a switch's links with this. `on` null clears the view state
   * so the fixtures follow the model again.
   */
  setFixturesOn(ids: string[], on: boolean | null): void {
    for (const id of ids) {
      if (on === null) this.overrides.delete(id);
      else this.overrides.set(id, on);
    }
    this.frameDirty = true;
    this.host.invalidate();
  }

  /**
   * Walk mode's switch: flips these fixtures in the view, all on when any of
   * them is off, else all off. False when none of them is a lamp in the view.
   */
  toggleLamps(ids: string[]): boolean {
    const known = ids.filter((id) => this.lamps.has(id));
    if (known.length === 0) return false;
    if (this.frameDirty) this.refreshFrame();
    const state = this.lampState(1);
    const anyOff = known.some((id) => !this.lamps.isOn(id, state));
    this.setFixturesOn(known, anyOff);
    return true;
  }

  /** Walk mode's switch states, for a render of this view. */
  fixtureOverrides(): ReadonlyMap<string, boolean> {
    return new Map(this.overrides);
  }

  /** Whether a fixture is lit in the view right now. */
  fixtureOn(id: string): boolean {
    return this.lamps.isOn(id, this.lampState(1));
  }

  // --------------------------------------------------------------- exposure

  private light(): LiveLight {
    return this.override ?? this.live;
  }

  private autoTarget(): number {
    const light = this.light();
    if (light.exposureEv !== null && Number.isFinite(light.exposureEv)) return this.frame.baseEv - light.exposureEv;
    return this.frame.baseEv - this.metered;
  }

  /** Sets where the exposure is heading; `jump` lands it at once. */
  private retarget(jump: boolean): void {
    if (this.frameDirty) this.refreshFrame();
    this.targetEv = this.autoTarget();
    if (jump || !motionOK()) this.ev = this.targetEv;
  }

  private refreshFrame(): void {
    this.frameDirty = false;
    const light = this.light();
    this.frame = lightFrame(light, this.site, this.north, this.year);
    this.host.env.setLight(this.frame);
    // The overlay follows whatever light is shown: in a study frame, that frame's date and time.
    if (this.sunPathOn) {
      const g = this.host.env.groundFrame();
      this.sunPath.update({
        site: this.site,
        year: this.year,
        month: light.month,
        day: light.day,
        minutes: light.minutes,
        northAngleDeg: this.north,
        center: g.center,
        radius: Math.max(g.radius * 1.35, 6),
      });
    }
    this.sunPath.group.visible = this.sunPathOn;
  }

  /** The EV offset a saved view locks: the explicit one, or what auto settled on. */
  lockedEv(): number {
    const e = this.live.exposureEv;
    return e !== null && Number.isFinite(e) ? e : this.metered;
  }

  /** The live light as saved with a view (`Camera::light`). */
  viewLight(): ViewLight {
    return toViewLight(this.live, this.lockedEv());
  }

  private lampState(scale: number): LampState {
    return { lampsLit: this.frame.lampsLit, ghostLit: this.frame.ghostLit, scale, overrides: this.overrides };
  }

  /**
   * Puts every intensity on the scene for an exposure. `still`: a capture or
   * a study frame, which must show the lamps now even if their shaders still
   * have to compile.
   */
  private applyScale(scale: number, still = false): void {
    this.host.env.setExposure(scale);
    const state = this.lampState(scale);
    this.lamps.apply(state);
    this.syncLamps(state, still);
  }

  /**
   * Lamps in or out of the scene. Their number is in every lit shader, so a
   * set that has not been drawn before is compiled in the background first
   * (`WebGLRenderer.compileAsync`, parallel where the browser can) and the
   * lamps come in when it is ready, a moment later, rather than the whole
   * app stalling on a compile.
   */
  private syncLamps(state: LampState, still = false): void {
    const want = this.lamps.wantsVisible(state);
    if (want === this.lamps.visible) return;
    const key = this.lamps.configKey(want);
    if (still) this.compiled.add(key);
    if (!this.compiled.has(key)) {
      this.prewarm(want, key);
      return;
    }
    if (want) this.lamps.assignShadows(this.host.camera, true);
    this.lamps.setVisible(want);
    this.host.shadowDirty();
  }

  private prewarm(visible: boolean, key: string): void {
    if (this.prewarming === key) return;
    this.prewarming = key;
    const r = this.host.renderer;
    const proxy = new THREE.Scene();
    proxy.environment = this.host.scene.environment;
    proxy.fog = this.host.scene.fog;
    const lights = [...this.host.env.proxyLights(), ...(visible ? this.lamps.proxyLights() : [])];
    for (const l of lights) proxy.add(l);
    const done = () => {
      for (const l of lights) l.dispose();
      if (this.disposed || this.prewarming !== key) return;
      this.prewarming = null;
      this.compiled.add(key);
      this.host.invalidate();
    };
    const compile = Promise.all(this.host.compileTargets().map((t) => r.compileAsync(t, this.host.camera, proxy)));
    // A compile that never reports ready (a lost context) must not keep the lamps out for good.
    const timeout = new Promise<void>((res) => setTimeout(res, 5000));
    void Promise.race([compile, timeout]).then(done, done);
  }

  // ------------------------------------------------------------ frame hooks

  /** Right before a live render (and a capture): the light on the scene, refine thrown away. */
  beforeRender(width?: number, height?: number): void {
    if (this.disposed) return;
    if (this.frameDirty) this.retarget(false);
    const r = this.host.renderer;
    // The model changed (the engine asked for a shadow redraw): the lamps'
    // shadow maps follow. A redraw only for the sun's jitter does not.
    if (r.shadowMap.needsUpdate && !this.sunStale) this.lamps.shadowsDirty();
    if (this.sunStale) {
      this.host.env.jitterSun(null);
      r.shadowMap.needsUpdate = true;
      this.sunStale = false;
    }
    this.applyScale(exposureScale(this.ev), width !== undefined);
    this.host.env.prepare(r);
    const size = r.getDrawingBufferSize(this.size);
    this.sunPath.setResolution(width ?? size.x, height ?? size.y);
    if (this.samples > 0 || this.refineDone) this.resetRefine();
    // A live frame is the start of a rest: ask for the frame that refines it.
    if (this.wantsIdle()) this.host.requestFrame();
  }

  private resetRefine(): void {
    this.samples = 0;
    this.refineDone = false;
    this.displayDirty = false;
    this.metering = false;
    this.accum?.reset();
  }

  private wantsIdle(): boolean {
    return (this.refineOn && !this.refineDone) || Math.abs(this.targetEv - this.ev) > 0.005;
  }

  /**
   * One frame of rest: exposure settling, then refine samples. Returns true
   * when it wants another frame.
   */
  idleFrame(now: number): boolean {
    if (this.disposed) return false;
    const dt = this.lastT > 0 ? Math.min((now - this.lastT) / 1000, 0.1) : 1 / 60;
    this.lastT = now;
    let more = false;

    // Exposure settles towards its target.
    const gap = this.targetEv - this.ev;
    if (Math.abs(gap) > 0.005) {
      this.ev = Math.abs(gap) < EXPOSURE_SNAP || !motionOK() ? this.targetEv : this.ev + gap * (1 - Math.exp(-dt / EXPOSURE_TAU));
      more = true;
      if (this.samples === 0) {
        // Nothing refined to show it on: draw the live view at the new exposure.
        this.host.renderLive();
        return true;
      }
      this.displayDirty = true;
    }

    if (this.refineOn && !this.refineDone) {
      this.refineStep(now);
      more = more || !this.refineDone;
    } else if (this.displayDirty && this.samples >= SHOW_AFTER) {
      this.display();
    }
    if (!more) this.lastT = 0;
    return more;
  }

  private refineStep(now: number): void {
    const r = this.host.renderer;
    const size = r.getDrawingBufferSize(this.size);
    if (size.x < 2 || size.y < 2) return;
    this.accum ??= new Accumulator();
    const accum = this.accum;
    if (this.samples === 0) {
      accum.ensure(size.x, size.y);
      accum.reset();
      this.accScale = exposureScale(this.ev);
      this.refineStart = now;
      this.perFrame = Math.max(1, Math.min(this.perFrame, 4));
      // Lamp shadows go to the lamps that matter from here.
      if (this.lamps.visible && this.lamps.assignShadows(this.host.camera)) r.shadowMap.needsUpdate = true;
    } else if (this.lastRefineAt > 0) {
      // Frames coming slower than 45 Hz: the GPU is busy, take fewer samples a frame.
      const interval = now - this.lastRefineAt;
      if (interval > 22) this.perFrame = Math.max(1, this.perFrame - 1);
    }
    this.lastRefineAt = now;
    const env = this.host.env;
    const jitterSun = env.sunCasts;
    const t0 = performance.now();
    let taken = 0;
    while (taken < this.perFrame && accum.samples < REFINE_SAMPLES) {
      if (jitterSun) {
        env.jitterSun(discPoint(accum.samples, REFINE_SAMPLES), SUN_DISC_DEG);
        r.shadowMap.needsUpdate = true;
        this.sunStale = true;
      }
      accum.sample(r, this.host.scene, this.host.camera);
      taken++;
      if (accum.samples === 1 && this.light().exposureEv === null && !this.metering) this.meter();
    }
    if (jitterSun) env.jitterSun(null);
    const cost = (performance.now() - t0) / Math.max(taken, 1);
    // Room for one more next frame if these were cheap.
    if (cost * (this.perFrame + 1) < SAMPLE_BUDGET_MS && this.perFrame < 8) this.perFrame++;
    this.samples = accum.samples;
    if (this.samples >= SHOW_AFTER) this.display();
    if (this.samples >= REFINE_SAMPLES) {
      this.refineDone = true;
      this.refineMs = performance.now() - this.refineStart;
      this.lastRefineAt = 0;
    }
  }

  private display(): void {
    this.displayDirty = false;
    const r = this.host.renderer;
    const prev = r.getRenderTarget();
    r.setRenderTarget(null);
    this.accum?.display(r, exposureScale(this.ev) / this.accScale);
    r.setRenderTarget(prev);
  }

  private meter(): void {
    const accum = this.accum;
    if (!accum) return;
    this.metering = true;
    const scale = this.accScale;
    const ev = this.ev;
    void accum.meter(this.host.renderer).then((log2) => {
      if (this.disposed || log2 === null || this.light().exposureEv !== null) return;
      // The reading was taken at `ev`: where it should have been, relative to base.
      const want = ev + (log2 - METER_KEY) - Math.log2(scale / exposureScale(ev));
      // At dusk and at night the dark is the picture: the meter may darken a
      // view, it barely brightens one (auto exposure that turns night into
      // day is what D5 and Twinmotion users complain about).
      const night = this.frame.daylight < 0.6;
      const offset = clamp(this.frame.baseEv - want, night ? -0.8 : -1, night ? 0.3 : 1);
      if (Math.abs(offset - this.metered) < METER_DEADBAND) return;
      this.metered = offset;
      this.targetEv = this.autoTarget();
      this.host.requestFrame();
    });
  }

  // ---------------------------------------------------------------- stills

  /**
   * Runs `fn` with another light on the scene (the shadow study), exposure
   * locked, then puts the live light back.
   */
  withLight<T>(light: LiveLight, fn: () => T): T {
    const ev = this.ev;
    const target = this.targetEv;
    this.override = light;
    this.frameDirty = true;
    this.refreshFrame();
    this.ev = this.autoTarget();
    this.applyScale(exposureScale(this.ev), true);
    this.host.env.prepare(this.host.renderer);
    this.host.shadowDirty();
    try {
      return fn();
    } finally {
      this.override = null;
      this.frameDirty = true;
      this.refreshFrame();
      this.ev = ev;
      this.targetEv = target;
      this.host.shadowDirty();
      this.host.invalidate();
    }
  }

  /** Lamp lights at their current intensities, for another scene. */
  cloneLamps(): THREE.Light[] {
    return this.lamps.cloneLights();
  }

  /** The light frame on screen. */
  currentFrame(): LightFrame {
    if (this.frameDirty) this.refreshFrame();
    return this.frame;
  }

  /** The exposure on screen, EV100, and the scale it gives. */
  exposure(): { ev: number; scale: number } {
    return { ev: this.ev, scale: exposureScale(this.ev) };
  }

  stats(): LightStats {
    return {
      sky: this.host.env.skyShown,
      ev: Math.round(this.ev * 100) / 100,
      targetEv: Math.round(this.targetEv * 100) / 100,
      baseEv: Math.round(this.frame.baseEv * 100) / 100,
      metered: this.metered,
      lampsLit: this.frame.lampsLit,
      lampsShown: this.lamps.visible,
      compiling: this.prewarming,
      lights: this.lamps.count,
      refineSamples: this.samples,
      refining: this.refineOn && !this.refineDone,
      refineMs: Math.round(this.refineMs),
      sunAltitude: Math.round(this.frame.sun.altitudeDeg * 10) / 10,
      sunAzimuth: Math.round(this.frame.sun.azimuthDeg * 10) / 10,
    };
  }

  /** The WebGL context came back: targets are gone, the sky is baked again. */
  contextRestored(): void {
    this.accum?.dispose();
    this.accum = null;
    this.resetRefine();
    this.frameDirty = true;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.unsubscribe();
    this.accum?.dispose();
    this.accum = null;
    this.lamps.dispose();
    this.sunPath.dispose();
  }
}
