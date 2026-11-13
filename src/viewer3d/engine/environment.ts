// Sky, ground, sun and image based light for the viewer.
//
// The light comes from the site, the date and the time (light/model.ts): the
// sun is where it is for the project's site, north applied, and the sky is
// one of three (docs/CONTRACT.md, "Sun and light"):
// - clear: a physical sky (light/sky.ts) that follows the sun, baked into a
//   cube for the background and a PMREM for the image based light;
// - cloudy: an overcast dome, soft light and faint shadows;
// - photo: the CC0 pack's HDRI (assets/ASSETS.md), turned so its own sun sits
//   at the computed azimuth. It loads async; until it lands, and whenever the
//   sun is down, the physical sky stands in.
//
// Units are pre-exposed (light/model.ts): `setExposure` scales the sun, the
// sky light, the background and the fill together, and the tone mapping
// exposure stays put, so highlights and tints look the same by day and night.

import * as THREE from "three";
import { HDRLoader } from "three/examples/jsm/loaders/HDRLoader.js";
import type { SkyKind } from "../../contract/bindings";
import type { ModelBounds } from "../geom/cameraMath";
import { LUX, type LightFrame } from "../light/model";
import { clearSkyRatio, SKY_CDM2, SkyRig } from "../light/sky";
import { PACK_BASE, texturePack } from "../scene/pack";
import { AO_LAYER, hookAo, type AoUniforms } from "../light/ao";

const GRASS = "#a3b183";

/**
 * Tone mapping of the live view, captures and path traced renders. AgX
 * (Blender's default since 4.0) rolls bright light off to white without the
 * hue shifts of ACES Filmic, which turned sunlit paint yellow and skies teal,
 * and keeps detail in the shade.
 */
export const TONE_MAPPING: THREE.ToneMapping = THREE.AgXToneMapping;
/**
 * Tone mapping exposure. Constant: exposure lives in the light intensities.
 * Set so a metered scene lands where it did under ACES Filmic, which scales
 * its input by 1 / 0.6 inside the curve.
 */
export const TONE_EXPOSURE = 1.25;
/**
 * The live view has no bounce light, so in daylight the sky light is turned
 * up to stand in for the light the sunlit ground throws back: shadows keep
 * their detail and interiors are readable. With the sun down there is
 * nothing to bounce, and it falls back to the physical value. The path tracer
 * computes real bounces and always uses the physical value.
 */
export function rasterFill(altitudeDeg: number): number {
  const t = Math.min(Math.max((altitudeDeg + 2) / 17, 0), 1);
  return 1 + 0.35 * t * t * (3 - 2 * t);
}
/** Ground bounce, as a share of the direct sun on the ground (grass reflects about a fifth). */
const BOUNCE = 0.14;
/** Moon and city glow on a clear night, lux, so a house at night is never pure black. */
const NIGHT_AMBIENT_LUX = 0.6;
/**
 * Cosine weighted mean luminance of the physical clear sky with the sun at
 * 50 degrees, cd/m2 (light/sky.ts: 2.28 raw at 60 degrees, `clearSkyRatio`
 * 1.148 there, times `SKY_CDM2`). The photo sky is scaled to give this much
 * sky light at 50 degrees, and dims with the sun like the clear sky does, so
 * changing the sky kind never changes how bright the shade is.
 */
export const SKY_MEAN_CDM2_AT_50 = (2.28 / 1.148) * SKY_CDM2;
/**
 * The photo's sun is clamped to this many times the 99.7th percentile of the
 * sky: the directional light is the sun, and an image based light that kept
 * it would light every sun-facing surface a second time, through walls too.
 */
const HDRI_SUN_CAP = 1.5;
const HDRI_SUN_PERCENTILE = 0.997;

function radialTexture(size: number, stops: [number, string][]): THREE.CanvasTexture {
  const c = document.createElement("canvas");
  c.width = c.height = size;
  const ctx = c.getContext("2d")!;
  const g = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
  for (const [at, color] of stops) g.addColorStop(at, color);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, size, size);
  return new THREE.CanvasTexture(c);
}

/** Soft rounded rectangle, dark in the middle. Built from nested shapes, no canvas filter. */
function contactTexture(size: number): THREE.CanvasTexture {
  const c = document.createElement("canvas");
  c.width = c.height = size;
  const ctx = c.getContext("2d")!;
  ctx.clearRect(0, 0, size, size);
  const steps = 32;
  for (let i = 0; i < steps; i++) {
    const t = i / (steps - 1);
    // fade zone: the outer 14 percent of the texture
    const inset = size * 0.14 * t + size * 0.005;
    const r = size * 0.12 * (1 - t) + size * 0.02;
    ctx.fillStyle = `rgba(0,0,0,${0.02 + 0.05 * t * t})`;
    ctx.beginPath();
    const s = size - inset * 2;
    ctx.moveTo(inset + r, inset);
    ctx.arcTo(inset + s, inset, inset + s, inset + s, r);
    ctx.arcTo(inset + s, inset + s, inset, inset + s, r);
    ctx.arcTo(inset, inset + s, inset, inset, r);
    ctx.arcTo(inset, inset, inset + s, inset, r);
    ctx.closePath();
    ctx.fill();
  }
  return new THREE.CanvasTexture(c);
}

/** Albedo of real grass, near enough. The ground disc is lit, not emissive. */
const GROUND_ALBEDO = 0.12;
/** How far under the horizon the photo's ground color is read, degrees. */
const GROUND_BAND_DEG = 12;
/** The lawn texture of the ground disc (assets/ASSETS.md), not a project material. */
export const GROUND_PRESET = "mat-ground-grass";
/** Lawn tile, meters, and the larger scale the second sample breaks its repeat with. */
const GRASS_TILE_M = 1.4;
const GRASS_MACRO = 0.137;
/** Share of the ground radius over which the lawn turns into the photo's ground color, before the edge fades out. */
const GROUND_FADE: [number, number] = [0.18, 0.5];

interface GroundUniforms {
  groundFar: THREE.IUniform<THREE.Color>;
  /** 1 once the lawn texture is on, 0 for the flat color. */
  groundTex: THREE.IUniform<number>;
  /** Mean luminance of the lawn texture, linear. */
  groundMean: THREE.IUniform<number>;
}

/**
 * The live view's ground shader: the lawn sampled a second time at a larger
 * scale and a turn, so its tile does not show from above, and blended into
 * the photo's own ground color with distance (`groundFar`), which the edge
 * then fades into. The path tracer reads the plain maps and skips this.
 */
function groundShader(uniforms: GroundUniforms) {
  return (shader: THREE.WebGLProgramParametersWithUniforms) => {
    shader.uniforms.groundFar = uniforms.groundFar;
    shader.uniforms.groundTex = uniforms.groundTex;
    shader.uniforms.groundMean = uniforms.groundMean;
    shader.fragmentShader = shader.fragmentShader
      .replace("#include <common>", "#include <common>\nuniform vec3 groundFar;\nuniform float groundTex;\nuniform float groundMean;")
      .replace(
        "#include <map_fragment>",
        /* glsl */ `#include <map_fragment>
        #ifdef USE_MAP
          {
            vec2 mUv = mat2(0.8, -0.6, 0.6, 0.8) * vMapUv * ${GRASS_MACRO.toFixed(3)};
            vec3 macro = texture2D(map, mUv).rgb;
            float m = dot(macro, vec3(0.2126, 0.7152, 0.0722));
            diffuseColor.rgb *= mix(1.0, clamp(m / max(groundMean, 1e-3), 0.6, 1.4), groundTex);
          }
        #endif
        #ifdef USE_ALPHAMAP
          {
            float r = length(vAlphaMapUv - 0.5) * 2.0;
            float far = smoothstep(${GROUND_FADE[0].toFixed(2)}, ${GROUND_FADE[1].toFixed(2)}, r);
            diffuseColor.rgb = mix(diffuseColor.rgb, groundFar, far * groundTex);
          }
        #endif`,
      );
  };
}
/** Strength of the soft darkening around the building. */
const CONTACT_OPACITY = 0.28;

export interface HdriScan {
  /** World direction towards the brightest spot above the horizon. */
  sun: THREE.Vector3 | null;
  /** Hue of everything below the horizon, at a plausible ground albedo. */
  ground: THREE.Color | null;
}

/**
 * Reads two things out of an equirect HDRI in one pass: where its sun is, and
 * what color its ground is.
 *
 * three samples an equirect with `u = atan2(z, x) / 2pi + 0.5` and
 * `v = asin(y) / pi + 0.5`, and HDRLoader hands back a DataTexture with
 * `flipY` true, so v = 1 is the first row of the data (the zenith). The sun
 * scan is coarse on purpose: the upper half is reduced to cells, the brightest
 * cell wins and its center is the direction. The ground is the mean color of
 * the band `GROUND_BAND_DEG` under the horizon, rescaled to a plausible
 * grass albedo.
 */
export function scanHdri(image: { data: ArrayLike<number>; width: number; height: number }): HdriScan {
  const empty: HdriScan = { sun: null, ground: null };
  const { data, width, height } = image;
  if (!data || !(width > 1) || !(height > 1)) return empty;
  // HDRLoader hands back half floats by default, so the samples are decoded.
  const half = !(data instanceof Float32Array);
  const at = (i: number): number => (half ? THREE.DataUtils.fromHalfFloat(data[i]) : data[i]);
  const cols = 64;
  const rows = 32;
  const sums = new Float64Array(cols * rows);
  const below = [0, 0, 0];
  let belowCount = 0;
  for (let y = 0; y < height; y++) {
    const v = 1 - (y + 0.5) / height;
    const cy = Math.min(rows - 1, Math.floor((1 - v) * 2 * rows));
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4;
      const r = at(i);
      const g = at(i + 1);
      const b = at(i + 2);
      if (v < 0.5) {
        // Only the band just under the horizon: that is what the edge of the
        // drawn lawn meets. What is straight down is often bare earth.
        if (v >= 0.5 - GROUND_BAND_DEG / 180 && Number.isFinite(r) && Number.isFinite(g) && Number.isFinite(b)) {
          below[0] += r;
          below[1] += g;
          below[2] += b;
          belowCount++;
        }
        continue;
      }
      const lum = r * 0.2126 + g * 0.7152 + b * 0.0722;
      if (!Number.isFinite(lum)) continue;
      sums[cy * cols + Math.min(cols - 1, Math.floor(((x + 0.5) / width) * cols))] += lum;
    }
  }

  let ground: THREE.Color | null = null;
  if (belowCount > 0) {
    const mean = new THREE.Color(below[0] / belowCount, below[1] / belowCount, below[2] / belowCount);
    const lum = mean.r * 0.2126 + mean.g * 0.7152 + mean.b * 0.0722;
    if (lum > 1e-4) ground = mean.multiplyScalar(GROUND_ALBEDO / lum);
  }

  let best = -1;
  let bestAt = -1;
  for (let i = 0; i < sums.length; i++) {
    if (sums[i] > best) {
      best = sums[i];
      bestAt = i;
    }
  }
  if (bestAt < 0 || !(best > 0)) return { sun: null, ground };
  const u = ((bestAt % cols) + 0.5) / cols;
  const v = 1 - (Math.floor(bestAt / cols) + 0.5) / (rows * 2);
  const theta = (u - 0.5) * Math.PI * 2;
  const phi = (v - 0.5) * Math.PI;
  const sun = new THREE.Vector3(Math.cos(theta) * Math.cos(phi), Math.sin(phi), Math.sin(theta) * Math.cos(phi));
  return { sun, ground };
}

/**
 * Takes the sun out of an equirect HDRI, in place, and measures the sky that
 * is left. Every pixel brighter than `HDRI_SUN_CAP` times the 99.7th
 * percentile of the upper half (by solid angle) is scaled down to that
 * level, hue kept: the sun disc and its glare go, clouds stay. Returns the
 * cosine weighted mean luminance of the upper half afterwards, in the file's
 * own units, which `Environment` scales to the physical sky. Run `scanHdri`
 * first: it finds the sun by its brightness.
 */
export function tameHdri(image: { data: ArrayLike<number> & { [i: number]: number }; width: number; height: number }): number {
  const { data, width, height } = image;
  if (!data || !(width > 1) || !(height > 1)) return 0;
  const half = !(data instanceof Float32Array);
  const at = (i: number): number => (half ? THREE.DataUtils.fromHalfFloat(data[i]) : data[i]);
  const put = (i: number, v: number) => {
    data[i] = half ? THREE.DataUtils.toHalfFloat(v) : v;
  };
  const rows = Math.floor(height / 2);
  // Solid angle weighted histogram of log2 luminance, upper half only.
  const BINS = 512;
  const LO = -16;
  const SPAN = 40;
  const hist = new Float64Array(BINS);
  let total = 0;
  const lumAt = (i: number) => at(i) * 0.2126 + at(i + 1) * 0.7152 + at(i + 2) * 0.0722;
  for (let y = 0; y < rows; y++) {
    const elev = (0.5 - (y + 0.5) / height) * Math.PI;
    const w = Math.cos(elev);
    for (let x = 0; x < width; x++) {
      const l = lumAt((y * width + x) * 4);
      if (!(l > 0) || !Number.isFinite(l)) continue;
      const b = Math.min(BINS - 1, Math.max(0, Math.floor(((Math.log2(l) - LO) / SPAN) * BINS)));
      hist[b] += w;
      total += w;
    }
  }
  if (!(total > 0)) return 0;
  let acc = 0;
  let capBin = BINS - 1;
  for (let b = 0; b < BINS; b++) {
    acc += hist[b];
    if (acc >= total * HDRI_SUN_PERCENTILE) {
      capBin = b;
      break;
    }
  }
  const cap = Math.pow(2, LO + ((capBin + 1) / BINS) * SPAN) * HDRI_SUN_CAP;
  let sum = 0;
  let weights = 0;
  for (let y = 0; y < height; y++) {
    const elev = (0.5 - (y + 0.5) / height) * Math.PI;
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4;
      let l = lumAt(i);
      if (!Number.isFinite(l)) continue;
      if (l > cap) {
        const k = cap / l;
        put(i, at(i) * k);
        put(i + 1, at(i + 1) * k);
        put(i + 2, at(i + 2) * k);
        l = cap;
      }
      if (y < rows) {
        const w = Math.sin(elev) * Math.cos(elev);
        sum += l * w;
        weights += w;
      }
    }
  }
  return weights > 0 ? sum / weights : 0;
}

/**
 * How much of the photo ground's color the live view's image based light
 * keeps. It has no walls to stop it, so the ground's light reaches every
 * ceiling indoors; at full color a lawn and bare earth turned white ceilings
 * tan. Interiors are lit from below by their own floors, which are mostly
 * neutral. The path tracer bounces real light and keeps the photo as it is.
 */
const LIVE_GROUND_SATURATION = 0.3;

/** Desaturates the lower half of an equirect HDRI in place, luminance kept: `keep` 0 is grey, 1 untouched. */
export function greyGround(image: { data: ArrayLike<number> & { [i: number]: number }; width: number; height: number }, keep: number): void {
  const { data, width, height } = image;
  if (!data || !(width > 1) || !(height > 1)) return;
  const half = !(data instanceof Float32Array);
  const at = (i: number): number => (half ? THREE.DataUtils.fromHalfFloat(data[i]) : data[i]);
  const put = (i: number, v: number) => {
    data[i] = half ? THREE.DataUtils.toHalfFloat(v) : v;
  };
  for (let y = Math.ceil(height / 2); y < height; y++) {
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4;
      const r = at(i);
      const g = at(i + 1);
      const b = at(i + 2);
      const l = r * 0.2126 + g * 0.7152 + b * 0.0722;
      put(i, l + (r - l) * keep);
      put(i + 1, l + (g - l) * keep);
      put(i + 2, l + (b - l) * keep);
    }
  }
}

/** Angle of a direction around +y, from +z towards +x. */
export const headingOf = (v: THREE.Vector3) => Math.atan2(v.x, v.z);

export interface EnvironmentCallbacks {
  /** The HDRI is on the scene: start the settle and redraw. */
  onHdriReady: () => void;
  /** The lawn texture is on the ground: redraw. */
  onGroundReady?: () => void;
}

export class Environment {
  readonly group = new THREE.Group();
  readonly sun: THREE.DirectionalLight;
  readonly sky = new SkyRig(512);
  private hemi: THREE.HemisphereLight;
  private ground: THREE.Mesh;
  private contact: THREE.Mesh;
  private disposables: { dispose(): void }[] = [];

  /** Baked HDRI light and the JPG sky, for the photo sky. Kept for the life of the engine. */
  private hdriTarget: THREE.WebGLRenderTarget | null = null;
  private hdriBackground: THREE.Texture | null = null;
  /** World direction towards the sun found in the HDRI, before any turn. */
  private hdriSun: THREE.Vector3 | null = null;
  private hdriGround: THREE.Color | null = null;
  /** The photo sky's mean light with its sun taken out, in the file's units (`tameHdri`). */
  private hdriSkyMean = 0;
  private readonly procGround = new THREE.Color(GRASS);
  private hdriLoading = false;
  /** The dev harness can keep the photo sky off to compare looks. */
  private hdriWanted = true;
  private cb: EnvironmentCallbacks | null = null;
  private lastFit: { bounds: ModelBounds; groundY: number; contact: { minX: number; minY: number; maxX: number; maxY: number } | null } | null = null;

  /** The light being shown, and its pre-exposure scale. */
  private frame: LightFrame | null = null;
  private scale = 1;
  /** 0 to 1 while the photo sky settles in after it loads. */
  private photoBlend = 1;
  /** World unit vector towards the sun, not jittered. */
  readonly sunDir = new THREE.Vector3(0.45, 0.75, 0.48).normalize();
  /** Where the sun is aimed from, and how far: kept so a jitter can turn it about the same target. */
  private sunTarget = new THREE.Vector3();
  private sunDistance = 30;
  private photoShown = false;
  /** The path tracer's scene: physical sky light, no fill, no painted contact shadow. */
  private physical = false;
  private readonly groundUniforms: GroundUniforms = {
    groundFar: { value: new THREE.Color(GRASS) },
    groundTex: { value: 0 },
    groundMean: { value: 0.1 },
  };
  /** This scene's copies of the lawn maps: their repeat follows the ground's size. */
  private grassMaps: THREE.Texture[] = [];
  private unsubscribeGround: (() => void) | null = null;

  /**
   * `ao`: the live view's ambient occlusion (light/ao.ts). The ground is an
   * occluder and reads it, so the lawn darkens along the walls.
   */
  constructor(
    private scene: THREE.Scene,
    ao: AoUniforms | null = null,
  ) {
    const groundAlpha = radialTexture(256, [
      [0, "#ffffff"],
      [0.45, "#ffffff"],
      [0.95, "#000000"],
      [1, "#000000"],
    ]);
    const groundGeo = new THREE.CircleGeometry(1, 64);
    groundGeo.rotateX(-Math.PI / 2);
    const groundMat = new THREE.MeshStandardMaterial({
      color: GRASS,
      roughness: 1,
      metalness: 0,
      alphaMap: groundAlpha,
      transparent: true,
      depthWrite: false,
      // Anything level with the lawn (a deck, a path) shows over it.
      polygonOffset: true,
      polygonOffsetFactor: 2,
      polygonOffsetUnits: 2,
    });
    groundMat.onBeforeCompile = groundShader(this.groundUniforms);
    groundMat.customProgramCacheKey = () => "guhit-ground";
    if (ao) hookAo(groundMat, ao);
    this.ground = new THREE.Mesh(groundGeo, groundMat);
    this.ground.name = "ground";
    this.ground.receiveShadow = true;
    this.ground.renderOrder = -5;
    this.ground.raycast = () => {};
    if (ao) this.ground.layers.enable(AO_LAYER);

    const contactTex = contactTexture(256);
    const contactGeo = new THREE.PlaneGeometry(1, 1);
    contactGeo.rotateX(-Math.PI / 2);
    const contactMat = new THREE.MeshBasicMaterial({
      map: contactTex,
      color: 0x1b2416,
      transparent: true,
      opacity: CONTACT_OPACITY,
      depthWrite: false,
      toneMapped: false,
    });
    this.contact = new THREE.Mesh(contactGeo, contactMat);
    this.contact.name = "contact";
    this.contact.renderOrder = -4;
    this.contact.raycast = () => {};

    // Bounce light: a neutral sky side and a warm ground side, so shadows
    // under a blue sky stay grey rather than turning teal.
    this.hemi = new THREE.HemisphereLight(0xe9ecf0, 0xc2bcaf, 0);
    this.sun = new THREE.DirectionalLight(0xfff1dc, 3.0);
    this.sun.name = "sun";
    this.sun.castShadow = true;
    this.sun.shadow.mapSize.set(2048, 2048);
    this.sun.shadow.bias = -0.0006;
    this.sun.shadow.normalBias = 0.045;
    this.sun.shadow.radius = 3;

    this.group.add(this.ground, this.contact, this.hemi, this.sun, this.sun.target);
    scene.add(this.group);
    this.disposables.push(groundAlpha, groundGeo, groundMat, contactTex, contactGeo, contactMat);
    // The lawn texture comes with the pack, async; the flat color stands in.
    this.unsubscribeGround = texturePack.onLoad((id) => {
      if (id === GROUND_PRESET && this.applyGrass()) this.cb?.onGroundReady?.();
    });
    if (!this.applyGrass()) texturePack.request(GROUND_PRESET);
  }

  /**
   * Puts the pack's lawn on the ground once it is loaded: copies of its maps,
   * whose repeat `fit` sets so a tile is `GRASS_TILE_M` across. False while
   * it is not loaded (or the pack is missing: the flat color stays).
   */
  private applyGrass(): boolean {
    const set = texturePack.get(GROUND_PRESET);
    if (!set || this.grassMaps.length > 0) return false;
    const mat = this.ground.material as THREE.MeshStandardMaterial;
    const copy = (t: THREE.Texture | null) => {
      if (!t) return null;
      const c = t.clone();
      c.wrapS = c.wrapT = THREE.RepeatWrapping;
      this.grassMaps.push(c);
      return c;
    };
    mat.map = copy(set.map);
    mat.normalMap = copy(set.normalMap);
    mat.normalScale.set(0.6, 0.6);
    mat.color.set(0xffffff);
    this.groundUniforms.groundTex.value = 1;
    this.groundUniforms.groundMean.value = set.albedo.r * 0.2126 + set.albedo.g * 0.7152 + set.albedo.b * 0.0722;
    this.grassAlbedo.copy(set.albedo);
    mat.needsUpdate = true;
    this.sizeGrass();
    this.applyLevels();
    return true;
  }

  /** Mean color of the lawn texture, linear: the far ground fades to it under a sky without a photo. */
  private grassAlbedo = new THREE.Color(GRASS);

  private sizeGrass(): void {
    const k = (this.ground.scale.x * 2) / GRASS_TILE_M;
    for (const t of this.grassMaps) t.repeat.set(k, k);
  }

  /** The sky kind on screen: photo only once the HDRI is up and the sun is. */
  get skyShown(): SkyKind {
    return this.photoShown ? "photo" : (this.frame?.sky === "cloudy" ? "cloudy" : "clear");
  }

  /**
   * Shows a light: the sun's direction, color and strength, and the sky that
   * goes with it. The sky cube is rebaked on the next `prepare`.
   */
  setLight(frame: LightFrame): void {
    this.frame = frame;
    const d = frame.sunDir;
    // Plan (x east, y north, z up) to world (x, up, -north).
    this.sunDir.set(d.x, d.z, -d.y).normalize();
    this.sky.set({ sunWorld: this.sunDir, altitudeDeg: frame.sun.altitudeDeg, cloudy: frame.sky === "cloudy" });
    this.sun.color.setRGB(frame.sunColor[0], frame.sunColor[1], frame.sunColor[2]);
    // Cloud softens the sun's shadow as well as dimming it.
    this.sun.shadow.radius = frame.sky === "cloudy" ? 8 : 3;
    this.refit();
    this.applyLevels();
  }

  /**
   * For a path traced scene: the sky light at its physical value and no
   * painted contact shadow, because the tracer computes both for real.
   */
  setPhysical(on: boolean): void {
    this.physical = on;
    this.contact.visible = !on && (this.lastFit?.contact ?? null) !== null;
    this.applyLevels();
  }

  /** Pre-exposure scale for every light and the sky (light/model.ts). */
  setExposure(scale: number): void {
    if (!(scale > 0) || scale === this.scale) return;
    this.scale = scale;
    this.applyLevels();
  }

  get exposureScale(): number {
    return this.scale;
  }

  /**
   * Rebakes the sky if the light changed, and puts the right sky and image
   * based light on the scene. Call right before rendering. Returns true when
   * the sky was baked (the caller may want to redraw the shadow map too).
   */
  prepare(renderer: THREE.WebGLRenderer): boolean {
    const baked = this.sky.bake(renderer);
    this.applySky();
    return baked;
  }

  /** Kept for the engine's constructor and context restore: rebakes the sky now. */
  bakeEnvironment(renderer: THREE.WebGLRenderer): void {
    this.sky.invalidate();
    this.prepare(renderer);
  }

  private wantsPhoto(): boolean {
    const f = this.frame;
    return (
      !!f && f.sky === "photo" && this.hdriWanted && this.hdriTarget !== null && this.hdriBackground !== null && f.sun.altitudeDeg > -1
    );
  }

  private applySky(): void {
    const photo = this.wantsPhoto();
    this.photoShown = photo;
    if (photo && this.hdriTarget && this.hdriBackground) {
      this.scene.environment = this.hdriTarget.texture;
      this.scene.background = this.hdriBackground;
      // Turn the photo so its sun sits where the computed sun is.
      const turn = this.hdriSun ? headingOf(this.sunDir) - headingOf(this.hdriSun) : 0;
      this.scene.backgroundRotation.set(0, turn, 0);
      this.scene.environmentRotation.set(0, turn, 0);
    } else {
      this.scene.environment = this.sky.environment;
      this.scene.background = this.sky.background;
      // The physical sky is already in world axes, north applied.
      this.scene.backgroundRotation.set(0, 0, 0);
      this.scene.environmentRotation.set(0, 0, 0);
    }
    this.applyLevels();
  }

  /** Every intensity from the light and the exposure. */
  private applyLevels(): void {
    const f = this.frame;
    const k = LUX * this.scale;
    const alt = f?.sun.altitudeDeg ?? 50;
    const sunLux = f ? f.sunLux : 100_000;
    this.sun.intensity = sunLux * k;
    const photo = this.photoShown;
    if (photo) {
      const settle = 0.85 + 0.15 * this.photoBlend;
      // A photo cannot change with the time: it dims with the sun the way a
      // clear sky does, and takes the exposure like everything else. Its sky
      // light matches the physical clear sky's (`photoGain`).
      const sky = clearSkyRatio(alt) * this.scale;
      this.scene.environmentIntensity = this.photoGain(alt) * settle;
      this.scene.backgroundIntensity = Math.min(Math.max(sky, 0.25), 1.2) * settle;
    } else {
      this.scene.environmentIntensity = SKY_CDM2 * k * (this.physical ? 1 : rasterFill(alt));
      this.scene.backgroundIntensity = SKY_CDM2 * k;
    }
    // Ground bounce from the sun, plus the moon and city glow of a night sky.
    // A path tracer bounces the light itself.
    const up = Math.max(this.sunDir.y, 0);
    this.hemi.intensity = this.physical ? 0 : (sunLux * up * BOUNCE + NIGHT_AMBIENT_LUX) * k;
    // The ground disc belongs to the sky it stands under: the lawn fades into
    // the photo's own ground with distance, or the flat color takes it on.
    const groundMat = this.ground.material as THREE.MeshStandardMaterial;
    const far = this.groundUniforms.groundFar.value;
    if (this.grassMaps.length > 0) {
      far.copy(this.grassAlbedo);
      if (photo && this.hdriGround) far.lerp(this.hdriGround, this.photoBlend);
    } else {
      groundMat.color.copy(this.procGround);
      if (photo && this.hdriGround) groundMat.color.lerp(this.hdriGround, this.photoBlend);
    }
  }

  /**
   * Environment intensity of the photo sky at a sun altitude: its sky light
   * (sun taken out) scaled to the physical clear sky's at that altitude, in
   * scene units at the current exposure. The live view's daylight fill
   * applies on top, as it does for the clear sky; a path traced scene uses
   * the physical value.
   */
  photoGain(altitudeDeg: number, skyMean = this.hdriSkyMean): number {
    if (!(skyMean > 0)) return 0;
    const fill = this.physical ? 1 : rasterFill(altitudeDeg);
    return (SKY_MEAN_CDM2_AT_50 / skyMean) * clearSkyRatio(altitudeDeg) * LUX * this.scale * fill;
  }

  // ------------------------------------------------------------------- HDRI

  /**
   * Loads the pack's HDRI and its 4K JPG background once, in the background.
   * The physical sky keeps rendering until both are ready; nothing here can
   * throw into a frame, and a missing pack simply leaves the physical sky up.
   */
  loadHdri(renderer: THREE.WebGLRenderer, cb: EnvironmentCallbacks): void {
    this.cb = cb;
    if (this.hdriLoading || this.hdriTarget || typeof fetch !== "function") return;
    this.hdriLoading = true;
    void (async () => {
      try {
        const hdr = await new HDRLoader().loadAsync(`/${PACK_BASE}hdri/sky.hdr`);
        const image = hdr.image as { data: Uint16Array | Float32Array; width: number; height: number };
        const scan = scanHdri(image);
        this.hdriSun = scan.sun;
        this.hdriGround = scan.ground;
        this.hdriSkyMean = tameHdri(image);
        greyGround(image, LIVE_GROUND_SATURATION);
        const pmrem = new THREE.PMREMGenerator(renderer);
        this.hdriTarget = pmrem.fromEquirectangular(hdr);
        pmrem.dispose();
        hdr.dispose();
        if (!this.hdriBackground) {
          const bg = await new THREE.TextureLoader().loadAsync(`/${PACK_BASE}hdri/sky.jpg`);
          bg.mapping = THREE.EquirectangularReflectionMapping;
          bg.colorSpace = THREE.SRGBColorSpace;
          bg.minFilter = THREE.LinearFilter;
          bg.generateMipmaps = false;
          this.hdriBackground = bg;
        }
        this.cb?.onHdriReady();
      } catch {
        // No pack, no HDRI: the physical sky is already on screen.
      } finally {
        this.hdriLoading = false;
      }
    })();
  }

  /** True while the photo sky is on the scene. */
  get hdriActive(): boolean {
    return this.photoShown;
  }

  get hdriLoaded(): boolean {
    return this.hdriTarget !== null;
  }

  /** The loaded HDRI's PMREM and preview, for another scene (the path tracer). */
  photoSky(): { environment: THREE.Texture; background: THREE.Texture; turn: number } | null {
    if (!this.hdriTarget || !this.hdriBackground) return null;
    const turn = this.hdriSun ? headingOf(this.sunDir) - headingOf(this.hdriSun) : 0;
    return { environment: this.hdriTarget.texture, background: this.hdriBackground, turn };
  }

  /**
   * The WebGL context came back: the render targets lost their contents, so
   * the sky is rebaked and the HDRI's PMREM made again from the file.
   */
  reloadHdri(renderer: THREE.WebGLRenderer): void {
    this.sky.invalidate();
    const cb = this.cb;
    if (!cb || (!this.hdriTarget && !this.hdriLoading)) return;
    this.hdriTarget?.dispose();
    this.hdriTarget = null;
    this.loadHdri(renderer, cb);
  }

  /** Dev toggle: false keeps the physical sky even when the photo sky is picked. */
  setHdriEnabled(on: boolean): void {
    this.hdriWanted = on;
    this.applySky();
  }

  /** The photo sky settling in after it loads: 0 just landed, 1 settled. */
  applyBlend(k: number, renderer: THREE.WebGLRenderer): void {
    renderer.toneMappingExposure = TONE_EXPOSURE;
    this.photoBlend = Math.min(Math.max(k, 0), 1);
    this.applyLevels();
  }

  private refit(): void {
    const f = this.lastFit;
    if (f) this.fit(f.bounds, f.groundY, 0, f.contact);
  }

  /**
   * Sizes the ground, the contact shadow and the sun shadow box to the model.
   * `northAngleDeg` is kept for callers; north is in the light frame now.
   */
  fit(
    bounds: ModelBounds,
    groundY: number,
    _northAngleDeg: number,
    contact: { minX: number; minY: number; maxX: number; maxY: number } | null,
  ): void {
    this.lastFit = { bounds, groundY, contact };
    const cx = (bounds.minX + bounds.maxX) / 2000;
    const cz = -(bounds.minY + bounds.maxY) / 2000;
    const sx = (bounds.maxX - bounds.minX) / 1000;
    const sz = (bounds.maxY - bounds.minY) / 1000;
    const height = (bounds.maxZ - bounds.minZ) / 1000;
    const radius = Math.max(Math.hypot(sx, sz, height) / 2, 2);

    const groundR = Math.max(radius * 14, 80);
    this.ground.scale.set(groundR, 1, groundR);
    this.sizeGrass();
    this.ground.position.set(cx, groundY, cz);

    this.contact.visible = contact !== null && !this.physical;
    if (contact) {
      this.contact.scale.set((contact.maxX - contact.minX) / 1000 + 2.4, 1, (contact.maxY - contact.minY) / 1000 + 2.4);
      this.contact.position.set((contact.minX + contact.maxX) / 2000, groundY + 0.004, -(contact.minY + contact.maxY) / 2000);
    }

    // A low sun throws long shadows: the box grows with them, up to a point.
    const up = Math.max(this.sunDir.y, 0.05);
    const long = Math.min(2.5, (0.6 * Math.sqrt(1 - up * up)) / up);
    const cy = groundY + height / 2;
    this.sunTarget.set(cx, cy, cz);
    this.sunDistance = radius * 3 * (1 + long * 0.5);
    this.sun.target.position.copy(this.sunTarget);
    this.placeSun(this.sunDir);
    const cam = this.sun.shadow.camera;
    const r = radius * 1.25 * (1 + long);
    cam.left = -r;
    cam.right = r;
    cam.top = r;
    cam.bottom = -r;
    cam.near = Math.max(this.sunDistance - radius * 2 * (1 + long), 0.1);
    cam.far = this.sunDistance + radius * 2 * (1 + long);
    cam.updateProjectionMatrix();
    this.sun.shadow.needsUpdate = true;
  }

  private placeSun(dir: THREE.Vector3): void {
    this.sun.position.copy(this.sunTarget).addScaledVector(dir, this.sunDistance);
    this.sun.updateMatrixWorld();
    this.sun.target.updateMatrixWorld();
  }

  /**
   * Moves the sun by a small angle across its disc (refine's soft shadows).
   * `null` puts it back where it is.
   */
  jitterSun(offset: [number, number] | null, radiusDeg = 0.6): void {
    if (!offset) {
      this.placeSun(this.sunDir);
      return;
    }
    const u = new THREE.Vector3(0, 1, 0).cross(this.sunDir);
    if (u.lengthSq() < 1e-8) u.set(1, 0, 0);
    u.normalize();
    const v = this.sunDir.clone().cross(u).normalize();
    const a = (radiusDeg * Math.PI) / 180;
    const dir = this.sunDir.clone().addScaledVector(u, Math.tan(a * offset[0])).addScaledVector(v, Math.tan(a * offset[1])).normalize();
    this.placeSun(dir);
  }

  /** True when the sun gives light and casts a shadow worth drawing. */
  get sunCasts(): boolean {
    return this.sun.castShadow && this.sun.intensity > 1e-4 && this.sunDir.y > 0;
  }

  setShadows(on: boolean): void {
    this.sun.castShadow = on;
  }

  /**
   * The X-ray and hidden shell modes thin the ground and the contact shadow
   * so drains under the slab read through them. 1 is the normal ground.
   */
  setGroundOpacity(k: number): void {
    const t = Math.min(Math.max(k, 0), 1);
    (this.ground.material as THREE.MeshStandardMaterial).opacity = t;
    (this.contact.material as THREE.MeshBasicMaterial).opacity = CONTACT_OPACITY * t;
  }

  /** Copies of the sun and the fill light, for compiling shaders ahead (light/LightRig.ts). */
  proxyLights(): THREE.Light[] {
    return [this.sun.clone(), this.hemi.clone()];
  }

  /** The environment's lit objects, for compiling shaders ahead. */
  compileTargets(): THREE.Object3D[] {
    return [this.ground];
  }

  /** The sky is a background now; nothing follows the camera. Kept for callers. */
  followCamera(_camera: THREE.Camera): void {}

  /** Ground radius, center and height, for the sun path and other overlays. */
  groundFrame(): { center: THREE.Vector3; radius: number } {
    const f = this.lastFit;
    if (!f) return { center: new THREE.Vector3(), radius: 10 };
    const b = f.bounds;
    const sx = (b.maxX - b.minX) / 1000;
    const sz = (b.maxY - b.minY) / 1000;
    return {
      center: new THREE.Vector3((b.minX + b.maxX) / 2000, f.groundY, -(b.minY + b.maxY) / 2000),
      radius: Math.max(Math.hypot(sx, sz) / 2, 3),
    };
  }

  dispose(): void {
    this.scene.remove(this.group);
    this.scene.environment = null;
    this.scene.background = null;
    this.sky.dispose();
    this.hdriTarget?.dispose();
    this.hdriTarget = null;
    this.hdriBackground?.dispose();
    this.hdriBackground = null;
    this.cb = null;
    this.sun.dispose();
    this.hemi.dispose();
    for (const d of this.disposables) d.dispose();
    this.unsubscribeGround?.();
    this.unsubscribeGround = null;
    for (const t of this.grassMaps) t.dispose();
    this.grassMaps.length = 0;
    this.disposables.length = 0;
  }
}
