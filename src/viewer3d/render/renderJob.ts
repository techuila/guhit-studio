// One render: a path traced still of one view (DECISIONS D23). The tracer is
// three-gpu-pathtracer 0.0.24 (WebGL 2), loaded on first use into a chunk of
// its own, and it runs in its own WebGLRenderer on a canvas that is never on
// screen, over a scene built from the document by renderScene.ts. The live
// view is never touched.
//
// Scheduling. The job never calls requestAnimationFrame: the live view keeps
// its one pending frame (AGENTS.md, the 3D frame loop invariant). It paces
// itself on the GPU instead (render/pacing.ts): it submits one slice of a
// sample (a band of rows, or part of one), puts a fence behind it and waits
// for the fence between message tasks; the next slice goes in only once the
// last one is done. So at most one slice is ever queued, the live view's
// frames slot in between, and the job stops the moment it is done or
// cancelled. Slices are short while the user works and long while the app is
// left alone.
//
// The tracer gets only the lamps that can light the view, the camera's room
// weighed up (render/lightSelect.ts). Samples go into two half averages
// (render/traceDriver.ts). At the end the denoiser filters the image guided
// by albedo, normals and depth from the raster scene and by the noise the two
// halves show (render/gbuffer.ts, render/denoise.ts).
//
// If the tracer cannot start (its shader does not compile, the context is
// lost, it takes too long to compile) the job falls back to an "Enhanced
// capture": the raster scene refined with jittered samples, labelled so.

import * as THREE from "three";
import { FullScreenQuad } from "three/examples/jsm/postprocessing/Pass.js";
import { TONE_EXPOSURE } from "../engine/environment";
import { renderExposureShift } from "../light/model";
import { Accumulator, discPoint, meterTarget, meterTexture } from "../light/refine";
import type { TraceQuality } from "../../contract/bindings";
import { ACTIVE_REST, gpuFence, InputWatch, nextSlice, previewEvery, restAfter, RowCosts, samplingDone, sleep, sliceTarget, type PaceMode } from "./pacing";
import { buildRenderScene, type RenderScene, type RenderSceneInput } from "./renderScene";
import { canBlendFloat, skipAreaLightHits, TraceDriver, tracerInternals } from "./traceDriver";
import { denoiseToCanvas, filterPlan } from "./denoise";
import { devKnobs, devNoise, devOut, dumpGBuffer, dumpRegion, SliceStats } from "./devProbe";
import { drawGBuffer } from "./gbuffer";

/** Quick or Final (the contract's `TraceQuality`). */
export type { TraceQuality } from "../../contract/bindings";
export type RenderSizeKey = "hd" | "qhd" | "4k" | "square";

export interface RenderSize {
  key: RenderSizeKey;
  label: string;
  width: number;
  height: number;
  note: string;
}

export const RENDER_SIZES: RenderSize[] = [
  { key: "hd", label: "HD", width: 1920, height: 1080, note: "1920 x 1080, screens and messages" },
  { key: "qhd", label: "QHD", width: 2560, height: 1440, note: "2560 x 1440, large screens" },
  { key: "4k", label: "4K", width: 3840, height: 2160, note: "3840 x 2160, sheets and print" },
  { key: "square", label: "Square", width: 2048, height: 2048, note: "2048 x 2048, social posts" },
];

/** Quick aims at a minute at HD, Final at five (the render research, "Render"). */
export const QUALITIES: Record<TraceQuality, { label: string; seconds: number; samples: number; bounces: number }> = {
  quick: { label: "Quick", seconds: 60, samples: 384, bounces: 5 },
  final: { label: "Final", seconds: 300, samples: 2400, bounces: 8 },
};

export type RenderPhase = "preparing" | "compiling" | "rendering" | "finishing" | "enhanced";

export interface RenderProgress {
  phase: RenderPhase;
  samples: number;
  targetSamples: number;
  /** Time spent sampling, not counting the shader compile. */
  elapsedMs: number;
  budgetMs: number;
  /** Estimated time left, or null before there is a rate to go on. */
  etaMs: number | null;
  gpu: string;
  note: string;
}

export interface RenderOutcome {
  png: string;
  kind: "path_traced" | "enhanced";
  samples: number;
  seconds: number;
  width: number;
  height: number;
  gpu: string;
  /** Why the path tracer did not run, for an Enhanced capture. */
  fallbackReason?: string;
}

/** three-gpu-pathtracer's RANDOM_TYPE for PCG random numbers. */
const RANDOM_PCG = 0;
/** The tracer's shader can take up to half a minute to compile on Windows. Past this, fall back. */
const COMPILE_LIMIT_MS = 120_000;
const ENHANCED_SAMPLES = 96;
const PREVIEW_EVERY_MS = 450;
const PREVIEW_MAX_W = 560;

type Tracer = import("three-gpu-pathtracer").WebGLPathTracer;
type TracerModule = typeof import("three-gpu-pathtracer");

/** What the denoiser reads: the two half averages, or one average twice (render/denoise.ts). */
interface DenoiseSource {
  a: THREE.Texture;
  b: THREE.Texture;
  wA: number;
  wB: number;
  nA: number;
  nB: number;
  /** The whole average as one linear target, for the dev comparisons. */
  combined: () => THREE.WebGLRenderTarget;
  /** The band driver, for dev readings. */
  driver?: TraceDriver;
  release: (t: THREE.WebGLRenderTarget) => void;
}

/** Unmasked GPU name, when the browser tells ("Apple M5", "NVIDIA GeForce RTX 3050 Laptop GPU"). */
export function gpuName(gl: WebGL2RenderingContext | WebGLRenderingContext): string {
  try {
    const ext = gl.getExtension("WEBGL_debug_renderer_info");
    const raw = ext ? String(gl.getParameter(ext.UNMASKED_RENDERER_WEBGL)) : String(gl.getParameter(gl.RENDERER));
    // "ANGLE (Apple, ANGLE Metal Renderer: Apple M5, Unspecified Version)" reads as "Apple M5".
    const inner = raw.replace(/^ANGLE \((.*)\)$/, "$1");
    const parts = inner.split(", ");
    const pick = (parts.length > 1 ? parts[1] : parts[0])
      .replace(/ANGLE Metal Renderer: /, "")
      .replace(/\(0x[0-9a-f]+\)/i, "")
      .replace(/Direct3D.*$/, "")
      .replace(/vs_\d_\d ps_\d_\d.*$/, "")
      .trim();
    return pick || raw;
  } catch {
    return "GPU";
  }
}

export class RenderJob {
  private request: "run" | "save" | "cancel" = "run";
  private canvas: HTMLCanvasElement;
  private renderer: THREE.WebGLRenderer | null = null;
  private gpu = "GPU";
  private contextLost = false;
  private shaderError = false;
  /** A small copy of the image as it refines, for the Visuals panel. */
  readonly preview: HTMLCanvasElement;
  private lastPreview = 0;
  private lastProgress = 0;
  private watch: InputWatch | null = null;

  constructor(
    private input: RenderSceneInput,
    private quality: TraceQuality,
    private onProgress: (p: RenderProgress) => void,
    private onPreview: () => void,
  ) {
    this.canvas = document.createElement("canvas");
    this.canvas.width = input.width;
    this.canvas.height = input.height;
    this.preview = document.createElement("canvas");
    const pw = Math.min(PREVIEW_MAX_W, input.width);
    this.preview.width = pw;
    this.preview.height = Math.max(1, Math.round((pw * input.height) / input.width));
  }

  /** Stops early: `save` keeps what is there, otherwise nothing is saved. */
  stop(save: boolean): void {
    if (this.request === "run") this.request = save ? "save" : "cancel";
  }

  get cancelled(): boolean {
    return this.request === "cancel";
  }

  /** Stop and save asked for. A getter, so a check after an await reads the current value. */
  private get saving(): boolean {
    return this.request === "save";
  }

  private makeRenderer(): THREE.WebGLRenderer {
    const r = new THREE.WebGLRenderer({
      canvas: this.canvas,
      antialias: false,
      alpha: false,
      preserveDrawingBuffer: true,
      powerPreference: "high-performance",
    });
    if (!r.capabilities.isWebGL2) {
      r.dispose();
      throw new Error("This computer's graphics do not support WebGL 2, which rendering needs.");
    }
    r.setPixelRatio(1);
    r.setSize(this.input.width, this.input.height, false);
    r.outputColorSpace = THREE.SRGBColorSpace;
    r.toneMapping = THREE.ACESFilmicToneMapping;
    r.toneMappingExposure = TONE_EXPOSURE;
    r.shadowMap.enabled = true;
    r.shadowMap.type = THREE.PCFShadowMap;
    r.shadowMap.autoUpdate = false;
    r.shadowMap.needsUpdate = true;
    r.debug.onShaderError = () => {
      this.shaderError = true;
    };
    this.canvas.addEventListener("webglcontextlost", this.onLost, false);
    this.gpu = gpuName(r.getContext());
    return r;
  }

  private onLost = (e: Event): void => {
    e.preventDefault();
    this.contextLost = true;
  };

  private progress(p: Omit<RenderProgress, "gpu">, force = false): void {
    const now = performance.now();
    if (!force && now - this.lastProgress < 200) return;
    this.lastProgress = now;
    this.onProgress({ ...p, gpu: this.gpu });
  }

  /** Waits until the GPU has done everything submitted so far, without blocking the page (render/pacing.ts). */
  private gpuDone(r: THREE.WebGLRenderer, predictedMs = 0): Promise<{ ms: number; cost: number }> {
    return gpuFence(r.getContext() as WebGL2RenderingContext, predictedMs, this.watch?.hidden ?? false, () => this.contextLost);
  }

  /** The pace the user allows right now (render/pacing.ts). */
  private mode(): PaceMode {
    return devKnobs().mode ?? this.watch?.mode() ?? "active";
  }

  /** Copies the canvas into the preview, at most every PREVIEW_EVERY_MS. */
  private snapshot(force = false): void {
    const now = performance.now();
    if (!force && now - this.lastPreview < PREVIEW_EVERY_MS) return;
    this.lastPreview = now;
    const ctx = this.preview.getContext("2d");
    if (!ctx) return;
    ctx.drawImage(this.canvas, 0, 0, this.preview.width, this.preview.height);
    this.onPreview();
  }

  async run(): Promise<RenderOutcome | null> {
    this.watch = new InputWatch();
    try {
      this.progress({ phase: "preparing", samples: 0, targetSamples: 0, elapsedMs: 0, budgetMs: 0, etaMs: null, note: "Loading the path tracer" }, true);
      const outcome = await this.trace();
      if (outcome !== "fallback") return outcome;
      return await this.enhanced(this.fallbackReason);
    } finally {
      this.watch.dispose();
      this.watch = null;
      this.canvas.removeEventListener("webglcontextlost", this.onLost, false);
      this.renderer?.dispose();
      this.renderer?.forceContextLoss();
      this.renderer = null;
    }
  }

  private fallbackReason = "";

  /** The path traced render. "fallback" when the tracer cannot run here. */
  private async trace(): Promise<RenderOutcome | null | "fallback"> {
    // Dev builds can force the fallback to check it (`__guhitForceEnhanced`).
    if (import.meta.env.DEV && (globalThis as { __guhitForceEnhanced?: boolean }).__guhitForceEnhanced) {
      this.fallbackReason = "The dev harness asked for the fallback.";
      return "fallback";
    }
    let mod: TracerModule;
    try {
      mod = await import("three-gpu-pathtracer");
    } catch {
      this.fallbackReason = "The path tracer could not be loaded.";
      return "fallback";
    }
    if (this.cancelled) return null;
    let r: THREE.WebGLRenderer;
    try {
      r = this.renderer = this.makeRenderer();
    } catch (e) {
      this.fallbackReason = e instanceof Error ? e.message : String(e);
      return "fallback";
    }
    let rs: RenderScene | null = null;
    let tracer: Tracer | null = null;
    try {
      const knobs = devKnobs();
      rs = await buildRenderScene(r, { ...this.input, keepAllLamps: knobs.keepAllLamps }, true);
      if (this.cancelled) return null;
      const q = QUALITIES[this.quality];
      devOut({ lamps: rs.lampCount });
      tracer = new mod.WebGLPathTracer(r);
      tracer.renderToCanvas = false;
      tracer.rasterizeScene = false;
      tracer.dynamicLowRes = false;
      tracer.renderDelay = 0;
      tracer.minSamples = 1;
      tracer.fadeDuration = 0;
      tracer.bounces = knobs.bounces ?? q.bounces;
      tracer.transmissiveBounces = knobs.transmissiveBounces ?? 4;
      tracer.filterGlossyFactor = knobs.filterGlossyFactor ?? 0.5;
      tracer.multipleImportanceSampling = true;
      tracer.synchronizeRenderSize = true;
      tracer.renderScale = 1;
      tracer.tiles.set(2, 2);
      // Independent random numbers per pixel and sample (PCG). The tracer's
      // default, one stratified set shared by every pixel with a fixed blue
      // noise offset per pixel, leaves the two half averages' errors
      // correlated (0.37 measured on a night ceiling): the denoiser would see
      // half the noise there is and keep the rest as detail. Measured in the
      // same spot, PCG is as clean per sample.
      const pt = tracerInternals(tracer);
      // Straight into the defines, before the first compile (TraceDriver.prepare).
      if (pt) pt.material.defines.RANDOM_TYPE = knobs.randomType ?? RANDOM_PCG;
      const areaHitsSkipped = pt && !knobs.areaHits ? skipAreaLightHits(pt, rs.scene) : false;
      devOut({ areaHitsSkipped });
      this.progress({ phase: "compiling", samples: 0, targetSamples: q.samples, elapsedMs: 0, budgetMs: q.seconds * 1000, etaMs: null, note: "Building the scene for the path tracer" }, true);
      await sleep(0);
      tracer.setScene(rs.scene, rs.camera);
      const driver = pt && !knobs.stockLoop && canBlendFloat(r) ? new TraceDriver(r, pt, this.input.width, this.input.height) : null;
      try {
        return driver ? await this.bandLoop(r, rs, tracer, mod, driver) : await this.tileLoop(r, rs, tracer, mod);
      } finally {
        driver?.dispose();
      }
    } catch (e) {
      console.warn("render: the path tracer failed, falling back", e);
      this.fallbackReason = "The path tracer could not start on this computer's graphics.";
      return "fallback";
    } finally {
      tracer?.dispose();
      rs?.dispose();
    }
  }

  /**
   * Waits for the tracer's shader, which compiles in parallel where the
   * browser can. "fallback" when it fails or takes too long, null when the
   * render was cancelled meanwhile.
   */
  private async compiled(isCompiling: () => boolean, kick: () => void, targetSamples: number, budgetMs: number): Promise<true | null | "fallback"> {
    const compileStart = performance.now();
    while (isCompiling()) {
      if (this.cancelled) return null;
      if (this.shaderError || this.contextLost) {
        this.fallbackReason = "The path tracer's shader did not compile on this computer's graphics.";
        return "fallback";
      }
      if (performance.now() - compileStart > COMPILE_LIMIT_MS) {
        this.fallbackReason = "The path tracer took too long to prepare on this computer's graphics.";
        return "fallback";
      }
      this.progress({
        phase: "compiling",
        samples: 0,
        targetSamples,
        elapsedMs: 0,
        budgetMs,
        etaMs: null,
        note: "Preparing the path tracer. The first time can take up to a minute.",
      });
      kick();
      await sleep(60);
    }
    if (this.shaderError) {
      this.fallbackReason = "The path tracer's shader did not compile on this computer's graphics.";
      return "fallback";
    }
    return true;
  }

  /** The quality's time and sample budget. */
  private budget(): { seconds: number; samples: number } {
    const q = QUALITIES[this.quality];
    const knobs = devKnobs();
    return { seconds: knobs.seconds ?? q.seconds, samples: knobs.samples ?? q.samples };
  }

  /** Progress in whole samples and time, and the ETA from the rate so far. */
  private sampleProgress(whole: number, t0: number, targetSamples: number, budgetMs: number, force = false): void {
    const now = performance.now();
    const rate = whole / Math.max((now - t0) / 1000, 0.001);
    const leftBySamples = rate > 0 ? ((targetSamples - whole) / rate) * 1000 : Infinity;
    const leftByTime = budgetMs - (now - t0);
    this.progress({
      phase: "rendering",
      samples: whole,
      targetSamples,
      elapsedMs: now - t0,
      budgetMs,
      etaMs: whole >= 2 ? Math.max(0, Math.min(leftBySamples, leftByTime)) : null,
      note: "",
    }, force);
  }

  /** Meters the render's own image once it has a few samples, and again later (light/model.ts, renderExposureShift). */
  private meter(r: THREE.WebGLRenderer, rs: RenderScene) {
    const state = { target: meterTarget(), pending: false, at: [6, 24] };
    return {
      maybe: (whole: number, texture: THREE.Texture) => {
        if (state.at.length === 0 || whole < state.at[0] || state.pending) return;
        state.at.shift();
        state.pending = true;
        void meterTexture(r, texture, state.target, undefined, "traced")
          .then((log2) => {
            if (log2 === null || this.cancelled) return;
            r.toneMappingExposure = TONE_EXPOSURE * Math.pow(2, renderExposureShift(log2, rs.frame.daylight));
          })
          .catch(() => undefined)
          .finally(() => {
            state.pending = false;
          });
      },
      dispose: () => state.target.dispose(),
    };
  }

  /** The render: bands of rows into two half averages (render/traceDriver.ts). */
  private async bandLoop(
    r: THREE.WebGLRenderer,
    rs: RenderScene,
    tracer: Tracer,
    mod: TracerModule,
    driver: TraceDriver,
  ): Promise<RenderOutcome | null | "fallback"> {
    const q = this.budget();
    const budgetMs = q.seconds * 1000;
    driver.prepare();
    const ready = await this.compiled(() => driver.compiling, () => undefined, q.samples, budgetMs);
    if (ready !== true) return ready;

    const knobs = devKnobs();
    const lights = this.tracerLights(rs.scene, tracer);
    const jitterSun = rs.env.sun.intensity > 0 && rs.env.sunDir.y > 0;
    if (import.meta.env.DEV) (globalThis as { __guhitRenderLive?: unknown }).__guhitRenderLive = { r, driver, tracer, rs };
    const costs = new RowCosts(driver.height);
    let warm = false;
    const stats = new SliceStats();
    const meter = this.meter(r, rs);
    let t0 = performance.now();
    // The shader is ready: tracing starts now, even if the first sample takes a while.
    this.sampleProgress(0, t0, q.samples, budgetMs, true);
    try {
      for (;;) {
        if (this.cancelled) return null;
        if (this.contextLost) {
          this.fallbackReason = "The graphics context was lost while rendering.";
          return "fallback";
        }
        // Dev only: a paused job waits with its clock stopped.
        if (import.meta.env.DEV && (globalThis as { __guhitRenderPause?: boolean }).__guhitRenderPause) {
          await sleep(50);
          t0 += 50;
          continue;
        }
        const elapsed = performance.now() - t0;
        if (samplingDone({ saving: this.saving, samples: driver.samples, targetSamples: q.samples, elapsedMs: elapsed, budgetMs })) break;

        const mode = this.mode();
        const at = driver.at;
        const target = mode === "active" && knobs.activeSliceMs ? knobs.activeSliceMs : sliceTarget(mode);
        const slice = nextSlice(costs, at, driver.width, mode, target);
        // Nap only on a prediction from measured rows (render/pacing.ts).
        const predicted = costs.known(at.row, slice.rows) ? slice.predictedMs : 0;
        const done = driver.trace(slice.rows, slice.cols);
        if (done.rows === 0) {
          // The shader is compiling again (a define changed): wait for it.
          await sleep(30);
          continue;
        }
        const wait = await this.gpuDone(r, predicted);
        // The first slice carries one-off costs (uploads, pipeline setup): not learned.
        if (warm) costs.record(done.row, done.rows, wait.cost, done.share);
        warm = true;
        stats.add(mode, wait.ms);
        const rest = restAfter(mode, wait.ms, knobs.activeGap ?? ACTIVE_REST);
        if (rest > 0) await sleep(rest);
        if (!done.done) {
          // The time bar moves between samples too, twice a second.
          if (performance.now() - this.lastProgress >= 500) this.sampleProgress(driver.samples, t0, q.samples, budgetMs);
          continue;
        }

        const whole = driver.samples;
        meter.maybe(whole, driver.targets[0].texture);
        // Soft sun shadows: every sample sees the sun from a point on its disc.
        if (jitterSun && lights) {
          rs.env.jitterSun(discPoint(whole % 256, 256), 0.35);
          lights.update();
        }
        this.sampleProgress(whole, t0, q.samples, budgetMs);
        if (performance.now() - this.lastPreview >= previewEvery(mode)) {
          driver.resolve(null);
          this.snapshot(true);
        }
      }
      const samples = driver.samples;
      const wallMs = performance.now() - t0;
      devOut({ slices: stats.summary(wallMs), samples, sampleMs: Math.round(costs.sample * 10) / 10, seconds: wallMs / 1000 });
      if (import.meta.env.DEV && knobs.measureNoise) devOut({ noise: await devNoise(r, driver) });
      if (import.meta.env.DEV && knobs.dumpRegion && (knobs.denoise === "none" || knobs.denoise === "old")) devOut(await dumpRegion(r, driver, null, knobs.dumpRegion));
      if (samples < 1) return null;
      this.progress({ phase: "finishing", samples, targetSamples: q.samples, elapsedMs: wallMs, budgetMs, etaMs: 0, note: "Removing noise" }, true);
      await sleep(0);
      const [wA, wB] = driver.weights();
      const source: DenoiseSource = {
        a: driver.targets[0].texture,
        b: driver.targets[1].texture,
        wA,
        wB,
        nA: driver.counts[0],
        nB: driver.counts[1],
        combined: () => {
          const t = new THREE.WebGLRenderTarget(driver.width, driver.height, { type: THREE.FloatType, minFilter: THREE.NearestFilter, magFilter: THREE.NearestFilter, depthBuffer: false });
          driver.resolve(t);
          return t;
        },
        release: (t) => t.dispose(),
        driver,
      };
      if (!(await this.finish(r, rs, mod, source, samples))) return this.cancelled ? null : "fallback";
      const png = this.canvas.toDataURL("image/png");
      this.snapshot(true);
      return {
        png,
        kind: "path_traced",
        samples,
        seconds: Math.round(wallMs / 100) / 10,
        width: this.input.width,
        height: this.input.height,
        gpu: this.gpu,
      };
    } finally {
      meter.dispose();
    }
  }

  /**
   * Denoises the render onto the canvas, tone mapped (render/denoise.ts),
   * guided by the raster scene's albedo, normals and depth
   * (render/gbuffer.ts). Every pass is paced like a slice. False when the
   * render was cancelled meanwhile.
   */
  private async finish(r: THREE.WebGLRenderer, rs: RenderScene, mod: TracerModule, src: DenoiseSource, samples: number): Promise<boolean> {
    const knobs = devKnobs();
    const pace = async () => {
      const wait = await this.gpuDone(r);
      const rest = restAfter(this.mode(), wait.ms);
      if (rest > 0) await sleep(rest);
    };
    if (knobs.denoise !== "none" && knobs.denoise !== "old") {
      try {
        const done = await this.denoise(r, rs, src, samples, pace);
        if (done !== "failed") return done;
      } catch (e) {
        console.warn("render: the denoiser failed, keeping the render with light denoising", e);
      }
      if (this.cancelled) return false;
    }
    // The plain average, or the tracer's colour-only denoiser.
    const combined = src.combined();
    try {
      if (knobs.denoise === "none" || !mod.DenoiseMaterial) {
        const display = new FullScreenQuad(new THREE.MeshBasicMaterial({ depthTest: false, depthWrite: false }));
        this.show(r, display, combined.texture);
        display.material.dispose();
        display.dispose();
      } else {
        const denoise = new mod.DenoiseMaterial({
          map: combined.texture,
          sigma: samples < 64 ? 4 : samples < 256 ? 3 : 2,
          kSigma: 1.4,
          threshold: samples < 64 ? 0.12 : 0.07,
        });
        const quad = new FullScreenQuad(denoise);
        r.setRenderTarget(null);
        quad.render(r);
        quad.dispose();
        denoise.dispose();
      }
      await pace();
    } finally {
      src.release(combined);
    }
    return !this.cancelled;
  }

  /**
   * The guided filter (render/gbuffer.ts, render/denoise.ts). "failed" when
   * a shader did not compile here: the caller keeps the render with the
   * tracer's own light denoising instead of losing it.
   */
  private async denoise(r: THREE.WebGLRenderer, rs: RenderScene, src: DenoiseSource, samples: number, pace: () => Promise<void>): Promise<boolean | "failed"> {
    const driver = src.driver;
    const { width, height } = this.input;
    const t0 = performance.now();
    const gbuffer = await drawGBuffer(r, rs.scene, rs.camera, width, height, pace, () => this.cancelled, { ...filterPlan(samples, height), ...devKnobs().filter }.planeShare);
    if (!gbuffer) return false;
    const t1 = performance.now();
    try {
      if (this.shaderError) return "failed";
      const ok = await denoiseToCanvas(
        r,
        { a: src.a, b: src.b, wA: src.wA, wB: src.wB, nA: src.nA, nB: src.nB, gbuffer, camera: rs.camera, samples, plan: devKnobs().filter },
        width,
        height,
        pace,
        () => this.cancelled,
      );
      if (this.shaderError) return "failed";
      if (ok) await this.gpuDone(r);
      if (import.meta.env.DEV) {
        devOut({ finishMs: { gbuffer: Math.round(t1 - t0), filter: Math.round(performance.now() - t1) } });
        if (devKnobs().dumpGBuffer) devOut(await dumpGBuffer(r, gbuffer));
        const box = devKnobs().dumpRegion;
        if (box && driver) devOut(await dumpRegion(r, driver, gbuffer, box));
      }
      return ok && !this.cancelled;
    } finally {
      gbuffer.dispose();
    }
  }

  /**
   * The tracer's own loop: a grid of tiles per sample, for a tracer or a GPU
   * the band driver cannot use (render/traceDriver.ts). Tiles follow the same
   * pace as bands, but can only change between samples.
   */
  private async tileLoop(r: THREE.WebGLRenderer, rs: RenderScene, tracer: Tracer, mod: TracerModule): Promise<RenderOutcome | null | "fallback"> {
    const q = this.budget();
    const budgetMs = q.seconds * 1000;
    const isCompiling = () => Boolean((tracer as unknown as { isCompiling?: boolean }).isCompiling);
    // A sample call is what starts the compile; it draws nothing until it is done.
    const ready = await this.compiled(isCompiling, () => tracer.renderSample(), q.samples, budgetMs);
    if (ready !== true) return ready;

    const lights = this.tracerLights(rs.scene, tracer);
    const jitterSun = rs.env.sun.intensity > 0 && rs.env.sunDir.y > 0;
    const t0 = performance.now();
    this.sampleProgress(0, t0, q.samples, budgetMs, true);
    let lastWhole = 0;
    let tiles = 2;
    let tileMs = 0;
    const display = new FullScreenQuad(new THREE.MeshBasicMaterial({ depthTest: false, depthWrite: false }));
    const meter = this.meter(r, rs);
    try {
      for (;;) {
        if (this.cancelled) return null;
        if (this.contextLost) {
          this.fallbackReason = "The graphics context was lost while rendering.";
          return "fallback";
        }
        const elapsed = performance.now() - t0;
        if (samplingDone({ saving: this.saving, samples: Math.floor(tracer.samples), targetSamples: q.samples, elapsedMs: elapsed, budgetMs })) break;

        tracer.renderSample();
        tileMs = (await this.gpuDone(r, tileMs)).cost;
        const whole = Math.floor(tracer.samples);
        if (whole !== lastWhole) {
          lastWhole = whole;
          // Tiles near the slice time the pace allows: split finer when slow.
          const target = sliceTarget(this.mode());
          const sampleMs = tileMs * tiles * tiles;
          tiles = Math.min(16, Math.max(1, Math.ceil(Math.sqrt(sampleMs / target))));
          if (tracer.tiles.x !== tiles) tracer.tiles.set(tiles, tiles);
          meter.maybe(whole, tracer.target.texture);
          if (jitterSun && lights) {
            rs.env.jitterSun(discPoint(whole % 256, 256), 0.35);
            lights.update();
          }
          this.sampleProgress(whole, t0, q.samples, budgetMs);
          if (performance.now() - this.lastPreview >= PREVIEW_EVERY_MS) {
            this.show(r, display, tracer.target.texture);
            this.snapshot();
          }
        }
      }
      const samples = Math.floor(tracer.samples);
      if (samples < 1) return null;
      this.progress({ phase: "finishing", samples, targetSamples: q.samples, elapsedMs: performance.now() - t0, budgetMs, etaMs: 0, note: "Removing noise" }, true);
      await sleep(0);
      const target = tracer.target;
      const source: DenoiseSource = { a: target.texture, b: target.texture, wA: 1, wB: 0, nA: samples, nB: 0, combined: () => target, release: () => undefined };
      if (!(await this.finish(r, rs, mod, source, samples))) return this.cancelled ? null : "fallback";
      const png = this.canvas.toDataURL("image/png");
      this.snapshot(true);
      return {
        png,
        kind: "path_traced",
        samples,
        seconds: Math.round((performance.now() - t0) / 100) / 10,
        width: this.input.width,
        height: this.input.height,
        gpu: this.gpu,
      };
    } finally {
      display.material.dispose();
      display.dispose();
      meter.dispose();
    }
  }

  /** Draws a float target onto the canvas, tone mapped. */
  private show(r: THREE.WebGLRenderer, quad: FullScreenQuad, texture: THREE.Texture): void {
    const mat = quad.material as THREE.MeshBasicMaterial;
    if (mat.map !== texture) {
      mat.map = texture;
      mat.needsUpdate = true;
    }
    r.setRenderTarget(null);
    quad.render(r);
  }

  /**
   * Moves the sun between samples without restarting the render: rewrites
   * the tracer's light table in place (three-gpu-pathtracer 0.0.24
   * internals; the version is pinned). Null when that is not possible, and the
   * sun then stays sharp.
   */
  private tracerLights(scene: THREE.Scene, tracer: Tracer): { update: () => void } | null {
    type LightTable = { updateFrom?: (lights: THREE.Light[], ies: THREE.Texture[]) => unknown };
    const table = (tracer as unknown as { _pathTracer?: { material?: { lights?: LightTable } } })._pathTracer?.material?.lights;
    if (!table || typeof table.updateFrom !== "function") return null;
    const update = table.updateFrom.bind(table);
    return {
      update: () => {
        const lights: THREE.Light[] = [];
        scene.traverse((c) => {
          const l = c as THREE.Light & { isRectAreaLight?: boolean; isSpotLight?: boolean; isPointLight?: boolean; isDirectionalLight?: boolean };
          if (c.visible && (l.isRectAreaLight || l.isSpotLight || l.isPointLight || l.isDirectionalLight)) lights.push(l);
        });
        // The tracer's own order (sceneUpdateUtils.getLights): by uuid, descending.
        lights.sort((a, b) => (a.uuid < b.uuid ? 1 : a.uuid > b.uuid ? -1 : 0));
        try {
          update(lights, []);
        } catch {
          /* the sun stays where it was */
        }
      },
    };
  }

  /** Enhanced capture: the raster scene, refined with jittered samples. */
  private async enhanced(reason: string): Promise<RenderOutcome | null> {
    if (this.cancelled) return null;
    this.renderer?.dispose();
    this.renderer?.forceContextLoss();
    this.renderer = null;
    this.contextLost = false;
    this.shaderError = false;
    // A lost context may take the canvas with it: draw on a fresh one.
    const fresh = document.createElement("canvas");
    fresh.width = this.input.width;
    fresh.height = this.input.height;
    this.canvas.removeEventListener("webglcontextlost", this.onLost, false);
    this.canvas = fresh;
    const r = (this.renderer = this.makeRenderer());
    const rs = await buildRenderScene(r, this.input, false);
    const accum = new Accumulator(THREE.HalfFloatType);
    const t0 = performance.now();
    try {
      accum.ensure(this.input.width, this.input.height);
      const jitterSun = rs.env.sunCasts;
      let perTick = 2;
      while (accum.samples < ENHANCED_SAMPLES) {
        if (this.cancelled) return null;
        if (this.saving && accum.samples > 0) break;
        if (this.contextLost) throw new Error("The graphics context was lost while rendering.");
        const tickStart = performance.now();
        for (let i = 0; i < perTick && accum.samples < ENHANCED_SAMPLES; i++) {
          if (jitterSun) {
            rs.env.jitterSun(discPoint(accum.samples, ENHANCED_SAMPLES), 0.55);
            r.shadowMap.needsUpdate = true;
          }
          accum.sample(r, rs.scene, rs.camera);
        }
        const gpuMs = (await this.gpuDone(r)).ms;
        const cost = performance.now() - tickStart + gpuMs;
        if (cost > 30 && perTick > 1) perTick--;
        else if (cost < 12 && perTick < 8) perTick++;
        r.setRenderTarget(null);
        accum.display(r);
        this.snapshot();
        const rate = accum.samples / Math.max((performance.now() - t0) / 1000, 0.001);
        this.progress({
          phase: "enhanced",
          samples: accum.samples,
          targetSamples: ENHANCED_SAMPLES,
          elapsedMs: performance.now() - t0,
          budgetMs: 0,
          etaMs: rate > 0 ? ((ENHANCED_SAMPLES - accum.samples) / rate) * 1000 : null,
          note: `${reason} Making an Enhanced capture instead.`,
        });
      }
      rs.env.jitterSun(null);
      r.setRenderTarget(null);
      accum.display(r);
      await this.gpuDone(r);
      const png = this.canvas.toDataURL("image/png");
      this.snapshot(true);
      return {
        png,
        kind: "enhanced",
        samples: accum.samples,
        seconds: Math.round((performance.now() - t0) / 100) / 10,
        width: this.input.width,
        height: this.input.height,
        gpu: this.gpu,
        fallbackReason: reason,
      };
    } finally {
      accum.dispose();
      rs.dispose();
    }
  }
}
