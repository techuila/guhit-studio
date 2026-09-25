// Dev builds only: knobs and readings for measuring a render job
// (render/renderJob.ts). A bench sets `globalThis.__guhitRenderDebug` before a
// render and reads `globalThis.__guhitRenderDebugOut` after it. Every call
// from the job sits behind `import.meta.env.DEV`, so none of this ships.

import * as THREE from "three";
import type { FilterPlan } from "./denoise";
import type { GBuffer } from "./gbuffer";
import type { PaceMode } from "./pacing";
import type { TraceDriver } from "./traceDriver";

/**
 * Dev builds only: knobs and readings for measuring the job
 * (`globalThis.__guhitRenderDebug`, `__guhitRenderDebugOut`).
 */
export interface DevKnobs {
  bounces?: number;
  transmissiveBounces?: number;
  filterGlossyFactor?: number;
  /** Keep the tracer's own tile loop. */
  stockLoop?: boolean;
  /** Pretend the user is always working (short slices) or always away. */
  mode?: PaceMode;
  /** "old": the tracer's colour-only denoiser, "none": no denoising. */
  denoise?: "old" | "none" | "new";
  /** Slice time while the user works, ms. */
  activeSliceMs?: number;
  /** While the user works, the GPU is left alone this many times the slice's own time after each slice. */
  activeGap?: number;
  /** Every lit lamp goes to the tracer (render/lightSelect.ts). */
  keepAllLamps?: boolean;
  /** Read the two half averages back at the end and report the noise. */
  measureNoise?: boolean;
  /** Report the G-buffer as images. */
  dumpGBuffer?: boolean;
  /** Time and sample budget overrides, for reference renders. */
  seconds?: number;
  samples?: number;
  /** Overrides for the denoiser's plan (render/denoise.ts). */
  filter?: Partial<FilterPlan>;
  /** The tracer's random numbers: 0 PCG, 1 Sobol, 2 stratified with a blue noise offset (its default). */
  randomType?: number;
  /** Keep the tracer's area light hit test (render/traceDriver.ts, skipAreaLightHits). */
  areaHits?: boolean;
  /** Report the raw halves, albedo and normal-depth of a region [x, y, w, h] (top-left origin) as base64 floats. */
  dumpRegion?: [number, number, number, number];
}

/** Dev only: raw floats of a region of the render's inputs, for offline checks. */
export async function dumpRegion(r: THREE.WebGLRenderer, driver: TraceDriver, g: GBuffer | null, box: [number, number, number, number]): Promise<Record<string, unknown>> {
  const [x, yTop, w, h] = box;
  const y = driver.height - yTop - h;
  const f32 = async (t: THREE.WebGLRenderTarget) => new Float32Array(await (r.readRenderTargetPixelsAsync(t, x, y, w, h, new Float32Array(w * h * 4)) as Promise<Float32Array>));
  const u16 = async (t: THREE.WebGLRenderTarget) => new Uint16Array(await (r.readRenderTargetPixelsAsync(t, x, y, w, h, new Uint16Array(w * h * 4)) as Promise<Uint16Array>));
  const b64 = (a: ArrayBufferView) => {
    const bytes = new Uint8Array(a.buffer, a.byteOffset, a.byteLength);
    let s = "";
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(s);
  };
  return {
    region: { box, counts: [...driver.counts], weights: driver.weights(), exposure: r.toneMappingExposure },
    regionA: b64(await f32(driver.targets[0])),
    regionB: b64(await f32(driver.targets[1])),
    ...(g ? { regionAlbedo: b64(await u16(g.albedo)), regionND: b64(await f32(g.normalDepth)), regionCover: b64(await u16(g.coverage)) } : {}),
  };
}

/** Dev only: the G-buffer as images, for checking it lines up with the render. */
export async function dumpGBuffer(r: THREE.WebGLRenderer, g: GBuffer): Promise<Record<string, string>> {
  const w = g.albedo.width;
  const h = g.albedo.height;
  const read = (t: THREE.WebGLRenderTarget, type: "half" | "float") =>
    r.readRenderTargetPixelsAsync(t, 0, 0, w, h, type === "half" ? new Uint16Array(w * h * 4) : new Float32Array(w * h * 4)) as Promise<Uint16Array | Float32Array>;
  const [alb, nd] = await Promise.all([read(g.albedo, "half"), read(g.normalDepth, "float")]);
  const toUrl = (px: (i: number) => [number, number, number]) => {
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    const ctx = c.getContext("2d")!;
    const img = ctx.createImageData(w, h);
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        const i = ((h - 1 - y) * w + x) * 4;
        const [R, G, B] = px(i);
        const o = (y * w + x) * 4;
        img.data[o] = R;
        img.data[o + 1] = G;
        img.data[o + 2] = B;
        img.data[o + 3] = 255;
      }
    }
    ctx.putImageData(img, 0, 0);
    return c.toDataURL("image/png");
  };
  const half = (v: number) => THREE.DataUtils.fromHalfFloat(v);
  const srgb = (v: number) => Math.round(255 * Math.min(Math.max(v <= 0.0031308 ? v * 12.92 : 1.055 * Math.pow(v, 1 / 2.4) - 0.055, 0), 1));
  const f = nd as Float32Array;
  const a = alb as Uint16Array;
  return {
    albedo: toUrl((i) => [srgb(half(a[i])), srgb(half(a[i + 1])), srgb(half(a[i + 2]))]),
    normal: toUrl((i) => [Math.round((f[i] * 0.5 + 0.5) * 255), Math.round((f[i + 1] * 0.5 + 0.5) * 255), Math.round((f[i + 2] * 0.5 + 0.5) * 255)]),
    depth: toUrl((i) => {
      const d = f[i + 3];
      const v = d === 0 ? 0 : Math.round(255 * Math.min(Math.abs(d) / 12, 1));
      return d < 0 ? [255, 0, 0] : [v, v, v];
    }),
  };
}

/** Luminance to display value: three's ACES fit on a grey, then sRGB. For dev noise readings. */
function displayOf(lum: number, exposure: number): number {
  const v = (lum * exposure) / 0.6;
  const a = v * (v + 0.0245786) - 0.000090537;
  const b = v * (0.983729 * v + 0.432951) + 0.238081;
  const c = Math.min(Math.max(a / b, 0), 1);
  return c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055;
}

/**
 * Dev only: noise of the render as it stands, from its two half averages.
 * Half their difference, squared, is the variance of the whole average, per
 * pixel; its mean root is the noise in display units (0 to 1).
 */
export async function devNoise(r: THREE.WebGLRenderer, driver: TraceDriver): Promise<Record<string, number>> {
  const w = driver.width;
  const h = driver.height;
  const [a, b] = await Promise.all(
    driver.targets.map((t) => r.readRenderTargetPixelsAsync(t, 0, 0, w, h, new Float32Array(w * h * 4)) as Promise<Float32Array>),
  );
  const [wa, wb] = driver.weights();
  const k = 1 / ((1 / Math.max(driver.counts[0], 1) + 1 / Math.max(driver.counts[1], 1)) * Math.max(driver.samples, 1));
  // Display noise at a fixed exposure, so runs whose metering differs compare.
  const exposure = 1;
  let varSum = 0;
  let varLin = 0;
  let meanLin = 0;
  let meanDisp = 0;
  const lum = (d: Float32Array, i: number) => 0.2126 * d[i] + 0.7152 * d[i + 1] + 0.0722 * d[i + 2];
  for (let i = 0; i < w * h * 4; i += 4) {
    const la = lum(a, i);
    const lb = lum(b, i);
    const l = la * wa + lb * wb;
    const d = displayOf(la, exposure) - displayOf(lb, exposure);
    varSum += d * d * k;
    varLin += (la - lb) * (la - lb) * k;
    meanLin += l;
    meanDisp += displayOf(l, exposure);
  }
  const n = w * h;
  const r4 = (v: number) => Math.round(v * 1e4) / 1e4;
  return {
    noiseRms: r4(Math.sqrt(varSum / n)),
    relNoise: r4(Math.sqrt(varLin / n) / Math.max(meanLin / n, 1e-9)),
    meanDisplay: r4(meanDisp / n),
    meanLinear: r4(meanLin / n),
    metered: r4(r.toneMappingExposure),
  };
}

export function devKnobs(): DevKnobs {
  if (!import.meta.env.DEV) return {};
  return ((globalThis as { __guhitRenderDebug?: DevKnobs }).__guhitRenderDebug ?? {}) as DevKnobs;
}

export function devOut(v: Record<string, unknown>): void {
  if (!import.meta.env.DEV) return;
  const g = globalThis as { __guhitRenderDebugOut?: Record<string, unknown> };
  g.__guhitRenderDebugOut = { ...(g.__guhitRenderDebugOut ?? {}), ...v };
}

/** Slice bookkeeping for the dev readout. */
export class SliceStats {
  bands = 0;
  gpuMs = 0;
  byMode = { active: { n: 0, ms: 0, worst: 0 }, idle: { n: 0, ms: 0, worst: 0 } };
  add(mode: PaceMode, ms: number): void {
    this.bands++;
    this.gpuMs += ms;
    const m = this.byMode[mode];
    m.n++;
    m.ms += ms;
    m.worst = Math.max(m.worst, ms);
  }
  summary(wallMs: number): Record<string, number> {
    const r = (v: number) => Math.round(v * 10) / 10;
    return {
      bands: this.bands,
      busyShare: r((this.gpuMs / Math.max(wallMs, 1)) * 100),
      activeBands: this.byMode.active.n,
      activeMeanMs: r(this.byMode.active.ms / Math.max(this.byMode.active.n, 1)),
      activeWorstMs: r(this.byMode.active.worst),
      idleBands: this.byMode.idle.n,
      idleMeanMs: r(this.byMode.idle.ms / Math.max(this.byMode.idle.n, 1)),
      idleWorstMs: r(this.byMode.idle.worst),
    };
  }
}
