// The one place where plan space becomes three.js world space.
//
// Plan: x east, y north, lengths in mm. Heights in mm above the project zero.
// World: meters. plan (x, y) -> three (x, -y on the z axis), height -> three y.

import type { Camera, Vec3 } from "../../contract/bindings";

export const MM_TO_M = 0.001;
export const M_TO_MM = 1000;

export interface Pt {
  x: number;
  y: number;
}

export type Triple = [number, number, number];

/** Plan point (mm) plus height (mm) to a three.js position in meters. */
export function planToWorld(x: number, y: number, heightMm: number): Triple {
  // "+ 0" turns a negative zero into a plain zero.
  return [x * MM_TO_M + 0, heightMm * MM_TO_M + 0, -y * MM_TO_M + 0];
}

/** Contract Vec3 (mm, x east, y north, z up) to a three.js position in meters. */
export function vec3ToWorld(v: Vec3): Triple {
  return planToWorld(v.x, v.y, v.z);
}

/** Three.js position in meters back to a contract Vec3 in mm. */
export function worldToVec3(x: number, y: number, z: number): Vec3 {
  return { x: x * M_TO_MM + 0, y: -z * M_TO_MM + 0, z: y * M_TO_MM + 0 };
}

/** Plan rotation (degrees, counter-clockwise) to a three.js rotation about +y. */
export function planRotationToWorld(deg: number): number {
  return (deg * Math.PI) / 180;
}

export interface WorldPose {
  position: Triple;
  target: Triple;
  fovDeg: number;
}

export function cameraToPose(camera: Pick<Camera, "position" | "target" | "fov_deg">): WorldPose {
  return {
    position: vec3ToWorld(camera.position),
    target: vec3ToWorld(camera.target),
    fovDeg: camera.fov_deg,
  };
}
