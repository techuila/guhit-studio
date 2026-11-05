// A tour's camera path drawn in the 3D view (lane L9): the path the frames
// take, a dot where each shot starts, and a marker at the playhead. Built
// with the rest of the model (buildScene.ts calls `buildTourPreview`), but
// it is one lasting group that this module fills from the frames, so showing
// another tour or moving the playhead never rebuilds the scene. Left out of
// exports (no build cache there), hidden while a tour plays or a video
// renders.

import * as THREE from "three";
import type { SceneHost } from "../scene/buildScene";
import { vec3ToWorld } from "../geom/coords";
import { useLiveView } from "../render/liveView";
import type { TourFrames } from "./frames";
import { poseAt } from "./frames";

/** The accent teal of tokens.css, for a line in the model. */
const PATH_COLOR = 0x0e8a8f;
const DOT_COLOR = 0x14283f;

const group = new THREE.Group();
group.name = "tour-path";
let line: THREE.Line | null = null;
let dots: THREE.Points | null = null;
// The playhead: a dot where the camera is and a short line where it looks.
// Points and lines only, never meshes, so nothing picks, shades or shadows them.
const markerGeo = new THREE.BufferGeometry();
markerGeo.setAttribute("position", new THREE.Float32BufferAttribute([0, 0, 0, 0, 0, 0], 3));
const dotGeo = new THREE.BufferGeometry();
dotGeo.setAttribute("position", new THREE.Float32BufferAttribute([0, 0, 0], 3));
const marker = new THREE.Group();
const markerDot = new THREE.Points(dotGeo, new THREE.PointsMaterial({ color: PATH_COLOR, size: 12, sizeAttenuation: false, depthTest: false, transparent: true }));
const markerLook = new THREE.Line(markerGeo, new THREE.LineBasicMaterial({ color: PATH_COLOR, depthTest: false, transparent: true }));
marker.add(markerDot, markerLook);
marker.renderOrder = markerDot.renderOrder = markerLook.renderOrder = 11;
marker.visible = false;
group.add(marker);
let shown: TourFrames | null = null;
let hiddenFor = 0;

function invalidate(): void {
  useLiveView.getState().engine?.invalidate();
}

function clearPath(): void {
  for (const o of [line, dots]) {
    if (!o) continue;
    group.remove(o);
    o.geometry.dispose();
    (o.material as THREE.Material).dispose();
  }
  line = dots = null;
}

/** Shows the path of these frames, or none. */
export function showTourPath(frames: TourFrames | null): void {
  if (frames === shown) return;
  shown = frames;
  clearPath();
  marker.visible = false;
  if (frames && frames.frames.length > 1) {
    const pts: number[] = [];
    for (const f of frames.frames) pts.push(...vec3ToWorld(f.position));
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
    line = new THREE.Line(geo, new THREE.LineBasicMaterial({ color: PATH_COLOR, transparent: true, opacity: 0.9, depthTest: false }));
    line.renderOrder = 9;
    const starts: number[] = [];
    for (const s of frames.shots) {
      const f = s.first_frame !== null ? frames.frames[s.first_frame] : null;
      if (f) starts.push(...vec3ToWorld(f.position));
    }
    const dg = new THREE.BufferGeometry();
    dg.setAttribute("position", new THREE.Float32BufferAttribute(starts, 3));
    dots = new THREE.Points(dg, new THREE.PointsMaterial({ color: DOT_COLOR, size: 7, sizeAttenuation: false, depthTest: false, transparent: true }));
    dots.renderOrder = 10;
    group.add(line, dots);
  }
  invalidate();
}

/** Puts the playhead marker at time t, seconds, looking along the camera. Null hides it. */
export function markTourPath(t: number | null): void {
  if (!shown || t === null || shown.frames.length === 0) {
    if (marker.visible) {
      marker.visible = false;
      invalidate();
    }
    return;
  }
  const p = poseAt(shown, t);
  const at = new THREE.Vector3(...vec3ToWorld(p.position));
  const look = new THREE.Vector3(...vec3ToWorld(p.target)).sub(at);
  if (look.lengthSq() > 0) look.setLength(0.8);
  look.add(at);
  const pos = markerGeo.getAttribute("position") as THREE.BufferAttribute;
  pos.setXYZ(0, at.x, at.y, at.z);
  pos.setXYZ(1, look.x, look.y, look.z);
  pos.needsUpdate = true;
  markerGeo.computeBoundingSphere();
  const dot = dotGeo.getAttribute("position") as THREE.BufferAttribute;
  dot.setXYZ(0, at.x, at.y, at.z);
  dot.needsUpdate = true;
  dotGeo.computeBoundingSphere();
  marker.visible = true;
  invalidate();
}

/**
 * Hides the path while a tour plays or a video renders; each call returns
 * the matching show. Nested hides stack.
 */
export function hideTourPath(): () => void {
  hiddenFor++;
  group.visible = false;
  invalidate();
  let done = false;
  return () => {
    if (done) return;
    done = true;
    hiddenFor = Math.max(0, hiddenFor - 1);
    group.visible = hiddenFor === 0;
    invalidate();
  };
}

/** The path of the tours to show, or null for none. The engine's builds only: an export has no cache. */
export function buildTourPreview(host: SceneHost): THREE.Object3D | null {
  if (!host.opts.cache) return null;
  return group;
}
