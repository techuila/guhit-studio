// How the building shell is drawn, so pipes stay readable (viewerStore.shell).
//
// Solid is the model as built. X-ray fades walls, roof, slabs and floors,
// openings, columns, stairs and objects to a faint, see-through look with
// readable outlines. Hidden leaves only ghosted floors and faint outlines.
// Pipes are never touched: they stay solid in every mode.
//
// The fade works on the library's shared materials, one set per part of the
// model (MaterialLibrary.category), so a wall and a stair made of the same
// concrete can fade by different amounts. Only scalars move per frame; a
// material flips between opaque and see-through once at the start or the end
// of a change, which is the only time its shader is swapped. The sun shadow
// fades with the shell instead of following it frame by frame: the shadow map
// is redrawn once, when a part of the building appears or disappears.

import * as THREE from "three";
import type { DocState } from "../../contract/bindings";
import { orientedRect } from "../geom/polygon";
import type { MaterialCategory, SolidLook } from "../scene/materials";
import type { ShellMode } from "../viewerStore";

export type ShellCategory = Exclude<MaterialCategory, "" | "pipe" | "ghost">;

export interface ShellLook {
  /** Opacity factor per part of the model. 1 is as modelled, 0 is gone. */
  cat: Record<ShellCategory, number>;
  /** Opacity of the building outlines (walls, columns, stairs, objects, floors). */
  outline: number;
  /** Opacity of the roof outline. */
  roofOutline: number;
  /** Ground disc opacity factor: lets pipes under the slab show. */
  ground: number;
  /** Sun shadow strength factor. */
  shadow: number;
}

export const SHELL_CATEGORIES: readonly ShellCategory[] = ["wall", "opening", "floor", "column", "stair", "asset", "roof"];

export const SHELL_LOOKS: Record<ShellMode, ShellLook> = {
  solid: {
    cat: { wall: 1, opening: 1, floor: 1, column: 1, stair: 1, asset: 1, roof: 1 },
    outline: 0,
    roofOutline: 0,
    ground: 1,
    shadow: 1,
  },
  xray: {
    cat: { wall: 0.12, opening: 0.16, floor: 0.26, column: 0.34, stair: 0.3, asset: 0.26, roof: 0.1 },
    outline: 0.34,
    roofOutline: 0.2,
    ground: 0.42,
    shadow: 0,
  },
  hidden: {
    cat: { wall: 0, opening: 0, floor: 0.16, column: 0, stair: 0, asset: 0, roof: 0 },
    outline: 0.2,
    roofOutline: 0,
    ground: 0.34,
    shadow: 0,
  },
};

const OUTLINE_COLOR = "#1f2d44";
const EDGE_THRESHOLD_DEG = 20;

export function mixLook(a: ShellLook, b: ShellLook, k: number): ShellLook {
  const t = Math.min(Math.max(k, 0), 1);
  const m = (x: number, y: number) => x + (y - x) * t;
  const cat = {} as Record<ShellCategory, number>;
  for (const c of SHELL_CATEGORIES) cat[c] = m(a.cat[c], b.cat[c]);
  return {
    cat,
    outline: m(a.outline, b.outline),
    roofOutline: m(a.roofOutline, b.roofOutline),
    ground: m(a.ground, b.ground),
    shadow: m(a.shadow, b.shadow),
  };
}

function isShellCategory(c: unknown): c is ShellCategory {
  return typeof c === "string" && (SHELL_CATEGORIES as readonly string[]).includes(c);
}

/**
 * Puts a look onto one library material. Opaque materials turn see-through
 * (and stop writing depth) the moment their factor drops below 1, and turn
 * back when it returns; the shader is only swapped on that flip. Returns true
 * when the material appeared or disappeared, which is what the sun sees.
 */
export function applyLook(mat: THREE.Material, look: ShellLook): boolean {
  const cat = mat.userData.shellCat;
  const solid = mat.userData.solid as SolidLook | undefined;
  if (!isShellCategory(cat) || !solid) return false;
  const f = look.cat[cat];
  const transparent = solid.transparent || f < 0.999;
  if (mat.transparent !== transparent) {
    mat.transparent = transparent;
    mat.needsUpdate = true;
  }
  mat.opacity = solid.opacity * f;
  mat.depthWrite = transparent ? false : solid.depthWrite;
  const visible = f > 0.002;
  const flipped = mat.visible !== visible;
  mat.visible = visible;
  return flipped;
}

/** A category for a mesh, from its material (clones carry the same userData). */
function categoryOf(mesh: THREE.Mesh): ShellCategory | null {
  const mat = mesh.material;
  if (!mat || Array.isArray(mat)) return null;
  const cat = mat.userData.shellCat;
  return isShellCategory(cat) ? cat : null;
}

interface BuiltLike {
  root: THREE.Group;
  roofGroup: THREE.Group;
}

/**
 * Outline segments of the building in world meters: real edges for walls,
 * floors, columns, stairs and the roof, a box for each object (its real edges
 * would be a scribble). `restY` gives a mesh's resting height while an
 * entrance fade still lifts it, so the outline never bakes in the lift.
 */
export function shellOutlinePositions(
  built: BuiltLike,
  doc: DocState | null,
  restY: (mesh: THREE.Mesh) => number | undefined,
): { main: Float32Array; roof: Float32Array } {
  const main: number[] = [];
  const roof: number[] = [];
  const edges = new Map<string, Float32Array>();
  const local = new THREE.Matrix4();
  const world = new THREE.Matrix4();
  const pos = new THREE.Vector3();
  const v = new THREE.Vector3();
  built.root.updateMatrixWorld(true);

  const inRoof = (o: THREE.Object3D) => {
    for (let p: THREE.Object3D | null = o; p; p = p.parent) if (p === built.roofGroup) return true;
    return false;
  };

  built.root.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (!mesh.isMesh || mesh.userData.batch || mesh.userData.pipe || mesh.userData.outline) return;
    const cat = categoryOf(mesh);
    if (!cat || cat === "opening" || cat === "asset") return;
    const geo = mesh.geometry;
    let e = edges.get(geo.uuid);
    if (!e) {
      const eg = new THREE.EdgesGeometry(geo, EDGE_THRESHOLD_DEG);
      e = eg.getAttribute("position").array as Float32Array;
      eg.dispose();
      edges.set(geo.uuid, e);
    }
    const rest = restY(mesh);
    pos.copy(mesh.position);
    if (rest !== undefined) pos.y = rest;
    local.compose(pos, mesh.quaternion, mesh.scale);
    world.multiplyMatrices(mesh.parent ? mesh.parent.matrixWorld : new THREE.Matrix4(), local);
    const out = cat === "roof" || inRoof(mesh) ? roof : main;
    for (let i = 0; i < e.length; i += 3) {
      v.set(e[i], e[i + 1], e[i + 2]).applyMatrix4(world);
      out.push(v.x, v.y, v.z);
    }
  });

  // Objects: the box they stand in.
  if (doc) {
    const assets = new Map(doc.project.elements.filter((e) => e.kind === "asset").map((e) => [e.id, e]));
    for (const child of built.root.children) {
      const id = child.userData.elementId as string | undefined;
      const a = id ? assets.get(id) : undefined;
      if (!a || a.kind !== "asset") continue;
      const y0 = child.position.y;
      const y1 = y0 + Math.max(a.height_mm, 1) / 1000;
      const c = orientedRect(a.position, a.width_mm, a.depth_mm, a.rotation_deg).map((p) => [p.x / 1000, -p.y / 1000] as const);
      for (let i = 0; i < 4; i++) {
        const [ax, az] = c[i];
        const [bx, bz] = c[(i + 1) % 4];
        main.push(ax, y0, az, bx, y0, bz, ax, y1, az, bx, y1, bz, ax, y0, az, ax, y1, az);
      }
    }
  }
  return { main: new Float32Array(main), roof: new Float32Array(roof) };
}

/** The shell mode of one engine: its current look and its outline objects. */
export class ShellView {
  mode: ShellMode = "solid";
  /** What is on screen now. */
  look: ShellLook = SHELL_LOOKS.solid;
  private from: ShellLook = SHELL_LOOKS.solid;
  private to: ShellLook = SHELL_LOOKS.solid;
  private lineMain = new THREE.LineBasicMaterial({ color: OUTLINE_COLOR, transparent: true, opacity: 0, depthWrite: false });
  private lineRoof = new THREE.LineBasicMaterial({ color: OUTLINE_COLOR, transparent: true, opacity: 0, depthWrite: false });
  private main: THREE.LineSegments | null = null;
  private roof: THREE.LineSegments | null = null;
  /** The outlines match the current build. */
  private fresh = false;

  /** Starts a change: the look moves from where it is now to `mode`'s. */
  begin(mode: ShellMode): void {
    this.mode = mode;
    this.from = this.look;
    this.to = SHELL_LOOKS[mode];
  }

  /** `k` from 0 (where the change started) to 1 (the new mode). */
  sample(k: number): void {
    this.look = k >= 1 ? this.to : mixLook(this.from, this.to, k);
  }

  /** Jumps to a mode's look, for a first frame or reduced motion. */
  jump(mode: ShellMode): void {
    this.mode = mode;
    this.from = this.to = this.look = SHELL_LOOKS[mode];
  }

  /** True while outlines are on screen now or about to be. */
  wantsOutlines(): boolean {
    return this.look.outline > 0 || this.look.roofOutline > 0 || this.to.outline > 0 || this.to.roofOutline > 0;
  }

  /**
   * Puts the current look on every library material. True when one of them
   * appeared or disappeared: the shadow map has to be drawn once more.
   */
  applyMaterials(materials: Iterable<THREE.Material>): boolean {
    let flipped = false;
    for (const m of materials) if (applyLook(m, this.look)) flipped = true;
    return flipped;
  }

  applyOutlines(): void {
    this.lineMain.opacity = this.look.outline;
    this.lineRoof.opacity = this.look.roofOutline;
    if (this.main) this.main.visible = this.look.outline > 0.002;
    if (this.roof) this.roof.visible = this.look.roofOutline > 0.002;
  }

  /** The model was rebuilt: the old outlines went with it. */
  invalidate(): void {
    this.main = null;
    this.roof = null;
    this.fresh = false;
  }

  /** Builds the outlines for the current build when they are wanted and not built yet. */
  ensureOutlines(built: BuiltLike | null, doc: DocState | null, restY: (mesh: THREE.Mesh) => number | undefined, track: (g: THREE.BufferGeometry) => void): void {
    if (!built || this.fresh || !this.wantsOutlines()) return;
    this.fresh = true;
    const { main, roof } = shellOutlinePositions(built, doc, restY);
    const make = (arr: Float32Array, mat: THREE.LineBasicMaterial, parent: THREE.Object3D): THREE.LineSegments | null => {
      if (arr.length === 0) return null;
      const g = new THREE.BufferGeometry();
      g.setAttribute("position", new THREE.BufferAttribute(arr, 3));
      g.computeBoundingSphere();
      track(g);
      const lines = new THREE.LineSegments(g, mat);
      lines.name = "shell-outline";
      lines.userData.outline = true;
      lines.raycast = () => {};
      lines.renderOrder = 1;
      parent.add(lines);
      return lines;
    };
    this.main = make(main, this.lineMain, built.root);
    this.roof = make(roof, this.lineRoof, built.roofGroup);
    this.applyOutlines();
  }

  dispose(): void {
    this.main?.removeFromParent();
    this.roof?.removeFromParent();
    this.main = this.roof = null;
    this.lineMain.dispose();
    this.lineRoof.dispose();
  }
}
