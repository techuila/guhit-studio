// The render's denoiser: an edge-aware a-trous wavelet filter in the style of
// SVGF (Schied et al. 2017, "Spatiotemporal Variance-Guided Filtering"),
// without its temporal part, for a path traced still. WebGL 2 only, in the
// job's offscreen renderer (DECISIONS D23: no WebGPU, no OIDN).
//
// 1. Demodulate: the traced color is divided by the albedo from the raster
//    G-buffer (render/gbuffer.ts), so what is filtered is the light falling
//    on each surface. Texture detail (tile grout, wood grain) never goes
//    through the filter; it comes back untouched when the light is
//    multiplied by the albedo again at the end.
// 2. Noise: half the difference of the two half averages (render/
//    traceDriver.ts), squared, is each pixel's variance. It shrinks as the
//    samples add up, so the filter's strength follows the sample count by
//    itself: a noisy pixel accepts neighbours that differ a lot, a converged
//    one hardly any.
// 3. Filter: `passes` a-trous steps of a 5 x 5 B3 spline kernel, 1, 2, 4, 8,
//    16 pixels apart. A neighbour counts by how alike it is: normal (a power
//    of their dot product), plane (its distance from this pixel's tangent
//    plane, relative to depth) and light (its luminance difference over the
//    pixel's noise). The variance is filtered along with the light.
// 4. Remodulate, tone map, draw onto the canvas. A pixel at an edge gets the
//    light of both surfaces it covers, by coverage (render/gbuffer.ts).
//
// Sky and glowing surfaces (a lit diffuser) are left as traced, and never
// used as neighbours. Pixels next to the sky (a roofline) are left as traced
// too: part of their color is sky, which demodulation cannot separate.
//
// Fireflies: a pixel far brighter than every neighbour on its surface is one
// rare light path, not detail. It is brought down to FIREFLY times its
// brightest neighbour before filtering; otherwise the filter, trusting the
// pixel more as its variance falls, keeps it as a speck or smears it into a
// blotch.

import * as THREE from "three";
import type { GBuffer } from "./gbuffer";
import { ScreenPass } from "./screenPass";

/** 1D B3 spline kernel by distance from the centre, in steps: 3/8, 1/4, 1/16. */
export const KERNEL = [3 / 8, 1 / 4, 1 / 16] as const;
/**
 * Normal weight: dot product to this power. SVGF uses 128 with plain normals;
 * with the normal maps in the G-buffer (render/gbuffer.ts) 64 keeps corners
 * apart and lets the filter work across the fine relief of tile and plaster
 * (measured against reference renders: marginally less error than 128).
 */
export const SIGMA_N = 64;
/**
 * Light weight: luminance difference over this many standard deviations of
 * the pixel's noise. SVGF uses 4 for a frame with temporal history; a still
 * without it keeps less mottle at 8 (measured against a 791 sample reference
 * of a night interior: downsampled error 3.9 against 4.6, the average bias a
 * third).
 */
export const SIGMA_L = 8;
/** Plane weight: distance from the pixel's tangent plane over this share of its depth. */
export const PLANE_SHARE = 0.015;
/** Albedo floor for demodulation: black paint still carries its light. */
export const ALBEDO_FLOOR = 0.02;
/** A pixel covered less than this by the surface at its centre is at an edge. */
const EDGE_COVER = 0.97;
/** A pixel brighter than this many times its brightest neighbour on the same surface is a firefly. */
export const FIREFLY = 3;
/** Neighbours whose normals are at least this alike count as the same surface for fireflies. */
const SAME_SURFACE = 0.9;

/** Weight of the tap `dx`, `dy` steps from the centre in the 5 x 5 kernel. */
export function kernelWeight(dx: number, dy: number): number {
  return (KERNEL[Math.abs(dx)] ?? 0) * (KERNEL[Math.abs(dy)] ?? 0);
}

type V3 = readonly [number, number, number];
const dot = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

/** How alike two unit normals are: 1 when equal, 0 at right angles or more. */
export function normalWeight(nP: V3, nQ: V3, sigma = SIGMA_N): number {
  return Math.pow(Math.max(dot(nP, nQ), 0), sigma);
}

/**
 * How close a neighbour at view space position `pQ` lies to the tangent
 * plane of the pixel at `pP` with normal `nP` and view depth `depthP`: 1 on
 * the plane, falling off with the distance over PLANE_SHARE of the depth.
 */
export function planeWeight(nP: V3, pP: V3, pQ: V3, depthP: number, share = PLANE_SHARE): number {
  const d = Math.abs(nP[0] * (pQ[0] - pP[0]) + nP[1] * (pQ[1] - pP[1]) + nP[2] * (pQ[2] - pP[2]));
  return Math.exp(-d / Math.max(share * depthP, 1e-6));
}

/** How alike two lights are for a pixel whose noise has variance `varianceP`: 1 when equal. */
export function luminanceWeight(lP: number, lQ: number, varianceP: number, sigma = SIGMA_L): number {
  return Math.exp(-Math.abs(lP - lQ) / (sigma * Math.sqrt(Math.max(varianceP, 0)) + 1e-6));
}

/**
 * The filter for a render: a-trous passes and the light weight's width.
 * Five passes reach 62 pixels out, enough for the soft light on walls at
 * HD; a larger image has more pixels for the same wall and gets one more.
 * With many samples the noise is low and the variance keeps the filter from
 * blurring on its own; one or two passes fewer keep it from reaching far
 * for nothing.
 */
export interface FilterPlan {
  passes: number;
  sigmaL: number;
  sigmaN: number;
  planeShare: number;
  firefly: number;
}

export function filterPlan(samples: number, height: number): FilterPlan {
  let passes = height > 1500 ? 6 : 5;
  if (samples >= 2048) passes -= 2;
  else if (samples >= 512) passes -= 1;
  return { passes: Math.max(passes, 1), sigmaL: SIGMA_L, sigmaN: SIGMA_N, planeShare: PLANE_SHARE, firefly: FIREFLY };
}

/** Per pixel variance of the average of two half averages of `nA` and `nB` samples, as a factor on their difference squared. */
export function halvesVarianceFactor(nA: number, nB: number): number {
  if (nA < 1 || nB < 1) return 0;
  return 1 / ((1 / nA + 1 / nB) * (nA + nB));
}

const f = (v: number) => v.toFixed(8);

const VERT = /* glsl */ `
  void main() {
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`;

const COMMON = /* glsl */ `
  float lum(vec3 c) { return dot(c, vec3(0.2126, 0.7152, 0.0722)); }
  bool bad(vec3 c) { return any(isnan(c)) || any(isinf(c)); }
`;

/** Demodulate and estimate each pixel's variance. */
const PREPASS = /* glsl */ `
  uniform sampler2D tA;
  uniform sampler2D tB;
  uniform float wA;
  uniform float wB;
  uniform float varK;
  uniform sampler2D tAlbedo;
  uniform sampler2D tND;
  uniform vec2 size;
  uniform float firefly;
  ${COMMON}
  vec3 illumAt(ivec2 q) {
    vec3 c = texelFetch(tA, q, 0).rgb * wA + texelFetch(tB, q, 0).rgb * wB;
    return c / max(texelFetch(tAlbedo, q, 0).rgb, vec3(${f(ALBEDO_FLOOR)}));
  }
  void main() {
    ivec2 p = ivec2(gl_FragCoord.xy);
    vec3 a = texelFetch(tA, p, 0).rgb;
    vec3 b = texelFetch(tB, p, 0).rgb;
    if (bad(a)) a = vec3(0.0);
    if (bad(b)) b = a;
    vec3 c = a * wA + b * wB;
    vec4 nd = texelFetch(tND, p, 0);
    ivec2 last = ivec2(size) - 1;
    // Sky, a glowing surface, or next to the sky: kept as traced. Otherwise
    // note the brightest neighbour on the same surface.
    bool keep = nd.w <= 0.0;
    float brightest = 0.0;
    for (int y = -1; y <= 1 && !keep; y++) {
      for (int x = -1; x <= 1; x++) {
        if (x == 0 && y == 0) continue;
        ivec2 q = clamp(p + ivec2(x, y), ivec2(0), last);
        vec4 ndQ = texelFetch(tND, q, 0);
        if (ndQ.w == 0.0) {
          keep = true;
          break;
        }
        if (ndQ.w > 0.0 && dot(nd.xyz, ndQ.xyz) > ${f(SAME_SURFACE)}) brightest = max(brightest, lum(illumAt(q)));
      }
    }
    if (keep) {
      gl_FragColor = vec4(c, -1.0);
      return;
    }
    vec3 floorAlb = max(texelFetch(tAlbedo, p, 0).rgb, vec3(${f(ALBEDO_FLOOR)}));
    vec3 illum = c / floorAlb;
    float l = lum(illum);
    if (brightest > 0.0 && l > firefly * brightest) illum *= firefly * brightest / l;
    float variance;
    #if DUAL
      // Half the halves' difference, squared, is one reading of the noise:
      // a noisy one. Averaged over the 3 x 3 neighbours on the same surface
      // it steadies, so no pixel trusts itself by luck.
      float d = lum((a - b) / floorAlb);
      float vs = d * d;
      float vn = 1.0;
      for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
          if (x == 0 && y == 0) continue;
          ivec2 q = clamp(p + ivec2(x, y), ivec2(0), last);
          vec4 ndQ = texelFetch(tND, q, 0);
          if (ndQ.w <= 0.0 || dot(nd.xyz, ndQ.xyz) <= ${f(SAME_SURFACE)}) continue;
          vec3 albQ = max(texelFetch(tAlbedo, q, 0).rgb, vec3(${f(ALBEDO_FLOOR)}));
          float dq = lum((texelFetch(tA, q, 0).rgb - texelFetch(tB, q, 0).rgb) / albQ);
          vs += dq * dq;
          vn += 1.0;
        }
      }
      variance = vs / vn * varK;
    #else
      // One average only: the spread of its neighbours on the same surface.
      float m1 = 0.0;
      float m2 = 0.0;
      float ws = 0.0;
      for (int y = -2; y <= 2; y++) {
        for (int x = -2; x <= 2; x++) {
          ivec2 q = clamp(p + ivec2(x, y), ivec2(0), last);
          vec4 ndQ = texelFetch(tND, q, 0);
          if (ndQ.w <= 0.0) continue;
          float w = pow(max(dot(nd.xyz, ndQ.xyz), 0.0), 16.0);
          float l = lum(illumAt(q));
          m1 += w * l;
          m2 += w * l * l;
          ws += w;
        }
      }
      m1 /= max(ws, 1e-6);
      variance = max(m2 / max(ws, 1e-6) - m1 * m1, 0.0);
    #endif
    gl_FragColor = vec4(illum, variance);
  }
`;

/** One a-trous step. Alpha carries the variance; a negative alpha marks a pixel kept as traced. */
const ATROUS = /* glsl */ `
  uniform sampler2D tIllum;
  uniform sampler2D tND;
  uniform int stepSize;
  uniform vec2 size;
  uniform vec2 tanHalf;
  uniform float sigmaL;
  uniform float sigmaN;
  uniform float planeShare;
  ${COMMON}
  vec3 viewPos(ivec2 q, float depth) {
    vec2 ndc = (vec2(q) + 0.5) / size * 2.0 - 1.0;
    return vec3(ndc * tanHalf, -1.0) * depth;
  }
  float kern(int i) { return i == 0 ? ${f(KERNEL[0])} : i == 1 ? ${f(KERNEL[1])} : ${f(KERNEL[2])}; }
  void main() {
    ivec2 p = ivec2(gl_FragCoord.xy);
    vec4 cP = texelFetch(tIllum, p, 0);
    if (cP.a < 0.0) {
      gl_FragColor = cP;
      return;
    }
    vec4 ndP = texelFetch(tND, p, 0);
    ivec2 last = ivec2(size) - 1;
    // The pixel's noise, smoothed over its 3 x 3 neighbours (SVGF's g3x3).
    float vs = 0.0;
    float vw = 0.0;
    for (int y = -1; y <= 1; y++) {
      for (int x = -1; x <= 1; x++) {
        vec4 cQ = texelFetch(tIllum, clamp(p + ivec2(x, y), ivec2(0), last), 0);
        if (cQ.a < 0.0) continue;
        float g = (x == 0 ? 0.5 : 0.25) * (y == 0 ? 0.5 : 0.25);
        vs += g * cQ.a;
        vw += g;
      }
    }
    float phiL = sigmaL * sqrt(max(vs / max(vw, 1e-6), 0.0)) + 1e-6;
    float lP = lum(cP.rgb);
    vec3 posP = viewPos(p, ndP.w);
    float tol = max(planeShare * ndP.w, 1e-6);
    float h0 = kern(0) * kern(0);
    vec3 sum = cP.rgb * h0;
    float wsum = h0;
    float vsum = h0 * h0 * cP.a;
    for (int y = -2; y <= 2; y++) {
      for (int x = -2; x <= 2; x++) {
        if (x == 0 && y == 0) continue;
        ivec2 q = p + ivec2(x, y) * stepSize;
        if (q.x < 0 || q.y < 0 || q.x > last.x || q.y > last.y) continue;
        vec4 cQ = texelFetch(tIllum, q, 0);
        if (cQ.a < 0.0) continue;
        vec4 ndQ = texelFetch(tND, q, 0);
        float wN = pow(max(dot(ndP.xyz, ndQ.xyz), 0.0), sigmaN);
        float wP = exp(-abs(dot(ndP.xyz, viewPos(q, ndQ.w) - posP)) / tol);
        float wL = exp(-abs(lP - lum(cQ.rgb)) / phiL);
        float w = kern(x < 0 ? -x : x) * kern(y < 0 ? -y : y) * wN * wP * wL;
        sum += cQ.rgb * w;
        wsum += w;
        vsum += w * w * cQ.a;
      }
    }
    gl_FragColor = vec4(sum / wsum, vsum / (wsum * wsum));
  }
`;

/**
 * Remodulate and draw: tone mapped onto the canvas. A pixel only partly
 * covered by the surface at its centre (render/gbuffer.ts) is an edge: its
 * color mixes that surface's light and albedo with the light and albedo of
 * its neighbour least like it, the other side of the edge, by coverage. The
 * pixel's albedo was averaged over both surfaces, so the centre surface's
 * own albedo is what is left after taking the other side's share out. That
 * is the tracer's anti-aliasing, kept, even where a dark metal meets a white
 * wall.
 */
const FINAL = /* glsl */ `
  uniform sampler2D tIllum;
  uniform sampler2D tAlbedo;
  uniform sampler2D tND;
  uniform sampler2D tCover;
  uniform vec2 size;
  uniform vec2 tanHalf;
  uniform float planeShare;
  ${COMMON}
  vec3 viewPos(ivec2 q, float depth) {
    vec2 ndc = (vec2(q) + 0.5) / size * 2.0 - 1.0;
    return vec3(ndc * tanHalf, -1.0) * depth;
  }
  void main() {
    ivec2 p = ivec2(gl_FragCoord.xy);
    vec4 c = texelFetch(tIllum, p, 0);
    vec3 col = c.rgb;
    if (c.a >= 0.0) {
      vec3 albedo = max(texelFetch(tAlbedo, p, 0).rgb, vec3(${f(ALBEDO_FLOOR)}));
      col = c.rgb * albedo;
      float cover = clamp(texelFetch(tCover, p, 0).r, 0.0, 1.0);
      if (cover < ${f(EDGE_COVER)}) {
        vec4 nd = texelFetch(tND, p, 0);
        vec3 pos = viewPos(p, nd.w);
        ivec2 last = ivec2(size) - 1;
        float least = 2.0;
        ivec2 other = p;
        for (int y = -1; y <= 1; y++) {
          for (int x = -1; x <= 1; x++) {
            if (x == 0 && y == 0) continue;
            ivec2 q = clamp(p + ivec2(x, y), ivec2(0), last);
            if (texelFetch(tIllum, q, 0).a < 0.0) continue;
            vec4 ndq = texelFetch(tND, q, 0);
            float alike = max(dot(nd.xyz, ndq.xyz), 0.0) * exp(-abs(dot(nd.xyz, viewPos(q, ndq.w) - pos)) / max(planeShare * nd.w, 1e-6));
            if (alike < least) {
              least = alike;
              other = q;
            }
          }
        }
        if (least < 0.5) {
          vec3 otherAlbedo = max(texelFetch(tAlbedo, other, 0).rgb, vec3(${f(ALBEDO_FLOOR)}));
          vec3 ownAlbedo = max((albedo - (1.0 - cover) * otherAlbedo) / max(cover, 0.05), vec3(${f(ALBEDO_FLOOR)}));
          col = cover * ownAlbedo * c.rgb + (1.0 - cover) * otherAlbedo * texelFetch(tIllum, other, 0).rgb;
        }
      }
    }
    gl_FragColor = vec4(col, 1.0);
    #include <tonemapping_fragment>
    #include <colorspace_fragment>
  }
`;

export interface DenoiseInput {
  /** The two half averages, or the same texture twice with one weight 0 when there is only one. */
  a: THREE.Texture;
  b: THREE.Texture;
  wA: number;
  wB: number;
  /** Samples in each half; with one half empty the variance comes from neighbours. */
  nA: number;
  nB: number;
  gbuffer: GBuffer;
  camera: THREE.PerspectiveCamera;
  samples: number;
  /** Dev tuning: overrides the plan `filterPlan` gives. */
  plan?: Partial<FilterPlan>;
}

function pass(fragmentShader: string, uniforms: Record<string, THREE.IUniform>, defines: Record<string, number> = {}): ScreenPass {
  return new ScreenPass(
    new THREE.ShaderMaterial({ uniforms, defines, vertexShader: VERT, fragmentShader, depthTest: false, depthWrite: false, blending: THREE.NoBlending }),
  );
}

/**
 * Filters a render and draws it onto the canvas, tone mapped. `pace` is
 * awaited after every pass, so the job can wait on the GPU between them.
 * False when `cancelled` turned true meanwhile.
 */
export async function denoiseToCanvas(
  r: THREE.WebGLRenderer,
  input: DenoiseInput,
  width: number,
  height: number,
  pace: () => Promise<void>,
  cancelled: () => boolean,
): Promise<boolean> {
  const make = () =>
    new THREE.WebGLRenderTarget(width, height, {
      type: THREE.HalfFloatType,
      minFilter: THREE.NearestFilter,
      magFilter: THREE.NearestFilter,
      depthBuffer: false,
      generateMipmaps: false,
    });
  let ping = make();
  let pong = make();
  const size = new THREE.Vector2(width, height);
  const dual = input.nA > 0 && input.nB > 0;
  const plan = { ...filterPlan(input.samples, height), ...input.plan };
  const pre = pass(
    PREPASS,
    {
      tA: { value: input.a },
      tB: { value: dual ? input.b : input.a },
      wA: { value: dual ? input.wA : 1 },
      wB: { value: dual ? input.wB : 0 },
      varK: { value: halvesVarianceFactor(input.nA, input.nB) },
      tAlbedo: { value: input.gbuffer.albedo.texture },
      tND: { value: input.gbuffer.normalDepth.texture },
      size: { value: size },
      firefly: { value: plan.firefly },
    },
    { DUAL: dual ? 1 : 0 },
  );
  const cam = input.camera;
  const tanY = Math.tan(THREE.MathUtils.degToRad(cam.fov) / 2) / cam.zoom;
  const atrous = pass(ATROUS, {
    tIllum: { value: null },
    tND: { value: input.gbuffer.normalDepth.texture },
    stepSize: { value: 1 },
    size: { value: size },
    tanHalf: { value: new THREE.Vector2(tanY * cam.aspect, tanY) },
    sigmaL: { value: plan.sigmaL },
    sigmaN: { value: plan.sigmaN },
    planeShare: { value: plan.planeShare },
  });
  const fin = pass(FINAL, {
    tIllum: { value: null },
    tAlbedo: { value: input.gbuffer.albedo.texture },
    tND: { value: input.gbuffer.normalDepth.texture },
    tCover: { value: input.gbuffer.coverage.texture },
    size: { value: size },
    tanHalf: { value: new THREE.Vector2(tanY * cam.aspect, tanY) },
    planeShare: { value: plan.planeShare },
  });
  const prev = r.getRenderTarget();
  try {
    await Promise.all([pre.compile(r, ping), atrous.compile(r, ping), fin.compile(r, null)]);
    if (cancelled()) return false;
    r.setRenderTarget(ping);
    pre.render(r);
    await pace();
    if (cancelled()) return false;
    const u = atrous.material.uniforms;
    for (let i = 0; i < plan.passes; i++) {
      u.tIllum.value = ping.texture;
      u.stepSize.value = 1 << i;
      r.setRenderTarget(pong);
      atrous.render(r);
      [ping, pong] = [pong, ping];
      await pace();
      if (cancelled()) return false;
    }
    fin.material.uniforms.tIllum.value = ping.texture;
    r.setRenderTarget(null);
    fin.render(r);
    return true;
  } finally {
    r.setRenderTarget(prev);
    for (const q of [pre, atrous, fin]) q.dispose();
    ping.dispose();
    pong.dispose();
  }
}
