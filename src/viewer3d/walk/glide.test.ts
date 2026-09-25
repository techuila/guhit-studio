import { describe, expect, it } from "vitest";
import { DUR } from "../../ui/motion";
import { isClear, WALKER_RADIUS_MM } from "../geom/collision";
import { NO_INPUT, WalkState } from "../engine/walker";
import { FACE_STANDOFF_MM, glideDurationMs, hitTarget, levelTarget } from "./glide";
import { rampHeight } from "./stairs";
import { L1, L2, STAIR, twoLevelDoc } from "./testModel";
import { WalkWorlds } from "./worlds";

const R = WALKER_RADIUS_MM;
const EYE = 1600;
const worlds = new WalkWorlds(twoLevelDoc(), EYE);

function walker(levelId: string, x: number, y: number): WalkState {
  const w = new WalkState();
  w.setNav(worlds, levelId);
  w.place(x, y, 0, 0);
  w.z = w.eyeHeight();
  return w;
}

describe("glide targets", () => {
  it("lands a minimap click in the open where it was clicked, on the floor of the level", () => {
    const t = levelTarget(worlds, L1, { x: 3000, y: 3000 });
    expect([t.x, t.y]).toEqual([3000, 3000]);
    expect(t.levelId).toBe(L1);
    expect(t.stair).toBeNull();
    expect(t.floorZ).toBe(0);
    const up = levelTarget(worlds, L2, { x: 3000, y: 3000 });
    expect(up.floorZ).toBe(3000);
  });

  it("steps a click on a wall out to the nearest clear spot", () => {
    // The inner wall at x = 5000, away from its door.
    const t = levelTarget(worlds, L1, { x: 5000, y: 1500 });
    expect(isClear(t, R, worlds.levelWorld(L1))).toBe(true);
    expect(Math.abs(t.x - 5000)).toBeGreaterThanOrEqual(50 + R - 0.5);
    expect(t.y).toBeCloseTo(1500, 6);
  });

  it("lands a click on the flight on the stair, at its walking line", () => {
    const t = levelTarget(worlds, L1, { x: 600, y: 2000 });
    expect(t.stair?.id).toBe(STAIR);
    expect(t.floorZ).toBeCloseTo(rampHeight(worlds.stairs[0], 800), 6);
    expect(t.levelId).toBe(L1);
    const high = levelTarget(worlds, L2, { x: 600, y: 4800 });
    expect(high.stair?.id).toBe(STAIR);
    expect(high.levelId).toBe(L2);
  });

  it("takes a floor hit to the level whose floor is at that height", () => {
    const up = hitTarget(worlds, { x: 3000, y: 3000, z: 3012, up: 1 }, { x: 2000, y: 2000, levelId: L1 });
    expect(up?.levelId).toBe(L2);
    expect(up?.floorZ).toBe(3000);
    const down = hitTarget(worlds, { x: 3000, y: 3000, z: 12, up: 1 }, { x: 2000, y: 2000, levelId: L2 });
    expect(down?.levelId).toBe(L1);
    expect(down?.floorZ).toBe(0);
    // The ground outside is the lowest level.
    const ground = hitTarget(worlds, { x: 12000, y: 3000, z: -150, up: 1 }, { x: 2000, y: 2000, levelId: L1 });
    expect(ground?.levelId).toBe(L1);
  });

  it("takes a tread hit onto the flight", () => {
    const u = 7.5 * 243.75;
    const t = hitTarget(worlds, { x: 600, y: 1200 + u, z: 8 * 187.5, up: 1 }, { x: 3000, y: 3000, levelId: L1 });
    expect(t?.stair?.id).toBe(STAIR);
    expect(t?.floorZ).toBeCloseTo(8 * 187.5, 6);
  });

  it("stops in front of a wall that was hit, toward the walker", () => {
    const t = hitTarget(worlds, { x: 4950, y: 1500, z: 1200, up: 0 }, { x: 2000, y: 1500, levelId: L1 });
    expect(t).not.toBeNull();
    expect(t!.levelId).toBe(L1);
    expect(t!.x).toBeCloseTo(4950 - FACE_STANDOFF_MM, 6);
    expect(isClear(t!, R, worlds.levelWorld(L1))).toBe(true);
  });

  it("lasts --dur-scene for a hop, at most twice that across a house", () => {
    expect(glideDurationMs(0)).toBe(DUR.scene);
    expect(glideDurationMs(4000)).toBeCloseTo(DUR.scene * 1.5, 6);
    expect(glideDurationMs(100000)).toBe(DUR.scene * 2);
  });
});

describe("gliding", () => {
  it("goes through walls and lands at eye height on the target floor", () => {
    const w = walker(L1, 2500, 1500);
    const t = levelTarget(worlds, L1, { x: 6500, y: 1500 });
    w.startGlide(t, 360);
    let frames = 0;
    while (w.gliding() && frames < 200) {
      expect(w.step(1 / 60, NO_INPUT)).toBe(true);
      frames++;
    }
    expect(frames).toBeGreaterThan(15);
    expect(frames).toBeLessThan(30);
    expect([w.x, w.y]).toEqual([6500, 1500]);
    expect(w.z).toBe(EYE);
    // Landed: no more frames wanted.
    expect(w.step(1 / 60, NO_INPUT)).toBe(false);
  });

  it("keeps eye height gliding to another level and switches the walker there", () => {
    const w = walker(L1, 2500, 1500);
    w.startGlide(hitTarget(worlds, { x: 3000, y: 3000, z: 3012, up: 1 }, w)!, 360);
    expect(w.levelId).toBe(L2);
    while (w.gliding()) w.step(1 / 60, NO_INPUT);
    expect(w.z).toBe(3000 + EYE);
    w.eyeMm = 1100;
    for (let i = 0; i < 120; i++) w.step(1 / 60, NO_INPUT);
    expect(w.z).toBe(3000 + 1100);
  });

  it("hands over to the keys wherever it got to, out of any wall", () => {
    const w = walker(L1, 2500, 1500);
    w.startGlide(levelTarget(worlds, L1, { x: 7500, y: 1500 }), 1000);
    for (let i = 0; i < 20; i++) w.step(1 / 60, NO_INPUT);
    expect(w.gliding()).toBe(true);
    w.step(1 / 60, { forward: 0, strafe: 1, up: 0, run: false });
    expect(w.gliding()).toBe(false);
    expect(isClear(w, R, worlds.levelWorld(L1))).toBe(true);
  });

  it("jumps under a zero duration (reduced motion)", () => {
    const w = walker(L1, 2500, 1500);
    w.startGlide(levelTarget(worlds, L1, { x: 3000, y: 4000 }), 0);
    expect(w.gliding()).toBe(false);
    expect([w.x, w.y, w.z]).toEqual([3000, 4000, EYE]);
  });
});
