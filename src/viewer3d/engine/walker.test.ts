import { describe, expect, it } from "vitest";
import type { DocState } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";
import { EYE_HEIGHT_MM } from "../geom/cameraMath";
import { buildCollisionWorld, WALKER_RADIUS_MM } from "../geom/collision";
import { MAX_DT_S, NO_INPUT, RUN_FACTOR, WALK_SPEED_MM_S, WalkState, type WalkInput } from "./walker";

const doc = fixture as unknown as DocState;
const LEVEL = doc.project.levels[0].id;
const FORWARD: WalkInput = { forward: 1, strafe: 0, up: 0, run: false };

function walkerAt(x: number, y: number, yaw: number): WalkState {
  const w = new WalkState();
  w.world = buildCollisionWorld(doc, LEVEL);
  w.place(x, y, yaw, 0);
  w.z = w.eyeHeight();
  return w;
}

function run(w: WalkState, input: WalkInput, seconds: number, fps: number): boolean {
  let moving = false;
  for (let i = 0; i < Math.round(seconds * fps); i++) moving = w.step(1 / fps, input);
  return moving;
}

describe("WalkState", () => {
  it("stands still: no key, no motion, no frames wanted", () => {
    const w = walkerAt(2500, 3000, Math.PI / 2);
    expect(w.step(1 / 60, NO_INPUT)).toBe(false);
    expect([w.x, w.y]).toEqual([2500, 3000]);
  });

  it("walks forward at walking speed, easing in", () => {
    const w = walkerAt(2500, 1000, Math.PI / 2);
    run(w, FORWARD, 1, 60);
    const moved = w.y - 1000;
    // Under a tenth of a second of ease-in is lost.
    expect(moved).toBeGreaterThan(WALK_SPEED_MM_S * 0.88);
    expect(moved).toBeLessThanOrEqual(WALK_SPEED_MM_S);
    expect(Math.abs(w.x - 2500)).toBeLessThan(1e-6);
  });

  it("moves the same distance at 30 and at 144 frames a second", () => {
    const a = walkerAt(2500, 1000, Math.PI / 2);
    const b = walkerAt(2500, 1000, Math.PI / 2);
    run(a, FORWARD, 1.5, 30);
    run(b, FORWARD, 1.5, 144);
    expect(Math.abs(a.y - b.y)).toBeLessThan(25);
  });

  it("clamps a long stall to one tenth of a second of movement", () => {
    const w = walkerAt(2500, 1000, Math.PI / 2);
    run(w, FORWARD, 1, 60);
    const y0 = w.y;
    w.step(5, FORWARD);
    expect(w.y - y0).toBeLessThanOrEqual(WALK_SPEED_MM_S * MAX_DT_S + 1);
  });

  it("runs with Shift", () => {
    const w = walkerAt(2500, 500, Math.PI / 2);
    run(w, { ...FORWARD, run: true }, 1, 60);
    expect(w.y - 500).toBeGreaterThan(WALK_SPEED_MM_S * RUN_FACTOR * 0.88);
  });

  it("comes to rest after the key is released, then asks for no more frames", () => {
    const w = walkerAt(2500, 1000, Math.PI / 2);
    run(w, FORWARD, 0.5, 60);
    let frames = 0;
    while (w.step(1 / 60, NO_INPUT) && frames < 120) frames++;
    expect(frames).toBeGreaterThan(0);
    expect(frames).toBeLessThan(40);
    expect(w.step(1 / 60, NO_INPUT)).toBe(false);
  });

  it("is stopped by a wall in walk mode, even running on slow frames", () => {
    // Facing south toward the window in the south wall.
    const w = walkerAt(3500, 1500, -Math.PI / 2);
    for (let i = 0; i < 40; i++) w.step(0.25, { ...FORWARD, run: true });
    expect(w.y).toBeGreaterThanOrEqual(75 + WALKER_RADIUS_MM - 0.5);
  });

  it("flies through walls and up", () => {
    const w = walkerAt(3500, 1500, -Math.PI / 2);
    w.mode = "fly";
    run(w, { forward: 1, strafe: 0, up: 1, run: false }, 2, 60);
    expect(w.y).toBeLessThan(0);
    expect(w.z).toBeGreaterThan(EYE_HEIGHT_MM + 2000);
  });

  it("settles back to eye height when it lands", () => {
    const w = walkerAt(2500, 3000, 0);
    w.z = 6000;
    w.mode = "walk";
    run(w, NO_INPUT, 1.5, 60);
    expect(w.z).toBe(w.eyeHeight());
  });

  it("looks around with the pointer and never flips over", () => {
    const w = walkerAt(2500, 3000, 0);
    w.look(100, 0);
    expect(w.yaw).toBeLessThan(0);
    w.look(0, -10000);
    expect(w.pitch).toBeLessThanOrEqual(1.35);
    w.look(0, 10000);
    expect(w.pitch).toBeGreaterThanOrEqual(-1.35);
  });
});
