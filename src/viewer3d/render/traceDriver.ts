// Drives three-gpu-pathtracer 0.0.24 one band of rows at a time, into two
// float targets of its own: even samples into one, odd samples into the
// other. Two half averages of the same picture give the denoiser a true
// per-pixel noise estimate (half their difference, squared, render/denoise.ts)
// instead of a guess from neighbours, which would also count real detail as
// noise.
//
// The tracer's own loop (PathTracingRenderer's `renderTask`) cuts a sample
// into a grid of tiles fixed for the whole sample. A band of rows can end
// anywhere, so the job can shorten the very next slice the moment the user
// touches the app (render/pacing.ts). This file repeats what `renderTask` does
// per sample and per tile, on the tracer's material and quad; the version is
// pinned, and `tracerInternals` checks every field it relies on. When any is
// missing, or the GPU cannot blend float targets, the job keeps the tracer's
// own loop (renderJob.ts).

import * as THREE from "three";
import { FullScreenQuad } from "three/examples/jsm/postprocessing/Pass.js";

interface StratifiedTexture {
  init(count: number, depth: number): void;
  next(): void;
}

/** The tracer's material, as `renderTask` uses it. Uniforms are properties (MaterialBase). */
export type TracerMaterial = THREE.ShaderMaterial & {
  opacity: number;
  seed: number;
  bounces: number;
  transmissiveBounces: number;
  resolution: THREE.Vector2;
  sobolTexture: THREE.Texture | null;
  stratifiedTexture: StratifiedTexture;
  onBeforeRender(): void;
};

/** The parts of the tracer's PathTracingRenderer this file drives. */
export interface TracerInternals {
  material: TracerMaterial;
  _fsQuad: FullScreenQuad;
  _sobolTarget: THREE.WebGLRenderTarget;
  readonly isCompiling: boolean;
}

/** The tracer's inner renderer when every part the driver needs is there, else null. */
export function tracerInternals(tracer: unknown): TracerInternals | null {
  const pt = (tracer as { _pathTracer?: Partial<TracerInternals> } | null)?._pathTracer;
  if (!pt || !pt.material || !pt._fsQuad || !pt._sobolTarget) return null;
  const m = pt.material as Partial<TracerMaterial>;
  if (typeof m.onBeforeRender !== "function" || !(m.resolution instanceof THREE.Vector2)) return null;
  if (!m.stratifiedTexture || typeof m.stratifiedTexture.init !== "function" || typeof m.stratifiedTexture.next !== "function") return null;
  if (!("seed" in m) || !("opacity" in m) || !("sobolTexture" in m)) return null;
  if (typeof (pt._fsQuad as { render?: unknown }).render !== "function") return null;
  return pt as TracerInternals;
}

/**
 * At every bounce the tracer tests the ray against every light, for rays
 * that hit an area light's surface (rectangle and disc lights). A render has
 * only spot, point and sun lights, which that test can never hit, yet it
 * reads six texels per light per bounce: with the lamp copies of
 * render/lightSelect.ts about a tenth of the sample. This compiles the test
 * out, before the shader's first compile. Changes nothing drawn. False, and
 * the shader untouched, when the scene has an area light or the tracer's
 * shader is not the one this was written against.
 */
export function skipAreaLightHits(pt: TracerInternals, scene: THREE.Scene): boolean {
  let area = false;
  scene.traverse((o) => {
    if ((o as THREE.RectAreaLight).isRectAreaLight) area = true;
  });
  if (area) return false;
  const m = pt.material;
  const next = m.fragmentShader.replace(AREA_HIT_TEST, "if ( AREA_LIGHT_HITS == 1 && ! state.firstRay && ! state.transmissiveRay ) {$1");
  if (next === m.fragmentShader) return false;
  m.fragmentShader = next;
  // Straight into the defines: `setDefine` would start a compile right away,
  // for the wrong target (TraceDriver.prepare compiles for the right one).
  m.defines.AREA_LIGHT_HITS = 0;
  return true;
}

const AREA_HIT_TEST = /if\s*\(\s*!\s*state\.firstRay\s*&&\s*!\s*state\.transmissiveRay\s*\)\s*\{(\s*LightRecord lightRec;\s*float lightDist)/;

/** True when this renderer can blend into float targets, which the running average needs. */
export function canBlendFloat(renderer: THREE.WebGLRenderer): boolean {
  return renderer.extensions.has("EXT_float_blend") && renderer.extensions.has("EXT_color_buffer_float");
}

const VERT = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`;

/** The two half averages as one picture, tone mapped for the canvas. */
const RESOLVE_FRAG = /* glsl */ `
  uniform sampler2D tA;
  uniform sampler2D tB;
  uniform float wA;
  uniform float wB;
  varying vec2 vUv;
  void main() {
    gl_FragColor = vec4(texture2D(tA, vUv).rgb * wA + texture2D(tB, vUv).rgb * wB, 1.0);
    #include <tonemapping_fragment>
    #include <colorspace_fragment>
  }
`;

export class TraceDriver {
  /** Even samples, odd samples. Linear, pre-exposed scene units. */
  readonly targets: [THREE.WebGLRenderTarget, THREE.WebGLRenderTarget];
  /** Whole samples in each target. */
  readonly counts: [number, number] = [0, 0];
  /** First row of the band in progress, counted from the top. 0 between samples. */
  private row = 0;
  /** Height of the band in progress, 0 between bands. */
  private bandRows = 0;
  /** Next column of the band in progress. */
  private col = 0;
  private resolveQuad: FullScreenQuad;

  constructor(
    private renderer: THREE.WebGLRenderer,
    private pt: TracerInternals,
    readonly width: number,
    readonly height: number,
  ) {
    const make = () => {
      const t = new THREE.WebGLRenderTarget(width, height, {
        type: THREE.FloatType,
        format: THREE.RGBAFormat,
        minFilter: THREE.NearestFilter,
        magFilter: THREE.NearestFilter,
        depthBuffer: false,
        generateMipmaps: false,
      });
      t.scissorTest = true;
      return t;
    };
    this.targets = [make(), make()];
    const prev = renderer.getRenderTarget();
    const clear = new THREE.Color();
    renderer.getClearColor(clear);
    const alpha = renderer.getClearAlpha();
    renderer.setClearColor(0x000000, 0);
    for (const t of this.targets) {
      t.scissorTest = false;
      renderer.setRenderTarget(t);
      renderer.clear(true, false, false);
      t.scissorTest = true;
    }
    renderer.setClearColor(clear, alpha);
    renderer.setRenderTarget(prev);
    this.resolveQuad = new FullScreenQuad(
      new THREE.ShaderMaterial({
        uniforms: { tA: { value: this.targets[0].texture }, tB: { value: this.targets[1].texture }, wA: { value: 1 }, wB: { value: 0 } },
        vertexShader: VERT,
        fragmentShader: RESOLVE_FRAG,
        depthTest: false,
        depthWrite: false,
      }),
    );
  }

  /** Whole samples traced. */
  get samples(): number {
    return this.counts[0] + this.counts[1];
  }

  /** True while the tracer's shader compiles; a sample cannot start then. */
  get compiling(): boolean {
    return this.pt.isCompiling;
  }

  /** Weights of the two targets in the average of every whole sample. */
  weights(): [number, number] {
    const n = this.samples;
    return n > 0 ? [this.counts[0] / n, this.counts[1] / n] : [1, 0];
  }

  /**
   * Makes sure the tracer's shader is built or building, without a stall:
   * its defines follow the scene here, which starts an async compile.
   */
  prepare(): void {
    // The program depends on the target it draws into (three picks tone
    // mapping and color space by target): compile it for a float target, or
    // the first band would compile it again and stall.
    const r = this.renderer;
    const prev = r.getRenderTarget();
    r.setRenderTarget(this.targets[0]);
    this.pt.material.onBeforeRender();
    // Nothing changed a define: still compile off the main thread first.
    if (!this.pt.isCompiling) this.pt.material.needsUpdate = true;
    r.setRenderTarget(prev);
  }

  /** Sets the tracer up for the next sample, as `renderTask` does. False while it compiles. */
  private beginSample(): boolean {
    const m = this.pt.material;
    m.onBeforeRender();
    if (this.pt.isCompiling) return false;
    const slot = this.samples % 2;
    m.opacity = 1 / (this.counts[slot] + 1);
    m.blending = THREE.NormalBlending;
    m.resolution.set(this.width, this.height);
    m.sobolTexture = this.pt._sobolTarget.texture;
    m.stratifiedTexture.init(20, m.bounces + m.transmissiveBounces + 5);
    m.stratifiedTexture.next();
    m.seed++;
    return true;
  }

  /** Where the next slice goes (render/pacing.ts). */
  get at(): { row: number; bandRows: number; col: number } {
    return { row: this.row, bandRows: this.bandRows, col: this.col };
  }

  /**
   * Traces the next slice: `cols` columns of the band in progress, or of a
   * new band `rows` high, starting a new sample when none is in progress.
   * Returns the slice traced (no rows while the shader compiles), its share
   * of the width, and whether it finished the sample.
   */
  trace(rows: number, cols: number): { row: number; rows: number; share: number; done: boolean } {
    if (this.col === 0) {
      if (this.row === 0 && !this.beginSample()) return { row: 0, rows: 0, share: 0, done: false };
      this.bandRows = Math.max(1, Math.min(rows, this.height - this.row));
    }
    const row = this.row;
    const n = this.bandRows;
    const x = this.col;
    const w = Math.max(1, Math.min(cols, this.width - x));
    const slot = this.samples % 2;
    const t = this.targets[slot];
    // Scissor rows count from the bottom in GL; bands go from the top.
    t.scissor.set(x, this.height - row - n, w, n);
    t.viewport.set(0, 0, this.width, this.height);
    const r = this.renderer;
    const prev = r.getRenderTarget();
    const autoClear = r.autoClear;
    r.setRenderTarget(t);
    r.autoClear = false;
    this.pt._fsQuad.render(r);
    r.autoClear = autoClear;
    r.setRenderTarget(prev);
    this.col = x + w;
    let done = false;
    if (this.col >= this.width) {
      this.col = 0;
      this.row += n;
      this.bandRows = 0;
      if (this.row >= this.height) {
        this.row = 0;
        this.counts[slot]++;
        done = true;
      }
    }
    return { row, rows: n, share: w / this.width, done };
  }

  /**
   * Draws the average of every sample so far: onto the canvas tone mapped
   * (null), or into a float target as it is (linear, pre-exposed).
   */
  resolve(target: THREE.WebGLRenderTarget | null): void {
    const mat = this.resolveQuad.material as THREE.ShaderMaterial;
    const [wA, wB] = this.weights();
    mat.uniforms.wA.value = wA;
    mat.uniforms.wB.value = wB;
    const r = this.renderer;
    const prev = r.getRenderTarget();
    r.setRenderTarget(target);
    this.resolveQuad.render(r);
    r.setRenderTarget(prev);
  }

  dispose(): void {
    for (const t of this.targets) t.dispose();
    this.resolveQuad.material.dispose();
    this.resolveQuad.dispose();
  }
}
