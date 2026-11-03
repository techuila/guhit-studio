// Where a model point lands on the 3D view, for other people's pointers
// (live/LivePointers3D.tsx). Pure three.js math, no renderer: tested in
// project.test.ts.

import * as THREE from "three";
import type { Vec3 } from "../../contract/bindings";
import { vec3ToWorld } from "../geom/coords";

export interface ScreenPoint {
  /** CSS pixels from the canvas's top left. */
  x: number;
  y: number;
  /** False behind the eyes or nearer than the near plane: x and y mean nothing then. */
  inFront: boolean;
}

const tmp = new THREE.Vector3();

/**
 * A model point (plan mm, z up from project zero) to CSS pixels on a
 * `width` x `height` canvas, through `camera` as it was last drawn
 * (`matrixWorldInverse` and `projectionMatrix` up to date).
 */
export function screenPoint(camera: THREE.PerspectiveCamera, p: Vec3, width: number, height: number): ScreenPoint {
  const [x, y, z] = vec3ToWorld(p);
  tmp.set(x, y, z).applyMatrix4(camera.matrixWorldInverse);
  // The camera looks down its own -z.
  const inFront = -tmp.z > camera.near;
  tmp.applyMatrix4(camera.projectionMatrix);
  return { x: ((tmp.x + 1) / 2) * width, y: ((1 - tmp.y) / 2) * height, inFront };
}

/** The same model point to the millimeter: presence rounds to 1 mm, so this is exact equality. */
export function samePoint(a: Vec3 | null, b: Vec3 | null): boolean {
  if (a === b) return true;
  return !!a && !!b && a.x === b.x && a.y === b.y && a.z === b.z;
}

/** In front and on the canvas, grown by `margin` pixels. */
export function onView(s: ScreenPoint, width: number, height: number, margin = 0): boolean {
  return s.inFront && s.x >= -margin && s.y >= -margin && s.x <= width + margin && s.y <= height + margin;
}

/**
 * Tells a drawn frame that moved the picture from one that did not: the
 * camera's matrices and the canvas size, compared exactly. Cheap enough for
 * every drawn frame (34 numbers).
 */
export class ProjectionWatch {
  private last = new Float64Array(34);
  private seen = false;

  /** True when the camera or the canvas changed since the last call. */
  changed(camera: THREE.Camera, width: number, height: number): boolean {
    const w = camera.matrixWorld.elements;
    const p = camera.projectionMatrix.elements;
    const last = this.last;
    let diff = !this.seen || last[32] !== width || last[33] !== height;
    for (let i = 0; i < 16; i++) {
      if (last[i] !== w[i]) {
        diff = true;
        last[i] = w[i];
      }
      if (last[16 + i] !== p[i]) {
        diff = true;
        last[16 + i] = p[i];
      }
    }
    last[32] = width;
    last[33] = height;
    this.seen = true;
    return diff;
  }
}
