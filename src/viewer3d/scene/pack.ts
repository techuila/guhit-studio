// The CC0 asset pack (assets/ASSETS.md, public/assets/pack/manifest.json):
// HDRI sky, PBR material maps and GLB furniture. Everything here is static app
// data, so the caches are module level and shared by every viewer that mounts:
// the manifest is fetched once, each texture set and each GLB is parsed once,
// and instances are plain clones over the shared geometry and materials.
//
// Nothing loads on import. The engine asks for what it needs; until a file is
// on screen the viewer keeps its procedural placeholder, so a missing or slow
// pack never blocks a frame and never throws. Under vitest there is no fetch
// target and every getter simply answers "not loaded".

import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { MeshoptDecoder } from "three/examples/jsm/libs/meshopt_decoder.module.js";

/** Served from `public/assets/pack`. Same origin, so the webview CSP allows it. */
export const PACK_BASE = "assets/pack/";

export interface PackHdri {
  file: string;
  preview_file: string;
}

export interface PackMaterialEntry {
  id: string;
  /** Real world size of one tile, mm. Null for the flat color presets. */
  physical_size_mm: [number, number] | null;
  maps: string[];
}

export interface PackModelEntry {
  /** Null for catalog keys no CC0 source covers (tree, car-sedan). */
  file: string | null;
  back_axis?: string;
  bbox_mm?: { width: number; depth: number; height: number };
}

export interface PackManifest {
  hdri?: PackHdri;
  materials?: Record<string, PackMaterialEntry>;
  models?: Record<string, PackModelEntry>;
}

function packUrl(file: string): string {
  return `/${PACK_BASE}${file}`;
}

let manifestPromise: Promise<PackManifest | null> | null = null;
let manifest: PackManifest | null = null;

/** Fetches the manifest once. Resolves to null when there is no pack. */
export function loadPackManifest(): Promise<PackManifest | null> {
  manifestPromise ??= (async () => {
    if (typeof fetch !== "function") return null;
    try {
      const res = await fetch(packUrl("manifest.json"));
      if (!res.ok) return null;
      manifest = (await res.json()) as PackManifest;
      return manifest;
    } catch {
      return null;
    }
  })();
  return manifestPromise;
}

/** The manifest, or null while it is still loading. Never fetches. */
export function packManifest(): PackManifest | null {
  return manifest;
}

// ------------------------------------------------------------------ textures

/** One preset's maps plus the repeat its physical tile size works out to. */
export interface PackTextureSet {
  map: THREE.Texture;
  normalMap: THREE.Texture | null;
  roughnessMap: THREE.Texture | null;
  /** Tiles per scene meter. Geometry UVs are in meters (geom/meshData.ts). */
  repeat: THREE.Vector2;
  /** Average linear color of the color map, for matching the preset's albedo. */
  albedo: THREE.Color;
}

const NAMED = ["color", "normal", "roughness"] as const;

/** Below this much spread a map carries no pattern at all, only a flat tone. */
const FLAT_LIMIT = 0.008;

interface MapStats {
  /**
   * Largest per channel standard deviation, 0 for a flat swatch. Per channel
   * because a normal map is almost all blue: its detail is in red and green,
   * and an average over the three would hide it.
   */
  spread: number;
  /** Mean color in linear space, which is what the shader multiplies. */
  mean: THREE.Color;
}

const FULL: MapStats = { spread: 1, mean: new THREE.Color(1, 1, 1) };
const BLANK: MapStats = { spread: 0, mean: new THREE.Color(1, 1, 1) };

/** sRGB to linear. three has this internally but does not export it. */
function toLinear(c: number): number {
  return c < 0.04045 ? c * 0.0773993808 : Math.pow(c * 0.9478672986 + 0.0521327014, 2.4);
}

/**
 * What a map actually contains, measured once from a 32 x 32 downsample: how
 * much it varies, and how bright it is on average. The spread catches pack
 * entries that came out of their source as a blank photo (see
 * assets/ASSETS.md); the mean is what lets a preset color be applied as a real
 * albedo instead of darkening the photo a second time.
 */
function measure(tex: THREE.Texture | null): MapStats {
  const image = tex?.image as CanvasImageSource | undefined;
  if (!image || typeof document === "undefined") return BLANK;
  try {
    const n = 32;
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = n;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) return FULL;
    ctx.drawImage(image, 0, 0, n, n);
    const { data } = ctx.getImageData(0, 0, n, n);
    const count = n * n;
    const sum = [0, 0, 0];
    const sumSq = [0, 0, 0];
    const linSum = [0, 0, 0];
    for (let i = 0; i < data.length; i += 4) {
      for (let c = 0; c < 3; c++) {
        const v = data[i + c] / 255;
        sum[c] += v;
        sumSq[c] += v * v;
        linSum[c] += toLinear(v);
      }
    }
    let spread = 0;
    for (let c = 0; c < 3; c++) {
      const mean = sum[c] / count;
      spread = Math.max(spread, Math.sqrt(Math.max(sumSq[c] / count - mean * mean, 0)));
    }
    return { spread, mean: new THREE.Color(linSum[0] / count, linSum[1] / count, linSum[2] / count) };
  } catch (e) {
    // A tainted or undecodable image: assume it is fine and use it.
    console.warn("viewer3d: could not measure a pack map", e);
    return FULL;
  }
}

class TexturePack {
  private sets = new Map<string, PackTextureSet>();
  private pending = new Set<string>();
  private missing = new Set<string>();
  /** Presets whose pack maps turned out to be blank. Reported, not used. */
  readonly flat = new Set<string>();
  private listeners = new Set<(presetId: string) => void>();
  private loader: THREE.TextureLoader | null = null;
  private anisotropy = 4;

  onLoad(fn: (presetId: string) => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  /** 4 is where the sharpness gain stops paying for the sample cost. */
  setAnisotropy(a: number): void {
    const want = Math.max(1, Math.min(a, 4));
    if (want === this.anisotropy) return;
    this.anisotropy = want;
    for (const set of this.sets.values()) {
      for (const t of [set.map, set.normalMap, set.roughnessMap]) {
        if (!t) continue;
        t.anisotropy = want;
        t.needsUpdate = true;
      }
    }
  }

  get(presetId: string): PackTextureSet | null {
    return this.sets.get(presetId) ?? null;
  }

  /** Starts the load for a preset that has a textured entry. Safe to call every build. */
  request(presetId: string): void {
    if (this.sets.has(presetId) || this.pending.has(presetId) || this.missing.has(presetId)) return;
    this.pending.add(presetId);
    void this.load(presetId);
  }

  stats(): { loaded: number; pending: number; flat: string[] } {
    return { loaded: this.sets.size, pending: this.pending.size, flat: [...this.flat] };
  }

  private async load(presetId: string): Promise<void> {
    try {
      const mf = await loadPackManifest();
      const entry = mf?.materials?.[presetId];
      const files = entry?.maps ?? [];
      const colorFile = files.find((f) => f.endsWith("/color.jpg"));
      if (!entry || !colorFile || typeof document === "undefined") {
        this.missing.add(presetId);
        return;
      }
      this.loader ??= new THREE.TextureLoader();
      const byName = new Map<string, string>();
      for (const name of NAMED) {
        const file = files.find((f) => f.endsWith(`/${name}.jpg`));
        if (file) byName.set(name, file);
      }
      const [map, normalMap, roughnessMap] = await Promise.all([
        this.texture(byName.get("color")!, true),
        this.texture(byName.get("normal"), false),
        this.texture(byName.get("roughness"), false),
      ]);
      if (!map) {
        this.missing.add(presetId);
        return;
      }
      const stats = measure(map);
      // Both the albedo and the surface are a flat tone: the procedural
      // pattern says more about the material than this does, so keep it.
      if (stats.spread < FLAT_LIMIT && measure(normalMap).spread < FLAT_LIMIT) {
        this.flat.add(presetId);
        this.missing.add(presetId);
        for (const t of [map, normalMap, roughnessMap]) t?.dispose();
        return;
      }
      // A 400 x 200 mm block repeats every 0.4 x 0.2 m of geometry, and the
      // geometry's UVs are already in meters, so repeat is 1 / tile size.
      const [tw, th] = entry.physical_size_mm ?? [1000, 1000];
      const repeat = new THREE.Vector2(1000 / Math.max(tw, 1), 1000 / Math.max(th, 1));
      for (const t of [map, normalMap, roughnessMap]) {
        if (t) t.repeat.copy(repeat);
      }
      this.sets.set(presetId, { map, normalMap, roughnessMap, repeat, albedo: stats.mean });
      for (const fn of this.listeners) fn(presetId);
    } catch {
      this.missing.add(presetId);
    } finally {
      this.pending.delete(presetId);
    }
  }

  private texture(file: string | undefined, srgb: boolean): Promise<THREE.Texture | null> {
    if (!file || !this.loader) return Promise.resolve(null);
    return new Promise((resolve) => {
      this.loader!.load(
        packUrl(file),
        (tex) => {
          tex.wrapS = tex.wrapT = THREE.RepeatWrapping;
          // sRGB on the color map only: normal and roughness are data.
          tex.colorSpace = srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
          tex.anisotropy = this.anisotropy;
          tex.generateMipmaps = true;
          tex.minFilter = THREE.LinearMipmapLinearFilter;
          tex.magFilter = THREE.LinearFilter;
          tex.needsUpdate = true;
          resolve(tex);
        },
        undefined,
        () => resolve(null),
      );
    });
  }
}

export const texturePack = new TexturePack();

// -------------------------------------------------------------------- models

/** A parsed catalog model, normalized to the viewer's asset frame. */
export interface PackModelSource {
  /** Footprint center at the origin, underside at y = 0, meters. */
  object: THREE.Object3D;
  /** Bounding box size in meters. */
  size: THREE.Vector3;
  /** Rotation about +y that brings the model's back to -z (the app convention). */
  backRotation: number;
}

/** Brings `back_axis` onto -z, which is plan +y at rotation 0 (assets.ts). */
function backRotation(axis: string | undefined): number {
  switch (axis) {
    case "+z":
      return Math.PI;
    // Every model in the pack is already "-z"; "n/a" (the symmetric dining
    // sets) needs no rotation either. Anything unexpected is left alone.
    default:
      return 0;
  }
}

/**
 * glTF may hand back an unlit material (KHR_materials_unlit), which three maps
 * to MeshBasicMaterial: no lighting, no shadow, and the highlight system only
 * tints MeshStandardMaterial. Furniture in an architectural model has to take
 * the sun, so a basic material is promoted, keeping its color and maps.
 */
function promoteUnlit(root: THREE.Object3D): void {
  const promoted = new Map<string, THREE.MeshStandardMaterial>();
  root.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (!mesh.isMesh || Array.isArray(mesh.material)) return;
    const mat = mesh.material as THREE.Material & { isMeshBasicMaterial?: boolean };
    if (!mat?.isMeshBasicMaterial) return;
    const basic = mat as unknown as THREE.MeshBasicMaterial;
    let next = promoted.get(basic.uuid);
    if (!next) {
      next = new THREE.MeshStandardMaterial({
        color: basic.color,
        map: basic.map,
        alphaMap: basic.alphaMap,
        transparent: basic.transparent,
        opacity: basic.opacity,
        alphaTest: basic.alphaTest,
        side: basic.side,
        roughness: 0.75,
        metalness: 0,
      });
      next.name = basic.name;
      promoted.set(basic.uuid, next);
    }
    mesh.material = next;
    basic.dispose();
  });
}

class ModelPack {
  private sources = new Map<string, PackModelSource>();
  private pending = new Set<string>();
  private missing = new Set<string>();
  private listeners = new Set<(key: string) => void>();
  private loader: GLTFLoader | null = null;

  onLoad(fn: (key: string) => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  has(key: string): boolean {
    return this.sources.has(key);
  }

  get(key: string): PackModelSource | null {
    return this.sources.get(key) ?? null;
  }

  /** Starts the load for a catalog key. Safe to call on every build. */
  request(key: string): void {
    if (this.sources.has(key) || this.pending.has(key) || this.missing.has(key)) return;
    this.pending.add(key);
    void this.load(key);
  }

  stats(): { loaded: number; pending: number; available: number } {
    const models = packManifest()?.models ?? {};
    let available = 0;
    for (const entry of Object.values(models)) if (entry.file) available++;
    return { loaded: this.sources.size, pending: this.pending.size, available };
  }

  private async load(key: string): Promise<void> {
    try {
      const mf = await loadPackManifest();
      const entry = mf?.models?.[key];
      if (!entry?.file) {
        this.missing.add(key);
        return;
      }
      this.loader ??= new GLTFLoader().setMeshoptDecoder(MeshoptDecoder);
      const gltf = await this.loader.loadAsync(packUrl(entry.file));
      const scene = gltf.scene;
      promoteUnlit(scene);
      scene.updateMatrixWorld(true);
      const box = new THREE.Box3().setFromObject(scene);
      if (box.isEmpty()) {
        this.missing.add(key);
        return;
      }
      const size = box.getSize(new THREE.Vector3());
      const center = box.getCenter(new THREE.Vector3());
      // Footprint center at the origin, underside at y = 0, like every
      // procedural form in assets.ts.
      const object = new THREE.Group();
      object.name = `pack-${key}`;
      scene.position.set(scene.position.x - center.x, scene.position.y - box.min.y, scene.position.z - center.z);
      object.add(scene);
      object.traverse((o) => {
        o.castShadow = true;
        o.receiveShadow = true;
      });
      object.updateMatrixWorld(true);
      this.sources.set(key, { object, size, backRotation: backRotation(entry.back_axis) });
      for (const fn of this.listeners) fn(key);
    } catch {
      this.missing.add(key);
    } finally {
      this.pending.delete(key);
    }
  }
}

export const modelPack = new ModelPack();
