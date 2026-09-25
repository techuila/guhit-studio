// MeshStandardMaterial cache built from contract Materials, with small
// procedural CanvasTexture patterns. Patterns are drawn in light greys and
// multiplied by the material color. UVs are in meters, so `repeat` is
// 1 / tile size and every pattern has its real world scale.
//
// A preset with a textured entry in the CC0 pack (scene/pack.ts) gets the real
// color, normal and roughness maps instead, at the same real world scale. The
// maps load async: the procedural pattern is what is on screen until they
// arrive, and then they are attached to the SAME material object, because the
// per-element merge in Kit.mergeByMaterial buckets by material identity.

import * as THREE from "three";
import type { Material, MaterialPattern } from "../../contract/bindings";
import { lampTint } from "../light/model";
import { texturePack, type PackTextureSet } from "./pack";

interface PatternSpec {
  /** Tile size in meters (u, v). */
  size: [number, number];
  px: [number, number];
  draw: (ctx: CanvasRenderingContext2D, w: number, h: number) => void;
}

function speckle(ctx: CanvasRenderingContext2D, w: number, h: number, count: number, alpha: number, seed: number) {
  let s = seed;
  const rnd = () => {
    s = (s * 1664525 + 1013904223) % 4294967296;
    return s / 4294967296;
  };
  for (let i = 0; i < count; i++) {
    const v = rnd() > 0.5 ? 255 : 0;
    ctx.fillStyle = `rgba(${v},${v},${v},${alpha * rnd()})`;
    const r = 1 + rnd() * 2.5;
    ctx.fillRect(rnd() * w, rnd() * h, r, r);
  }
}

const PATTERNS: Partial<Record<MaterialPattern, PatternSpec>> = {
  // 400 x 200 CHB in running bond: the tile holds two courses.
  chb: {
    size: [0.8, 0.4],
    px: [512, 256],
    draw(ctx, w, h) {
      ctx.fillStyle = "#f4f4f2";
      ctx.fillRect(0, 0, w, h);
      speckle(ctx, w, h, 900, 0.1, 7);
      ctx.strokeStyle = "rgba(90,90,86,0.75)";
      ctx.lineWidth = 5;
      const ch = h / 2;
      for (let row = 0; row < 2; row++) {
        const y = row * ch;
        ctx.beginPath();
        ctx.moveTo(0, y + 2.5);
        ctx.lineTo(w, y + 2.5);
        ctx.stroke();
        for (let k = 0; k <= 2; k++) {
          const x = (k * w) / 2 + (row === 1 ? w / 4 : 0);
          ctx.beginPath();
          ctx.moveTo(x % w, y);
          ctx.lineTo(x % w, y + ch);
          ctx.stroke();
        }
      }
    },
  },
  // Fair-faced concrete: 1200 x 600 formwork panels with tie holes.
  concrete: {
    size: [1.2, 0.6],
    px: [512, 256],
    draw(ctx, w, h) {
      ctx.fillStyle = "#f1f1ef";
      ctx.fillRect(0, 0, w, h);
      const g = ctx.createLinearGradient(0, 0, w, h);
      g.addColorStop(0, "rgba(0,0,0,0.05)");
      g.addColorStop(0.5, "rgba(255,255,255,0.04)");
      g.addColorStop(1, "rgba(0,0,0,0.06)");
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, w, h);
      speckle(ctx, w, h, 1400, 0.12, 11);
      ctx.strokeStyle = "rgba(60,60,58,0.35)";
      ctx.lineWidth = 2;
      ctx.strokeRect(1, 1, w - 2, h - 2);
      ctx.fillStyle = "rgba(50,50,48,0.45)";
      for (const [x, y] of [
        [0.12, 0.2],
        [0.88, 0.2],
        [0.12, 0.8],
        [0.88, 0.8],
      ]) {
        ctx.beginPath();
        ctx.arc(x * w, y * h, 5, 0, Math.PI * 2);
        ctx.fill();
      }
    },
  },
  // 600 x 600 tile with a thin grout line.
  tile: {
    size: [0.6, 0.6],
    px: [256, 256],
    draw(ctx, w, h) {
      ctx.fillStyle = "#f7f7f5";
      ctx.fillRect(0, 0, w, h);
      const g = ctx.createLinearGradient(0, 0, w, h);
      g.addColorStop(0, "rgba(255,255,255,0.35)");
      g.addColorStop(1, "rgba(0,0,0,0.05)");
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, w, h);
      speckle(ctx, w, h, 250, 0.05, 3);
      ctx.strokeStyle = "rgba(95,92,85,0.7)";
      ctx.lineWidth = 3;
      ctx.strokeRect(0, 0, w, h);
    },
  },
  // Planks 150 wide running along u, with staggered butt joints.
  wood_plank: {
    size: [1.8, 0.6],
    px: [512, 256],
    draw(ctx, w, h) {
      const rows = 4;
      const rh = h / rows;
      const tones = ["#f3eee8", "#e6dfd6", "#f8f4ee", "#ddd5ca"];
      for (let r = 0; r < rows; r++) {
        ctx.fillStyle = tones[r];
        ctx.fillRect(0, r * rh, w, rh);
        ctx.strokeStyle = "rgba(70,45,25,0.10)";
        ctx.lineWidth = 1;
        for (let k = 0; k < 5; k++) {
          const y = r * rh + ((k + 0.5) * rh) / 5 + Math.sin(r * 7 + k) * 2;
          ctx.beginPath();
          ctx.moveTo(0, y);
          ctx.bezierCurveTo(w * 0.3, y + 3, w * 0.6, y - 3, w, y);
          ctx.stroke();
        }
        ctx.strokeStyle = "rgba(50,30,15,0.55)";
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(0, r * rh + 1);
        ctx.lineTo(w, r * rh + 1);
        ctx.stroke();
        const jx = ((r * 0.37 + 0.2) % 1) * w;
        ctx.beginPath();
        ctx.moveTo(jx, r * rh);
        ctx.lineTo(jx, (r + 1) * rh);
        ctx.stroke();
      }
    },
  },
  // Corrugated or rib type sheet: ribs run down the slope (v), 250 mm apart.
  roof_sheet: {
    size: [0.25, 1],
    px: [128, 16],
    draw(ctx, w, h) {
      const g = ctx.createLinearGradient(0, 0, w, 0);
      g.addColorStop(0, "#c9c9c9");
      g.addColorStop(0.12, "#ffffff");
      g.addColorStop(0.3, "#f2f2f2");
      g.addColorStop(0.5, "#ededed");
      g.addColorStop(0.72, "#f4f4f4");
      g.addColorStop(0.9, "#d5d5d5");
      g.addColorStop(1, "#a9a9a9");
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, w, h);
    },
  },
  // Clay tiles: 300 wide, 330 exposed courses, half offset.
  roof_tile: {
    size: [0.6, 0.66],
    px: [256, 256],
    draw(ctx, w, h) {
      ctx.fillStyle = "#efefef";
      ctx.fillRect(0, 0, w, h);
      const cw = w / 2;
      const chh = h / 2;
      for (let row = 0; row < 2; row++) {
        for (let k = -1; k < 3; k++) {
          const x = k * cw + (row === 1 ? cw / 2 : 0);
          const y = row * chh;
          const g = ctx.createLinearGradient(x, 0, x + cw, 0);
          g.addColorStop(0, "rgba(0,0,0,0.30)");
          g.addColorStop(0.25, "rgba(255,255,255,0.25)");
          g.addColorStop(0.75, "rgba(255,255,255,0.05)");
          g.addColorStop(1, "rgba(0,0,0,0.30)");
          ctx.fillStyle = g;
          ctx.fillRect(x, y, cw, chh);
        }
        const s = ctx.createLinearGradient(0, row * chh, 0, row * chh + chh);
        s.addColorStop(0, "rgba(0,0,0,0.35)");
        s.addColorStop(0.18, "rgba(0,0,0,0)");
        ctx.fillStyle = s;
        ctx.fillRect(0, row * chh, w, chh);
      }
    },
  },
};

const FALLBACK: Material = {
  id: "fallback",
  name: "Fallback",
  category: "generic",
  color: "#d8d5cd",
  roughness: 0.85,
  metalness: 0,
  opacity: 1,
  pattern: "none",
  builtin: true,
};

/** Used when the project does not carry the built-in material any more. */
const BUILTIN_FALLBACKS: Record<string, Partial<Material>> = {
  "mat-chb-painted": { color: "#f2efe8", roughness: 0.9 },
  "mat-tile-ceramic": { color: "#e4e0d8", roughness: 0.35, pattern: "tile" },
  "mat-roof-longspan": { color: "#7a3b33", roughness: 0.45, metalness: 0.6, pattern: "roof_sheet" },
  "mat-glass-clear": { color: "#bfe3ee", roughness: 0.05, opacity: 0.35, pattern: "glass" },
  "mat-wood-door": { color: "#6e4428", roughness: 0.6, pattern: "wood_plank" },
  "mat-aluminum-frame": { color: "#3b3f45", roughness: 0.4, metalness: 0.8 },
  "mat-floor-concrete": { color: "#b3b1ab", roughness: 0.6, pattern: "concrete" },
  "mat-concrete-fairface": { color: "#bdbdb8", roughness: 0.85, pattern: "concrete" },
  "mat-steel": { color: "#4a4f57", roughness: 0.5, metalness: 0.7 },
};

/** How far a photo may be pushed to reach the preset's albedo. */
const TINT_LIMIT = 3;

/**
 * The preset color is the material's average albedo and the pack map is the
 * variation around it, so the color that multiplies the map is the preset
 * divided by what the photo already averages. Both are linear, which is where
 * the shader multiplies them.
 *
 * Multiplying the preset in as shot would darken the surface twice: a white
 * painted wall over a mid-grey plaster photo came out at 56 percent grey.
 * With this it lands on the preset's own value, and a dark red roof still
 * reads red over the grey steel it is painted on.
 */
function packTint(hex: string, albedo: THREE.Color): THREE.Color {
  const preset = new THREE.Color();
  try {
    preset.set(hex);
  } catch {
    preset.set(FALLBACK.color);
  }
  const at = (p: number, a: number) => (a > 0.004 ? Math.min(p / a, TINT_LIMIT) : 1);
  return new THREE.Color(at(preset.r, albedo.r), at(preset.g, albedo.g), at(preset.b, albedo.b));
}

/**
 * Swaps a preset's procedural pattern for the pack maps, in place. The
 * material object stays the same one every mesh already points at, so nothing
 * has to rebuild and the merged draw calls keep their buckets.
 */
function applyPack(mat: THREE.MeshStandardMaterial, set: PackTextureSet): void {
  mat.map = set.map;
  mat.normalMap = set.normalMap;
  mat.roughnessMap = set.roughnessMap;
  const hex = mat.userData.packColor as string | undefined;
  if (hex) mat.color.copy(packTint(hex, set.albedo));
  // A roughness map multiplies `roughness`, so the preset value would apply
  // twice. The map is the roughness now.
  if (set.roughnessMap) mat.roughness = 1;
  mat.needsUpdate = true;
}

/**
 * The part of the model a material dresses. The same preset used by a wall
 * and by a stair becomes two materials, so the X-ray and hidden shell modes
 * (engine/shell.ts) can fade walls and stairs by different amounts.
 */
export type MaterialCategory = "" | "wall" | "opening" | "floor" | "column" | "stair" | "asset" | "roof" | "pipe" | "ghost";

/** What a material looks like in the solid shell, kept so a shell mode can put it back. */
export interface SolidLook {
  opacity: number;
  transparent: boolean;
  depthWrite: boolean;
}

function rememberSolid(mat: THREE.Material, category: MaterialCategory): void {
  mat.userData.shellCat = category;
  mat.userData.solid = { opacity: mat.opacity, transparent: mat.transparent, depthWrite: mat.depthWrite } satisfies SolidLook;
}

export class MaterialLibrary {
  private textures = new Map<MaterialPattern, THREE.Texture>();
  private materials = new Map<string, THREE.MeshStandardMaterial>();
  /** Library copies of pack model materials (see `adopt`). */
  private adopted = new Map<string, THREE.Material>();
  /** Category of the materials handed out from now on. `buildScene` sets it per section and puts it back to "". */
  category: MaterialCategory = "";
  private used = new Set<string>();
  private byId = new Map<string, Material>();
  private anisotropy = 1;
  private packOn = true;
  /** Bumped when the pack is toggled, so old materials are rebuilt not reused. */
  private packGen = 0;
  private unsubscribe: (() => void) | null = null;
  /** A pack map just arrived: redraw the shadow map and ask for one frame. */
  onPackTexture: (() => void) | null = null;

  constructor() {
    this.unsubscribe = texturePack.onLoad((presetId) => this.attachPack(presetId));
  }

  /** 4 is where the sharpness gain stops paying for the sample cost. */
  setAnisotropy(a: number): void {
    this.anisotropy = Math.max(1, Math.min(a, 4));
    texturePack.setAnisotropy(a);
  }

  /** Dev toggle: false keeps the procedural CanvasTextures everywhere. */
  setPackEnabled(on: boolean): void {
    if (on === this.packOn) return;
    this.packOn = on;
    this.packGen++;
  }

  packEnabled(): boolean {
    return this.packOn;
  }

  /** Counts for the dev readout. */
  packStats(): { loaded: number; pending: number; flat: string[] } {
    return texturePack.stats();
  }

  /** The pack set for a preset, requesting the load the first time it is asked for. */
  private packFor(m: Material): PackTextureSet | null {
    if (!this.packOn) return null;
    const set = texturePack.get(m.id);
    if (set) return set;
    texturePack.request(m.id);
    return null;
  }

  /** Attaches a set that finished loading to the materials that asked for it. */
  private attachPack(presetId: string): void {
    if (!this.packOn) return;
    const set = texturePack.get(presetId);
    if (!set) return;
    let touched = false;
    for (const mat of this.materials.values()) {
      if (mat.userData.packId !== presetId || mat.map === set.map) continue;
      applyPack(mat, set);
      touched = true;
    }
    if (touched) this.onPackTexture?.();
  }

  /** Call before a rebuild with the project's materials. */
  begin(materials: Material[]): void {
    this.byId = new Map(materials.map((m) => [m.id, m]));
    this.used.clear();
  }

  /**
   * Marks a material this library made as still in use. Meshes reused from
   * the build cache never ask for their material again, so without this
   * `end` would dispose a material that is still on screen.
   */
  keep(mat: THREE.Material): void {
    const key = mat.userData?.libKey as string | undefined;
    if (key && (this.materials.get(key) === mat || this.adopted.get(key) === mat)) this.used.add(key);
  }

  /** Call after a rebuild: materials that were not requested are disposed. */
  end(): void {
    for (const map of [this.materials, this.adopted] as Map<string, THREE.Material>[]) {
      for (const [key, mat] of map) {
        if (!this.used.has(key)) {
          mat.dispose();
          map.delete(key);
        }
      }
    }
  }

  /**
   * This library's own copy of a material that came with a pack model. Pack
   * models share their materials with every engine and with the exporter, so
   * the shell modes fade the copy, never the original. Textures stay shared:
   * a material copy only points at them.
   */
  adopt(source: THREE.Material): THREE.Material {
    const key = ["adopt", source.uuid, this.category].join("|");
    this.used.add(key);
    let mat = this.adopted.get(key);
    if (mat) return mat;
    mat = source.clone();
    mat.userData = { ...source.userData, libKey: key };
    rememberSolid(mat, this.category);
    this.adopted.set(key, mat);
    return mat;
  }

  resolve(id: string | null | undefined, fallbackId?: string): Material {
    for (const key of [id, fallbackId]) {
      if (!key) continue;
      const m = this.byId.get(key);
      if (m) return m;
      const fb = BUILTIN_FALLBACKS[key];
      if (fb) return { ...FALLBACK, id: key, ...fb };
    }
    return FALLBACK;
  }

  /**
   * Material for a contract material id. `textured` is only for geometry that
   * carries UVs in meters (walls, floors, roofs); boxes and props stay plain.
   */
  get(id: string | null | undefined, fallbackId?: string, textured = true): THREE.MeshStandardMaterial {
    return this.fromSpec(this.resolve(id, fallbackId), textured);
  }

  /** A plain colored material that is not in the project list (furniture and so on). */
  plain(color: string, roughness = 0.8, metalness = 0): THREE.MeshStandardMaterial {
    return this.fromSpec({ ...FALLBACK, id: `plain-${color}`, color, roughness, metalness }, false);
  }

  /** Translucent tint for a ghost overlay (an AI proposal's removed elements). */
  ghost(color: string, opacity = 0.32): THREE.MeshStandardMaterial {
    return this.fromSpec({ ...FALLBACK, id: `ghost-${color}`, color, roughness: 0.9, metalness: 0, opacity }, false);
  }

  /**
   * A pipe system's material: the token color, with a little of it as
   * emissive so a pipe still reads in shadow and through an X-ray shell.
   */
  pipe(color: string): THREE.MeshStandardMaterial {
    const key = ["pipe", color, this.category].join("|");
    this.used.add(key);
    let mat = this.materials.get(key);
    if (mat) return mat;
    const c = new THREE.Color();
    try {
      c.set(color);
    } catch {
      c.set(FALLBACK.color);
    }
    mat = new THREE.MeshStandardMaterial({
      color: c,
      roughness: 0.42,
      metalness: 0.05,
      emissive: c.clone().multiplyScalar(0.28),
    });
    mat.userData.glass = false;
    mat.userData.libKey = key;
    rememberSolid(mat, this.category);
    this.materials.set(key, mat);
    return mat;
  }

  /**
   * The diffuser of one light fixture: off-white, with an emissive glow in the
   * lamp's color that the light rig (light/LightRig.ts) turns up while the
   * lamp is lit. One per fixture, so a single fixture can be switched on or
   * off in the view (walk mode switches), and kept across rebuilds so a lit
   * lamp never flickers when the model changes. It casts no shadow in the
   * path tracer either (`castShadow`, read by three-gpu-pathtracer).
   */
  lampGlow(elementId: string, kelvin: number): THREE.MeshStandardMaterial {
    const key = ["glow", elementId, Math.round(kelvin), this.category].join("|");
    this.used.add(key);
    let mat = this.materials.get(key);
    if (mat) return mat;
    const [r, g, b] = lampTint(kelvin);
    mat = new THREE.MeshStandardMaterial({
      color: new THREE.Color(0.9, 0.89, 0.86),
      roughness: 0.55,
      metalness: 0,
      emissive: new THREE.Color(r, g, b),
      emissiveIntensity: 0,
      polygonOffset: true,
      polygonOffsetFactor: 1,
      polygonOffsetUnits: 1,
    });
    (mat as THREE.MeshStandardMaterial & { castShadow?: boolean }).castShadow = false;
    mat.userData.glass = false;
    mat.userData.libKey = key;
    mat.userData.lampGlow = elementId;
    rememberSolid(mat, this.category);
    this.materials.set(key, mat);
    return mat;
  }

  /** Every material the library holds right now. The shell modes walk it. */
  *all(): IterableIterator<THREE.Material> {
    yield* this.materials.values();
    yield* this.adopted.values();
  }

  private fromSpec(m: Material, textured: boolean): THREE.MeshStandardMaterial {
    const opacity = Number.isFinite(m.opacity) ? Math.min(Math.max(m.opacity, 0.05), 1) : 1;
    // Only geometry with UVs in meters takes a texture, so `textured` also
    // decides whether the pack maps apply.
    const pack = textured ? this.packFor(m) : null;
    const packId = textured && this.packOn ? m.id : "";
    const key = [m.color, m.roughness, m.metalness, opacity, textured ? m.pattern : "none", packId, this.packGen, this.category].join("|");
    this.used.add(key);
    let mat = this.materials.get(key);
    if (mat) return mat;
    const color = new THREE.Color();
    try {
      color.set(m.color);
    } catch {
      color.set(FALLBACK.color);
    }
    const glass = opacity < 0.999;
    // Painted metal sheets: keep the sun highlight from washing out the color.
    const roughFloor = m.metalness > 0.4 && !glass ? 0.55 : 0.02;
    mat = new THREE.MeshStandardMaterial({
      color,
      roughness: Math.min(Math.max(m.roughness, roughFloor), 1),
      metalness: Math.min(Math.max(m.metalness, 0), 1),
      transparent: glass,
      opacity,
      depthWrite: !glass,
      side: glass ? THREE.DoubleSide : THREE.FrontSide,
      // Lets selection outlines sit on top of the faces without fighting.
      polygonOffset: true,
      polygonOffsetFactor: 1,
      polygonOffsetUnits: 1,
    });
    if (glass) mat.envMapIntensity = 1.6;
    const tex = textured ? this.texture(m.pattern) : null;
    if (tex) mat.map = tex;
    mat.userData.glass = glass;
    mat.userData.libKey = key;
    rememberSolid(mat, this.category);
    if (packId) {
      mat.userData.packId = packId;
      mat.userData.packColor = m.color;
      // Loaded already: the real maps go on straight away, no placeholder frame.
      if (pack) applyPack(mat, pack);
    }
    this.materials.set(key, mat);
    return mat;
  }

  private texture(pattern: MaterialPattern): THREE.Texture | null {
    const spec = PATTERNS[pattern];
    if (!spec || typeof document === "undefined") return null;
    let tex = this.textures.get(pattern);
    if (tex) return tex;
    const canvas = document.createElement("canvas");
    canvas.width = spec.px[0];
    canvas.height = spec.px[1];
    const ctx = canvas.getContext("2d");
    if (!ctx) return null;
    spec.draw(ctx, canvas.width, canvas.height);
    tex = new THREE.CanvasTexture(canvas);
    tex.wrapS = tex.wrapT = THREE.RepeatWrapping;
    tex.repeat.set(1 / spec.size[0], 1 / spec.size[1]);
    tex.colorSpace = THREE.SRGBColorSpace;
    tex.anisotropy = this.anisotropy;
    // The mip chain is built once, on first upload. These textures never
    // change after that, and a model change reuses this same cache entry.
    tex.generateMipmaps = true;
    tex.minFilter = THREE.LinearMipmapLinearFilter;
    tex.magFilter = THREE.LinearFilter;
    tex.needsUpdate = true;
    this.textures.set(pattern, tex);
    return tex;
  }

  /** Counts for the leak check in the dev harness. */
  stats(): { materials: number; textures: number } {
    return { materials: this.materials.size + this.adopted.size, textures: this.textures.size };
  }

  dispose(): void {
    this.unsubscribe?.();
    this.unsubscribe = null;
    this.onPackTexture = null;
    for (const m of this.materials.values()) m.dispose();
    for (const m of this.adopted.values()) m.dispose();
    this.adopted.clear();
    // Only the procedural CanvasTextures belong to this library. Pack textures
    // are shared app data owned by scene/pack.ts and outlive every engine.
    for (const t of this.textures.values()) t.dispose();
    this.materials.clear();
    this.textures.clear();
  }
}
