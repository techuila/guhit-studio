// The scene a render job draws: built from the document with the same
// builders and light modules as the live view, in the job's own renderer, so
// the live view is never touched and stays usable while a render runs.
//
// Two flavours: `physical` for the path tracer (sky light at its physical
// value, the sky as a cube map the tracer can sample, fixtures that never
// block their own lamp, glows that do not light the room twice), and raster
// for the Enhanced capture fallback (the live view's look, refined).

import * as THREE from "three";
import { HDRLoader } from "three/examples/jsm/loaders/HDRLoader.js";
import type { Camera, DocState } from "../../contract/bindings";
import { Environment, headingOf, scanHdri } from "../engine/environment";
import { cameraToPose } from "../geom/coords";
import { LampRig } from "../light/lamps";
import { exposureScale, lightFrame, type LightFrame } from "../light/model";
import { MANILA } from "../light/sun";
import { buildScene, type BuiltScene } from "../scene/buildScene";
import { MaterialLibrary } from "../scene/materials";
import { PACK_BASE } from "../scene/pack";
import type { LiveLight } from "../viewerStore";
import { cameraRooms, lightCopies, OUTSIDE, planOf, roomLinks, type LightSpot } from "./lightSelect";

export interface RenderSceneInput {
  doc: DocState;
  camera: Camera;
  light: LiveLight;
  /** EV offset from the auto value, locked for the whole render. */
  lockedEv: number;
  width: number;
  height: number;
  view: { cutaway: boolean; roofVisible: boolean; activeLevelId: string | null };
  packModels: boolean;
  /** Walk mode's switches, view only: a render shows what the view shows. */
  lampOverrides: ReadonlyMap<string, boolean>;
  /** Keep every lit lamp in the path tracer, not only the ones that can light the view (render/lightSelect.ts). */
  keepAllLamps?: boolean;
}

export interface RenderScene {
  scene: THREE.Scene;
  camera: THREE.PerspectiveCamera;
  env: Environment;
  frame: LightFrame;
  /** Pre-exposure scale the lights are at. */
  scale: number;
  built: BuiltScene;
  lamps: LampRig;
  /** Lit lamps the path tracer got, of how many, and the lights they made with copies (render/lightSelect.ts). */
  lampCount: { kept: number; lit: number; lights: number };
  dispose: () => void;
}

/** Diffuser glow in the path tracer: bright enough to read white, too small to light the room twice. */
const TRACER_GLOW = 0.05;

let hdrPromise: Promise<THREE.DataTexture | null> | null = null;

/** The pack's HDRI as an equirect texture, for the tracer's photo sky. Loaded once, shared. */
function loadHdr(): Promise<THREE.DataTexture | null> {
  hdrPromise ??= new HDRLoader()
    .loadAsync(`/${PACK_BASE}hdri/sky.hdr`)
    .then((t) => {
      t.mapping = THREE.EquirectangularReflectionMapping;
      return t;
    })
    .catch(() => null);
  return hdrPromise;
}

export async function buildRenderScene(renderer: THREE.WebGLRenderer, input: RenderSceneInput, physical: boolean): Promise<RenderScene> {
  const { doc, width, height } = input;
  const scene = new THREE.Scene();
  const lib = new MaterialLibrary();
  lib.setAnisotropy(renderer.capabilities.getMaxAnisotropy());
  const built = buildScene(doc, lib, {
    cutaway: input.view.cutaway,
    activeLevelId: input.view.activeLevelId,
    packModels: input.packModels,
  });
  built.roofGroup.visible = input.view.roofVisible && !input.view.cutaway;
  scene.add(built.root);

  const settings = doc.project.settings;
  const site = settings?.site ?? MANILA;
  const north = Number.isFinite(settings?.north_angle_deg) ? settings.north_angle_deg : 0;
  const frame = lightFrame(input.light, site, north);
  const scale = exposureScale(frame.baseEv - input.lockedEv);

  const env = new Environment(scene);
  env.setPhysical(physical);
  env.setLight(frame);
  env.fit(built.bounds, built.groundY, north, built.contact);
  env.setExposure(scale);
  env.prepare(renderer);

  const pose = cameraToPose(input.camera);
  const camera = new THREE.PerspectiveCamera(Math.min(Math.max(pose.fovDeg || 45, 5), 110), width / height, 0.05, 3000);
  camera.position.set(...pose.position);
  camera.lookAt(new THREE.Vector3(...pose.target));
  camera.updateMatrixWorld();
  camera.updateProjectionMatrix();

  // Lamps: the same rig as the live view, lit the same way.
  const lamps = new LampRig();
  const glows = new Map<string, THREE.MeshStandardMaterial>();
  built.root.traverse((o) => {
    const mesh = o as THREE.Mesh;
    const mat = mesh.isMesh ? (mesh.material as THREE.MeshStandardMaterial) : null;
    const id = mat?.userData?.lampGlow as string | undefined;
    if (mat && id) glows.set(id, mat);
  });
  lamps.build(built.lamps, built.lightRooms, glows);
  const lampState = {
    lampsLit: frame.lampsLit,
    ghostLit: frame.ghostLit,
    scale,
    overrides: input.lampOverrides,
    glowGain: physical ? TRACER_GLOW : 1,
  };
  lamps.apply(lampState);
  const lampCount = { kept: 0, lit: 0, lights: 0 };
  if (physical) {
    // The tracer collects every light it meets: only the lit ones go in, and
    // of those only the ones that can light the view, the camera's room
    // weighed up (render/lightSelect.ts).
    const lit = lamps.cloneLights();
    const copies = input.keepAllLamps ? null : lampsForView(lit, built, doc, input, camera);
    lampCount.lit = lit.length;
    for (const l of lit) {
      const n = copies?.get(l.uuid) ?? 1;
      if (n <= 0) {
        l.dispose();
        continue;
      }
      lampCount.kept++;
      l.intensity /= n;
      for (let i = 0; i < n; i++) {
        const c = i === 0 ? l : copyLight(l);
        lampCount.lights++;
        scene.add(c);
        if (c instanceof THREE.SpotLight) scene.add(c.target);
      }
    }
    // A set sun gives no light but would still take its share of the
    // tracer's light samples.
    if (!(env.sun.intensity > 0)) env.sun.visible = false;
  } else {
    // Offscreen: a compile here stalls nothing the user is looking at.
    lamps.setVisible(lamps.wantsVisible(lampState));
    scene.add(lamps.group);
  }

  const owned: { dispose(): void }[] = [];
  if (physical) {
    // The pack's models are quantized (meshopt: normalized 8 and 16 bit,
    // interleaved). The tracer's geometry merge copies such attributes
    // without scaling them back and would draw every pack model collapsed
    // near the origin, so the tracer gets plain float copies. The live
    // view's shared geometry is left as it is.
    const floats = new Map<THREE.BufferGeometry, THREE.BufferGeometry>();
    built.root.traverse((o) => {
      const mesh = o as THREE.Mesh;
      if (!mesh.isMesh || !needsFloat(mesh.geometry)) return;
      let copy = floats.get(mesh.geometry);
      if (!copy) {
        copy = floatGeometry(mesh.geometry);
        floats.set(mesh.geometry, copy);
        owned.push(copy);
      }
      mesh.geometry = copy;
    });
    // A fixture never blocks its own lamp: the tracer reads `castShadow` off
    // the material, so lit fixtures get their own copies.
    const lit = new Set(built.lamps.map((l) => l.id));
    const copies = new Map<THREE.Material, THREE.Material>();
    for (const id of lit) {
      for (const mesh of built.byElement.get(id) ?? []) {
        const mat = mesh.material;
        if (Array.isArray(mat)) continue;
        let copy = copies.get(mat);
        if (!copy) {
          copy = mat.clone();
          (copy as THREE.Material & { castShadow?: boolean }).castShadow = false;
          copies.set(mat, copy);
          owned.push(copy);
        }
        mesh.material = copy;
      }
    }
    // The tracer samples a cube or an equirect, not a PMREM.
    if (input.light.sky === "photo" && frame.sun.altitudeDeg > -1) {
      const hdr = await loadHdr();
      if (hdr) {
        scene.environment = hdr;
        scene.background = hdr;
        // Turned so the photo's sun sits at the computed azimuth, as live.
        const sun = scanHdri(hdr.image as { data: ArrayLike<number>; width: number; height: number }).sun;
        const turn = sun ? headingOf(env.sunDir) - headingOf(sun) : 0;
        scene.environmentRotation.set(0, turn, 0);
        scene.backgroundRotation.set(0, turn, 0);
      } else {
        scene.environment = env.sky.background;
        scene.background = env.sky.background;
      }
    } else {
      scene.environment = env.sky.background;
      scene.background = env.sky.background;
    }
  }

  // The raster fallback shades the lamps that matter from this camera.
  if (!physical && lamps.visible) lamps.assignShadows(camera, true);

  scene.updateMatrixWorld(true);

  return {
    scene,
    camera,
    env,
    frame,
    scale,
    built,
    lamps,
    lampCount,
    dispose: () => {
      for (const d of owned) d.dispose();
      lamps.dispose();
      env.dispose();
      built.kit.dispose();
      lib.dispose();
    },
  };
}

/** A copy of a light with a target of its own (a spot's clone would share its target). */
function copyLight(l: THREE.Light): THREE.Light {
  const c = l.clone();
  if (c instanceof THREE.SpotLight && l instanceof THREE.SpotLight) {
    c.target = new THREE.Object3D();
    c.target.position.copy(l.target.position);
    (c as THREE.SpotLight & { radius?: number }).radius = (l as THREE.SpotLight & { radius?: number }).radius ?? 0;
  }
  return c;
}

/** True when an object and every parent are visible (a raycast does not skip hidden ones: the roof in a cutaway). */
function shown(o: THREE.Object3D | null): boolean {
  for (let p = o; p; p = p.parent) if (!p.visible) return false;
  return true;
}

/** A lamp this close behind the first thing a ray from the camera meets still counts as seen: its own fixture, meters. */
const SEEN_WITHIN_M = 0.6;

/**
 * Copies of each lit lamp for the tracer, by uuid: 0 for lamps that cannot
 * light what the camera sees (render/lightSelect.ts). The rig names its
 * lights `lamp:<fixture ids>` and `ghost:<room id>`; a light it does not name
 * that way is kept. A lamp is seen when a ray from the camera reaches it
 * past everything but glass.
 */
function lampsForView(lights: THREE.Light[], built: BuiltScene, doc: DocState, input: RenderSceneInput, camera: THREE.PerspectiveCamera): Map<string, number> {
  const plan = planOf(doc);
  const specs = new Map(built.lamps.map((s) => [s.id, s]));
  const ghostLevel = new Map(built.lightRooms.map((r) => [r.roomId, r.levelId]));
  const spots: LightSpot[] = lights.map((l) => {
    const [kind, rest = ""] = l.name.split(":");
    const position: [number, number, number] = [l.position.x, l.position.y, l.position.z];
    if (kind === "lamp") {
      const found = rest.split(",").map((id) => specs.get(id));
      if (found.some((f) => !f)) return { key: l.uuid, rooms: [], levelId: null, position };
      return { key: l.uuid, rooms: found.map((f) => f!.roomId ?? OUTSIDE), levelId: found[0]?.levelId ?? null, position };
    }
    if (kind === "ghost" && rest) return { key: l.uuid, rooms: [rest], levelId: ghostLevel.get(rest) ?? null, position };
    return { key: l.uuid, rooms: [], levelId: null, position };
  });
  const at = cameraRooms(plan, { x: input.camera.position.x, y: input.camera.position.y }, input.camera.position.z);
  const frustum = new THREE.Frustum().setFromProjectionMatrix(new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
  const eye = camera.position.clone();
  const ray = new THREE.Raycaster();
  built.root.updateMatrixWorld(true);
  const seen = (p: [number, number, number]): boolean => {
    const target = new THREE.Vector3(p[0], p[1], p[2]);
    if (!frustum.containsPoint(target)) return false;
    const dist = eye.distanceTo(target);
    ray.set(eye, target.clone().sub(eye).normalize());
    ray.far = dist;
    for (const hit of ray.intersectObject(built.root, true)) {
      const mat = (hit.object as THREE.Mesh).material as THREE.Material | THREE.Material[];
      const glass = !Array.isArray(mat) && mat.transparent && mat.opacity < 1;
      if (glass || !shown(hit.object)) continue;
      return hit.distance >= dist - SEEN_WITHIN_M;
    }
    return true;
  };
  return lightCopies(spots, at, roomLinks(plan.rooms, plan.openings), seen);
}

type AnyAttribute = THREE.BufferAttribute | THREE.InterleavedBufferAttribute;

/** True when a geometry has an attribute that is not plain 32 bit float: quantized, normalized or interleaved. */
export function needsFloat(g: THREE.BufferGeometry): boolean {
  return Object.values(g.attributes).some((a) => {
    const attr = a as AnyAttribute;
    return attr instanceof THREE.InterleavedBufferAttribute || attr.normalized || !(attr.array instanceof Float32Array);
  });
}

/** A copy of a geometry with every attribute as plain 32 bit floats, values scaled back from their quantized form. */
export function floatGeometry(src: THREE.BufferGeometry): THREE.BufferGeometry {
  const out = new THREE.BufferGeometry();
  for (const [name, a] of Object.entries(src.attributes)) {
    const attr = a as AnyAttribute;
    const size = attr.itemSize;
    const data = new Float32Array(attr.count * size);
    for (let i = 0; i < attr.count; i++) {
      const o = i * size;
      data[o] = attr.getX(i);
      if (size > 1) data[o + 1] = attr.getY(i);
      if (size > 2) data[o + 2] = attr.getZ(i);
      if (size > 3) data[o + 3] = attr.getW(i);
    }
    out.setAttribute(name, new THREE.BufferAttribute(data, size));
  }
  if (src.index) out.setIndex(src.index.clone());
  for (const g of src.groups) out.addGroup(g.start, g.count, g.materialIndex);
  out.name = src.name;
  return out;
}
