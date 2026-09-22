// Temporary opacity and lift for a set of meshes, used by every 3D entrance,
// exit and toggle. Fading needs transparent materials, and the scene's
// materials are shared, so a fade clones one material per distinct base
// material (not per mesh) for as long as it runs and puts the originals back
// when it ends. `release` disposes every clone, so geometry, material and
// texture counts return to their steady values once the animation is over.

import * as THREE from "three";

interface Member {
  mesh: THREE.Mesh;
  base: THREE.Material;
  baseY: number;
  castShadow: boolean;
  renderOrder: number;
  visible: boolean;
}

export class MeshFade {
  private members: Member[] = [];
  private clones = new Map<string, THREE.Material>();
  private released = false;
  private lift = 0;
  private opacity = 1;

  constructor(meshes: Iterable<THREE.Mesh>) {
    for (const mesh of meshes) {
      const base = mesh.material;
      // Arrays are not produced by this scene builder, and a fade of one is
      // not worth the bookkeeping: leave those meshes alone.
      if (Array.isArray(base)) continue;
      this.members.push({
        mesh,
        base,
        baseY: mesh.position.y,
        castShadow: mesh.castShadow,
        renderOrder: mesh.renderOrder,
        visible: mesh.visible,
      });
    }
  }

  get empty(): boolean {
    return this.members.length === 0;
  }

  /**
   * True while this fade is driving that mesh's material. A highlight must
   * not fight a fade over the same mesh: it waits for the fade to land.
   */
  owns(mesh: THREE.Mesh): boolean {
    return !this.released && this.members.some((m) => m.mesh === mesh);
  }

  private cloneFor(base: THREE.Material): THREE.Material {
    let clone = this.clones.get(base.uuid);
    if (clone) return clone;
    clone = base.clone();
    clone.userData = { ...base.userData };
    clone.transparent = true;
    clone.depthWrite = false;
    this.clones.set(base.uuid, clone);
    return clone;
  }

  /** 1 puts the original materials back, anything less swaps in the clones. */
  setOpacity(o: number): void {
    if (this.released) return;
    const clamped = Math.min(Math.max(o, 0), 1);
    this.opacity = clamped;
    const solid = clamped >= 0.999;
    for (const m of this.members) {
      if (solid) {
        m.mesh.material = m.base;
        m.mesh.castShadow = m.castShadow;
        m.mesh.renderOrder = m.renderOrder;
        m.mesh.visible = m.visible;
        continue;
      }
      const clone = this.cloneFor(m.base);
      clone.opacity = (m.base.opacity ?? 1) * clamped;
      m.mesh.material = clone;
      // A half-faded mesh casting a hard shadow reads as a bug, so the
      // shadow comes back with the solid material.
      m.mesh.castShadow = false;
      m.mesh.visible = m.visible && clamped > 0.002;
    }
  }

  /** Offsets every mesh this many meters above its resting height. */
  setLift(dy: number): void {
    if (this.released) return;
    this.lift = dy;
    for (const m of this.members) m.mesh.position.y = m.baseY + dy;
  }

  /** Current values, for the dev readout. */
  state(): { opacity: number; lift: number; meshes: number; clones: number } {
    return { opacity: this.opacity, lift: this.lift, meshes: this.members.length, clones: this.clones.size };
  }

  /** Restores the original materials and positions and disposes every clone. */
  release(): void {
    if (this.released) return;
    this.released = true;
    for (const m of this.members) {
      m.mesh.material = m.base;
      m.mesh.position.y = m.baseY;
      m.mesh.castShadow = m.castShadow;
      m.mesh.renderOrder = m.renderOrder;
      m.mesh.visible = m.visible;
    }
    for (const c of this.clones.values()) c.dispose();
    this.clones.clear();
    this.members.length = 0;
  }
}

/** Every mesh under `root`, including `root` itself when it is one. */
export function meshesUnder(root: THREE.Object3D): THREE.Mesh[] {
  const out: THREE.Mesh[] = [];
  root.traverse((o) => {
    if ((o as THREE.Mesh).isMesh) out.push(o as THREE.Mesh);
  });
  return out;
}
