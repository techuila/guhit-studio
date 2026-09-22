// Sky, ground, sun and image based light for the viewer.
//
// Two looks, one rig. The procedural one is a shader sky dome baked to a small
// PMREM: it costs nothing and is what the first frame is drawn with. The real
// one is the CC0 pack's HDRI (assets/ASSETS.md), run through PMREMGenerator
// for the image based light, with the pack's 512 JPG preview as an equirect
// background. The HDRI loads async; when it lands the scene swaps to it and
// the exposure, the ambient fill and the sun strength cross-fade over one
// short beat, so there is no brightness pop. Reduced motion swaps outright.
//
// The sun that casts shadows is aimed at the brightest point of the HDRI,
// found by scanning the equirect once at load time. `north_angle_deg` rotates
// the sun, the environment and the background together.

import * as THREE from "three";
import { HDRLoader } from "three/examples/jsm/loaders/HDRLoader.js";
import { sunDirection, type ModelBounds } from "../geom/cameraMath";
import { vec3ToWorld } from "../geom/coords";
import { PACK_BASE } from "../scene/pack";

const ZENITH = "#5f9fe0";
const HORIZON = "#e6f0f7";
const HAZE = "#d9ddd0";
const GRASS = "#a3b183";

/** Procedural look: the values the viewer shipped with. */
const PROC = { exposure: 0.95, env: 0.6, hemi: 0.8, sun: 3.0 };
/**
 * HDRI look. A real 1k sky carries far more ambient energy than the pale
 * shader dome, so the environment intensity and the fill come down and the sun
 * keeps most of the contrast. Tuned on the bungalow until plain white walls
 * (`mat-chb-painted`, #f2efe8) read white and the roof keeps its shape.
 */
const HDRI = { exposure: 1.1, env: 0.52, hemi: 0.13, sun: 2.7 };

function skyMaterial(zenith = ZENITH, horizon = HORIZON, haze = HAZE): THREE.ShaderMaterial {
  return new THREE.ShaderMaterial({
    side: THREE.BackSide,
    depthWrite: false,
    depthTest: false,
    fog: false,
    toneMapped: false,
    uniforms: {
      zenith: { value: new THREE.Color(zenith) },
      horizon: { value: new THREE.Color(horizon) },
      haze: { value: new THREE.Color(haze) },
    },
    vertexShader: /* glsl */ `
      varying vec3 vDir;
      void main() {
        vDir = position;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: /* glsl */ `
      uniform vec3 zenith;
      uniform vec3 horizon;
      uniform vec3 haze;
      varying vec3 vDir;
      void main() {
        float h = normalize(vDir).y;
        vec3 col = h >= 0.0
          ? mix(horizon, zenith, pow(h, 0.55))
          : mix(horizon, haze, min(-h * 8.0, 1.0));
        gl_FragColor = vec4(col, 1.0);
        #include <colorspace_fragment>
      }
    `,
  });
}

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

interface HdriScan {
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
 * everything below the horizon, rescaled to a plausible grass albedo, so the
 * viewer's own ground disc sits in the same landscape instead of next to it.
 * One pass over a 1k image, a few milliseconds, once.
 */
function scanHdri(image: { data: ArrayLike<number>; width: number; height: number }): HdriScan {
  const empty: HdriScan = { sun: null, ground: null };
  const { data, width, height } = image;
  if (!data || !(width > 1) || !(height > 1)) return empty;
  // HDRLoader hands back half floats by default (full float is not linearly
  // filterable everywhere, including the software renderer the scripted
  // checks run on), so the samples are decoded before they are summed.
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
        // Below the horizon is ground, never the sun.
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
    // The HDRI stores radiance, not reflectance: keep the hue, put the level
    // where a real lawn is, so the disc responds to the sun like a surface.
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
  // cy 0 is the top half-band just under the zenith; undo the mapping above.
  const v = 1 - (Math.floor(bestAt / cols) + 0.5) / (rows * 2);
  const theta = (u - 0.5) * Math.PI * 2;
  const phi = (v - 0.5) * Math.PI;
  const sun = new THREE.Vector3(Math.cos(theta) * Math.cos(phi), Math.sin(phi), Math.sin(theta) * Math.cos(phi));
  // A sun on the horizon gives grazing shadows that swallow the model.
  if (sun.y < 0.15) {
    sun.y = 0.15;
    sun.normalize();
  }
  return { sun, ground };
}

export interface EnvironmentCallbacks {
  /** The HDRI is on the scene: start the exposure cross-fade and redraw. */
  onHdriReady: () => void;
}

export class Environment {
  readonly group = new THREE.Group();
  readonly sun: THREE.DirectionalLight;
  private hemi: THREE.HemisphereLight;
  private sky: THREE.Mesh;
  private ground: THREE.Mesh;
  private contact: THREE.Mesh;
  private envTarget: THREE.WebGLRenderTarget | null = null;
  private disposables: { dispose(): void }[] = [];

  /** Baked HDRI light and the JPG sky. Kept for the life of the engine. */
  private hdriTarget: THREE.WebGLRenderTarget | null = null;
  private hdriBackground: THREE.Texture | null = null;
  /** World direction towards the sun found in the HDRI, before north. */
  private hdriSun: THREE.Vector3 | null = null;
  /** Ground color that matches the HDRI's own landscape. */
  private hdriGround: THREE.Color | null = null;
  /** The procedural look's grass, kept so the two can be blended. */
  private readonly procGround = new THREE.Color(GRASS);
  private hdriLoading = false;
  /** The HDRI is loaded and the user has not turned it off. */
  private hdriOn = false;
  private hdriWanted = true;
  private cb: EnvironmentCallbacks | null = null;
  private lastFit: { bounds: ModelBounds; groundY: number; north: number; contact: { minX: number; minY: number; maxX: number; maxY: number } | null } | null = null;

  constructor(private scene: THREE.Scene) {
    const skyGeo = new THREE.SphereGeometry(1000, 32, 16);
    const skyMat = skyMaterial();
    this.sky = new THREE.Mesh(skyGeo, skyMat);
    this.sky.renderOrder = -10;
    this.sky.frustumCulled = false;
    this.sky.raycast = () => {};

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
      opacity: 0.28,
      depthWrite: false,
      toneMapped: false,
    });
    this.contact = new THREE.Mesh(contactGeo, contactMat);
    this.contact.renderOrder = -4;
    this.contact.raycast = () => {};

    this.hemi = new THREE.HemisphereLight(0xeef3fa, 0xc2b8a2, 0.8);
    this.sun = new THREE.DirectionalLight(0xfff1dc, 3.0);
    this.sun.castShadow = true;
    this.sun.shadow.mapSize.set(2048, 2048);
    this.sun.shadow.bias = -0.0006;
    this.sun.shadow.normalBias = 0.045;
    this.sun.shadow.radius = 3;

    this.group.add(this.sky, this.ground, this.contact, this.hemi, this.sun, this.sun.target);
    scene.add(this.group);
    this.disposables.push(skyGeo, skyMat, groundAlpha, groundGeo, groundMat, contactTex, contactGeo, contactMat);
  }

  /** Image based light from the same sky, so metal roofs and glass have something to reflect. */
  bakeEnvironment(renderer: THREE.WebGLRenderer): void {
    this.envTarget?.dispose();
    // A paler sky than the visible one: reflections and fill light stay
    // neutral instead of turning floors and shadows blue.
    const envScene = new THREE.Scene();
    const envSky = skyMaterial("#c3d6ea", "#f4f4ef", "#cfcbbd");
    const dome = new THREE.Mesh(this.sky.geometry, envSky);
    envScene.add(dome);
    const pmrem = new THREE.PMREMGenerator(renderer);
    this.envTarget = pmrem.fromScene(envScene, 0.03, 1, 2000);
    pmrem.dispose();
    envSky.dispose();
    // The HDRI wins when it is up: this is only the fallback light.
    if (!this.hdriOn) {
      this.scene.environment = this.envTarget.texture;
      this.scene.environmentIntensity = PROC.env;
    }
  }

  // ------------------------------------------------------------------- HDRI

  /**
   * Loads the pack's HDRI and its JPG preview once, in the background. The
   * current lights keep rendering until both are ready; nothing here can throw
   * into a frame, and a missing pack simply leaves the procedural sky up.
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
        if (this.hdriWanted) this.applyHdri(true);
        this.cb?.onHdriReady();
      } catch {
        // No pack, no HDRI: the procedural sky is already on screen.
      } finally {
        this.hdriLoading = false;
      }
    })();
  }

  /** True once the HDRI is on the scene. */
  get hdriActive(): boolean {
    return this.hdriOn;
  }

  get hdriLoaded(): boolean {
    return this.hdriTarget !== null;
  }

  /**
   * The WebGL context came back: the PMREM render target lost its contents, so
   * it is baked again from the file (the browser serves it from its cache).
   * Plain textures are re-uploaded by three on their own.
   */
  reloadHdri(renderer: THREE.WebGLRenderer): void {
    const cb = this.cb;
    if (!cb || (!this.hdriTarget && !this.hdriLoading)) return;
    this.applyHdri(false);
    this.hdriTarget?.dispose();
    this.hdriTarget = null;
    this.loadHdri(renderer, cb);
  }

  /** Dev toggle between the HDRI look and the procedural sky. */
  setHdriEnabled(on: boolean): void {
    this.hdriWanted = on;
    this.applyHdri(on && this.hdriTarget !== null);
  }

  private applyHdri(on: boolean): void {
    if (on === this.hdriOn) return;
    this.hdriOn = on;
    if (on && this.hdriTarget) {
      this.scene.environment = this.hdriTarget.texture;
      this.scene.background = this.hdriBackground;
      this.sky.visible = false;
    } else {
      this.scene.environment = this.envTarget?.texture ?? null;
      this.scene.background = null;
      this.sky.visible = true;
    }
    this.refit();
  }

  /**
   * Blends the two looks. `k` is 0 on the procedural sky and 1 on the HDRI;
   * the engine drives it through its animator so the swap is a short settle
   * rather than a jump. Only scalars move here: the environment map and the
   * background are swapped once, so no shader is recompiled per frame.
   */
  applyBlend(k: number, renderer: THREE.WebGLRenderer): void {
    const t = this.hdriOn ? Math.min(Math.max(k, 0), 1) : 0;
    const mix = (a: number, b: number) => a + (b - a) * t;
    renderer.toneMappingExposure = mix(PROC.exposure, HDRI.exposure);
    this.scene.environmentIntensity = mix(PROC.env, HDRI.env);
    this.hemi.intensity = mix(PROC.hemi, HDRI.hemi);
    this.sun.intensity = mix(PROC.sun, HDRI.sun);
    // The ground disc belongs to the sky it stands under.
    const groundMat = this.ground.material as THREE.MeshStandardMaterial;
    groundMat.color.copy(this.procGround);
    if (this.hdriGround) groundMat.color.lerp(this.hdriGround, t);
  }

  private refit(): void {
    const f = this.lastFit;
    if (f) this.fit(f.bounds, f.groundY, f.north, f.contact);
  }

  /** Sizes the ground, the contact shadow and the sun shadow box to the model. */
  fit(
    bounds: ModelBounds,
    groundY: number,
    northAngleDeg: number,
    contact: { minX: number; minY: number; maxX: number; maxY: number } | null,
  ): void {
    this.lastFit = { bounds, groundY, north: northAngleDeg, contact };
    const cx = (bounds.minX + bounds.maxX) / 2000;
    const cz = -(bounds.minY + bounds.maxY) / 2000;
    const sx = (bounds.maxX - bounds.minX) / 1000;
    const sz = (bounds.maxY - bounds.minY) / 1000;
    const height = (bounds.maxZ - bounds.minZ) / 1000;
    const radius = Math.max(Math.hypot(sx, sz, height) / 2, 2);

    const groundR = Math.max(radius * 14, 80);
    this.ground.scale.set(groundR, 1, groundR);
    this.ground.position.set(cx, groundY, cz);

    // Soft darkening around the building only, so it sits on the ground.
    this.contact.visible = contact !== null;
    if (contact) {
      this.contact.scale.set((contact.maxX - contact.minX) / 1000 + 2.4, 1, (contact.maxY - contact.minY) / 1000 + 2.4);
      this.contact.position.set((contact.minX + contact.maxX) / 2000, groundY + 0.004, -(contact.minY + contact.maxY) / 2000);
    }

    // North rotates the whole sky, not just the sun, so reflections and the
    // background agree with the shadows.
    const northRad = (northAngleDeg * Math.PI) / 180;
    this.scene.backgroundRotation.set(0, northRad, 0);
    this.scene.environmentRotation.set(0, northRad, 0);

    let dx: number;
    let dy: number;
    let dz: number;
    if (this.hdriOn && this.hdriSun) {
      // The HDRI's own sun, turned by north the same way the sky is.
      const d = this.hdriSun.clone().applyEuler(new THREE.Euler(0, northRad, 0));
      [dx, dy, dz] = [d.x, d.y, d.z];
    } else {
      // `vec3ToWorld` reads mm, so the unit direction comes back scaled by
      // 1/1000: undo that to keep a unit vector.
      const d = sunDirection(northAngleDeg);
      [dx, dy, dz] = vec3ToWorld(d).map((v) => v * 1000) as [number, number, number];
    }
    const cy = groundY + height / 2;
    this.sun.target.position.set(cx, cy, cz);
    this.sun.position.set(cx + dx * radius * 3, cy + dy * radius * 3, cz + dz * radius * 3);
    const cam = this.sun.shadow.camera;
    const r = radius * 1.25;
    cam.left = -r;
    cam.right = r;
    cam.top = r;
    cam.bottom = -r;
    cam.near = radius * 0.5;
    cam.far = radius * 6;
    cam.updateProjectionMatrix();
    this.sun.shadow.needsUpdate = true;
  }

  setShadows(on: boolean): void {
    this.sun.castShadow = on;
  }

  followCamera(camera: THREE.Camera): void {
    this.sky.position.copy(camera.position);
  }

  dispose(): void {
    this.scene.remove(this.group);
    this.scene.environment = null;
    this.scene.background = null;
    this.envTarget?.dispose();
    this.envTarget = null;
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
