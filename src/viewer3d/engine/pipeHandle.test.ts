import * as THREE from "three";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApplyResult, DocState, Pipe } from "../../contract/bindings";
import { emptyTyped, typedKey } from "../../editor2d/typed";
import { Animator } from "./animator";
import { PipeCutSweep } from "./pipeCut";
import {
  editable,
  PipeHandle,
  type HandleActions,
  type HandleHost,
  axisDelta,
  axisParam,
  axisWorld,
  handleAnchor,
  metersPerPixel,
  moveCommand,
  nearbyHeights,
  nextAxis,
  pickIndices,
  segmentDistancePx,
  snapMove,
  typedMove,
} from "./pipeHandle";

const run: Pipe = {
  id: "run",
  level_id: "L1",
  system: "cold_water",
  material: "ppr",
  diameter_mm: 20,
  name: "",
  points: [
    { x: 0, y: 0, z: 300 },
    { x: 4000, y: 0, z: 300 },
    { x: 4000, y: 0, z: 1100 },
  ],
};

/** A ray from `eye` through `at`. */
function rayTo(eye: THREE.Vector3, at: THREE.Vector3): THREE.Ray {
  return new THREE.Ray(eye.clone(), at.clone().sub(eye).normalize());
}

describe("ray to the handle's plane", () => {
  const origin = new THREE.Vector3(0, 1, 0);
  const up = axisWorld("z");

  it("reads the height where the ray meets the vertical plane facing the camera", () => {
    const eye = new THREE.Vector3(5, 1.5, 0);
    expect(axisParam(rayTo(eye, new THREE.Vector3(0, 2, 0)), origin, up, eye)).toBeCloseTo(1, 9);
    // Off to the side of the handle the height is the same: the plane, not the line, is hit.
    expect(axisParam(rayTo(eye, new THREE.Vector3(0, 2, 0.3)), origin, up, eye)).toBeCloseTo(1, 9);
    expect(axisParam(rayTo(eye, new THREE.Vector3(0, 0.25, -0.2)), origin, up, eye)).toBeCloseTo(-0.75, 9);
  });

  it("tracks 1:1 from wherever the camera looks, above or below", () => {
    for (const eye of [new THREE.Vector3(-3, 6, 4), new THREE.Vector3(2, -1, -7), new THREE.Vector3(0.5, 1, 9)]) {
      for (const h of [-1.2, 0, 0.35, 2.4]) {
        const p = origin.clone().addScaledVector(up, h);
        expect(axisParam(rayTo(eye, p), origin, up, eye)).toBeCloseTo(h, 6);
      }
    }
  });

  it("reads plan axes the same way", () => {
    const eye = new THREE.Vector3(0, 5, 5);
    expect(axisParam(rayTo(eye, new THREE.Vector3(0.7, 1, 0)), origin, axisWorld("x"), eye)).toBeCloseTo(0.7, 9);
    // Plan y is north: world -z.
    expect(axisParam(rayTo(eye, new THREE.Vector3(0, 1, -0.4)), origin, axisWorld("y"), eye)).toBeCloseTo(0.4, 9);
  });

  it("gives nothing when the eye looks along the axis or the ray runs flat to the plane", () => {
    const above = new THREE.Vector3(0, 10, 0);
    expect(axisParam(rayTo(above, new THREE.Vector3(0, 0, 0)), origin, up, above)).toBeNull();
    const eye = new THREE.Vector3(5, 1, 0);
    // Parallel to the plane x = 0.
    expect(axisParam(new THREE.Ray(eye, new THREE.Vector3(0, 0, 1)), origin, up, eye)).toBeNull();
    // Pointing away from it.
    expect(axisParam(new THREE.Ray(eye, new THREE.Vector3(1, 0, 0)), origin, up, eye)).toBeNull();
  });

  it("maps plan axes to world directions and back to plan deltas", () => {
    expect(axisWorld("x").toArray()).toEqual([1, 0, 0]);
    expect(axisWorld("y").toArray()).toEqual([0, 0, -1]);
    expect(axisWorld("z").toArray()).toEqual([0, 1, 0]);
    expect(axisDelta("z", 350)).toEqual({ x: 0, y: 0, z: 350 });
    expect(axisDelta("y", -20)).toEqual({ x: 0, y: -20, z: 0 });
  });
});

describe("snaps", () => {
  const base = { axis: "z" as const, fromZ: 300, levelHeight: 3000, heights: [] as number[], tolerance: 8 };

  it("moves in 10 mm steps", () => {
    expect(snapMove({ ...base, raw: 347 })).toEqual({ mm: 350, snap: "step" });
    expect(snapMove({ ...base, raw: -4.9 })).toEqual({ mm: 0, snap: "step" });
    expect(snapMove({ ...base, raw: -15.1 })).toEqual({ mm: -20, snap: "step" });
  });

  it("catches the floor, the level height and runs nearby, the nearest first", () => {
    expect(snapMove({ ...base, raw: -294 })).toEqual({ mm: -300, snap: "floor" });
    expect(snapMove({ ...base, fromZ: 2900, raw: 95 })).toEqual({ mm: 100, snap: "level" });
    expect(snapMove({ ...base, heights: [650], raw: 343 })).toEqual({ mm: 350, snap: "run" });
    expect(snapMove({ ...base, heights: [650, 655], raw: 353 })).toEqual({ mm: 355, snap: "run" });
    // Out of reach: steps again.
    expect(snapMove({ ...base, heights: [650], raw: 330 })).toEqual({ mm: 330, snap: "step" });
  });

  it("does not catch heights along the plan axes", () => {
    expect(snapMove({ ...base, axis: "x", heights: [650], raw: 347 })).toEqual({ mm: 350, snap: "step" });
  });
});

describe("arrow keys lock the axis", () => {
  it("Up or Down vertical, Right plan x, Left plan y, the same key again back to vertical", () => {
    expect(nextAxis("x", "ArrowUp")).toBe("z");
    expect(nextAxis("y", "ArrowDown")).toBe("z");
    expect(nextAxis("z", "ArrowRight")).toBe("x");
    expect(nextAxis("x", "ArrowRight")).toBe("z");
    expect(nextAxis("z", "ArrowLeft")).toBe("y");
    expect(nextAxis("y", "ArrowLeft")).toBe("z");
    expect(nextAxis("x", "ArrowLeft")).toBe("y");
    expect(nextAxis("z", "a")).toBeNull();
  });
});

describe("what the handle moves", () => {
  it("moves a picked point, a segment's two points or the whole run", () => {
    expect(pickIndices(run, null)).toEqual([]);
    expect(pickIndices(run, { pipeId: "run", kind: "point", index: 2 })).toEqual([2]);
    expect(pickIndices(run, { pipeId: "run", kind: "segment", index: 1 })).toEqual([1, 2]);
    // Another run's pick, or one past the end: the whole run.
    expect(pickIndices(run, { pipeId: "other", kind: "point", index: 0 })).toEqual([]);
    expect(pickIndices(run, { pipeId: "run", kind: "segment", index: 2 })).toEqual([]);
  });

  it("stands on the point, the segment's middle, or the middle of the run's longest segment", () => {
    expect(handleAnchor(run, [])).toEqual({ x: 2000, y: 0, z: 300 });
    expect(handleAnchor(run, [1, 2])).toEqual({ x: 4000, y: 0, z: 700 });
    expect(handleAnchor(run, [2])).toEqual({ x: 4000, y: 0, z: 1100 });
  });

  it("stands on what the cutaway left, never above the cut", () => {
    // The riser 300 to 1100 cut at 700: the middle of what is left.
    expect(handleAnchor(run, [1, 2], 700)).toEqual({ x: 4000, y: 0, z: 500 });
    // A point above the cut, or a run the cut took away: no handle.
    expect(handleAnchor(run, [2], 700)).toBeNull();
    const high: Pipe = { ...run, points: [{ x: 0, y: 0, z: 2450 }, { x: 3000, y: 0, z: 2450 }] };
    expect(handleAnchor(high, [], 1200)).toBeNull();
    // A vent rising through the cut: the part below it.
    const vent: Pipe = { ...run, points: [{ x: 0, y: 0, z: -400 }, { x: 0, y: 0, z: 3400 }] };
    expect(handleAnchor(vent, [], 1200)).toEqual({ x: 0, y: 0, z: 400 });
    // A run wholly under the cut is unchanged.
    expect(handleAnchor(run, [], 1200)).toEqual(handleAnchor(run, []));
  });

  it("sends one MovePipePoints, keeping joins unless Alt broke them", () => {
    expect(moveCommand("run", [1, 2], axisDelta("z", 350), true)).toEqual({
      type: "move_pipe_points",
      targets: [{ pipe_id: "run", indices: [1, 2] }],
      delta: { x: 0, y: 0, z: 350 },
      keep_joins: true,
    });
    expect(moveCommand("run", [], axisDelta("x", -10), false)).toMatchObject({ targets: [{ pipe_id: "run", indices: [] }], keep_joins: false });
  });

  it("collects the heights of runs nearby and of the run's own points that stay put", () => {
    const near: Pipe = { ...run, id: "near", points: [{ x: 2000, y: 1500, z: 650 }, { x: 2500, y: 1500, z: 650 }] };
    const far: Pipe = { ...run, id: "far", points: [{ x: 2000, y: 9000, z: 900 }, { x: 2500, y: 9000, z: 900 }] };
    const upstairs: Pipe = { ...run, id: "up", level_id: "L2", points: [{ x: 2000, y: 0, z: 1500 }, { x: 2100, y: 0, z: 1500 }] };
    const doc = { project: { elements: [run, near, far, upstairs].map((p) => ({ kind: "pipe", ...p })) } } as unknown as DocState;
    expect(nearbyHeights(doc, run, [], { x: 2000, y: 0, z: 300 }).sort()).toEqual([650]);
    // A point pick also lines up with the run's other points.
    expect(nearbyHeights(doc, run, [2], { x: 4000, y: 0, z: 1100 }).sort((a, b) => a - b)).toEqual([300, 650]);
  });
});

describe("typed distance", () => {
  const typed = (s: string) => [...s].reduce<ReturnType<typeof typedKey>>((t, k) => (t ? typedKey(t, k) : null), emptyTyped("height"));

  it("is how far; the pointer says which way, a minus turns it around", () => {
    expect(typedMove(typed("350"), "mm", 12)).toBe(350);
    expect(typedMove(typed("350"), "mm", -40)).toBe(-350);
    expect(typedMove(typed("-350"), "mm", 12)).toBe(-350);
    expect(typedMove(typed("0.35"), "m", 1)).toBeCloseTo(350, 9);
  });

  it("is nothing until a number is typed", () => {
    expect(typedMove(null, "mm", 10)).toBeNull();
    expect(typedMove(typed("-"), "mm", 10)).toBeNull();
  });
});

describe("screen size", () => {
  it("keeps the handle a constant size on screen", () => {
    const cam = new THREE.PerspectiveCamera(60, 1.5, 0.1, 100);
    cam.position.set(0, 0, 10);
    cam.lookAt(0, 0, 0);
    cam.updateMatrixWorld();
    const mpp = metersPerPixel(cam, new THREE.Vector3(0, 0, 0), 1000);
    expect(mpp).toBeCloseTo((2 * 10 * Math.tan(Math.PI / 6)) / 1000, 9);
    expect(metersPerPixel(cam, new THREE.Vector3(0, 0, -10), 1000)).toBeCloseTo(mpp * 2, 9);
  });

  it("measures the pointer's distance to the handle's line in pixels", () => {
    expect(segmentDistancePx(5, 0, 0, -10, 0, 10)).toBe(5);
    expect(segmentDistancePx(0, 13, 0, -10, 0, 10)).toBe(3);
    expect(segmentDistancePx(3, 4, 0, 0, 0, 0)).toBe(5);
  });
});

describe("the cutaway sweep and highlight copies", () => {
  it("puts the live plane on a tint copy made mid-sweep, and takes it off at the end", () => {
    const base = new THREE.MeshStandardMaterial();
    const mats: THREE.Material[] = [base];
    const sweep = new PipeCutSweep();
    sweep.start(1.2, 3.4, () => mats);
    sweep.apply(0.5);
    // A hover or selection tint clones the base: its plane is a frozen copy.
    const copy = base.clone();
    expect(copy.clippingPlanes?.[0]).not.toBe(base.clippingPlanes?.[0]);
    mats.push(copy);
    sweep.apply(0.75);
    expect(copy.clippingPlanes?.[0]).toBe(base.clippingPlanes?.[0]);
    expect(copy.clippingPlanes?.[0].constant).toBeCloseTo(1.2 + 2.2 * 0.75, 9);
    sweep.clear();
    expect(base.clippingPlanes).toBeNull();
    expect(copy.clippingPlanes).toBeNull();
  });
});

describe("the handle in a view", () => {
  /** Just enough of a DOM node for the handle: listeners, children, a rect. */
  class Node {
    className = "";
    dataset: Record<string, string> = {};
    style: Record<string, string> = {};
    textContent = "";
    tagName = "DIV";
    isContentEditable = false;
    kids: Node[] = [];
    private on = new Map<string, Set<(e: unknown) => void>>();
    append(...n: Node[]) {
      this.kids.push(...n);
    }
    remove() {}
    contains(n: unknown): boolean {
      return n === this || this.kids.some((k) => k.contains(n));
    }
    addEventListener(type: string, fn: (e: unknown) => void) {
      if (!this.on.has(type)) this.on.set(type, new Set());
      this.on.get(type)!.add(fn);
    }
    removeEventListener(type: string, fn: (e: unknown) => void) {
      this.on.get(type)?.delete(fn);
    }
    fire(type: string, e: object) {
      for (const fn of [...(this.on.get(type) ?? [])]) fn(e);
    }
    setPointerCapture() {}
    hasPointerCapture() {
      return false;
    }
    releasePointerCapture() {}
    getBoundingClientRect() {
      return { left: 0, top: 0, width: 800, height: 600 };
    }
  }

  afterEach(() => vi.unstubAllGlobals());

  function setup(withMesh = true) {
    const win = new Node();
    vi.stubGlobal("window", win);
    vi.stubGlobal("document", { createElement: () => new Node() });
    const camera = new THREE.PerspectiveCamera(45, 800 / 600, 0.1, 100);
    camera.position.set(2, 0.3, 6);
    camera.lookAt(2, 0.3, 0);
    camera.updateMatrixWorld();
    const container = new Node();
    const canvas = new Node();
    container.append(canvas);
    const level = { id: "L1", name: "", elevation_mm: 0, height_mm: 3000 };
    const straight: Pipe = { ...run, level_id: "L1", points: [{ x: 0, y: 0, z: 300 }, { x: 4000, y: 0, z: 300 }] };
    const doc = { revision: 0, project: { elements: [{ kind: "pipe", ...straight }], levels: [level], settings: { display_unit: "mm" } } } as unknown as DocState;
    const solo = new THREE.Group();
    if (withMesh) solo.add(new THREE.Mesh(new THREE.BufferGeometry(), new THREE.MeshBasicMaterial()));
    let grabs = 0;
    let orbit = true;
    const host: HandleHost = {
      scene: new THREE.Scene(),
      camera,
      anim: new Animator(() => false),
      container: container as unknown as HTMLElement,
      canvas: canvas as unknown as HTMLCanvasElement,
      setOrbitEnabled: (on) => {
        orbit = on;
      },
      invalidate: () => {},
      doc: () => doc,
      selection: () => [straight.id],
      orbiting: () => true,
      solo: () => solo,
      pipeCutY: () => null,
      grab: () => {
        grabs++;
      },
    };
    const result = { state: doc, diff: { added: [], modified: [straight.id], removed: [], summary: "" } } as unknown as ApplyResult;
    let shown: ApplyResult | null = null;
    let settle: (r: ApplyResult | null) => void = () => {};
    const actions: HandleActions = {
      preview: async () => result,
      showPreview: (r) => {
        shown = r;
      },
      currentPreview: () => shown,
      commit: () => new Promise((res) => (settle = res)),
      pick: () => null,
      onPick: () => () => {},
      unit: () => "mm",
    };
    const handle = new PipeHandle(host, actions);
    handle.sync();
    handle.beforeRender(800, 600);
    const pointer = (x: number, y: number, target: unknown = canvas) => ({
      clientX: x,
      clientY: y,
      button: 0,
      buttons: 1,
      pointerId: 1,
      altKey: false,
      target,
      stopPropagation() {},
      preventDefault() {},
    });
    const flush = () => new Promise((r) => setTimeout(r, 0));
    return { handle, container, win, pointer, flush, result, shown: () => shown, settle: (r: ApplyResult | null) => settle(r), grabs: () => grabs, orbit: () => orbit };
  }

  it("counts a press as a hand on the view: following stops, a camera glide stops", () => {
    const v = setup();
    // The handle stands at the middle of the run: the middle of the screen.
    v.container.fire("pointerdown", v.pointer(400, 300));
    expect(v.handle.dragging()).toBe(true);
    expect(v.grabs()).toBe(1);
    expect(v.orbit()).toBe(false);
  });

  it("clears its preview when the view closes while a released move is still being applied", async () => {
    const v = setup();
    v.container.fire("pointerdown", v.pointer(400, 300));
    v.container.fire("pointermove", v.pointer(400, 240));
    v.handle.beforeRender(800, 600);
    await v.flush();
    expect(v.shown()).toBe(v.result);
    v.container.fire("pointerup", v.pointer(400, 240));
    expect(v.handle.dragging()).toBe(false);
    // The commit is in flight; the 3D view closes.
    v.handle.dispose();
    expect(v.shown()).toBeNull();
    v.settle(v.result);
    await v.flush();
    expect(v.shown()).toBeNull();
  });

  it("lets a picked-up move go when a press lands outside the view, and never takes keys from a field", () => {
    const v = setup();
    v.container.fire("pointerdown", v.pointer(400, 300));
    v.container.fire("pointerup", v.pointer(400, 300));
    expect(v.handle.dragging()).toBe(true);
    let taken = false;
    const field = { tagName: "INPUT", isContentEditable: false };
    v.win.fire("keydown", { key: "2", target: field, metaKey: false, ctrlKey: false, preventDefault: () => (taken = true), stopPropagation() {} });
    expect(taken).toBe(false);
    expect(editable(field as unknown as EventTarget)).toBe(true);
    expect(editable({ tagName: "DIV", isContentEditable: true } as unknown as EventTarget)).toBe(true);
    expect(editable({ tagName: "CANVAS", isContentEditable: false } as unknown as EventTarget)).toBe(false);
    // A press inside the view does not cancel; one on the inspector does.
    v.win.fire("pointerdown", v.pointer(10, 10, new Node()));
    expect(v.handle.dragging()).toBe(false);
    expect(v.orbit()).toBe(true);
  });

  it("shows no handle for a run the cutaway took away entirely", () => {
    const v = setup(false);
    expect(v.handle.stats().shown).toBe(false);
    v.handle.dispose();
  });
});
