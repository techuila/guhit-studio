// Low-poly forms for the asset catalog in crates/guhit-model/src/defaults.rs.
//
// Local frame, meters: x spans the width, z spans the depth, y is up from the
// underside of the asset. The BACK of an item (headboard, sofa back, WC tank,
// counter splash side) is at -z, which is plan +y at rotation 0. Unknown keys
// get a plain box.
//
// Wall-mounted items (switches, outlets, panelboard, wall lights, split
// indoor units, meters) have their back on the wall face, z = -d/2, and
// nothing behind it. Ceiling items touch the ceiling with their top, y = h.
// Small devices use the kit's low-poly rounds: a switch plate stays under 200
// triangles, an aircon unit under 3000 (tested in assets.test.ts).
//
// Light fixtures, for the light module (docs/CONTRACT.md, "Sun and light"):
//
// - The glowing part (diffuser, lens, tube, bulb, lamp shade) is drawn with
//   the fixture's glow material. When the library has a per-fixture one,
//   `MaterialLibrary.lampGlow(elementId, kelvin)` (tagged
//   `material.userData.lampGlow`, driven by the light rig in light/lamps.ts
//   so walk mode can switch one fixture), that is it. Otherwise it is one
//   shared material per color temperature, `lampMaterial(lib, kelvin)`,
//   tagged `material.userData.lampKelvin`. Either way the emissive color is
//   the lamp's and `emissiveIntensity` starts at 0: turning a lamp on is
//   raising it. Every mesh drawn with it has `mesh.userData.lampPart = true`
//   and casts no shadow, so a light inside the fixture is not blocked by its
//   own diffuser. A fixture with no `Asset::light` gives no light: plain
//   diffuser, no tag.
// - The asset group carries `group.userData.lamp: LampInfo`: `anchor`, the
//   point the light comes from, in this same local frame (meters, rotate and
//   place it with the group, or use `group.localToWorld`); `aim`, the main
//   direction the light leaves the fixture (down for ceiling fixtures, null
//   for all around); and the light's kelvin, lumens and on.
//   `lampAnchor(key, w, d, h)` gives the same anchor without building.

import * as THREE from "three";
import type { Asset, AssetLight } from "../../contract/bindings";
import { kelvinColor } from "../light/model";
import type { Kit, RoundOptions } from "./kit";
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
  /** Signed underside height above the floor, meters: below zero for a septic tank. */
  elevation: number;
  /** Height of the ceiling above this asset's underside, meters. A pendant's cord reaches it. */
  ceiling: number;
  /** Material of the glowing part: the shared lamp material, or a plain diffuser when the fixture gives no light. */
  glow: THREE.MeshStandardMaterial;
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

/** `box` with a material instead of a color (glow, glass, faint). */
function mbox(c: Ctx, mat: THREE.Material, sx: number, sy: number, sz: number, x: number, y: number, z: number) {
  return c.kit.box(c.g, mat, sx, sy, sz, x, y, z, { baseY: c.baseY });
}

/** Box standing on the wall face (the back, z = -d/2): `t` thick, `gap` off the face. */
function wallBox(c: Ctx, color: string, sx: number, sy: number, t: number, x: number, y: number, gap = 0, rough = 0.5, metal = 0) {
  return box(c, color, sx, sy, t, x, y, -c.d / 2 + gap + t / 2, rough, metal);
}

/** Upright round part, underside at y, cut at the cutaway like `box`. */
function round(c: Ctx, mat: THREE.Material, radius: number, height: number, x: number, y: number, z: number, opts: Omit<RoundOptions, "clip"> = {}) {
  return c.kit.round(c.g, mat, radius, height, x, y, z, { ...opts, clip: { baseY: c.baseY } });
}

/** True when a part reaching up to local `top` is above the cutaway and should be left out. */
function cutAway(c: Ctx, top: number): boolean {
  return c.kit.cutY !== null && c.baseY + top > c.kit.cutY + 0.001;
}

/** Horizontal round bar centered on (x, y, z), along x or along z (facing front). */
function rod(c: Ctx, mat: THREE.Material, r: number, length: number, axis: "x" | "z", x: number, y: number, z: number, segments = 12) {
  if (cutAway(c, y + r)) return null;
  const m = c.kit.round(c.g, mat, r, length, 0, 0, 0, { segments });
  if (!m) return null;
  if (axis === "x") m.rotation.z = Math.PI / 2;
  else m.rotation.x = Math.PI / 2;
  m.position.set(x, y, z);
  return m;
}

/** Round ball centered on (x, y, z), left out above the cutaway. */
function bulb(c: Ctx, mat: THREE.Material, r: number, x: number, y: number, z: number) {
  if (cutAway(c, y + r)) return null;
  return c.kit.ball(c.g, mat, r, r, r, x, y, z);
}

const paint = (c: Ctx, color: string, rough = 0.5, metal = 0) => c.lib.plain(color, rough, metal);

// ------------------------------------------------------------------ lamps

/** What a light fixture tells the light module. See the header. */
export interface LampInfo {
  /** Where the actual light goes, meters, in the asset group's local frame. */
  anchor: [number, number, number];
  /** Main direction the light leaves the fixture, unit vector, same frame. Null: all around. */
  aim: [number, number, number] | null;
  kelvin: number;
  lumens: number;
  on: boolean;
}

const DIFFUSER = new THREE.Color("#f7f5f0");

/**
 * The glowing part's material for one color temperature, shared by every
 * fixture of that temperature in this library. Unlit it reads as a white
 * diffuser with a hint of its light color; see the header for the tags.
 */
export function lampMaterial(lib: MaterialLibrary, kelvin: number): THREE.MeshStandardMaterial {
  const k = Math.round(Math.min(Math.max(Number.isFinite(kelvin) ? kelvin : 3000, 1500), 12000) / 100) * 100;
  // Linear RGB, brightest channel at 1: the light module's own lamp color.
  const glow = new THREE.Color(...kelvinColor(k));
  const base = DIFFUSER.clone().lerp(glow, 0.1);
  // The roughness carries the kelvin in its last digits, so every color
  // temperature is its own library material and none is shared with a plain
  // part that happens to have the same color.
  const mat = lib.plain(`#${base.getHexString()}`, 0.3 + k / 1e7, 0);
  if (mat.userData.lampKelvin !== k) {
    mat.userData.lampKelvin = k;
    mat.emissive.copy(glow);
    mat.emissiveIntensity = 0;
  }
  return mat;
}

/** The per-fixture glow API, looked up by shape so the forms work with or without it. */
type PerFixtureGlow = { lampGlow?: (elementId: string, kelvin: number) => THREE.MeshStandardMaterial };

/** The glowing part's material for a fixture that gives light (see the header). */
function glowMaterial(lib: MaterialLibrary, asset: Asset, light: AssetLight): THREE.MeshStandardMaterial {
  const perFixture = (lib as unknown as PerFixtureGlow).lampGlow;
  if (typeof perFixture === "function" && asset.id) return perFixture.call(lib, asset.id, light.kelvin);
  return lampMaterial(lib, light.kelvin);
}

const DOWN: [number, number, number] = [0, -1, 0];

/** Pendant shade height as a share of the item height; the bulb sits 40 percent up inside it. */
const PENDANT_SHADE = 0.55;
/** Floor and table lamp shades: the top share of the height, bulb height inside it. */
const FLOOR_SHADE = 0.22;
const TABLE_SHADE = 0.5;

/**
 * Where the light of a fixture goes, meters, local frame (w, d, h in meters,
 * as the form is built): just under a ceiling fixture's diffuser, at the bulb
 * of a pendant or lamp, in the middle of a wall light's glass. Null for keys
 * that are not light fixtures.
 */
export function lampAnchor(key: string, _w: number, d: number, h: number): { anchor: [number, number, number]; aim: [number, number, number] | null } | null {
  switch (key) {
    case "light-ceiling":
    case "light-downlight":
    case "light-tube":
      return { anchor: [0, -0.03, 0], aim: DOWN };
    case "light-pendant":
      return { anchor: [0, h * PENDANT_SHADE * 0.4, 0], aim: DOWN };
    case "light-wall":
      return { anchor: [0, h * 0.5, -d / 2 + d * 0.58], aim: null };
    case "light-outdoor":
      return { anchor: [0, h * 0.47, -d / 2 + d * 0.56], aim: null };
    case "light-floor-lamp":
      return { anchor: [0, h * (1 - FLOOR_SHADE) + h * FLOOR_SHADE * 0.35, 0], aim: null };
    case "light-table-lamp":
      return { anchor: [0, h * (1 - TABLE_SHADE) + h * TABLE_SHADE * 0.3, 0], aim: null };
    default:
      return null;
  }
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

// ------------------------------------------------------------------ devices

const PLATE = "#f2f0ea";
const PLATE_2 = "#e6e3db";
const ROCKER = "#f8f7f3";
const HOLE = "#33373c";
const METAL_WHITE = "#ecebe6";
const METAL_DARK = "#3b3f45";
const BRASS = "#b08d57";

/** Switch and outlet plates stand 9 mm off the wall; the rest of their depth is the box in the wall. */
const PLATE_T = 0.009;

/** Wide-series switch plate: one to three rockers stacked in the frame. */
const switchPlate =
  (gangs: number): Builder =>
  (c) => {
    const t = Math.min(PLATE_T, c.d);
    wallBox(c, PLATE, c.w, c.h, t, 0, 0, 0, 0.4);
    const fw = c.w * 0.72;
    const fh = c.h * 0.76;
    const fy = (c.h - fh) / 2;
    wallBox(c, PLATE_2, fw, fh, 0.002, 0, fy, t, 0.45);
    const gap = Math.min(0.003, fh * 0.04);
    const rh = (fh - gap * (gangs + 1)) / gangs;
    for (let i = 0; i < gangs; i++) {
      const rocker = wallBox(c, ROCKER, fw - 2 * gap, rh, 0.005, 0, fy + gap + i * (rh + gap), t + 0.002, 0.35);
      // A rocker sits tilted, one end pressed in.
      if (rocker) rocker.rotation.x = 0.06;
    }
  };

/** One outlet face with its holes, centered at height `cy`, `z0` off the wall face. */
function socket(c: Ctx, cy: number, z0: number, tandem: boolean): void {
  const fw = Math.min(0.042, c.w * 0.64);
  const fh = Math.min(0.034, c.h * 0.3);
  wallBox(c, PLATE_2, fw, fh, 0.002, 0, cy - fh / 2, z0, 0.5);
  const z = z0 + 0.002;
  const hole = (sx: number, sy: number, x: number, y: number) => wallBox(c, HOLE, sx, sy, 0.001, x, y - sy / 2, z, 0.7);
  if (tandem) {
    // 220 V special purpose: two tandem slots over a ground.
    hole(fw * 0.26, 0.0025, -fw * 0.2, cy + fh * 0.12);
    hole(fw * 0.26, 0.0025, fw * 0.2, cy + fh * 0.12);
    hole(0.0028, fh * 0.26, 0, cy - fh * 0.22);
  } else {
    // Universal face: two flat pin slots over a round ground.
    hole(0.0025, fh * 0.3, -fw * 0.16, cy + fh * 0.06);
    hole(0.0025, fh * 0.3, fw * 0.16, cy + fh * 0.06);
    hole(0.004, 0.004, 0, cy - fh * 0.26);
  }
}

/** Outlet plate: a duplex, one special purpose outlet, or one with its own switch and pilot light (aircon). */
const outlet =
  (kind: "duplex" | "single" | "switched"): Builder =>
  (c) => {
    const t = Math.min(PLATE_T, c.d);
    wallBox(c, PLATE, c.w, c.h, t, 0, 0, 0, 0.4);
    if (kind === "duplex") {
      socket(c, c.h * 0.7, t, false);
      socket(c, c.h * 0.3, t, false);
      return;
    }
    socket(c, kind === "single" ? c.h * 0.5 : c.h * 0.34, t, true);
    if (kind === "switched") {
      // Its own switch in a small frame, and a red pilot light.
      const fw = c.w * 0.5;
      const fh = c.h * 0.24;
      const fx = -c.w * 0.1;
      wallBox(c, PLATE_2, fw, fh, 0.002, fx, c.h * 0.6, t, 0.45);
      const rocker = wallBox(c, ROCKER, fw - 0.006, fh - 0.006, 0.005, fx, c.h * 0.6 + 0.003, t + 0.002, 0.35);
      if (rocker) rocker.rotation.x = 0.06;
      wallBox(c, "#c0392b", 0.006, 0.006, 0.002, c.w * 0.3, c.h * 0.72, t, 0.3);
    }
  };

/** Split type indoor unit on the wall, with its louver. */
const splitIndoor: Builder = (c) => {
  const back = -c.d / 2;
  const bodyD = c.d * 0.92;
  const front = back + bodyD;
  const white = "#f4f4f1";
  box(c, white, c.w, c.h, bodyD, 0, 0, back + bodyD / 2, 0.35);
  box(c, "#fafaf8", c.w * 0.995, c.h * 0.6, 0.008, 0, c.h * 0.36, front + 0.004, 0.2);
  // Air outlet along the bottom front, and the louver across it, tilted down.
  box(c, "#3a3f45", c.w * 0.82, c.h * 0.14, 0.04, 0, c.h * 0.07, front - 0.018, 0.6);
  const louver = box(c, white, c.w * 0.8, 0.006, c.d * 0.24, 0, c.h * 0.08, front - 0.01, 0.35);
  if (louver) louver.rotation.x = 0.5;
  // Intake grille on top, display window and a status light at the front right.
  for (let i = 0; i < 5; i++) box(c, "#d3d6d8", c.w * 0.9, 0.002, 0.008, 0, c.h, back + c.d * (0.2 + i * 0.14), 0.6);
  box(c, "#23272c", Math.min(0.06, c.w * 0.08), c.h * 0.07, 0.002, c.w * 0.34, c.h * 0.5, front + 0.009, 0.25);
  box(c, "#4fc3f7", 0.004, 0.004, 0.002, c.w * 0.34 + 0.018, c.h * 0.44, front + 0.009, 0.3);
};

/** Outdoor unit on its feet: fan grille in front, coil fins at the back and the left side. */
const splitOutdoor: Builder = (c) => {
  const feet = Math.min(0.05, c.h * 0.1);
  const bodyH = c.h - feet;
  const casing = "#e9e8e3";
  const front = c.d / 2;
  for (const s of [-1, 1]) box(c, "#4a4f55", 0.05, feet, c.d * 0.96, s * (c.w / 2 - 0.09), 0, 0, 0.6, 0.3);
  box(c, casing, c.w, bodyH - 0.012, c.d, 0, feet, 0, 0.45, 0.2);
  box(c, casing, c.w + 0.008, 0.012, c.d + 0.008, 0, c.h - 0.012, 0, 0.45, 0.2);
  // Fan behind a round grille, left of center.
  const fr = Math.min(bodyH * 0.38, c.w * 0.3);
  const fx = -c.w * 0.12;
  const fy = feet + bodyH * 0.5;
  const grille = paint(c, "#2e3237", 0.55, 0.3);
  rod(c, paint(c, "#16181b", 0.8), fr, 0.004, "z", fx, fy, front + 0.002, 24);
  const blade = paint(c, "#5b6168", 0.5, 0.2);
  for (let i = 0; i < 3; i++) {
    const b = mbox(c, blade, fr * 1.6, fr * 0.26, 0.004, fx, fy - fr * 0.13, front + 0.004);
    if (b) b.rotation.z = (i * Math.PI) / 3;
  }
  rod(c, grille, fr * 0.12, 0.012, "z", fx, fy, front + 0.006, 12);
  if (!cutAway(c, fy + fr)) c.kit.ring(c.g, grille, fr, 0.007, fx, fy, front + 0.008, 24);
  for (let i = -3; i <= 3; i++) {
    const o = (i / 3.5) * fr;
    const half = Math.sqrt(Math.max(fr * fr - o * o, 0));
    mbox(c, grille, 0.003, half * 2, 0.003, fx + o, fy - half, front + 0.008);
    mbox(c, grille, half * 2, 0.003, 0.003, fx, fy + o - 0.0015, front + 0.008);
  }
  // Service valve cover on the right side.
  box(c, "#d9d8d2", 0.004, bodyH * 0.3, c.d * 0.4, c.w / 2 + 0.002, feet + bodyH * 0.12, c.d * 0.15, 0.45);
  // Coil fins behind and on the left.
  const fin = paint(c, "#b9bcbf", 0.4, 0.6);
  const nb = Math.max(8, Math.round(c.w / 0.025));
  for (let i = 0; i < nb; i++) mbox(c, fin, 0.0015, bodyH * 0.8, 0.01, -c.w / 2 + 0.02 + (i * (c.w - 0.04)) / (nb - 1), feet + bodyH * 0.1, -c.d / 2 - 0.004);
  const ns = Math.max(4, Math.round(c.d / 0.025));
  for (let i = 0; i < ns; i++) mbox(c, fin, 0.01, bodyH * 0.8, 0.0015, -c.w / 2 - 0.004, feet + bodyH * 0.1, -c.d / 2 + 0.02 + (i * (c.d - 0.04)) / (ns - 1));
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

  // ---------------------------------------------------------- plumbing and utility

  "floor-drain": (c) => {
    // Flush with the 12 mm floor finish buildScene lays in rooms, 2 mm proud.
    const top = Math.min(0.014, c.h);
    box(c, "#c4c8cc", c.w, top, c.d, 0, 0, 0, 0.3, 0.8);
    const slot = paint(c, "#2a2d31", 0.7);
    const n = 5;
    for (let i = 0; i < n; i++) {
      mbox(c, slot, c.w * 0.64, 0.0006, Math.min(0.008, c.d * 0.06), 0, top, -c.d * 0.3 + (i * c.d * 0.6) / (n - 1));
    }
  },

  "water-heater": (c) => {
    const bodyD = c.d * 0.9;
    const front = -c.d / 2 + bodyD;
    box(c, "#f5f5f2", c.w, c.h, bodyD, 0, 0, -c.d / 2 + bodyD / 2, 0.25);
    box(c, "#dde1e4", c.w * 0.86, c.h * 0.34, 0.003, 0, c.h * 0.46, front + 0.0015, 0.3);
    box(c, "#1f2428", c.w * 0.34, c.h * 0.07, 0.002, 0, c.h * 0.7, front + 0.003, 0.25);
    rod(c, paint(c, "#aeb3b8", 0.35, 0.5), Math.min(0.022, c.w * 0.1), 0.012, "z", 0, c.h * 0.58, front + 0.006, 16);
    // Water in and out under it.
    const pipe = paint(c, STEEL, 0.3, 0.8);
    for (const s of [-1, 1]) round(c, pipe, 0.009, 0.07, s * c.w * 0.22, -0.07, -c.d / 2 + bodyD * 0.5, { segments: 8 });
  },

  "water-meter": (c) => {
    const brass = paint(c, BRASS, 0.35, 0.7);
    const pr = Math.min(0.02, c.h * 0.14, c.d * 0.14);
    const py = pr + 0.01;
    rod(c, brass, pr, c.w, "x", 0, py, 0, 12);
    for (const s of [-1, 1]) rod(c, paint(c, "#8e7447", 0.4, 0.7), pr * 1.4, 0.022, "x", s * (c.w / 2 - 0.011), py, 0, 6);
    const rr = Math.min(c.d * 0.42, c.w * 0.28);
    const regH = Math.max(c.h - 0.03, 0.02);
    round(c, brass, rr, regH, 0, 0.01, 0, { segments: 16 });
    round(c, paint(c, "#20262c", 0.2), rr * 0.84, 0.003, 0, 0.01 + regH, 0, { segments: 16 });
    round(c, paint(c, "#2f6db0", 0.5), rr * 1.04, 0.01, 0, 0.01 + regH + 0.003, 0, { segments: 16 });
  },

  "water-tank": (c) => {
    const r = Math.min(c.w, c.d) / 2;
    const pe = paint(c, "#3f73a8", 0.55);
    const body = c.h * 0.84;
    round(c, pe, r * 0.96, body, 0, 0, 0, { segments: 28 });
    round(c, pe, r * 0.96, c.h * 0.11, 0, body, 0, { segments: 28, top: 0.42 });
    round(c, paint(c, "#2f5f8f", 0.5), r * 0.36, c.h * 0.05, 0, body + c.h * 0.11, 0, { segments: 20 });
    for (const f of [0.2, 0.42, 0.64]) round(c, pe, r, 0.04, 0, body * f, 0, { segments: 28 });
    rod(c, paint(c, "#e7e5df", 0.5), Math.min(0.03, r * 0.06), 0.08, "z", 0, 0.12, r * 0.96 + 0.035, 10);
  },

  "septic-tank": (c) => {
    // Below grade: the tank and its baffle are faint so they read through the
    // ground (which writes no depth); only the access covers at grade are solid.
    const sink = Math.min(c.elevation, 0);
    const faint = c.lib.ghost("#8f9aa1", 0.3);
    const top = sink + c.h;
    mbox(c, faint, c.w, c.h, c.d, 0, sink, 0);
    mbox(c, faint, 0.08, c.h * 0.9, c.d * 0.98, -c.w / 2 + (2 * c.w) / 3, sink, 0);
    const cover = paint(c, "#a9a79f", 0.9);
    const s = Math.min(0.6, c.d * 0.45, (c.w / 3) * 0.8);
    const coverY = Math.max(top, 0) - 0.04;
    // Access openings over the middle of each chamber, as in the plan symbol.
    for (const x of [-c.w / 6, c.w / 3]) {
      if (top < coverY - 0.01) mbox(c, faint, s * 0.8, coverY - top, s * 0.8, x, top, -0.18 * c.d);
      mbox(c, cover, s, 0.05, s, x, coverY, -0.18 * c.d);
    }
  },

  "lpg-cylinder": (c) => {
    const r = Math.min(c.w, c.d) / 2;
    const body = paint(c, "#c24b3a", 0.45, 0.35);
    const dark = paint(c, "#3b3f45", 0.5, 0.5);
    const foot = Math.min(0.045, c.h * 0.08);
    round(c, dark, r * 0.82, foot, 0, 0, 0, { segments: 20, open: true });
    const bh = c.h * 0.66;
    const sh = c.h * 0.1;
    round(c, body, r, bh, 0, foot - 0.005, 0, { segments: 24 });
    round(c, body, r, sh, 0, foot - 0.005 + bh, 0, { segments: 24, top: 0.5 });
    const collarY = foot - 0.005 + bh + sh;
    round(c, dark, r * 0.56, c.h * 0.14, 0, collarY - 0.01, 0, { segments: 20, open: true });
    round(c, paint(c, BRASS, 0.35, 0.7), Math.min(0.016, r * 0.12), c.h * 0.08, 0, collarY - 0.01, 0, { segments: 10 });
    round(c, paint(c, "#2c5f9e", 0.45), Math.min(0.032, r * 0.22), c.h * 0.06, 0, collarY + c.h * 0.08 - 0.012, 0, { segments: 12 });
  },

  "electric-meter": (c) => {
    const baseT = c.d * 0.35;
    wallBox(c, "#8f969c", c.w, c.h, baseT, 0, 0, 0, 0.6);
    const r = Math.min(c.w * 0.44, c.h * 0.3);
    const cy = c.h * 0.58;
    const bodyL = c.d * 0.25;
    const z0 = -c.d / 2 + baseT;
    rod(c, paint(c, "#2f3439", 0.5, 0.3), r, bodyL, "z", 0, cy, z0 + bodyL / 2, 20);
    rod(c, paint(c, "#f1f1ec", 0.4), r * 0.84, 0.004, "z", 0, cy, z0 + bodyL + 0.002, 20);
    box(c, "#1d2125", r * 0.8, 0.02, 0.002, 0, cy + r * 0.22, z0 + bodyL + 0.005, 0.3);
    const glassL = c.d - baseT - bodyL;
    rod(c, c.lib.get("mat-glass-clear", undefined, false), r * 0.92, glassL, "z", 0, cy, z0 + bodyL + glassL / 2, 20);
    wallBox(c, "#7d8489", c.w * 0.7, c.h * 0.16, baseT + 0.02, 0, c.h * 0.05, 0, 0.6);
  },

  // ---------------------------------------------------------- light fixtures

  "light-ceiling": (c) => {
    const r = Math.min(c.w, c.d) / 2;
    const plateH = Math.min(0.018, c.h * 0.3);
    round(c, paint(c, METAL_WHITE, 0.5, 0.2), r * 0.94, plateH, 0, c.h - plateH, 0, { segments: 24 });
    // Diffuser, narrowing toward the room.
    round(c, c.glow, r * 0.86, c.h - plateH, 0, 0, 0, { segments: 24, top: 1 / 0.86 });
  },

  "light-downlight": (c) => {
    const r = Math.min(c.w, c.d) / 2;
    round(c, paint(c, METAL_WHITE, 0.5, 0.2), r, c.h - 0.003, 0, 0.003, 0, { segments: 20 });
    // Lens, just below the trim.
    round(c, c.glow, r * 0.76, 0.005, 0, 0, 0, { segments: 20 });
  },

  "light-tube": (c) => {
    const alongX = c.w >= c.d;
    const L = Math.max(c.w, c.d);
    const W = Math.min(c.w, c.d);
    const bh = Math.min(0.03, c.h * 0.5);
    const tr = Math.max(Math.min(0.013, W * 0.2, (c.h - bh) / 2 - 0.002), 0.003);
    // Batten channel against the ceiling, the T8 tube held under it at both ends.
    box(c, METAL_WHITE, alongX ? L : W * 0.55, bh, alongX ? W * 0.55 : L, 0, c.h - bh, 0, 0.5, 0.2);
    for (const s of [-1, 1]) {
      const at = s * (L / 2 - 0.015);
      box(c, PLATE_2, alongX ? 0.02 : W * 0.4, c.h - bh, alongX ? W * 0.4 : 0.02, alongX ? at : 0, 0, alongX ? 0 : at, 0.5);
    }
    rod(c, c.glow, tr, L - 0.05, alongX ? "x" : "z", 0, tr + 0.002, 0, 16);
  },

  "light-pendant": (c) => {
    const r = Math.min(c.w, c.d) / 2;
    const shadeH = c.h * PENDANT_SHADE;
    const shade = paint(c, "#2f3338", 0.55, 0.3);
    round(c, shade, r, shadeH, 0, 0, 0, { segments: 24, top: 0.22, open: true });
    round(c, shade, r * 0.22, 0.004, 0, shadeH - 0.004, 0, { segments: 16 });
    const socketH = c.h * 0.14;
    round(c, paint(c, "#1f2226", 0.6), Math.min(0.02, r * 0.2), socketH, 0, shadeH - socketH * 0.6, 0, { segments: 12 });
    const a = lampAnchor("light-pendant", c.w, c.d, c.h);
    if (a) bulb(c, c.glow, Math.min(0.045, r * 0.28), 0, a.anchor[1], 0);
    // Cord up to the ceiling, and the canopy that hides the box.
    const top = Math.max(c.ceiling, shadeH + socketH);
    const cordFrom = shadeH + socketH * 0.4;
    round(c, paint(c, "#1b1d20", 0.7), 0.003, Math.max(top - 0.02 - cordFrom, 0.001), 0, cordFrom, 0, { segments: 6 });
    round(c, paint(c, METAL_WHITE, 0.5, 0.2), Math.min(0.05, r * 0.4), 0.02, 0, top - 0.02, 0, { segments: 20 });
  },

  "light-wall": (c) => {
    const a = lampAnchor("light-wall", c.w, c.d, c.h);
    const zc = a ? a.anchor[2] : 0;
    const metal = paint(c, METAL_DARK, 0.45, 0.5);
    wallBox(c, METAL_DARK, Math.min(c.w * 0.45, 0.09), c.h * 0.5, 0.012, 0, c.h * 0.25, 0, 0.45, 0.5);
    wallBox(c, METAL_DARK, 0.016, 0.016, Math.max(zc + c.d / 2 - 0.012, 0.001), 0, c.h * 0.5 - 0.008, 0.012, 0.45, 0.5);
    const rr = Math.min(c.w * 0.4, c.d * 0.4);
    // Frosted glass cylinder, light up and down, capped top and bottom.
    round(c, c.glow, rr, c.h * 0.6, 0, c.h * 0.2, zc, { segments: 20 });
    round(c, metal, rr * 1.06, 0.012, 0, c.h * 0.2 - 0.012, zc, { segments: 20 });
    round(c, metal, rr * 1.06, 0.012, 0, c.h * 0.8, zc, { segments: 20 });
  },

  "light-outdoor": (c) => {
    const a = lampAnchor("light-outdoor", c.w, c.d, c.h);
    const zc = a ? a.anchor[2] : 0;
    const dark = "#2a2d31";
    const metal = paint(c, dark, 0.5, 0.45);
    wallBox(c, dark, c.w * 0.5, c.h * 0.8, 0.012, 0, c.h * 0.1, 0, 0.5, 0.45);
    // Lantern: frosted glass between four posts, a base and a roof.
    const bw = Math.min(c.w, c.d) * 0.6;
    const y0 = c.h * 0.2;
    const gh = c.h * 0.54;
    mbox(c, c.glow, bw, gh, bw, 0, y0, zc);
    for (const sx of [-1, 1]) for (const sz of [-1, 1]) mbox(c, metal, 0.008, gh, 0.008, (sx * bw) / 2, y0, zc + (sz * bw) / 2);
    mbox(c, metal, bw * 1.12, 0.014, bw * 1.12, 0, y0 - 0.014, zc);
    mbox(c, metal, bw * 1.25, 0.016, bw * 1.25, 0, y0 + gh, zc);
    const cap = round(c, metal, bw * 0.62, c.h * 0.12, 0, y0 + gh + 0.016, zc, { segments: 4, top: 0.15 });
    if (cap) cap.rotation.y = Math.PI / 4;
    wallBox(c, dark, 0.018, 0.018, Math.max(zc + c.d / 2 - 0.012 - bw / 2, 0.001), 0, y0 + gh * 0.6, 0.012, 0.5, 0.45);
  },

  "light-floor-lamp": (c) => {
    const r = Math.min(c.w, c.d) / 2;
    const metal = paint(c, METAL_DARK, 0.45, 0.5);
    const shadeY = c.h * (1 - FLOOR_SHADE);
    round(c, metal, r * 0.62, 0.025, 0, 0, 0, { segments: 24 });
    round(c, metal, 0.011, shadeY + 0.03 - 0.025, 0, 0.025, 0, { segments: 8 });
    // Drum shade: the fabric glows when the lamp is on.
    round(c, c.glow, r, c.h * FLOOR_SHADE, 0, shadeY, 0, { segments: 24, top: 0.78, open: true });
    const a = lampAnchor("light-floor-lamp", c.w, c.d, c.h);
    if (a) bulb(c, c.glow, Math.min(0.035, r * 0.2), 0, a.anchor[1], 0);
  },

  "light-table-lamp": (c) => {
    const r = Math.min(c.w, c.d) / 2;
    const shadeY = c.h * (1 - TABLE_SHADE);
    round(c, paint(c, "#c9b79c", 0.35), r * 0.42, shadeY * 0.7, 0, 0, 0, { segments: 20, top: 0.55 });
    round(c, paint(c, METAL_DARK, 0.45, 0.5), 0.008, shadeY * 0.3 + 0.02, 0, shadeY * 0.7, 0, { segments: 8 });
    round(c, c.glow, r, c.h * TABLE_SHADE, 0, shadeY, 0, { segments: 24, top: 0.72, open: true });
    const a = lampAnchor("light-table-lamp", c.w, c.d, c.h);
    if (a) bulb(c, c.glow, Math.min(0.03, r * 0.2), 0, a.anchor[1], 0);
  },

  // ---------------------------------------------------------- electrical devices

  "outlet-duplex": outlet("duplex"),
  "outlet-counter": outlet("duplex"),
  "outlet-spo": outlet("single"),
  "outlet-aircon": outlet("switched"),

  "outlet-outdoor": (c) => {
    // Weatherproof box with a flip lid over the outlet.
    const t = Math.min(0.028, c.d * 0.5);
    wallBox(c, "#8e959b", c.w, c.h, t, 0, 0, 0, 0.6);
    wallBox(c, "#9fa6ac", c.w * 0.88, c.h * 0.74, 0.02, 0, c.h * 0.08, t, 0.45);
    wallBox(c, "#80878d", c.w * 0.88, 0.012, 0.024, 0, c.h * 0.82, t, 0.6);
    wallBox(c, "#80878d", c.w * 0.2, 0.01, 0.026, 0, c.h * 0.05, t, 0.6);
  },

  "switch-1": switchPlate(1),
  "switch-2": switchPlate(2),
  "switch-3": switchPlate(3),

  panelboard: (c) => {
    const t = c.d * 0.86;
    wallBox(c, "#b3b8bc", c.w, c.h, t, 0, 0, 0, 0.45, 0.4);
    // Door, a hair proud of the box with its gap all round, latch and hinges.
    wallBox(c, "#c2c6ca", c.w - 0.014, c.h - 0.014, 0.004, 0, 0.007, t, 0.4, 0.4);
    wallBox(c, METAL_DARK, 0.012, 0.05, 0.008, c.w / 2 - 0.03, c.h * 0.5 - 0.025, t + 0.004, 0.4, 0.5);
    for (const hy of [0.14, 0.86]) wallBox(c, METAL_DARK, 0.008, 0.03, 0.006, -c.w / 2 + 0.004, c.h * hy - 0.015, t, 0.4, 0.5);
    wallBox(c, "#e3c14a", Math.min(0.08, c.w * 0.3), 0.022, 0.001, 0, c.h * 0.78, t + 0.004, 0.5);
  },

  "smoke-detector": (c) => {
    const r = Math.min(c.w, c.d) / 2;
    round(c, paint(c, "#f1f0ec", 0.5), r, c.h * 0.45, 0, c.h * 0.55, 0, { segments: 16 });
    round(c, paint(c, "#e8e7e2", 0.45), r * 0.84, c.h * 0.55 - 0.003, 0, 0.003, 0, { segments: 16, top: 1.12 });
    round(c, paint(c, "#d4d2cb", 0.4), r * 0.3, 0.004, 0, 0, 0, { segments: 12 });
    box(c, "#c0392b", 0.005, 0.002, 0.005, r * 0.55, 0.001, 0, 0.3);
  },

  "doorbell-button": (c) => {
    const t = Math.min(0.012, c.d);
    wallBox(c, PLATE, c.w, c.h, t, 0, 0, 0, 0.4);
    const face = -c.d / 2 + t;
    const br = Math.min(c.w, c.h) * 0.3;
    rod(c, paint(c, "#d9d6cf", 0.35), br, 0.004, "z", 0, c.h * 0.55, face + 0.002, 16);
    rod(c, paint(c, "#f5f4ef", 0.3), br * 0.72, 0.008, "z", 0, c.h * 0.55, face + 0.004, 16);
  },

  "doorbell-chime": (c) => {
    const t = c.d * 0.8;
    wallBox(c, PLATE, c.w, c.h, t, 0, 0, 0, 0.45);
    wallBox(c, "#e2dfd8", c.w * 0.9, c.h * 0.9, 0.002, 0, c.h * 0.05, t, 0.5);
    for (let i = 0; i < 4; i++) wallBox(c, HOLE, c.w * 0.5, 0.004, 0.001, 0, c.h * (0.28 + i * 0.12), t + 0.002, 0.7);
  },

  // ---------------------------------------------------------- aircon

  "aircon-indoor-1hp": splitIndoor,
  "aircon-indoor-2hp": splitIndoor,
  "aircon-indoor-3hp": splitIndoor,
  "aircon-outdoor-1hp": splitOutdoor,
  "aircon-outdoor-3hp": splitOutdoor,

  "aircon-window": (c) => {
    // Set through the wall opening: the grille and controls face the room
    // (+z), the condenser fins the outside (-z, the back).
    box(c, "#e4e1da", c.w, c.h, c.d, 0, 0, 0, 0.45, 0.2);
    const front = c.d / 2;
    box(c, "#efede8", c.w * 0.98, c.h * 0.96, 0.012, 0, c.h * 0.02, front + 0.006, 0.45);
    const vane = paint(c, "#cfcbc3", 0.5);
    const gw = c.w * 0.62;
    const gx = -c.w * 0.17;
    for (let i = 0; i < 6; i++) mbox(c, vane, gw, 0.004, 0.012, gx, c.h * (0.6 + i * 0.055), front + 0.014);
    for (let i = 0; i < 9; i++) mbox(c, vane, gw, 0.003, 0.008, gx, c.h * (0.1 + i * 0.045), front + 0.014);
    box(c, "#d8d4cc", c.w * 0.24, c.h * 0.86, 0.004, c.w * 0.36, c.h * 0.07, front + 0.014, 0.45);
    const knob = paint(c, "#8e939a", 0.4, 0.3);
    for (const ky of [0.66, 0.42]) rod(c, knob, Math.min(0.018, c.w * 0.05), 0.012, "z", c.w * 0.36, c.h * ky, front + 0.022, 16);
    const fin = paint(c, "#b9bcbf", 0.4, 0.6);
    const nb = Math.max(8, Math.round(c.w / 0.03));
    for (let i = 0; i < nb; i++) mbox(c, fin, 0.0015, c.h * 0.8, 0.008, -c.w / 2 + 0.02 + (i * (c.w - 0.04)) / (nb - 1), c.h * 0.1, -c.d / 2 - 0.003);
    for (const s of [-1, 1]) {
      for (let i = 0; i < 6; i++) mbox(c, vane, 0.004, 0.01, c.d * 0.34, s * (c.w / 2 + 0.002), c.h * (0.25 + i * 0.1), -c.d * 0.28);
    }
  },
};

export const KNOWN_ASSET_KEYS = Object.keys(BUILDERS);

/** The form builder for a catalog key; aircon sizes not in the list use their family's form. */
function builderFor(key: string): Builder | null {
  if (Object.prototype.hasOwnProperty.call(BUILDERS, key)) return BUILDERS[key];
  if (key.startsWith("aircon-indoor-")) return splitIndoor;
  if (key.startsWith("aircon-outdoor-")) return splitOutdoor;
  return null;
}

/** True when `key` gets its own form rather than the fallback box. */
export function hasAssetForm(key: string): boolean {
  return builderFor(key) !== null;
}

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

/** Level height the catalog's ceiling items assume (guhit_model::defaults::DEFAULT_LEVEL_HEIGHT_MM). */
const DEFAULT_CEILING_MM = 3000;

export interface AssetFormOptions {
  /** Height of the ceiling above the level floor, mm: where a pendant's cord ends. Default 3000. */
  ceilingMm?: number;
}

/**
 * Group with its origin at the center of the footprint, underside at y = 0.
 * `baseY` is the world Y (meters) of that origin, used to clip parts against
 * the cutaway height set on `kit.cutY`. With `usePack`, the pack's GLB is used
 * when it is loaded and the procedural form is the placeholder until then.
 * Light fixtures come out tagged for the light module (see the header).
 */
export function buildAssetForm(kit: Kit, lib: MaterialLibrary, asset: Asset, baseY = 0, usePack = false, opts: AssetFormOptions = {}): THREE.Group {
  if (usePack) {
    const packed = packAssetForm(asset);
    if (packed) return packed;
  }
  const g = new THREE.Group();
  const elevationMm = Number.isFinite(asset.elevation_mm) ? asset.elevation_mm : 0;
  const lift = Math.max(elevationMm, 0) / 1000;
  const ceilingMm = opts.ceilingMm !== undefined && opts.ceilingMm > 0 ? opts.ceilingMm : DEFAULT_CEILING_MM;
  // Older files and test scenes may leave `light` out entirely.
  const light: AssetLight | null = asset.light ?? null;
  const c: Ctx = {
    kit,
    lib,
    g,
    w: clampDim(asset.width_mm, 0.5),
    d: clampDim(asset.depth_mm, 0.5),
    h: clampDim(asset.height_mm, 0.5),
    lift,
    baseY,
    elevation: elevationMm / 1000,
    ceiling: Math.max(ceilingMm / 1000 - lift, 0),
    glow: light ? glowMaterial(lib, asset, light) : lib.plain("#f4f2ed", 0.35),
  };
  const builder = builderFor(asset.catalog_key);
  if (builder) builder(c);
  else box(c, "#c9c5bb", c.w, c.h, c.d, 0, 0, 0);
  const lamp = light ? lampAnchor(asset.catalog_key, c.w, c.d, c.h) : null;
  const glowing = (o: THREE.Object3D): o is THREE.Mesh => (o as THREE.Mesh).isMesh === true && (o as THREE.Mesh).material === c.glow;
  // A light inside the fixture must not be shadowed by its own diffuser.
  if (lamp) g.traverse((o) => glowing(o) && (o.castShadow = false));
  // One draw call per material instead of one per little box. Still one
  // element, so picking and highlighting are unchanged.
  kit.mergeByMaterial(g);
  if (light && lamp) {
    g.traverse((o) => glowing(o) && (o.userData.lampPart = true));
    const info: LampInfo = { anchor: lamp.anchor, aim: lamp.aim, kelvin: light.kelvin, lumens: light.lumens, on: light.on };
    g.userData.lamp = info;
  }
  return g;
}
