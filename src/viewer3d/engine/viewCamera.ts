// A camera of its own at a contract pose, for a capture that must not move
// the view: a tour video's frames, or a picture an agent asks for while one
// exports (`ViewerEngine.captureCanvas`, GR3 P1-1).

import * as THREE from "three";
import type { Camera } from "../../contract/bindings";
import { cameraToPose } from "../geom/coords";

/** `into` at `pose` (plan mm), with the lens range, layers, up and aspect of `like`, the view's camera. */
export function poseCamera(pose: Pick<Camera, "position" | "target" | "fov_deg">, like: THREE.PerspectiveCamera, into = new THREE.PerspectiveCamera()): THREE.PerspectiveCamera {
  const p = cameraToPose(pose);
  into.near = like.near;
  into.far = like.far;
  into.layers.mask = like.layers.mask;
  into.up.copy(like.up);
  into.aspect = like.aspect;
  into.fov = Math.min(Math.max(p.fovDeg || 45, 5), 110);
  into.position.set(...p.position);
  into.lookAt(new THREE.Vector3(...p.target));
  into.updateProjectionMatrix();
  into.updateMatrixWorld();
  return into;
}
