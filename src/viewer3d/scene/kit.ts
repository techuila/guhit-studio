// Small mesh construction kit shared by the scene builders. Tracks every
// geometry it creates so a rebuild can dispose all of them.

import * as THREE from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import type { MeshData } from "../geom/meshData";

/** Shape of a `Kit.round` part. */
export interface RoundOptions {
  /** Sides around. 24 reads round at room scale; small devices use 8 to 16. */
  segments?: number;
  /** Top radius as a fraction of the bottom one: 1 a cylinder, less a cone frustum, 0 a cone. */
  top?: number;
  /** No end caps, and the wall drawn from both sides: a lamp shade you can look into. */
  open?: boolean;
  /** Depth radius when it differs from `radius` (an oval). */
  radiusZ?: number;
  /** Cut at the cutaway height, as in `box`. */
  clip?: { baseY: number };
}

/**
 * The same triangles again with the winding and the normals reversed, merged
 * with the originals: an open surface that shows from inside and outside
 * without a double-sided material, so it merges with everything else.
 */
function doubleSided(g: THREE.BufferGeometry): THREE.BufferGeometry {
  const outer = g.index ? g.toNonIndexed() : g;
  const inner = outer.clone();
  const attrs = ["position", "normal", "uv"]
    .map((name) => inner.getAttribute(name))
    .filter((a): a is THREE.BufferAttribute => !!a && a instanceof THREE.BufferAttribute);
  for (const a of attrs) {
    for (let i = 0; i + 2 < a.count; i += 3) {
      for (let k = 0; k < a.itemSize; k++) {
        const t = a.array[(i + 1) * a.itemSize + k];
        a.array[(i + 1) * a.itemSize + k] = a.array[(i + 2) * a.itemSize + k];
        a.array[(i + 2) * a.itemSize + k] = t;
      }
    }
  }
  const n = inner.getAttribute("normal");
  if (n) for (let i = 0; i < n.count; i++) n.setXYZ(i, -n.getX(i), -n.getY(i), -n.getZ(i));
  const merged = mergeGeometries([outer, inner], false) ?? outer.clone();
  if (outer !== g) outer.dispose();
  inner.dispose();
  g.dispose();
  return merged;
}

export class Kit {
  readonly geometries: THREE.BufferGeometry[] = [];
  private unitBox: THREE.BufferGeometry | null = null;
  private unitCyl: THREE.BufferGeometry | null = null;
  private unitBall: THREE.BufferGeometry | null = null;
  /** Other unit geometries (low-poly rounds, frustums, shades), made once per kit. */
  private units = new Map<string, THREE.BufferGeometry>();
  /** Cutaway height in world meters, or null. Used by `box` when `clip` is set. */
  cutY: number | null = null;

  track<T extends THREE.BufferGeometry>(g: T): T {
    this.geometries.push(g);
    return g;
  }

  /** Takes over another kit's geometries, so one `dispose` covers both. */
  adopt(other: Kit): void {
    this.geometries.push(...other.geometries);
    other.geometries.length = 0;
    other.unitBox = other.unitCyl = other.unitBall = null;
    other.units.clear();
  }

  /** A unit geometry shared by every mesh of this kit that asks for the same key. */
  private unit(key: string, make: () => THREE.BufferGeometry): THREE.BufferGeometry {
    let g = this.units.get(key);
    if (!g) {
      g = this.track(make());
      this.units.set(key, g);
    }
    return g;
  }

  fromMeshData(md: MeshData): THREE.BufferGeometry {
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.Float32BufferAttribute(md.positions, 3));
    g.setAttribute("normal", new THREE.Float32BufferAttribute(md.normals, 3));
    g.setAttribute("uv", new THREE.Float32BufferAttribute(md.uvs, 2));
    g.computeBoundingSphere();
    return this.track(g);
  }

  mesh(geometry: THREE.BufferGeometry, material: THREE.Material, elementId?: string): THREE.Mesh {
    const m = new THREE.Mesh(geometry, material);
    const glass = material.userData.glass === true;
    m.castShadow = !glass;
    m.receiveShadow = !glass;
    if (glass) m.renderOrder = 2;
    if (elementId) m.userData.elementId = elementId;
    return m;
  }

  /**
   * Box with its center at (x, z) and its underside at `y`. Local meters.
   * With `clip`, the box is cut at the cutaway height (parent y offset given
   * by `baseY`), or left out when it starts above it.
   */
  box(
    parent: THREE.Object3D,
    material: THREE.Material,
    sx: number,
    sy: number,
    sz: number,
    x: number,
    y: number,
    z: number,
    clip?: { baseY: number },
  ): THREE.Mesh | null {
    if (clip && this.cutY !== null) {
      const room = this.cutY - (clip.baseY + y);
      if (room <= 0.001) return null;
      sy = Math.min(sy, room);
    }
    if (!(sx > 0 && sy > 0 && sz > 0)) return null;
    this.unitBox ??= this.track(new THREE.BoxGeometry(1, 1, 1));
    const m = this.mesh(this.unitBox, material);
    m.scale.set(sx, sy, sz);
    m.position.set(x, y + sy / 2, z);
    parent.add(m);
    return m;
  }

  /** Upright cylinder, center (x, z), underside at y. */
  cylinder(
    parent: THREE.Object3D,
    material: THREE.Material,
    radius: number,
    height: number,
    x: number,
    y: number,
    z: number,
    radiusZ?: number,
  ): THREE.Mesh;
  /**
   * With `clip`, the cylinder is cut at the cutaway height (parent y offset
   * given by `baseY`), or left out when it starts above it.
   */
  cylinder(
    parent: THREE.Object3D,
    material: THREE.Material,
    radius: number,
    height: number,
    x: number,
    y: number,
    z: number,
    radiusZ: number | undefined,
    clip: { baseY: number },
  ): THREE.Mesh | null;
  cylinder(
    parent: THREE.Object3D,
    material: THREE.Material,
    radius: number,
    height: number,
    x: number,
    y: number,
    z: number,
    radiusZ = radius,
    clip?: { baseY: number },
  ): THREE.Mesh | null {
    if (clip && this.cutY !== null) {
      const room = this.cutY - (clip.baseY + y);
      if (room <= 0.001) return null;
      height = Math.min(height, room);
    }
    this.unitCyl ??= this.track(new THREE.CylinderGeometry(1, 1, 1, 24));
    const m = this.mesh(this.unitCyl, material);
    m.scale.set(radius, height, radiusZ);
    m.position.set(x, y + height / 2, z);
    parent.add(m);
    return m;
  }

  ball(
    parent: THREE.Object3D,
    material: THREE.Material,
    rx: number,
    ry: number,
    rz: number,
    x: number,
    y: number,
    z: number,
  ): THREE.Mesh {
    this.unitBall ??= this.track(new THREE.IcosahedronGeometry(1, 1));
    const m = this.mesh(this.unitBall, material);
    m.scale.set(rx, ry, rz);
    m.position.set(x, y, z);
    parent.add(m);
    return m;
  }

  /**
   * Upright round part with its center at (x, z) and its underside at y: a
   * cylinder with as few sides as the part needs, a cone frustum, or an open
   * shade. The unit geometry is made once per kit for each shape and shared,
   * like the unit box. Returns null for an empty part, or one cut away.
   */
  round(
    parent: THREE.Object3D,
    material: THREE.Material,
    radius: number,
    height: number,
    x: number,
    y: number,
    z: number,
    opts: RoundOptions = {},
  ): THREE.Mesh | null {
    if (opts.clip && this.cutY !== null) {
      const room = this.cutY - (opts.clip.baseY + y);
      if (room <= 0.001) return null;
      height = Math.min(height, room);
    }
    const rz = opts.radiusZ ?? radius;
    if (!(radius > 0 && rz > 0 && height > 0)) return null;
    const segments = Math.max(3, Math.min(64, Math.round(opts.segments ?? 24)));
    const top = Math.max(0, Math.min(4, opts.top ?? 1));
    const open = opts.open === true;
    // Rounded so near-equal tapers share one geometry.
    const taper = Math.round(top * 100) / 100;
    const geo = this.unit(`round|${segments}|${taper}|${open ? "open" : "closed"}`, () => {
      const g = new THREE.CylinderGeometry(taper, 1, 1, segments, 1, open);
      return open ? doubleSided(g) : g;
    });
    const m = this.mesh(geo, material);
    m.scale.set(radius, height, rz);
    m.position.set(x, y + height / 2, z);
    parent.add(m);
    return m;
  }

  /**
   * Ring in the x-y plane facing +z, centered on (x, y, z): a fan grille rim.
   * `tube` is the thickness radius of the ring itself.
   */
  ring(parent: THREE.Object3D, material: THREE.Material, radius: number, tube: number, x: number, y: number, z: number, segments = 24): THREE.Mesh | null {
    if (!(radius > 0 && tube > 0)) return null;
    const t = Math.round(Math.min(Math.max(tube / radius, 0.01), 0.5) * 100) / 100;
    const n = Math.max(8, Math.min(64, Math.round(segments)));
    const geo = this.unit(`ring|${n}|${t}`, () => new THREE.TorusGeometry(1, t, 6, n));
    const m = this.mesh(geo, material);
    m.scale.setScalar(radius);
    m.position.set(x, y, z);
    parent.add(m);
    return m;
  }

  /**
   * Collapses the meshes under `root` into one mesh per material. An item made
   * of twenty little boxes (a sofa, a car, a jalousie window) becomes two or
   * three draw calls instead of twenty, which is what a scene with hundreds of
   * furniture pieces spends all of its time on.
   *
   * The merge stays inside one element: every mesh that comes out still
   * belongs to a single element id, so picking, highlights, fades and the
   * exporter keep working exactly as before. `tagElement` runs after this.
   */
  mergeByMaterial(root: THREE.Object3D): void {
    root.updateMatrixWorld(true);
    const toRoot = root.matrixWorld.clone().invert();
    const buckets = new Map<string, { material: THREE.Material; sample: THREE.Mesh; meshes: THREE.Mesh[] }>();
    root.traverse((o) => {
      const mesh = o as THREE.Mesh;
      if (!mesh.isMesh || Array.isArray(mesh.material) || !mesh.geometry) return;
      const mat = mesh.material;
      const key = [mat.uuid, mesh.castShadow, mesh.receiveShadow, mesh.renderOrder, mesh.visible].join("|");
      const bucket = buckets.get(key);
      if (bucket) bucket.meshes.push(mesh);
      else buckets.set(key, { material: mat, sample: mesh, meshes: [mesh] });
    });

    for (const bucket of buckets.values()) {
      if (bucket.meshes.length < 2) continue;
      const parts: THREE.BufferGeometry[] = [];
      for (const mesh of bucket.meshes) {
        // Unit box, cylinder and ball geometries are shared: never bake a
        // transform into the original, always into a throwaway copy.
        let g = mesh.geometry.clone();
        if (g.index) g = g.toNonIndexed();
        g.applyMatrix4(toRoot.clone().multiply(mesh.matrixWorld));
        // mergeGeometries needs one identical attribute set across the parts.
        for (const name of Object.keys(g.attributes)) {
          if (name !== "position" && name !== "normal" && name !== "uv") g.deleteAttribute(name);
        }
        if (!g.attributes.uv) {
          const count = g.attributes.position.count;
          g.setAttribute("uv", new THREE.BufferAttribute(new Float32Array(count * 2), 2));
        }
        parts.push(g);
      }
      const merged = mergeGeometries(parts, false);
      for (const g of parts) g.dispose();
      if (!merged) continue;
      merged.computeBoundingSphere();
      const mesh = new THREE.Mesh(this.track(merged), bucket.material);
      mesh.castShadow = bucket.sample.castShadow;
      mesh.receiveShadow = bucket.sample.receiveShadow;
      mesh.renderOrder = bucket.sample.renderOrder;
      mesh.visible = bucket.sample.visible;
      for (const old of bucket.meshes) old.removeFromParent();
      root.add(mesh);
    }
    // Pivot groups (door leaves, sashes) are empty once their meshes moved.
    for (const child of [...root.children]) {
      if (!(child as THREE.Mesh).isMesh && child.children.length === 0) child.removeFromParent();
    }
  }

  dispose(): void {
    for (const g of this.geometries) g.dispose();
    this.geometries.length = 0;
    this.unitBox = this.unitCyl = this.unitBall = null;
    this.units.clear();
  }
}

/** Marks every mesh under `root` with the element id used for picking. */
export function tagElement(root: THREE.Object3D, elementId: string, locked: boolean): void {
  root.traverse((o) => {
    o.userData.elementId = elementId;
    if (locked) o.userData.locked = true;
  });
}
