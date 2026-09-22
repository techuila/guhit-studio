// Camera presets and sun direction from model bounds. Pure math, contract
// units: world mm, x east, y north, z up.

import type { CameraPreset, Vec3 } from "../../contract/bindings";
import type { Pt } from "./coords";
import { longAxis, pointInPolygon } from "./polygon";

export interface ModelBounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
  /** Lowest and highest point, mm. */
  minZ: number;
  maxZ: number;
}

export interface PosePreset {
  preset: CameraPreset;
  name: string;
  position: Vec3;
  target: Vec3;
  fov_deg: number;
}

export const EYE_HEIGHT_MM = 1600;

export const EMPTY_BOUNDS: ModelBounds = {
  minX: -4000,
  minY: -4000,
  maxX: 4000,
  maxY: 4000,
  minZ: 0,
  maxZ: 3000,
};

export function boundsCenter(b: ModelBounds): Vec3 {
  return { x: (b.minX + b.maxX) / 2, y: (b.minY + b.maxY) / 2, z: (b.minZ + b.maxZ) / 2 };
}

export function boundsRadius(b: ModelBounds): number {
  return Math.max(Math.hypot(b.maxX - b.minX, b.maxY - b.minY, b.maxZ - b.minZ) / 2, 1000);
}

/** Distance at which a sphere of `radius` fits the view in both directions. */
export function fitDistance(radius: number, fovDeg: number, aspect: number): number {
  const v = (fovDeg * Math.PI) / 180 / 2;
  const h = Math.atan(Math.tan(v) * Math.max(aspect, 0.2));
  return radius / Math.sin(Math.min(v, h));
}

function along(center: Vec3, dir: Vec3, dist: number): Vec3 {
  const len = Math.hypot(dir.x, dir.y, dir.z) || 1;
  return {
    x: center.x + (dir.x / len) * dist,
    y: center.y + (dir.y / len) * dist,
    z: center.z + (dir.z / len) * dist,
  };
}

/** Three quarter view from the south-west, above the eaves. */
export function exteriorCorner(b: ModelBounds, aspect: number): PosePreset {
  const fov = 45;
  const target = boundsCenter(b);
  const dist = fitDistance(boundsRadius(b), fov, aspect) * 1.02;
  const position = along(target, { x: -0.62, y: -0.78, z: 0.42 }, dist);
  return { preset: "exterior_corner", name: "Exterior corner", position, target, fov_deg: fov };
}

/** A person standing in front of the house (south side), eyes at 1600 mm. */
export function eyeLevel(b: ModelBounds, aspect: number): PosePreset {
  const fov = 55;
  const c = boundsCenter(b);
  const radius = boundsRadius(b);
  const dist = fitDistance(radius, fov, aspect) * 1.1;
  const eye = b.minZ + EYE_HEIGHT_MM;
  const flat = along({ x: c.x, y: c.y, z: 0 }, { x: -0.32, y: -1, z: 0 }, dist);
  return {
    preset: "eye_level",
    name: "Eye level",
    position: { x: flat.x, y: flat.y, z: eye },
    target: { x: c.x, y: c.y, z: Math.max(eye, b.minZ + (b.maxZ - b.minZ) * 0.42) },
    fov_deg: fov,
  };
}

/** Straight down, north up. The tiny y offset keeps north at the top. */
export function topView(b: ModelBounds, aspect: number): PosePreset {
  const fov = 35;
  const c = boundsCenter(b);
  const flatRadius = Math.max(Math.hypot(b.maxX - b.minX, b.maxY - b.minY) / 2, 1000);
  const dist = fitDistance(flatRadius, fov, aspect);
  return {
    preset: "top",
    name: "Top",
    position: { x: c.x, y: c.y - dist * 0.0005, z: b.maxZ + dist },
    target: { x: c.x, y: c.y, z: b.minZ },
    fov_deg: fov,
  };
}

/** Isometric direction with a long lens, so it reads as a parallel projection. */
export function axonometric(b: ModelBounds, aspect: number): PosePreset {
  const fov = 12;
  const target = boundsCenter(b);
  const dist = fitDistance(boundsRadius(b), fov, aspect) * 1.02;
  const position = along(target, { x: -1, y: -1, z: Math.SQRT2 * Math.tan((35.264 * Math.PI) / 180) }, dist);
  return { preset: "axonometric", name: "Axonometric", position, target, fov_deg: fov };
}

/** Whole model from the current view direction. */
export function fitFromDirection(b: ModelBounds, aspect: number, dir: Vec3, fovDeg: number): PosePreset {
  const target = boundsCenter(b);
  const dist = fitDistance(boundsRadius(b), fovDeg, aspect) * 1.02;
  return { preset: "custom", name: "Fit", position: along(target, dir, dist), target, fov_deg: fovDeg };
}

/**
 * Stand inside a room near one end and look along its long axis.
 * `anchor` must be inside the polygon (RoomGeometry.label_point).
 */
export function roomInterior(polygon: Pt[], anchor: Pt, floorZ: number, name: string): PosePreset {
  const axis = longAxis(polygon);
  const reach = axis.length * 0.38;
  let inside = anchor;
  // Walk back from the anchor along the axis and keep the last point that is
  // still inside the room and not hugging the wall.
  for (let k = 1; k <= 8; k++) {
    const d = (reach * k) / 8;
    const p = { x: anchor.x - axis.dir.x * d, y: anchor.y - axis.dir.y * d };
    const margin = { x: p.x - axis.dir.x * 300, y: p.y - axis.dir.y * 300 };
    if (pointInPolygon(p, polygon) && pointInPolygon(margin, polygon)) inside = p;
    else break;
  }
  const eye = floorZ + EYE_HEIGHT_MM;
  const look = Math.max(axis.length * 0.5, 1500);
  return {
    preset: "room_interior",
    name,
    position: { x: inside.x, y: inside.y, z: eye },
    target: { x: inside.x + axis.dir.x * look, y: inside.y + axis.dir.y * look, z: eye - 250 },
    fov_deg: 72,
  };
}

/** Smallest bounds that contain the given elements, padded. Null when empty. */
export function mergeBounds(list: ModelBounds[]): ModelBounds | null {
  if (list.length === 0) return null;
  const out = { ...list[0] };
  for (const b of list) {
    out.minX = Math.min(out.minX, b.minX);
    out.minY = Math.min(out.minY, b.minY);
    out.minZ = Math.min(out.minZ, b.minZ);
    out.maxX = Math.max(out.maxX, b.maxX);
    out.maxY = Math.max(out.maxY, b.maxY);
    out.maxZ = Math.max(out.maxZ, b.maxZ);
  }
  return out;
}

/**
 * Unit vector from the model toward the sun, world mm axes (x east, y north,
 * z up). Late morning tropical sun: from the south-east, high. `northAngleDeg`
 * rotates true north from +y, counter-clockwise.
 */
export function sunDirection(northAngleDeg: number, azimuthDeg = 140, altitudeDeg = 52): Vec3 {
  // Compass azimuth is clockwise from true north.
  const a = ((90 + northAngleDeg - azimuthDeg) * Math.PI) / 180;
  const alt = (altitudeDeg * Math.PI) / 180;
  return { x: Math.cos(a) * Math.cos(alt), y: Math.sin(a) * Math.cos(alt), z: Math.sin(alt) };
}
