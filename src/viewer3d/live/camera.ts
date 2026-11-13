// The 3D camera as a live session sends it (DECISIONS D37, `Presence::camera`)
// and the test that keeps a resting view quiet. Pure: tested in camera.test.ts.
//
// Units are the contract's: plan mm, x east, y north, z up from project zero,
// like `ViewerEngine.currentCamera()`. The engine converts from three.js
// meters through geom/coords.ts.

import type { LiveCamera, NavMode } from "../../contract/bindings";
import { worldToVec3 } from "../geom/coords";

export interface XYZ {
  x: number;
  y: number;
  z: number;
}

/**
 * The orbit camera's polar limit while following: a walking or flying leader
 * may look level or up, which the usual limit (just past the horizon) would
 * clamp. Straight up stays out, where OrbitControls flips.
 */
export const FOLLOW_MAX_POLAR = Math.PI - 0.01;

/** Rounded to 1 mm like presence (`clean_presence`), and "+ 0" drops a negative zero. */
const mm = (v: number): number => Math.round(v) + 0;

/** A three.js pose (meters, y up) as a presence camera, rounded to 1 mm and 0.01 degree. */
export function liveCameraOf(position: XYZ, target: XYZ, fovDeg: number, nav: NavMode): LiveCamera {
  const p = worldToVec3(position.x, position.y, position.z);
  const t = worldToVec3(target.x, target.y, target.z);
  return {
    position: { x: mm(p.x), y: mm(p.y), z: mm(p.z) },
    target: { x: mm(t.x), y: mm(t.y), z: mm(t.z) },
    fov_deg: Math.round(fovDeg * 100) / 100,
    nav,
  };
}

/**
 * Nothing anyone could see moved: every coordinate within `epsMm`, the field
 * of view within `epsDeg`, the same navigation. A resting view compares equal
 * frame after frame, so it sends nothing.
 */
export function sameLiveCamera(a: LiveCamera | null, b: LiveCamera, epsMm = 0.5, epsDeg = 0.005): boolean {
  if (!a) return false;
  if (a.nav !== b.nav || Math.abs(a.fov_deg - b.fov_deg) > epsDeg) return false;
  const close = (u: XYZ, v: XYZ) => Math.abs(u.x - v.x) <= epsMm && Math.abs(u.y - v.y) <= epsMm && Math.abs(u.z - v.z) <= epsMm;
  return close(a.position, b.position) && close(a.target, b.target);
}

/** A camera worth flying to: finite everywhere, the eye apart from what it looks at. */
export function usableLiveCamera(c: LiveCamera | null | undefined): c is LiveCamera {
  if (!c || !c.position || !c.target) return false;
  const nums = [c.position.x, c.position.y, c.position.z, c.target.x, c.target.y, c.target.z, c.fov_deg];
  if (!nums.every(Number.isFinite)) return false;
  return Math.hypot(c.position.x - c.target.x, c.position.y - c.target.y, c.position.z - c.target.z) > 1;
}

/** The field of view a follower uses: the contract's 5 to 110 degrees. */
export function followFov(fovDeg: number): number {
  return Math.min(Math.max(Number.isFinite(fovDeg) ? fovDeg : 45, 5), 110);
}

/**
 * Following an agent (DECISIONS D47): `from` moved so it looks at `point`,
 * the same direction and distance, the same field of view. Agents have no
 * camera of their own, only a point they work at. Always an orbit camera.
 */
export function panCamera(from: LiveCamera, point: XYZ): LiveCamera {
  const dx = point.x - from.target.x;
  const dy = point.y - from.target.y;
  const dz = point.z - from.target.z;
  return {
    position: { x: mm(from.position.x + dx), y: mm(from.position.y + dy), z: mm(from.position.z + dz) },
    target: { x: mm(point.x), y: mm(point.y), z: mm(point.z) },
    fov_deg: from.fov_deg,
    nav: "orbit",
  };
}
