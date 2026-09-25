// Doors and windows as simple frames, leaves, sashes and glass.
//
// Everything is built inside a group that sits in the host wall's frame:
// local +x runs along the wall from its start, +y is up, +z is the RIGHT side
// of the wall direction (so the left side, where a door with flip_side false
// swings, is -z). Units are meters.
//
// Shared conventions with the 2D plan and the exports:
// flip_side false = leaf swings to the left of the wall direction,
// flip_hinge false = hinge on the jamb nearer to the wall start.
//
// Door leaves (a swing leaf on its hinge, the moving panel of a sliding door)
// keep a group of their own, tagged `userData.doorLeaf`, so walk mode can
// open and close them in the view (walk/doors.ts). Everywhere else they rest
// ajar, as drawn here; exports and captures outside walk mode show that.

import * as THREE from "three";
import type { Opening, Wall } from "../../contract/bindings";
import { planToWorld } from "../geom/coords";
import type { Kit } from "./kit";
import type { MaterialLibrary } from "./materials";

const FRAME = 0.045;
const AJAR = (32 * Math.PI) / 180;

/** A swing leaf fully open, about its hinge. */
export const DOOR_SWING_OPEN_RAD = Math.PI / 2;

/** On a door leaf's group (`userData.doorLeaf`): how to open it. `open` runs from 0 (closed) to 1 (fully open). */
export interface DoorLeafTag {
  /** swing: turns about its hinge. slide: moves along the wall over the fixed panel. */
  kind: "swing" | "slide";
  /** swing: rotation.y = sign * open * DOOR_SWING_OPEN_RAD. */
  sign: number;
  /** slide: position.x closed and fully open, meters along the wall. */
  closedX: number;
  openX: number;
  /** How open the leaf is drawn outside walk mode. */
  rest: number;
}

/** Puts a tagged leaf at `open` (0 closed, 1 fully open). */
export function setLeafOpen(obj: THREE.Object3D, tag: DoorLeafTag, open: number): void {
  if (tag.kind === "swing") obj.rotation.y = tag.sign * open * DOOR_SWING_OPEN_RAD;
  else obj.position.x = tag.closedX + open * (tag.openX - tag.closedX);
}

export function buildOpening(
  kit: Kit,
  lib: MaterialLibrary,
  o: Opening,
  wall: Wall,
  elevationMm: number,
): THREE.Group | null {
  const dx = wall.end.x - wall.start.x;
  const dy = wall.end.y - wall.start.y;
  const len = Math.hypot(dx, dy);
  if (!(len > 1) || !(o.width_mm > 1) || !(o.height_mm > 1)) return null;

  const group = new THREE.Group();
  const [px, py, pz] = planToWorld(wall.start.x, wall.start.y, elevationMm);
  group.position.set(px, py, pz);
  group.rotation.y = Math.atan2(dy, dx);

  const t = Math.max(wall.thickness_mm, 20) / 1000;
  const w = o.width_mm / 1000;
  const h = o.height_mm / 1000;
  const sill = Math.max(o.sill_mm, 0) / 1000;
  const u0 = o.offset_mm / 1000;
  const clip = { baseY: py };

  const isDoor = o.opening_type === "door";
  const frameMat = lib.get(o.material_id, isDoor ? "mat-wood-door" : "mat-aluminum-frame", false);
  const leafMat = frameMat;
  const glass = lib.get("mat-glass-clear", undefined, false);
  const metal = lib.get("mat-aluminum-frame", undefined, false);

  const box = (
    parent: THREE.Object3D,
    mat: THREE.Material,
    sx: number,
    sy: number,
    sz: number,
    x: number,
    y: number,
    z: number,
  ) => kit.box(parent, mat, sx, sy, sz, x, y, z, clip);

  // Outer frame. Doors line the full reveal, windows sit in the middle of it.
  const depth = isDoor ? t + 0.02 : Math.min(0.09, t);
  const x0 = u0 - w / 2;
  const x1 = u0 + w / 2;
  box(group, frameMat, FRAME, h, depth, x0 + FRAME / 2, sill, 0);
  box(group, frameMat, FRAME, h, depth, x1 - FRAME / 2, sill, 0);
  box(group, frameMat, w - 2 * FRAME, FRAME, depth, u0, sill + h - FRAME, 0);
  if (!isDoor) box(group, frameMat, w - 2 * FRAME, FRAME, depth + 0.04, u0, sill, 0);

  const clearW = w - 2 * FRAME;
  const clearH = isDoor ? h - FRAME : h - 2 * FRAME;
  const clearY = isDoor ? sill : sill + FRAME;
  const side = o.flip_side ? 1 : -1; // local z of the swing side

  /** A glazed panel: four rails and a pane. Centered on (cx, z), underside at y. */
  const glazed = (parent: THREE.Object3D, mat: THREE.Material, pw: number, ph: number, cx: number, y: number, z: number, rail = 0.04) => {
    const d = 0.03;
    box(parent, mat, rail, ph, d, cx - pw / 2 + rail / 2, y, z);
    box(parent, mat, rail, ph, d, cx + pw / 2 - rail / 2, y, z);
    box(parent, mat, pw - 2 * rail, rail, d, cx, y, z);
    box(parent, mat, pw - 2 * rail, rail, d, cx, y + ph - rail, z);
    box(parent, glass, pw - 2 * rail, ph - 2 * rail, 0.008, cx, y + rail, z);
  };

  /** A hinged child group. `hingeAtStart` picks the jamb, `angle` opens toward `side`. */
  const hinged = (hingeAtStart: boolean, angle: number, z: number): THREE.Group => {
    const pivot = new THREE.Group();
    pivot.position.set(hingeAtStart ? x0 + FRAME : x1 - FRAME, 0, z);
    // +angle about y turns local +x toward -z.
    pivot.rotation.y = angle * (hingeAtStart ? 1 : -1) * (side < 0 ? 1 : -1);
    group.add(pivot);
    return pivot;
  };

  /** Door leaves and sliding panels: merged apart from the frame, so they can move. */
  const leaves: THREE.Object3D[] = [];

  const doorLeaf = (hingeAtStart: boolean, lw: number) => {
    const z = side * (t / 2 - 0.02);
    const pivot = hinged(hingeAtStart, AJAR, z);
    const dir = hingeAtStart ? 1 : -1;
    box(pivot, leafMat, lw, clearH - 0.01, 0.04, (dir * lw) / 2, clearY + 0.005, 0);
    // lever handles on both faces
    for (const s of [-1, 1]) {
      box(pivot, metal, 0.12, 0.025, 0.05, dir * (lw - 0.11), clearY + 1.0, s * 0.035);
    }
    const tag: DoorLeafTag = {
      kind: "swing",
      sign: (hingeAtStart ? 1 : -1) * (side < 0 ? 1 : -1),
      closedX: pivot.position.x,
      openX: pivot.position.x,
      rest: AJAR / DOOR_SWING_OPEN_RAD,
    };
    pivot.userData.doorLeaf = tag;
    leaves.push(pivot);
  };

  let style = o.style;
  if (isDoor && (style === "casement" || style === "jalousie")) style = "swing_single";
  if (!isDoor && (style === "swing_single" || style === "swing_double")) style = "casement";

  if (isDoor) {
    if (style === "swing_single") doorLeaf(!o.flip_hinge, clearW - 0.006);
    else if (style === "swing_double") {
      doorLeaf(true, clearW / 2 - 0.004);
      doorLeaf(false, clearW / 2 - 0.004);
    } else if (style === "sliding") {
      const pw = clearW / 2 + 0.03;
      const startFixed = !o.flip_hinge;
      const fixedX = startFixed ? x0 + FRAME + pw / 2 : x1 - FRAME - pw / 2;
      // The moving panel closes over the other half and opens over the fixed one; drawn a fifth open.
      const closedX = startFixed ? x1 - FRAME - pw / 2 : x0 + FRAME + pw / 2;
      const slideX = startFixed ? closedX - clearW * 0.22 : closedX + clearW * 0.22;
      glazed(group, metal, pw, clearH - 0.01, fixedX, clearY + 0.005, -0.018, 0.05);
      const panel = new THREE.Group();
      group.add(panel);
      glazed(panel, metal, pw, clearH - 0.01, 0, clearY + 0.005, 0.018, 0.05);
      const tag: DoorLeafTag = { kind: "slide", sign: 1, closedX, openX: fixedX, rest: (slideX - closedX) / (fixedX - closedX) };
      panel.userData.doorLeaf = tag;
      setLeafOpen(panel, tag, tag.rest);
      leaves.push(panel);
    }
    // "fixed" door style: a cased opening, frame only.
  } else if (style === "fixed") {
    box(group, glass, clearW, clearH, 0.008, u0, clearY, 0);
  } else if (style === "sliding") {
    const pw = clearW / 2 + 0.02;
    glazed(group, frameMat, pw, clearH, x0 + FRAME + pw / 2, clearY, -0.016, 0.035);
    glazed(group, frameMat, pw, clearH, x1 - FRAME - pw / 2, clearY, 0.016, 0.035);
  } else if (style === "casement") {
    const two = clearW > 0.75;
    if (two) {
      box(group, frameMat, 0.04, clearH, depth, u0, clearY, 0);
      const pw = (clearW - 0.04) / 2;
      // one sash stands slightly open so it reads as a casement
      const openSash = hinged(!o.flip_hinge, (18 * Math.PI) / 180, 0);
      const dir = !o.flip_hinge ? 1 : -1;
      glazed(openSash, frameMat, pw, clearH, (dir * pw) / 2, clearY, 0, 0.035);
      const closedX = !o.flip_hinge ? x1 - FRAME - pw / 2 : x0 + FRAME + pw / 2;
      glazed(group, frameMat, pw, clearH, closedX, clearY, 0, 0.035);
    } else {
      glazed(group, frameMat, clearW, clearH, u0, clearY, 0, 0.035);
    }
  } else if (style === "jalousie") {
    const bays = clearW > 0.95 ? 2 : 1;
    if (bays === 2) box(group, frameMat, 0.04, clearH, depth, u0, clearY, 0);
    const bayW = (clearW - (bays - 1) * 0.04) / bays;
    const pitch = 0.1;
    const count = Math.max(1, Math.floor(clearH / pitch));
    const gap = clearH / count;
    for (let b = 0; b < bays; b++) {
      const cx = x0 + FRAME + bayW / 2 + b * (bayW + 0.04);
      for (let k = 0; k < count; k++) {
        const cy = clearY + gap * (k + 0.5);
        if (kit.cutY !== null && py + cy > kit.cutY) continue;
        const slat = kit.box(group, glass, bayW - 0.01, 0.006, 0.105, cx, cy - 0.003, 0);
        if (slat) slat.rotation.x = (side < 0 ? 1 : -1) * ((40 * Math.PI) / 180);
      }
    }
  }

  // Frame, rails and glass collapse to one mesh per material, and so does
  // each leaf inside its own group.
  for (const leaf of leaves) {
    kit.mergeByMaterial(leaf);
    leaf.removeFromParent();
  }
  kit.mergeByMaterial(group);
  for (const leaf of leaves) group.add(leaf);
  return group;
}
