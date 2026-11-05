// An agent's look while a tour video exports (GR3 P1-1): the window's view
// stays where it is and the picture is taken off screen from the camera the
// agent asked for, so neither the video nor the agent's picture is wrong.
import * as THREE from "three";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import type { Camera, DocState } from "../contract/bindings";

const captured: { camera: unknown }[] = [];
vi.mock("../contract/ipc", async (orig) => ({
  ...(await orig<typeof import("../contract/ipc")>()),
  call: vi.fn(async () => null),
  ipc: {
    docState: vi.fn(async () => null),
    renderCapture: vi.fn(async (camera: unknown) => {
      captured.push({ camera });
      return { id: "rec-1", camera };
    }),
  },
}));
vi.mock("../viewer3d/render/conditioning", () => ({ captureInfo: () => undefined, queueMapsFor: async () => null }));

import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { poseCamera } from "../viewer3d/engine/viewCamera";
import type { ViewerEngine } from "../viewer3d/engine/ViewerEngine";
import { worldToVec3 } from "../viewer3d/geom/coords";
import { useLiveView } from "../viewer3d/render/liveView";
import { useTours } from "../viewer3d/tour/tourStore";
import { runWindowTask } from "./windowTasks";

/** The view's camera, and every picture's camera, plan mm. */
function fakeEngine() {
  const live = new THREE.PerspectiveCamera(45, 16 / 9, 0.1, 3000);
  live.position.set(10, 6, 10);
  const shots: Array<{ x: number; y: number; z: number }> = [];
  const engine = {
    camera: live,
    viewCamera: (pose: Camera, into?: THREE.PerspectiveCamera) => poseCamera(pose, live, into),
    captureWith: (_w: number, _h: number, _show: unknown, view?: THREE.PerspectiveCamera) => {
      const c = view ?? live;
      shots.push(worldToVec3(c.position.x, c.position.y, c.position.z));
      return "data:image/png;base64,AAAA";
    },
    lightRig: () => ({ withLight: <T,>(_l: unknown, fn: () => T) => fn(), viewLight: () => null }),
    currentCamera: (name: string) => ({ id: "", name, preset: "custom", position: worldToVec3(10, 6, 10), target: { x: 0, y: 0, z: 0 }, fov_deg: 45, light: null }),
  };
  return { engine, shots };
}

const LOOK: Camera = { id: "", name: "Tour at 1.0 s", preset: "custom", position: { x: 1969.2, y: 17465.5, z: 15082 }, target: { x: 4000, y: 3000, z: 2175 }, fov_deg: 42.5, light: null };

beforeEach(() => {
  captured.length = 0;
  // The preview is made through an Image: here it gives the picture back as it is.
  vi.stubGlobal(
    "Image",
    class {
      onerror: (() => void) | null = null;
      set src(_v: string) {
        queueMicrotask(() => this.onerror?.());
      }
    },
  );
});
afterEach(() => {
  vi.unstubAllGlobals();
  useTours.setState({ exporting: null });
});

describe("an agent's look while a tour video exports (GR3 P1-1)", () => {
  it("leaves the view where it is and takes the picture from the asked camera", async () => {
    const { engine, shots } = fakeEngine();
    useLiveView.getState().register(engine as unknown as ViewerEngine, () => "Custom view");
    useApp.setState({ screen: "editor", doc: structuredClone(fixture) as unknown as DocState, viewMode: "3d", captureView: async () => ({ png: "", camera: LOOK }) });
    useTours.setState({ exporting: { tourId: "t1", size: "hd", done: 85, total: 600, codec: "avc", phase: "Rendering", cancelled: false } });
    const moved = vi.fn();
    const off = bus.on("apply_camera", moved);
    try {
      const reply = await runWindowTask({ id: "job-1", task: { type: "capture_view", camera_id: null, visibility: null, camera: LOOK } });
      expect(moved).not.toHaveBeenCalled();
      expect(shots).toHaveLength(1);
      expect(shots[0].x).toBeCloseTo(LOOK.position.x, 1);
      expect(shots[0].y).toBeCloseTo(LOOK.position.y, 1);
      expect(shots[0].z).toBeCloseTo(LOOK.position.z, 1);
      // The record keeps the asked camera, so MCP finds it matches the view.
      expect(captured[0].camera).toMatchObject({ position: LOOK.position, target: LOOK.target, fov_deg: LOOK.fov_deg, name: LOOK.name });
      expect(reply.render_ids).toEqual(["rec-1"]);
      expect(reply.note).toMatch(/tour video is exporting/);
      // The view's own camera did not move.
      expect(engine.camera.position.toArray()).toEqual([10, 6, 10]);
    } finally {
      off();
      useLiveView.getState().unregister(engine as unknown as ViewerEngine);
    }
  });
});
