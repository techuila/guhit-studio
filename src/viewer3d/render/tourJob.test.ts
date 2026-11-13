// A tour video's frames draw from the job's own camera (GR3 P1-1): a preset,
// a saved view or an agent's look while the video exports moves the view,
// never the video, and the export never moves the view.
import * as THREE from "three";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Vec3 } from "../../contract/bindings";
import { Animator } from "../engine/animator";
import type { ViewerEngine } from "../engine/ViewerEngine";
import { poseCamera } from "../engine/viewCamera";
import { worldToVec3 } from "../geom/coords";
import type { DoorAt, TourFrame, TourFrames } from "../tour/frames";
import { TourJob } from "./tourJob";

/** What the job needs of the engine, with the engine's capture order. */
class FakeEngine {
  camera = new THREE.PerspectiveCamera(45, 16 / 9, 0.1, 3000);
  controls = { target: new THREE.Vector3() };
  anim = new Animator(() => true);
  scene = new THREE.Scene();
  renderer = { getContext: () => ({}) };
  /** A fly the view was sent on (a preset clicked, an agent's look) while the video exports. */
  fly: { position: THREE.Vector3; target: THREE.Vector3 } | null = null;
  /** Where each captured picture was drawn from, plan mm. */
  drawnFrom: Vec3[] = [];

  viewCamera(pose: TourFrame, into?: THREE.PerspectiveCamera): THREE.PerspectiveCamera {
    return poseCamera(pose, this.camera, into);
  }

  captureCanvas<T>(_w: number, _h: number, use: (c: HTMLCanvasElement) => T, view?: THREE.PerspectiveCamera): T {
    // Before every capture the engine lands what animates (`applyAnimated`),
    // a fly of the view's camera included.
    if (this.fly) {
      this.camera.position.copy(this.fly.position);
      this.controls.target.copy(this.fly.target);
      this.camera.lookAt(this.fly.target);
    }
    const cam = view ?? this.camera;
    cam.updateMatrixWorld();
    this.drawnFrom.push(worldToVec3(cam.position.x, cam.position.y, cam.position.z));
    return use({} as HTMLCanvasElement);
  }

  lightRig() {
    return { withLight: <T>(_light: unknown, fn: () => T) => fn() };
  }

  /** The doors each frame asked for, by frame. */
  doorsAsked: Array<ReadonlyMap<string, DoorAt> | null> = [];
  setTourDoors(doors: ReadonlyMap<string, DoorAt> | null): void {
    this.doorsAsked.push(doors);
  }
}

const frame = (i: number): TourFrame => ({
  t: i / 30,
  shot: 0,
  position: { x: 1000 * i, y: -6000, z: 1600 },
  target: { x: 1000 * i, y: 3000, z: 1400 },
  fov_deg: 50,
});

const FRAMES: TourFrames = { tour_id: "t1", name: "Tour", fps: 30, seconds: 6 / 30, frame_count: 6, shots: [], frames: [0, 1, 2, 3, 4, 5].map(frame) };

beforeEach(() => {
  // The job composes on a 2D canvas: a stand-in that draws nothing.
  const ctx = { drawImage: () => {}, fillRect: () => {}, globalAlpha: 1, fillStyle: "" };
  vi.stubGlobal("document", { createElement: () => ({ width: 0, height: 0, getContext: () => ctx }), visibilityState: "visible" });
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe("tour video frames (GR3 P1-1)", () => {
  it("draws every frame from its own pose when the view flies elsewhere mid export", async () => {
    const engine = new FakeEngine();
    const job = new TourJob(engine as unknown as ViewerEngine, FRAMES, 64, 36, 1);
    await job.frame(0);
    await job.frame(1);
    // The Top preset, clicked 2 frames in: the view flies to look down on the roof.
    engine.fly = { position: new THREE.Vector3(4, 30, -1), target: new THREE.Vector3(4, 0, -1) };
    for (let i = 2; i < FRAMES.frame_count; i++) await job.frame(i);
    expect(engine.drawnFrom).toHaveLength(FRAMES.frame_count);
    engine.drawnFrom.forEach((p, i) => {
      expect(p.x).toBeCloseTo(FRAMES.frames[i].position.x, 3);
      expect(p.y).toBeCloseTo(FRAMES.frames[i].position.y, 3);
      expect(p.z).toBeCloseTo(FRAMES.frames[i].position.z, 3);
    });
    job.dispose();
  });

  it("opens the doors each frame's time asks for, and puts them back after (GA P1-9)", async () => {
    const engine = new FakeEngine();
    // Door d1 swings open over frames 2 to 4 (1/15 s to 2/15 s).
    const data: TourFrames = { ...FRAMES, doors: [{ id: "d1", from: 1 / 30, to: 3 / 30, open: 1 }] };
    const job = new TourJob(engine as unknown as ViewerEngine, data, 64, 36, 1);
    for (let i = 0; i < data.frame_count; i++) await job.frame(i);
    const at = engine.doorsAsked.map((d) => d?.get("d1") ?? null);
    expect(at[0]).toBeNull();
    expect(at[2]).toMatchObject({ from: null, to: 1 });
    expect(at[2]!.k).toBeCloseTo(0.5, 6);
    expect(at[5]).toEqual({ from: 1, to: 1, k: 1 });
    job.dispose();
    expect(engine.doorsAsked[engine.doorsAsked.length - 1]).toBeNull();
  });

  it("never moves the view's camera", async () => {
    const engine = new FakeEngine();
    engine.camera.position.set(12, 8, 12);
    const job = new TourJob(engine as unknown as ViewerEngine, FRAMES, 64, 36, 1);
    for (let i = 0; i < FRAMES.frame_count; i++) await job.frame(i);
    expect(engine.camera.position.toArray()).toEqual([12, 8, 12]);
    // Each frame looks where its own target is.
    const dir = job.camera.getWorldDirection(new THREE.Vector3());
    expect(dir.z).toBeLessThan(-0.99);
    job.dispose();
  });
});
