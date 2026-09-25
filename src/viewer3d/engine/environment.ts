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
import { PACK_BASE } from "../scene/pack";

const GRASS = "#a3b183";

/** Tone mapping exposure. Constant: exposure lives in the light intensities. */
export const TONE_EXPOSURE = 1;
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
/** The photo sky at the look it was tuned with (environment 0.52, exposure 1.1). */
const PHOTO_ENV = 0.57;

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
 * everything below the horizon, rescaled to a plausible grass albedo.
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
        if (Number.isFinite(r) && Number.isFinite(g) && Number.isFinite(b)) {
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

/** Angle of a direction around +y, from +z towards +x. */
export const headingOf = (v: THREE.Vector3) => Math.atan2(v.x, v.z);

export interface EnvironmentCallbacks {
  /** The HDRI is on the scene: start the settle and redraw. */
  onHdriReady: () => void;
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

  constructor(private scene: THREE.Scene) {
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
    });
    this.ground = new THREE.Mesh(groundGeo, groundMat);
    this.ground.name = "ground";
    this.ground.receiveShadow = true;
    this.ground.renderOrder = -5;
    this.ground.raycast = () => {};

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
    this.hemi = new THREE.HemisphereLight(0xe9ecf0, 0xb8ad90, 0);
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
      // clear sky does, and takes the exposure like everything else.
      const sky = clearSkyRatio(alt) * this.scale;
      this.scene.environmentIntensity = PHOTO_ENV * sky * settle;
      this.scene.backgroundIntensity = Math.min(Math.max(sky, 0.25), 1.2) * settle;
    } else {
      this.scene.environmentIntensity = SKY_CDM2 * k * (this.physical ? 1 : rasterFill(alt));
      this.scene.backgroundIntensity = SKY_CDM2 * k;
    }
    // Ground bounce from the sun, plus the moon and city glow of a night sky.
    // A path tracer bounces the light itself.
    const up = Math.max(this.sunDir.y, 0);
    this.hemi.intensity = this.physical ? 0 : (sunLux * up * BOUNCE + NIGHT_AMBIENT_LUX) * k;
    // The ground disc belongs to the sky it stands under.
    const groundMat = this.ground.material as THREE.MeshStandardMaterial;
    groundMat.color.copy(this.procGround);
    if (photo && this.hdriGround) groundMat.color.lerp(this.hdriGround, this.photoBlend);
  }

  // ------------------------------------------------------------------- HDRI

  /**
   * Loads the pack's HDRI and its JPG preview once, in the background. The
   * physical sky keeps rendering until both are ready; nothing here can throw
   * into a frame, and a missing pack simply leaves the physical sky up.
   */
  loadHdri(renderer: THREE.WebGLRenderer, cb: EnvironmentCallbacks): void {
    this.cb = cb;
    if (this.hdriLoading || this.hdriTarget || typeof fetch !== "function") return;
    this.hdriLoading = true;
    void (async () => {
      try {
        const hdr = await new HDRLoader().loadAsync(`/${PACK_BASE}hdri/sky.hdr`);
        const scan = scanHdri(hdr.image as { data: ArrayLike<number>; width: number; height: number });
        this.hdriSun = scan.sun;
        this.hdriGround = scan.ground;
        const pmrem = new THREE.PMREMGenerator(renderer);
        this.hdriTarget = pmrem.fromEquirectangular(hdr);
        pmrem.dispose();
        hdr.dispose();
        if (!this.hdriBackground) {
          const bg = await new THREE.TextureLoader().loadAsync(`/${PACK_BASE}hdri/sky-preview.jpg`);
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
    this.disposables.length = 0;
  }
}
