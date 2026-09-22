// Small mesh construction kit shared by the scene builders. Tracks every
// geometry it creates so a rebuild can dispose all of them.

import * as THREE from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import type { MeshData } from "../geom/meshData";

export class Kit {
  readonly geometries: THREE.BufferGeometry[] = [];
  private unitBox: THREE.BufferGeometry | null = null;
  private unitCyl: THREE.BufferGeometry | null = null;
  private unitBall: THREE.BufferGeometry | null = null;
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
  }
}

/** Marks every mesh under `root` with the element id used for picking. */
export function tagElement(root: THREE.Object3D, elementId: string, locked: boolean): void {
  root.traverse((o) => {
    o.userData.elementId = elementId;
    if (locked) o.userData.locked = true;
  });
}
