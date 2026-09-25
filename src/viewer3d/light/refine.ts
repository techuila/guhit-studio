// Accumulates jittered frames into a clean picture: each sample renders the
// scene with the camera shifted by a fraction of a pixel (and, done by the
// caller, the sun moved across its disc), and blends it into a running
// average. The average is HDR, in the scene's pre-exposed units; the display
// pass tone maps it onto the canvas. Used by the live view when the camera
// rests (light/LightRig.ts) and by the "Enhanced capture" render fallback.
//
// No frame loop in here: the owner calls `sample` and `display` from its own
// scheduled frame.

import * as THREE from "three";
import { FullScreenQuad } from "three/examples/jsm/postprocessing/Pass.js";

/** Low discrepancy sequence for the sub-pixel jitter. */
export function halton(index: number, base: number): number {
  let f = 1;
  let r = 0;
  let i = index;
  while (i > 0) {
    f /= base;
    r += f * (i % base);
    i = Math.floor(i / base);
  }
  return r;
}

/** Sub-pixel offset of sample `i`, in pixels, a little wider than one pixel for smooth edges. */
export function jitter(i: number): [number, number] {
  return [(halton(i + 1, 2) - 0.5) * 1.2, (halton(i + 1, 3) - 0.5) * 1.2];
}

/** Point `i` of `n` on a unit disc, spread evenly (Vogel's golden angle spiral). */
export function discPoint(i: number, n: number): [number, number] {
  const r = Math.sqrt((i + 0.5) / Math.max(n, 1));
  const a = i * 2.399963229728653;
  return [r * Math.cos(a), r * Math.sin(a)];
}

const BLEND_FRAG = /* glsl */ `
  uniform sampler2D map;
  uniform float weight;
  varying vec2 vUv;
  void main() {
    gl_FragColor = vec4(texture2D(map, vUv).rgb, weight);
  }
`;

const METER_FRAG = /* glsl */ `
  uniform sampler2D map;
  uniform vec2 cells;
  varying vec2 vUv;
  void main() {
    // A TAPS x TAPS grid of taps over this output texel's cell of the image.
    // The live view averages log luminance per tap; a noisy path traced
    // image averages light first (MEAN_FIRST), so black, unconverged pixels
    // do not drag the reading down.
    float sum = 0.0;
    for (int y = 0; y < TAPS; y++) {
      for (int x = 0; x < TAPS; x++) {
        vec2 uv = vUv + (vec2(float(x), float(y)) - 0.5 * float(TAPS - 1)) / (float(TAPS) * cells);
        vec3 c = texture2D(map, uv).rgb;
        float l = max(dot(c, vec3(0.2126, 0.7152, 0.0722)), 1e-6);
        #if MEAN_FIRST
          sum += l;
        #else
          sum += log2(l);
        #endif
      }
    }
    float n = float(TAPS * TAPS);
    #if MEAN_FIRST
      float lg = log2(max(sum / n, 1e-6));
    #else
      float lg = sum / n;
    #endif
    // log2 luminance from -16 to +8 in 8 bits.
    gl_FragColor = vec4(clamp((lg + 16.0) / 24.0, 0.0, 1.0), 0.0, 0.0, 1.0);
  }
`;

const VERT = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`;

const METER_W = 32;
const METER_H = 18;

/** How `meterTexture` reads a picture: the live view's clean raster frames, or a noisy path traced one. */
export type MeterMode = "raster" | "traced";

const meterQuads = new Map<MeterMode, FullScreenQuad>();

function meterQuadFor(mode: MeterMode): FullScreenQuad {
  let quad = meterQuads.get(mode);
  if (!quad) {
    quad = new FullScreenQuad(
      new THREE.ShaderMaterial({
        uniforms: { map: { value: null }, cells: { value: new THREE.Vector2(METER_W, METER_H) } },
        defines: mode === "traced" ? { TAPS: 8, MEAN_FIRST: 1 } : { TAPS: 4, MEAN_FIRST: 0 },
        vertexShader: VERT,
        fragmentShader: METER_FRAG,
        depthTest: false,
        depthWrite: false,
        toneMapped: false,
      }),
    );
    meterQuads.set(mode, quad);
  }
  return quad;
}

/**
 * Log2 of the average luminance of an HDR texture (a log average, lightly
 * weighted to the middle of the picture), read back without stalling the
 * GPU. Null when the browser cannot read back asynchronously. Used by the
 * live view's refine and by a render metering its own image.
 */
export async function meterTexture(
  renderer: THREE.WebGLRenderer,
  texture: THREE.Texture,
  target: THREE.WebGLRenderTarget,
  buffer = new Uint8Array(METER_W * METER_H * 4),
  mode: MeterMode = "raster",
): Promise<number | null> {
  if (typeof renderer.readRenderTargetPixelsAsync !== "function") return null;
  const meterQuad = meterQuadFor(mode);
  const mat = meterQuad.material as THREE.ShaderMaterial;
  mat.uniforms.map.value = texture;
  const prev = renderer.getRenderTarget();
  renderer.setRenderTarget(target);
  meterQuad.render(renderer);
  renderer.setRenderTarget(prev);
  mat.uniforms.map.value = null;
  const data = (await renderer.readRenderTargetPixelsAsync(target, 0, 0, METER_W, METER_H, buffer)) as Uint8Array;
  let sum = 0;
  let weights = 0;
  for (let y = 0; y < METER_H; y++) {
    for (let x = 0; x < METER_W; x++) {
      const dx = (x + 0.5) / METER_W - 0.5;
      const dy = (y + 0.5) / METER_H - 0.5;
      // Center weighted: the middle counts about twice the corners.
      const w = 1 - 0.55 * Math.min(1, (dx * dx + dy * dy) * 4);
      sum += w * ((data[(y * METER_W + x) * 4] / 255) * 24 - 16);
      weights += w;
    }
  }
  return weights > 0 ? sum / weights : null;
}

/** The small target `meterTexture` draws into. */
export function meterTarget(): THREE.WebGLRenderTarget {
  return new THREE.WebGLRenderTarget(METER_W, METER_H, { depthBuffer: false, generateMipmaps: false });
}

/**
 * True when this renderer can draw into half float targets, which the
 * accumulation needs (WebGL 2 with EXT_color_buffer_float or
 * EXT_color_buffer_half_float). Without them a target would be incomplete
 * and a refined frame would come out black, so the caller keeps the live
 * frame instead.
 */
export function canAccumulate(renderer: THREE.WebGLRenderer): boolean {
  if (!renderer.capabilities.isWebGL2) return false;
  const ext = renderer.extensions;
  return ext.has("EXT_color_buffer_float") || ext.has("EXT_color_buffer_half_float");
}

export class Accumulator {
  private frame: THREE.WebGLRenderTarget | null = null;
  private accum: THREE.WebGLRenderTarget | null = null;
  private meterTarget: THREE.WebGLRenderTarget | null = null;
  private blend: FullScreenQuad;
  private show: FullScreenQuad;
  private meterBuffer = new Uint8Array(METER_W * METER_H * 4);
  private metering = false;
  samples = 0;
  width = 0;
  height = 0;

  constructor(private type: THREE.TextureDataType = THREE.HalfFloatType) {
    this.blend = new FullScreenQuad(
      new THREE.ShaderMaterial({
        uniforms: { map: { value: null }, weight: { value: 1 } },
        vertexShader: VERT,
        fragmentShader: BLEND_FRAG,
        transparent: true,
        blending: THREE.NormalBlending,
        depthTest: false,
        depthWrite: false,
        toneMapped: false,
      }),
    );
    this.show = new FullScreenQuad(new THREE.MeshBasicMaterial({ map: null, depthTest: false, depthWrite: false }));
  }

  /** Allocates the targets at this size (device pixels). Resets when the size changed. */
  ensure(width: number, height: number): void {
    const w = Math.max(1, Math.round(width));
    const h = Math.max(1, Math.round(height));
    if (this.frame && this.width === w && this.height === h) return;
    this.frame?.dispose();
    this.accum?.dispose();
    const opts = { type: this.type, minFilter: THREE.LinearFilter, magFilter: THREE.LinearFilter, generateMipmaps: false };
    this.frame = new THREE.WebGLRenderTarget(w, h, { ...opts, depthBuffer: true });
    this.accum = new THREE.WebGLRenderTarget(w, h, { ...opts, depthBuffer: false });
    this.width = w;
    this.height = h;
    this.samples = 0;
  }

  /** True when the targets exist. */
  get ready(): boolean {
    return this.frame !== null;
  }

  reset(): void {
    this.samples = 0;
  }

  /**
   * Renders one jittered sample of the scene and blends it into the average.
   * The camera's view offset is set and cleared here; anything else the
   * sample should vary (the sun) the caller moves before and puts back after.
   */
  sample(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.PerspectiveCamera): void {
    const frame = this.frame;
    const accum = this.accum;
    if (!frame || !accum) return;
    const [jx, jy] = jitter(this.samples);
    camera.setViewOffset(this.width, this.height, jx, jy, this.width, this.height);
    const prev = renderer.getRenderTarget();
    renderer.setRenderTarget(frame);
    renderer.render(scene, camera);
    camera.clearViewOffset();
    const mat = this.blend.material as THREE.ShaderMaterial;
    mat.uniforms.map.value = frame.texture;
    mat.uniforms.weight.value = 1 / (this.samples + 1);
    renderer.setRenderTarget(accum);
    // The blend adds to what is there: a clear would throw the average away.
    const autoClear = renderer.autoClear;
    renderer.autoClear = false;
    this.blend.render(renderer);
    renderer.autoClear = autoClear;
    renderer.setRenderTarget(prev);
    this.samples++;
  }

  /**
   * Draws the average onto the current target (the canvas), tone mapped at
   * the renderer's exposure times `ratio`: an exposure that moved after the
   * samples were taken is applied here, without taking them again.
   */
  display(renderer: THREE.WebGLRenderer, ratio = 1): void {
    if (!this.accum || this.samples === 0) return;
    const mat = this.show.material as THREE.MeshBasicMaterial;
    mat.map = this.accum.texture;
    const prevExposure = renderer.toneMappingExposure;
    renderer.toneMappingExposure = prevExposure * ratio;
    this.show.render(renderer);
    renderer.toneMappingExposure = prevExposure;
  }

  /**
   * Log2 of the average luminance of the last sample (a log average, lightly
   * weighted to the middle of the picture), read back without stalling the
   * GPU. Null when a reading is already in flight or the browser cannot.
   */
  async meter(renderer: THREE.WebGLRenderer): Promise<number | null> {
    const frame = this.frame;
    if (!frame || this.metering) return null;
    this.metering = true;
    try {
      this.meterTarget ??= meterTarget();
      return await meterTexture(renderer, frame.texture, this.meterTarget, this.meterBuffer);
    } catch {
      return null;
    } finally {
      this.metering = false;
    }
  }

  /** The last sample, as a texture, for a caller that meters it another way. */
  get frameTexture(): THREE.Texture | null {
    return this.frame?.texture ?? null;
  }

  dispose(): void {
    this.frame?.dispose();
    this.accum?.dispose();
    this.meterTarget?.dispose();
    this.frame = this.accum = this.meterTarget = null;
    this.blend.material.dispose();
    this.blend.dispose();
    this.show.material.dispose();
    this.show.dispose();
    this.samples = 0;
  }
}
