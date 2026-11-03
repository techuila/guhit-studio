// The pipe part of the cutaway (DECISIONS D40). The cutaway cuts runs on the
// active level in a solid shell and builds them with a cap at the cut (scene/
// pipes.ts); X-ray and hidden show them whole. A shell change while the cut
// is on sweeps a clipping plane over the pipe materials alone, the way the
// cutaway's own plane sweeps the building: up from the cut when runs become
// whole, down to it before they are built cut again (ViewerEngine.syncPipeCut).

import * as THREE from "three";
import type { PipeScene } from "../scene/pipes";

/** One clipping plane over the pipe materials, from one height to another. */
export class PipeCutSweep {
  private plane: THREE.Plane | null = null;
  private from = 0;
  private to = 0;
  private clipped = new Set<THREE.Material>();
  private source: () => Iterable<THREE.Material> = () => [];

  get active(): boolean {
    return this.plane !== null;
  }

  /** World height of the plane now, meters, or null without a sweep. */
  height(): number | null {
    return this.plane ? this.plane.constant : null;
  }

  /**
   * Starts a sweep from `fromY` to `toY` (world meters) over the materials
   * `source` gives. A sweep already running goes on from where its plane is:
   * no jump.
   */
  start(fromY: number, toY: number, source: () => Iterable<THREE.Material>): void {
    this.from = this.plane ? this.plane.constant : fromY;
    this.to = toY;
    this.source = source;
    this.plane ??= new THREE.Plane(new THREE.Vector3(0, -1, 0), this.from);
    this.plane.constant = this.from;
    this.cover();
  }

  /**
   * `k` from 0 (where it started) to 1 (where it goes). Materials that came
   * up since the last frame get the plane too: a hover or selection tint made
   * mid-sweep is a copy whose plane is a frozen clone, and `clear` has to
   * reach it as well.
   */
  apply(k: number): void {
    if (!this.plane) return;
    this.plane.constant = this.from + (this.to - this.from) * k;
    this.cover();
  }

  private cover(): void {
    const plane = this.plane;
    if (!plane) return;
    for (const m of this.source()) {
      const planes = m.clippingPlanes;
      if (planes && planes.length === 1 && planes[0] === plane) continue;
      // Same number of planes: the shader stays, only the plane changes.
      if (!planes || planes.length !== 1) m.needsUpdate = true;
      m.clippingPlanes = [plane];
      this.clipped.add(m);
    }
  }

  /** Takes the plane off every material it was put on. */
  clear(): void {
    for (const m of this.clipped) {
      if (m.clippingPlanes && m.clippingPlanes[0] === this.plane) {
        m.clippingPlanes = null;
        m.needsUpdate = true;
      }
    }
    this.clipped.clear();
    this.plane = null;
    this.source = () => [];
  }
}

/** The materials a build draws its pipes with now: solos (and their highlight copies), batches and sleeves. */
export function pipeMaterials(pipes: PipeScene): THREE.Material[] {
  const out = new Set<THREE.Material>();
  const add = (mesh: THREE.Mesh) => {
    const mat = mesh.material;
    if (mat && !Array.isArray(mat)) out.add(mat);
  };
  for (const solo of pipes.solos.values()) {
    for (const o of solo.children) if ((o as THREE.Mesh).isMesh && o.userData.pipe) add(o as THREE.Mesh);
  }
  for (const b of pipes.batches) add(b.mesh);
  return [...out];
}
