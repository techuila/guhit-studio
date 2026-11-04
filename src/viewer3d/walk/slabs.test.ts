// Walking with slabs (DECISIONS D50): rails and void edges stop the walker,
// balconies and decks carry it at their own height.
import { describe, expect, it } from "vitest";
import { isClear, WALKER_RADIUS_MM } from "../geom/collision";
import { WalkState, type WalkInput } from "../engine/walker";
import { BALCONY, L1, L2, slabDoc, VOID } from "./testModel";
import { WalkWorlds } from "./worlds";

const R = WALKER_RADIUS_MM;
const EYE = 1600;
const NORTH = Math.PI / 2;
const SOUTH = -Math.PI / 2;
const FORWARD: WalkInput = { forward: 1, strafe: 0, up: 0, run: false };

function walker(worlds: WalkWorlds, levelId: string, x: number, y: number, yaw: number): WalkState {
  const w = new WalkState();
  w.setNav(worlds, levelId);
  w.place(x, y, yaw, 0);
  w.z = w.eyeHeight();
  return w;
}

function run(w: WalkState, input: WalkInput, seconds: number): void {
  for (let i = 0; i < Math.round(seconds * 60); i++) w.step(1 / 60, input);
}

describe("slabs in walk mode", () => {
  it("stops at the rail round a void", () => {
    const worlds = new WalkWorlds(slabDoc(), EYE);
    const w = walker(worlds, L2, 3000, 1000, NORTH);
    run(w, FORWARD, 4);
    expect(w.levelId).toBe(L2);
    expect(w.y).toBeLessThan(2000 - R + 1);
    expect(w.y).toBeGreaterThan(2000 - R - 60);
  });

  it("stops at a void's edge with no rail too", () => {
    const d = slabDoc();
    d.derived.railings = d.derived.railings.filter((r) => r.host_id !== VOID);
    const worlds = new WalkWorlds(d, EYE);
    const w = walker(worlds, L2, 3000, 1000, NORTH);
    run(w, FORWARD, 4);
    expect(w.y).toBeLessThanOrEqual(2000 - R + 1);
    expect(worlds.levelWorld(L2).colliders.some((c) => c.source === "void" && c.id === VOID)).toBe(true);
  });

  it("walks out a door onto a balcony and stops at its glass rail", () => {
    const worlds = new WalkWorlds(slabDoc(), EYE);
    const w = walker(worlds, L2, 2500, 800, SOUTH);
    run(w, FORWARD, 5);
    expect(w.levelId).toBe(L2);
    // Through the door in the south wall, out onto the balcony, up to the rail.
    expect(w.y).toBeLessThan(-75 - R);
    expect(w.y).toBeGreaterThan(-1575 + R - 1);
    expect(w.floorZ).toBe(3000);
    expect(worlds.inside(L2, { x: 2500, y: -800 })).toBe(true);
  });

  it("walks on a sunk deck at its own height", () => {
    const worlds = new WalkWorlds(slabDoc(), EYE);
    expect(worlds.approach(L1, { x: 9000, y: 2000 })).toBe(-150);
    expect(worlds.approach(L1, { x: 12000, y: 2000 })).toBe(0);
    const w = walker(worlds, L1, 11500, 2000, -Math.PI);
    run(w, FORWARD, 1.2);
    expect(w.x).toBeLessThan(10075);
    expect(w.floorZ).toBe(-150);
    expect(w.x).toBeGreaterThan(8075);
  });

  it("never lands a fly in a void, and lands on a balcony", () => {
    const worlds = new WalkWorlds(slabDoc(), EYE);
    expect(worlds.inside(L2, { x: 3000, y: 3000 })).toBe(false);
    expect(worlds.landing({ x: 3000, y: 3000 }, 3100)?.levelId).toBe(L1);
    expect(worlds.landing({ x: 2500, y: -800 }, 3100)?.levelId).toBe(L2);
  });

  it("lets nothing block when the Slabs layer is hidden", () => {
    const d = slabDoc();
    const layer = d.project.layers.find((l) => l.key === "slabs");
    if (layer) layer.visible = false;
    else d.project.layers.push({ key: "slabs", visible: false, locked: false });
    const worlds = new WalkWorlds(d, EYE);
    expect(isClear({ x: 3000, y: 3000 }, R, worlds.levelWorld(L2))).toBe(true);
    expect(worlds.levelWorld(L2).colliders.some((c) => c.id === VOID || c.id === BALCONY)).toBe(false);
    expect(worlds.approach(L1, { x: 9000, y: 2000 })).toBe(0);
  });
});
