// Low-poly forms for the asset catalog in crates/guhit-model/src/defaults.rs.
//
// Local frame, meters: x spans the width, z spans the depth, y is up from the
// underside of the asset. The BACK of an item (headboard, sofa back, WC tank,
// counter splash side) is at -z, which is plan +y at rotation 0. Unknown keys
// get a plain box.

import * as THREE from "three";
import type { Asset } from "../../contract/bindings";
import type { Kit } from "./kit";
import type { MaterialLibrary } from "./materials";
import { modelPack } from "./pack";

interface Ctx {
  kit: Kit;
  lib: MaterialLibrary;
  g: THREE.Group;
  w: number;
  d: number;
  h: number;
  /** Underside height above the floor, meters. */
  lift: number;
  /** World Y of this asset's local origin, meters. Used to clip parts under cutaway. */
  baseY: number;
}

type Builder = (c: Ctx) => void;

const WOOD = "#8b6844";
const WOOD_DARK = "#5f4630";
const FABRIC = "#8e9aa3";
const FABRIC_2 = "#a7b1b8";
const LINEN = "#f3f0e9";
const WHITE = "#f6f6f4";
const STEEL = "#b9bec4";
const DARK = "#2c3036";
const STONE = "#5d5e63";
const LEAF = "#5f8f4e";
const LEAF_2 = "#4d7d43";

function box(c: Ctx, color: string, sx: number, sy: number, sz: number, x: number, y: number, z: number, rough = 0.8, metal = 0) {
  return c.kit.box(c.g, c.lib.plain(color, rough, metal), sx, sy, sz, x, y, z, { baseY: c.baseY });
}

function legs(c: Ctx, color: string, size: number, height: number, inset: number, w = c.w, d = c.d) {
  for (const sx of [-1, 1]) {
    for (const sz of [-1, 1]) {
      box(c, color, size, height, size, sx * (w / 2 - inset), 0, sz * (d / 2 - inset));
    }
  }
}

const bed =
  (pillows: number): Builder =>
  (c) => {
    const base = Math.min(0.28, c.h * 0.55);
    box(c, WOOD, c.w, base, c.d, 0, 0, 0);
    box(c, LINEN, c.w - 0.06, c.h - base, c.d - 0.1, 0, base, 0.02, 0.95);
    box(c, WOOD_DARK, c.w, c.h + 0.45, 0.06, 0, 0, -c.d / 2 + 0.03);
    // blanket over the foot end
    box(c, "#7f9aa8", c.w - 0.04, 0.035, c.d * 0.58, 0, c.h - 0.01, c.d / 2 - c.d * 0.29 - 0.03, 0.95);
    const pw = Math.min(0.55, (c.w - 0.2) / pillows - 0.06);
    for (let i = 0; i < pillows; i++) {
      const x = pillows === 1 ? 0 : (i === 0 ? -1 : 1) * (c.w / 4);
      box(c, WHITE, pw, 0.11, 0.36, x, c.h, -c.d / 2 + 0.32, 0.95);
    }
  };

const sofa =
  (seats: number): Builder =>
  (c) => {
    const arm = Math.min(0.18, c.w * 0.18);
    const seatH = Math.min(0.3, c.h * 0.4);
    box(c, FABRIC, c.w, seatH, c.d, 0, 0.06, 0, 0.95);
    box(c, FABRIC, c.w, c.h - 0.06, 0.22, 0, 0.06, -c.d / 2 + 0.11, 0.95);
    for (const s of [-1, 1]) box(c, FABRIC, arm, c.h * 0.72, c.d, s * (c.w / 2 - arm / 2), 0.06, 0, 0.95);
    legs(c, WOOD_DARK, 0.05, 0.06, 0.06);
    const inner = c.w - 2 * arm;
    const cw = inner / seats;
    for (let i = 0; i < seats; i++) {
      const x = -inner / 2 + cw * (i + 0.5);
      box(c, FABRIC_2, cw - 0.02, 0.12, c.d - 0.26, x, 0.06 + seatH, 0.1, 0.95);
      box(c, FABRIC_2, cw - 0.04, c.h * 0.42, 0.12, x, 0.06 + seatH + 0.1, -c.d / 2 + 0.28, 0.95);
    }
  };

const table =
  (chairsPerSide: number): Builder =>
  (c) => {
    box(c, WOOD, c.w, 0.04, c.d, 0, c.h - 0.04, 0, 0.6);
    legs(c, WOOD_DARK, 0.06, c.h - 0.04, 0.08);
    for (let i = 0; i < chairsPerSide; i++) {
      const x = -c.w / 2 + (c.w / chairsPerSide) * (i + 0.5);
      for (const s of [-1, 1]) {
        const z = s * (c.d / 2 - 0.08);
        box(c, WOOD_DARK, 0.4, 0.04, 0.4, x, 0.43, z);
        box(c, WOOD_DARK, 0.4, 0.45, 0.04, x, 0.43, z + s * 0.18);
        for (const lx of [-1, 1]) for (const lz of [-1, 1]) box(c, WOOD_DARK, 0.035, 0.43, 0.035, x + lx * 0.17, 0, z + lz * 0.17);
      }
    }
  };

const counter =
  (sink: boolean): Builder =>
  (c) => {
    box(c, DARK, c.w, 0.1, c.d - 0.08, 0, 0, -0.02);
    box(c, "#ece9e2", c.w, c.h - 0.14, c.d - 0.03, 0, 0.1, -0.015, 0.7);
    box(c, STONE, c.w, 0.04, c.d, 0, c.h - 0.04, 0, 0.3);
    // door gaps
    const doors = Math.max(2, Math.round(c.w / 0.45));
    for (let i = 1; i < doors; i++) {
      box(c, "#bdb9b0", 0.006, c.h - 0.2, 0.004, -c.w / 2 + (c.w / doors) * i, 0.13, c.d / 2 - 0.03 + 0.001);
    }
    if (sink) {
      box(c, STEEL, Math.min(0.55, c.w * 0.5), 0.006, c.d * 0.6, 0, c.h - 0.002, 0.02, 0.25, 0.9);
      box(c, "#70757c", Math.min(0.5, c.w * 0.45), 0.004, c.d * 0.5, 0, c.h + 0.005, 0.02, 0.4, 0.8);
      c.kit.cylinder(c.g, c.lib.plain(STEEL, 0.25, 0.9), 0.015, 0.22, 0, c.h, -c.d * 0.36, undefined, { baseY: c.baseY });
      box(c, STEEL, 0.025, 0.02, 0.14, 0, c.h + 0.2, -c.d * 0.36 + 0.07, 0.25, 0.9);
    }
  };

const BUILDERS: Record<string, Builder> = {
  "bed-single": bed(1),
  "bed-double": bed(2),
  "bed-queen": bed(2),

  wardrobe: (c) => {
    box(c, WOOD, c.w, c.h, c.d - 0.02, 0, 0, -0.01, 0.6);
    const doors = c.w > 0.9 ? 2 : 1;
    const dw = c.w / doors;
    for (let i = 0; i < doors; i++) {
      const x = -c.w / 2 + dw * (i + 0.5);
      box(c, "#9a7650", dw - 0.012, c.h - 0.1, 0.02, x, 0.08, c.d / 2 - 0.01, 0.6);
      const hx = doors === 1 ? x + dw / 2 - 0.06 : x + (i === 0 ? 1 : -1) * (dw / 2 - 0.05);
      box(c, STEEL, 0.015, 0.22, 0.025, hx, c.h * 0.48, c.d / 2 + 0.012, 0.3, 0.8);
    }
  },

  "sofa-3": sofa(3),
  "sofa-2": sofa(2),
  armchair: sofa(1),

  "coffee-table": (c) => {
    box(c, WOOD, c.w, 0.04, c.d, 0, c.h - 0.04, 0, 0.55);
    box(c, WOOD, c.w - 0.2, 0.02, c.d - 0.16, 0, c.h * 0.35, 0, 0.6);
    legs(c, WOOD_DARK, 0.05, c.h - 0.04, 0.06);
  },

  "tv-console": (c) => {
    box(c, WOOD, c.w, c.h - 0.1, c.d, 0, 0.1, 0, 0.6);
    legs(c, WOOD_DARK, 0.04, 0.1, 0.08);
    const tw = Math.min(1.15, c.w * 0.75);
    // The screen stands against the back, matching the 2D symbol.
    const zb = -c.d / 2 + 0.09;
    box(c, DARK, 0.3, 0.02, 0.18, 0, c.h, zb, 0.4);
    box(c, DARK, 0.04, 0.08, 0.04, 0, c.h + 0.02, zb, 0.4);
    box(c, "#15171a", tw, tw * 0.57, 0.035, 0, c.h + 0.09, zb, 0.25, 0.2);
  },

  "dining-4": table(2),
  "dining-6": table(3),

  desk: (c) => {
    box(c, WOOD, c.w, 0.035, c.d, 0, c.h - 0.035, 0, 0.55);
    for (const s of [-1, 1]) box(c, WOOD_DARK, 0.03, c.h - 0.035, c.d - 0.06, s * (c.w / 2 - 0.04), 0, 0);
    box(c, WOOD_DARK, c.w - 0.1, c.h * 0.4, 0.02, 0, c.h * 0.5, -c.d / 2 + 0.06);
    box(c, DARK, 0.2, 0.015, 0.14, 0, c.h, -c.d * 0.2, 0.4);
    box(c, "#15171a", 0.55, 0.33, 0.02, 0, c.h + 0.08, -c.d * 0.2, 0.25, 0.2);
  },

  wc: (c) => {
    const tankD = Math.min(0.2, c.d * 0.3);
    const bowlH = Math.min(0.4, c.h * 0.52);
    box(c, WHITE, c.w * 0.92, c.h - bowlH + 0.04, tankD, 0, bowlH - 0.04, -c.d / 2 + tankD / 2, 0.25);
    box(c, WHITE, c.w * 0.96, 0.03, tankD + 0.02, 0, c.h - 0.03, -c.d / 2 + tankD / 2, 0.25);
    const bowlLen = c.d - tankD;
    const white = c.lib.plain(WHITE, 0.25);
    c.kit.cylinder(c.g, white, c.w * 0.3, bowlH * 0.55, 0, 0, -c.d / 2 + tankD + bowlLen * 0.42, bowlLen * 0.36, { baseY: c.baseY });
    c.kit.cylinder(c.g, white, c.w * 0.46, bowlH * 0.45, 0, bowlH * 0.55, -c.d / 2 + tankD + bowlLen / 2, bowlLen / 2, { baseY: c.baseY });
    c.kit.cylinder(c.g, c.lib.plain("#e4e4e0", 0.3), c.w * 0.47, 0.025, 0, bowlH, -c.d / 2 + tankD + bowlLen / 2, bowlLen / 2 + 0.005, { baseY: c.baseY });
  },

  lavatory: (c) => {
    box(c, WHITE, c.w, c.h, c.d, 0, 0, 0, 0.25);
    const white = c.lib.plain("#dfe3e4", 0.2);
    c.kit.cylinder(c.g, white, c.w * 0.36, 0.012, 0, c.h - 0.008, 0.03, c.d * 0.3, { baseY: c.baseY });
    c.kit.cylinder(c.g, c.lib.plain(STEEL, 0.25, 0.9), 0.014, 0.12, 0, c.h, -c.d / 2 + 0.06, undefined, { baseY: c.baseY });
    box(c, STEEL, 0.022, 0.018, 0.1, 0, c.h + 0.1, -c.d / 2 + 0.11, 0.25, 0.9);
    // pedestal down to the floor
    if (c.lift > 0.05) c.kit.cylinder(c.g, c.lib.plain(WHITE, 0.25), 0.09, c.lift, 0, -c.lift, -c.d * 0.12);
  },

  shower: (c) => {
    box(c, "#e9ecec", c.w, Math.max(c.h, 0.04), c.d, 0, 0, 0, 0.3);
    const glass = c.lib.get("mat-glass-clear", undefined, false);
    c.kit.box(c.g, glass, c.w, 1.9, 0.01, 0, c.h, c.d / 2 - 0.005, { baseY: c.baseY });
    c.kit.box(c.g, glass, 0.01, 1.9, c.d, c.w / 2 - 0.005, c.h, 0, { baseY: c.baseY });
    const steel = c.lib.plain(STEEL, 0.25, 0.9);
    c.kit.cylinder(c.g, steel, 0.012, 2.0, -c.w / 2 + 0.08, c.h, -c.d / 2 + 0.06, undefined, { baseY: c.baseY });
    c.kit.cylinder(c.g, steel, 0.09, 0.015, -c.w / 2 + 0.16, c.h + 1.98, -c.d / 2 + 0.14, undefined, { baseY: c.baseY });
  },

  bathtub: (c) => {
    box(c, WHITE, c.w, c.h, c.d, 0, 0, 0, 0.2);
    box(c, "#cfe3e8", c.w - 0.16, 0.01, c.d - 0.16, 0, c.h - 0.004, 0, 0.1);
    // Faucet end matches the 2D symbol: the +x end of the tub.
    c.kit.cylinder(c.g, c.lib.plain(STEEL, 0.25, 0.9), 0.015, 0.14, c.w / 2 - 0.12, c.h, 0, undefined, { baseY: c.baseY });
  },

  "kitchen-counter": counter(false),
  "kitchen-sink": counter(true),

  range: (c) => {
    box(c, "#d9dbdd", c.w, c.h, c.d, 0, 0, 0, 0.35, 0.5);
    box(c, DARK, c.w - 0.04, 0.012, c.d - 0.06, 0, c.h, 0, 0.3, 0.2);
    const burner = c.lib.plain("#15171a", 0.6);
    for (const sx of [-1, 1]) for (const sz of [-1, 1]) c.kit.cylinder(c.g, burner, 0.085, 0.02, sx * c.w * 0.22, c.h + 0.012, sz * c.d * 0.2, undefined, { baseY: c.baseY });
    box(c, DARK, c.w - 0.1, c.h * 0.48, 0.01, 0, c.h * 0.16, c.d / 2 + 0.004, 0.2, 0.3);
    box(c, STEEL, c.w - 0.16, 0.02, 0.03, 0, c.h * 0.68, c.d / 2 + 0.02, 0.3, 0.9);
    box(c, "#d9dbdd", c.w, 0.1, 0.03, 0, c.h, -c.d / 2 + 0.015, 0.35, 0.5);
  },

  refrigerator: (c) => {
    box(c, "#c9ced3", c.w, c.h, c.d - 0.05, 0, 0, -0.025, 0.35, 0.6);
    const split = c.h * 0.68;
    box(c, "#d5dade", c.w - 0.01, split - 0.03, 0.05, 0, 0.02, c.d / 2 - 0.025, 0.3, 0.6);
    box(c, "#d5dade", c.w - 0.01, c.h - split - 0.02, 0.05, 0, split, c.d / 2 - 0.025, 0.3, 0.6);
    box(c, DARK, 0.02, 0.35, 0.03, -c.w / 2 + 0.06, split - 0.45, c.d / 2 + 0.015, 0.4, 0.5);
    box(c, DARK, 0.02, 0.22, 0.03, -c.w / 2 + 0.06, split + 0.08, c.d / 2 + 0.015, 0.4, 0.5);
  },

  "washing-machine": (c) => {
    box(c, WHITE, c.w, c.h, c.d, 0, 0, 0, 0.3);
    box(c, "#d8dcdf", c.w, 0.1, 0.01, 0, c.h - 0.12, c.d / 2 + 0.003, 0.4);
    const door = c.kit.cylinder(c.g, c.lib.plain("#3a4048", 0.2, 0.4), c.w * 0.3, 0.03, 0, 0, 0);
    door.rotation.x = Math.PI / 2;
    door.position.set(0, c.h * 0.45, c.d / 2 + 0.012);
  },

  "plant-pot": (c) => {
    const potH = Math.min(0.38, c.h * 0.32);
    const r = Math.min(c.w, c.d) / 2;
    c.kit.cylinder(c.g, c.lib.plain("#b9704f", 0.85), r * 0.55, potH, 0, 0, 0);
    c.kit.cylinder(c.g, c.lib.plain("#4a3a2c", 0.95), r * 0.5, 0.01, 0, potH, 0);
    c.kit.cylinder(c.g, c.lib.plain("#6b5138", 0.9), 0.02, c.h * 0.35, 0, potH, 0);
    const leaf = c.lib.plain(LEAF, 0.9);
    const leaf2 = c.lib.plain(LEAF_2, 0.9);
    const top = c.h - potH;
    c.kit.ball(c.g, leaf, r * 0.85, top * 0.36, r * 0.85, 0, potH + top * 0.55, 0);
    c.kit.ball(c.g, leaf2, r * 0.6, top * 0.28, r * 0.6, r * 0.3, potH + top * 0.78, r * 0.1);
    c.kit.ball(c.g, leaf2, r * 0.55, top * 0.25, r * 0.55, -r * 0.35, potH + top * 0.4, -r * 0.2);
  },

  tree: (c) => {
    const r = Math.min(c.w, c.d) / 2;
    const trunkH = c.h * 0.45;
    c.kit.cylinder(c.g, c.lib.plain("#6d5238", 0.95), Math.max(0.1, r * 0.09), trunkH, 0, 0, 0);
    const leaf = c.lib.plain(LEAF, 0.95);
    const leaf2 = c.lib.plain(LEAF_2, 0.95);
    const crown = c.h - trunkH;
    c.kit.ball(c.g, leaf, r, crown * 0.52, r, 0, trunkH + crown * 0.42, 0);
    c.kit.ball(c.g, leaf2, r * 0.7, crown * 0.4, r * 0.7, r * 0.42, trunkH + crown * 0.25, r * 0.2);
    c.kit.ball(c.g, leaf2, r * 0.66, crown * 0.38, r * 0.66, -r * 0.4, trunkH + crown * 0.3, -r * 0.3);
    c.kit.ball(c.g, leaf, r * 0.6, crown * 0.34, r * 0.6, 0.1 * r, trunkH + crown * 0.72, -0.1 * r);
  },

  "car-sedan": (c) => {
    const paint = "#9aa7b3";
    const wheelR = Math.min(0.33, c.h * 0.24);
    const bodyY = wheelR * 0.7;
    const bodyH = c.h * 0.42;
    box(c, paint, c.w, bodyH, c.d, 0, bodyY, 0, 0.35, 0.5);
    box(c, paint, c.w * 0.96, c.h * 0.08, c.d * 0.9, 0, bodyY + bodyH, 0, 0.35, 0.5);
    const cabH = c.h - bodyY - bodyH;
    box(c, "#27313b", c.w * 0.86, cabH - 0.04, c.d * 0.46, 0, bodyY + bodyH, -c.d * 0.04, 0.15, 0.3);
    box(c, paint, c.w * 0.84, 0.05, c.d * 0.4, 0, c.h - 0.05, -c.d * 0.04, 0.35, 0.5);
    box(c, "#f3efd8", c.w * 0.2, 0.08, 0.02, -c.w * 0.3, bodyY + bodyH * 0.55, c.d / 2 + 0.005, 0.2);
    box(c, "#f3efd8", c.w * 0.2, 0.08, 0.02, c.w * 0.3, bodyY + bodyH * 0.55, c.d / 2 + 0.005, 0.2);
    box(c, "#a3352d", c.w * 0.2, 0.08, 0.02, -c.w * 0.3, bodyY + bodyH * 0.6, -c.d / 2 - 0.005, 0.3);
    box(c, "#a3352d", c.w * 0.2, 0.08, 0.02, c.w * 0.3, bodyY + bodyH * 0.6, -c.d / 2 - 0.005, 0.3);
    const tire = c.lib.plain("#1b1d20", 0.9);
    for (const sx of [-1, 1]) {
      for (const sz of [-1, 1]) {
        const wheel = c.kit.cylinder(c.g, tire, wheelR, 0.22, 0, 0, 0);
        wheel.rotation.z = Math.PI / 2;
        wheel.position.set(sx * (c.w / 2 - 0.1), wheelR, sz * c.d * 0.31);
      }
    }
  },
};

export const KNOWN_ASSET_KEYS = Object.keys(BUILDERS);

/** Non-uniform scale past this much distortion is worse than a smaller model. */
const SQUASH_LIMIT = 1.15;

const clampDim = (v: number, fallback: number) => (Number.isFinite(v) && v > 1 ? v / 1000 : fallback);

/**
 * Instance of the pack's GLB for this catalog key, or null when the model is
 * not loaded (or there is none, like `tree` and `car-sedan`). The clone shares
 * the loaded geometry and materials, so three hundred chairs are three hundred
 * light objects over one set of buffers: no merge, no per-instance geometry.
 *
 * The model comes out of scene/pack.ts with its footprint center at the origin
 * and its underside at y = 0, the same frame the procedural forms use, so the
 * placement in buildScene is unchanged.
 */
export function packAssetForm(asset: Asset): THREE.Group | null {
  const src = modelPack.get(asset.catalog_key);
  if (!src) return null;
  const g = new THREE.Group();
  const inner = new THREE.Group();
  inner.add(src.object.clone(true));
  // A back that is not already at -z is turned onto it; a quarter turn swaps
  // which element dimension the model's own width and depth have to match.
  const quarter = Math.abs(Math.sin(src.backRotation)) > 0.5;
  const w = clampDim(asset.width_mm, 0.5);
  const d = clampDim(asset.depth_mm, 0.5);
  const h = clampDim(asset.height_mm, 0.5);
  const tw = quarter ? d : w;
  const td = quarter ? w : d;
  let sx = tw / Math.max(src.size.x, 1e-4);
  let sy = h / Math.max(src.size.y, 1e-4);
  let sz = td / Math.max(src.size.z, 1e-4);
  const lo = Math.min(sx, sy, sz);
  const hi = Math.max(sx, sy, sz);
  if (!(lo > 0) || hi / lo > SQUASH_LIMIT) {
    // Too far from the model's own proportions to stretch: scale to the
    // element width instead. The model is already centered on its footprint.
    sy = sz = sx;
  }
  inner.scale.set(sx, sy, sz);
  inner.rotation.y = src.backRotation;
  g.add(inner);
  return g;
}

/**
 * Group with its origin at the center of the footprint, underside at y = 0.
 * `baseY` is the world Y (meters) of that origin, used to clip parts against
 * the cutaway height set on `kit.cutY`. With `usePack`, the pack's GLB is used
 * when it is loaded and the procedural form is the placeholder until then.
 */
export function buildAssetForm(kit: Kit, lib: MaterialLibrary, asset: Asset, baseY = 0, usePack = false): THREE.Group {
  if (usePack) {
    const packed = packAssetForm(asset);
    if (packed) return packed;
  }
  const g = new THREE.Group();
  const c: Ctx = {
    kit,
    lib,
    g,
    w: clampDim(asset.width_mm, 0.5),
    d: clampDim(asset.depth_mm, 0.5),
    h: clampDim(asset.height_mm, 0.5),
    lift: Math.max(asset.elevation_mm, 0) / 1000,
    baseY,
  };
  const builder = BUILDERS[asset.catalog_key];
  if (builder) builder(c);
  else box(c, "#c9c5bb", c.w, c.h, c.d, 0, 0, 0);
  // One draw call per material instead of one per little box. Still one
  // element, so picking and highlighting are unchanged.
  kit.mergeByMaterial(g);
  return g;
}
