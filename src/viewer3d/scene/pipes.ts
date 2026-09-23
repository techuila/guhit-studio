// Pipes in the live 3D view: a tube along every segment of a run, a round
// joint at every bend, a fitting at every tee and a grey sleeve at every
// penetration (Derived.pipes). Guhit coordinates pipes, it does not size them:
// the size drawn is the nominal size the user picked, as the outside diameter.
//
// Two forms of the same geometry:
//
// - A "solo" per pipe: its own group, tagged with the element id, so picking,
//   highlights, fades, focus and the exporter treat a pipe like any other
//   element. Solos are cached by signature (BuildCache), so a wall edit does
//   not rebuild every pipe run in the house.
// - A "batch" per system, plus one for every sleeve: all runs of that system
//   merged into one mesh. This is what is drawn at rest, so a house full of
//   pipes costs five draw calls however many runs, bends and fittings it has.
//
// Solos sit on a layer the camera does not draw and the raycaster does, so a
// click still finds the exact run. When one pipe has to look different from
// its batch (hover, selection, an AI preview tint, an enter or exit fade), the
// engine promotes it: the solo moves to the drawn layer and its triangles in
// the batch collapse to nothing until it is demoted again.

import * as THREE from "three";
import type { DocState, LayerKey, Level, Pipe, PipeFitting, PipePenetration, PipeSystem } from "../../contract/bindings";
import { PIPE_COLOR_HEX, PIPE_SYSTEM_ORDER } from "../../contract/pipes";
import type { BuildCache } from "./buildCache";
import { tagElement, type Kit } from "./kit";
import type { MaterialLibrary } from "./materials";

// System order and colors come from the pipe contract (src/contract/pipes.ts),
// the same four tokens the plan, the legends and the exports use.
// `pipes.test.ts` checks them against tokens.css.

/** A sleeve reads as a separate part: neutral grey, a little larger than the pipe. */
export const SLEEVE_COLOR = "#a4a8ad";

/** Solos rest on this layer: never drawn by the camera, always seen by the raycaster. */
export const PIPE_PICK_LAYER = 1;

/** Each pipe system has a layer with the same name (docs/CONTRACT.md, "Pipes"). */
export function pipeLayer(system: PipeSystem): LayerKey {
  return system;
}

// --------------------------------------------------------------- geometry

type V3 = [number, number, number];

const add = (a: V3, b: V3): V3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const mul = (a: V3, k: number): V3 => [a[0] * k, a[1] * k, a[2] * k];
const dot = (a: V3, b: V3): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: V3, b: V3): V3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const length = (a: V3): number => Math.hypot(a[0], a[1], a[2]);
const unit = (a: V3): V3 => {
  const l = length(a);
  return l > 1e-12 ? mul(a, 1 / l) : [0, 1, 0];
};

/** Two unit vectors across `axis`, with u x v = axis. */
function basis(axis: V3): [V3, V3] {
  const helper: V3 = Math.abs(axis[1]) < 0.9 ? [0, 1, 0] : [1, 0, 0];
  const u = unit(cross(axis, helper));
  return [u, cross(axis, u)];
}

/** Indexed triangle soup in world meters. */
export class GeoBuf {
  readonly positions: number[] = [];
  readonly normals: number[] = [];
  readonly indices: number[] = [];

  get vertexCount(): number {
    return this.positions.length / 3;
  }

  get triangleCount(): number {
    return this.indices.length / 3;
  }

  private v(p: V3, n: V3): number {
    this.positions.push(p[0], p[1], p[2]);
    this.normals.push(n[0], n[1], n[2]);
    return this.vertexCount - 1;
  }

  /** Open cylinder from a to b. */
  tube(a: V3, b: V3, r: number, segments: number): void {
    const d = sub(b, a);
    if (length(d) < 1e-7 || !(r > 0)) return;
    const [u, v] = basis(unit(d));
    const base = this.vertexCount;
    for (let i = 0; i < segments; i++) {
      const t = (i / segments) * Math.PI * 2;
      const n = add(mul(u, Math.cos(t)), mul(v, Math.sin(t)));
      this.v(add(a, mul(n, r)), n);
      this.v(add(b, mul(n, r)), n);
    }
    for (let i = 0; i < segments; i++) {
      const i0 = base + 2 * i;
      const i1 = base + 2 * ((i + 1) % segments);
      this.indices.push(i0, i1, i0 + 1, i0 + 1, i1, i1 + 1);
    }
  }

  sphere(c: V3, r: number, around = 12, rings = 6): void {
    if (!(r > 0)) return;
    const base = this.vertexCount;
    for (let j = 0; j <= rings; j++) {
      const th = (j / rings) * Math.PI;
      for (let i = 0; i <= around; i++) {
        const ph = (i / around) * Math.PI * 2;
        const n: V3 = [Math.sin(th) * Math.cos(ph), Math.cos(th), Math.sin(th) * Math.sin(ph)];
        this.v(add(c, mul(n, r)), n);
      }
    }
    for (let j = 0; j < rings; j++) {
      for (let i = 0; i < around; i++) {
        const a = base + j * (around + 1) + i;
        const b = a + around + 1;
        if (j > 0) this.indices.push(a, a + 1, b);
        if (j < rings - 1) this.indices.push(b, a + 1, b + 1);
      }
    }
  }

  /** Flat disc facing `normal`. */
  disc(c: V3, normal: V3, r: number, segments: number): void {
    this.annulus(c, normal, 0, r, segments);
  }

  /** Flat ring between r1 and r2 facing `normal`. r1 = 0 makes a disc. */
  annulus(c: V3, normal: V3, r1: number, r2: number, segments: number): void {
    if (!(r2 > r1)) return;
    const n = unit(normal);
    const [u, v] = basis(n);
    const base = this.vertexCount;
    if (r1 <= 0) {
      this.v(c, n);
      for (let i = 0; i < segments; i++) {
        const t = (i / segments) * Math.PI * 2;
        this.v(add(c, add(mul(u, Math.cos(t) * r2), mul(v, Math.sin(t) * r2))), n);
      }
      for (let i = 0; i < segments; i++) this.indices.push(base, base + 1 + i, base + 1 + ((i + 1) % segments));
      return;
    }
    for (let i = 0; i < segments; i++) {
      const t = (i / segments) * Math.PI * 2;
      const dir = add(mul(u, Math.cos(t)), mul(v, Math.sin(t)));
      this.v(add(c, mul(dir, r1)), n);
      this.v(add(c, mul(dir, r2)), n);
    }
    for (let i = 0; i < segments; i++) {
      const in0 = base + 2 * i;
      const in1 = base + 2 * ((i + 1) % segments);
      this.indices.push(in0, in0 + 1, in1 + 1, in0, in1 + 1, in1);
    }
  }

  /** A collar along `axis` through `c`: a tube with ring ends from rIn to rOut. */
  collar(c: V3, axis: V3, halfLength: number, rIn: number, rOut: number, segments: number): void {
    const ax = unit(axis);
    const a = sub(c, mul(ax, halfLength));
    const b = add(c, mul(ax, halfLength));
    this.tube(a, b, rOut, segments);
    this.annulus(a, mul(ax, -1), rIn, rOut, segments);
    this.annulus(b, ax, rIn, rOut, segments);
  }

  toGeometry(): THREE.BufferGeometry | null {
    if (this.indices.length === 0) return null;
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.Float32BufferAttribute(this.positions, 3));
    g.setAttribute("normal", new THREE.Float32BufferAttribute(this.normals, 3));
    g.setIndex(new THREE.Uint32BufferAttribute(this.indices, 1));
    g.computeBoundingSphere();
    return g;
  }
}

/** Plan mm plus height mm to world meters, like `planToWorld`. */
function w(x: number, y: number, h: number): V3 {
  return [x / 1000 + 0, h / 1000 + 0, -y / 1000 + 0];
}

/** Plan direction (x east, y north, z up) to a world direction. */
function wDir(d: { x: number; y: number; z: number }): V3 {
  return unit([d.x, d.z, -d.y]);
}

/** Segments around a pipe: enough for a round look up close, few enough for a house full of runs. */
function radialSegments(diameterMm: number): number {
  return diameterMm >= 75 ? 16 : 12;
}

const BEND_COS = Math.cos((1 * Math.PI) / 180);
const JOINT_SCALE = 1.12;
const TEE_SCALE = 1.22;
const SLEEVE_SCALE = 1.45;

export interface PipeParts {
  body: GeoBuf;
  sleeves: GeoBuf;
}

/** Nearest segment direction of a run to a point, world. */
function runAxisAt(pts: V3[], at: V3): V3 {
  let best: V3 = [1, 0, 0];
  let bestD = Infinity;
  for (let i = 0; i + 1 < pts.length; i++) {
    const a = pts[i];
    const d = sub(pts[i + 1], a);
    const l2 = dot(d, d);
    if (l2 < 1e-12) continue;
    const t = Math.min(Math.max(dot(sub(at, a), d) / l2, 0), 1);
    const dist = length(sub(at, add(a, mul(d, t))));
    if (dist < bestD) {
      bestD = dist;
      best = unit(d);
    }
  }
  return best;
}

/**
 * Tubes, joints and end caps of one run, plus its tees (the runs joined on
 * it) and its sleeves. `floorMm` is the elevation of the run's level; `pipe`
 * heights are above it. `other` finds another run, for a tee's branch.
 */
export function buildPipeParts(
  pipe: Pipe,
  floorMm: number,
  fittings: PipeFitting[],
  penetrations: PipePenetration[],
  sleeveLengthMm: (p: PipePenetration) => number,
  other: (id: string) => Pipe | undefined,
): PipeParts {
  const body = new GeoBuf();
  const sleeves = new GeoBuf();
  const r = Math.max(pipe.diameter_mm, 1) / 2000;
  const seg = radialSegments(pipe.diameter_mm);
  const pts = pipe.points.map((p) => w(p.x, p.y, floorMm + p.z));
  if (pts.length < 2) return { body, sleeves };

  for (let i = 0; i + 1 < pts.length; i++) body.tube(pts[i], pts[i + 1], r, seg);
  const interior: V3[] = [];
  for (let i = 1; i + 1 < pts.length; i++) {
    const d1 = sub(pts[i], pts[i - 1]);
    const d2 = sub(pts[i + 1], pts[i]);
    const l = length(d1) * length(d2);
    interior.push(pts[i]);
    if (l > 1e-12 && dot(d1, d2) / l < BEND_COS) body.sphere(pts[i], r * JOINT_SCALE);
  }
  body.disc(pts[0], unit(sub(pts[0], pts[1])), r, seg);
  const n = pts.length;
  body.disc(pts[n - 1], unit(sub(pts[n - 1], pts[n - 2])), r, seg);

  for (const f of fittings) {
    const at = w(f.position.x, f.position.y, floorMm + f.position.z);
    const fr = Math.max(f.diameter_mm, pipe.diameter_mm, 1) / 2000;
    if (f.kind === "elbow") {
      // Interior bends already have their joint; this is two runs meeting end to end.
      if (interior.some((p) => length(sub(p, at)) < 0.001)) continue;
      body.sphere(at, fr * JOINT_SCALE);
      continue;
    }
    const axis = runAxisAt(pts, at);
    const half = Math.max(fr * 2.4, 0.02);
    const rOut = fr * TEE_SCALE;
    body.collar(at, axis, half, r, rOut, seg);
    const branch = f.branch_pipe_id ? other(f.branch_pipe_id) : undefined;
    if (branch && branch.points.length >= 2) {
      // The branch leaves from whichever of its ends sits on the tee.
      const first = branch.points[0];
      const last = branch.points[branch.points.length - 1];
      const dFirst = Math.hypot(first.x - f.position.x, first.y - f.position.y, first.z - f.position.z);
      const dLast = Math.hypot(last.x - f.position.x, last.y - f.position.y, last.z - f.position.z);
      const [end, next] = dFirst <= dLast ? [first, branch.points[1]] : [last, branch.points[branch.points.length - 2]];
      const bd = unit(sub(w(next.x, next.y, next.z), w(end.x, end.y, end.z)));
      const stub = rOut + half * 0.9;
      const tip = add(at, mul(bd, stub));
      body.tube(at, tip, rOut, seg);
      body.annulus(tip, bd, Math.max(branch.diameter_mm, 1) / 2000, rOut, seg);
    }
  }

  for (const p of penetrations) {
    const at = w(p.position.x, p.position.y, floorMm + p.position.z);
    const axis = wDir(p.direction);
    const pr = Math.max(p.diameter_mm, pipe.diameter_mm, 1) / 2000;
    const len = Math.max(sleeveLengthMm(p), 80) / 1000;
    sleeves.collar(at, axis, len / 2, pr * 1.02, pr * SLEEVE_SCALE + 0.006, seg);
  }
  return { body, sleeves };
}

// ------------------------------------------------------------------ batch

/**
 * One merged mesh for many solos. Hiding a solo's part collapses its index
 * range to one vertex (degenerate triangles draw nothing) and showing it
 * copies the original indices back: no rebuild, one small buffer upload.
 */
export class PipeBatch {
  readonly mesh: THREE.Mesh;
  private ranges = new Map<string, [number, number]>();
  private original: Uint32Array;
  private hidden = new Set<string>();

  constructor(parts: { id: string; geometry: THREE.BufferGeometry }[], material: THREE.Material, kit: Kit) {
    let vertices = 0;
    let indices = 0;
    for (const p of parts) {
      vertices += p.geometry.getAttribute("position").count;
      indices += p.geometry.getIndex()?.count ?? 0;
    }
    const pos = new Float32Array(vertices * 3);
    const nrm = new Float32Array(vertices * 3);
    const idx = new Uint32Array(indices);
    let v = 0;
    let i = 0;
    for (const p of parts) {
      const gp = p.geometry.getAttribute("position").array as Float32Array;
      const gn = p.geometry.getAttribute("normal").array as Float32Array;
      const gi = p.geometry.getIndex();
      pos.set(gp, v * 3);
      nrm.set(gn, v * 3);
      const start = i;
      if (gi) {
        const src = gi.array;
        for (let k = 0; k < src.length; k++) idx[i++] = src[k] + v;
      }
      this.ranges.set(p.id, [start, i - start]);
      v += gp.length / 3;
    }
    const geometry = kit.track(new THREE.BufferGeometry());
    geometry.setAttribute("position", new THREE.BufferAttribute(pos, 3));
    geometry.setAttribute("normal", new THREE.BufferAttribute(nrm, 3));
    geometry.setIndex(new THREE.BufferAttribute(idx, 1));
    geometry.computeBoundingSphere();
    this.original = idx.slice();
    this.mesh = new THREE.Mesh(geometry, material);
    this.mesh.castShadow = true;
    this.mesh.receiveShadow = true;
    this.mesh.userData.batch = true;
    this.mesh.name = "pipe-batch";
    // Never picked: the solos answer the raycaster.
    this.mesh.raycast = () => {};
  }

  has(id: string): boolean {
    return this.ranges.has(id);
  }

  /** Hides exactly `ids` (the promoted solos). True when the buffer changed. */
  setHidden(ids: ReadonlySet<string>): boolean {
    const index = this.mesh.geometry.getIndex();
    if (!index) return false;
    const arr = index.array as Uint32Array;
    let changed = false;
    const touch = (id: string, hide: boolean) => {
      const range = this.ranges.get(id);
      if (!range || range[1] === 0) return;
      const [start, count] = range;
      if (hide) arr.fill(arr[start], start, start + count);
      else arr.set(this.original.subarray(start, start + count), start);
      index.addUpdateRange(start, count);
      changed = true;
    };
    for (const id of this.hidden) if (!ids.has(id)) touch(id, false);
    for (const id of ids) if (!this.hidden.has(id)) touch(id, true);
    this.hidden = new Set([...ids].filter((id) => this.ranges.has(id)));
    if (changed) index.needsUpdate = true;
    return changed;
  }
}

/** Everything the pipe layer put into one build. */
export class PipeScene {
  /** One group per pipe, a direct child of the model root. */
  readonly solos = new Map<string, THREE.Group>();
  readonly batches: PipeBatch[] = [];
  private promoted = new Set<string>();

  get size(): number {
    return this.solos.size;
  }

  /** Pipe ids drawn by their solo right now. */
  promotedIds(): ReadonlySet<string> {
    return this.promoted;
  }

  meshesOf(id: string): THREE.Mesh[] {
    const out: THREE.Mesh[] = [];
    this.solos.get(id)?.traverse((o) => {
      if ((o as THREE.Mesh).isMesh && o.userData.pipe) out.push(o as THREE.Mesh);
    });
    return out;
  }

  /**
   * Draws exactly `ids` from their solos and everything else from the
   * batches. Cheap when nothing changed. True when something did.
   */
  promote(ids: Iterable<string>): boolean {
    const want = new Set<string>();
    for (const id of ids) if (this.solos.has(id)) want.add(id);
    let same = want.size === this.promoted.size;
    if (same) for (const id of want) if (!this.promoted.has(id)) same = false;
    if (same) return false;
    for (const [id, group] of this.solos) {
      const on = want.has(id);
      if (on === this.promoted.has(id)) continue;
      group.traverse((o) => {
        if ((o as THREE.Mesh).isMesh && o.userData.pipe) o.layers.set(on ? 0 : PIPE_PICK_LAYER);
      });
    }
    for (const b of this.batches) b.setHidden(want);
    this.promoted = want;
    return true;
  }
}

// ------------------------------------------------------------------ build

export interface PipeBuildContext {
  doc: DocState;
  lib: MaterialLibrary;
  kit: Kit;
  cache?: BuildCache;
  levelOf: (levelId: string) => Level | null;
  levelShown: (l: Level) => boolean;
  layerVisible: (key: LayerKey) => boolean;
  layerLocked: (key: LayerKey) => boolean;
  /** Depth of the slab under a level, mm. Slab sleeves are that long. */
  slabDepth: (l: Level) => number;
  /** Adds a run to the model bounds: plan points, lowest and highest height mm. */
  addBounds?: (points: { x: number; y: number }[], z0: number, z1: number) => void;
}

/** True when a build draws this pipe: its system layer is on and its level is shown. */
export function pipeShown(
  pipe: Pipe,
  levelOf: (levelId: string) => Level | null,
  levelShown: (l: Level) => boolean,
  layerVisible: (key: LayerKey) => boolean,
): boolean {
  if (pipe.points.length < 2 || !layerVisible(pipeLayer(pipe.system))) return false;
  const level = levelOf(pipe.level_id);
  return !!level && levelShown(level);
}

/** Pipes on visible layers and shown levels, as solos and batches. */
export function buildPipes(ctx: PipeBuildContext): PipeScene {
  const out = new PipeScene();
  const { doc, lib, kit } = ctx;
  const pipes = doc.project.elements.filter((e): e is Extract<typeof e, { kind: "pipe" }> => e.kind === "pipe");
  if (pipes.length === 0) return out;
  const byId = new Map(pipes.map((p) => [p.id, p as Pipe]));
  const network = doc.derived?.pipes;
  const fittingsOf = new Map<string, PipeFitting[]>();
  for (const f of network?.fittings ?? []) {
    const list = fittingsOf.get(f.pipe_id) ?? [];
    list.push(f);
    fittingsOf.set(f.pipe_id, list);
  }
  const pensOf = new Map<string, PipePenetration[]>();
  for (const p of network?.penetrations ?? []) {
    const list = pensOf.get(p.pipe_id) ?? [];
    list.push(p);
    pensOf.set(p.pipe_id, list);
  }
  const wallThickness = new Map<string, number>();
  for (const e of doc.project.elements) if (e.kind === "wall") wallThickness.set(e.id, e.thickness_mm);
  const roofThickness = doc.project.roof?.thickness_mm ?? 100;

  lib.category = "pipe";
  const sleeveMat = lib.plain(SLEEVE_COLOR, 0.6);
  const bodies = new Map<PipeSystem, { id: string; geometry: THREE.BufferGeometry }[]>();
  const sleeveParts: { id: string; geometry: THREE.BufferGeometry }[] = [];

  for (const pipe of pipes) {
    if (!pipeShown(pipe, ctx.levelOf, ctx.levelShown, ctx.layerVisible)) continue;
    const layer = pipeLayer(pipe.system);
    const level = ctx.levelOf(pipe.level_id) as Level;
    const mat = lib.pipe(PIPE_COLOR_HEX[pipe.system] ?? PIPE_COLOR_HEX.cold_water);
    const locked = ctx.layerLocked(layer);
    const fittings = fittingsOf.get(pipe.id) ?? [];
    const pens = pensOf.get(pipe.id) ?? [];
    const sleeveLength = (p: PipePenetration): number => {
      const host = p.kind === "wall" ? (p.host_id ? wallThickness.get(p.host_id) : undefined) : p.kind === "slab" ? ctx.slabDepth(level) : roofThickness;
      return (host ?? 150) + 60;
    };
    const branchKey = fittings
      .filter((f) => f.branch_pipe_id)
      .map((f) => JSON.stringify(byId.get(f.branch_pipe_id as string)?.points ?? null))
      .join(";");
    const key = [
      "pipe",
      JSON.stringify(pipe),
      level.elevation_mm,
      JSON.stringify(fittings),
      JSON.stringify(pens.map((p) => [p, sleeveLength(p)])),
      branchKey,
      locked,
      mat.uuid,
      sleeveMat.uuid,
    ].join("|");

    let group = ctx.cache?.take(key) ?? null;
    if (!group) {
      const parts = buildPipeParts(pipe, level.elevation_mm, fittings, pens, sleeveLength, (id) => byId.get(id));
      group = new THREE.Group();
      group.name = `pipe-${pipe.id}`;
      const geometries: THREE.BufferGeometry[] = [];
      const bodyGeo = parts.body.toGeometry();
      if (bodyGeo) {
        geometries.push(bodyGeo);
        const m = new THREE.Mesh(bodyGeo, mat);
        m.castShadow = true;
        m.receiveShadow = true;
        m.userData.pipe = true;
        m.userData.part = "body";
        group.add(m);
      }
      const sleeveGeo = parts.sleeves.toGeometry();
      if (sleeveGeo) {
        geometries.push(sleeveGeo);
        const m = new THREE.Mesh(sleeveGeo, sleeveMat);
        m.castShadow = true;
        m.receiveShadow = true;
        m.userData.pipe = true;
        m.userData.part = "sleeve";
        group.add(m);
      }
      tagElement(group, pipe.id, locked);
      if (ctx.cache) ctx.cache.put(key, group, geometries);
      else for (const g of geometries) kit.track(g);
    }
    // A reused solo may still be on the drawn layer from its last build.
    group.traverse((o) => {
      if ((o as THREE.Mesh).isMesh && o.userData.pipe) o.layers.set(PIPE_PICK_LAYER);
    });
    out.solos.set(pipe.id, group);
    for (const child of group.children) {
      const mesh = child as THREE.Mesh;
      if (!mesh.isMesh) continue;
      if (mesh.userData.part === "sleeve") sleeveParts.push({ id: pipe.id, geometry: mesh.geometry });
      else {
        const list = bodies.get(pipe.system) ?? [];
        list.push({ id: pipe.id, geometry: mesh.geometry });
        bodies.set(pipe.system, list);
      }
    }
    if (ctx.addBounds) {
      let top = -Infinity;
      for (const p of pipe.points) top = Math.max(top, p.z);
      ctx.addBounds(pipe.points, level.elevation_mm, level.elevation_mm + Math.max(top, 0));
    }
  }

  for (const system of PIPE_SYSTEM_ORDER) {
    const parts = bodies.get(system);
    if (!parts || parts.length === 0) continue;
    out.batches.push(new PipeBatch(parts, lib.pipe(PIPE_COLOR_HEX[system]), kit));
  }
  if (sleeveParts.length > 0) out.batches.push(new PipeBatch(sleeveParts, sleeveMat, kit));
  return out;
}

/** A plain run for the red ghost of a pipe an AI proposal would remove. */
export function buildPipeGhost(pipe: Pipe, floorMm: number, material: THREE.Material, kit: Kit): THREE.Mesh | null {
  const parts = buildPipeParts(pipe, floorMm, [], [], () => 0, () => undefined);
  const geo = parts.body.toGeometry();
  if (!geo) return null;
  const mesh = new THREE.Mesh(kit.track(geo), material);
  mesh.castShadow = false;
  mesh.receiveShadow = false;
  mesh.renderOrder = 3;
  return mesh;
}
