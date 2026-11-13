// A stair rising inside a void (a double height living room): the walker
// follows the rail runs `Derived.railings` gives, on the flight too. A run
// across the head is a rail and stops the walker; with the head left open,
// it climbs out onto the floor above.
import { describe, expect, it } from "vitest";
import type { DocState, RailingRun } from "../../contract/bindings";
import { WalkState, type WalkInput } from "../engine/walker";
import { L1, L2, STAIR, twoLevelDoc } from "./testModel";
import { WalkWorlds } from "./worlds";

const NORTH = Math.PI / 2;
const FORWARD: WalkInput = { forward: 1, strafe: 0, up: 0, run: false };
const rect = (x0: number, y0: number, x1: number, y1: number) => [
  { x: x0, y: y0 },
  { x: x1, y: y0 },
  { x: x1, y: y1 },
  { x: x0, y: y1 },
];
const run = (pts: { x: number; y: number }[], extra: Partial<RailingRun> = {}): RailingRun => ({
  host_id: "v",
  level_id: L2,
  kind: "slab_edge",
  points: pts.map((p) => ({ ...p, z: 0 })),
  height_mm: 1000,
  style: "bars",
  ...extra,
});

/** The upper floor with a void over the stair and the room east of it (x 75..2500, y 1000..5100). */
function voidOverStair(rails: RailingRun[]): DocState {
  const d = twoLevelDoc();
  d.project.elements.push({ kind: "slab", id: "v", level_id: L2, slab_kind: "void", polygon: rect(75, 1000, 2500, 5100), thickness_mm: 0, top_offset_mm: 0, material_id: null, railing: { height_mm: 1000, style: "bars" } });
  d.derived.railings = rails;
  return d;
}

function climb(d: DocState): WalkState {
  const worlds = new WalkWorlds(d, 1600);
  const w = new WalkState();
  w.setNav(worlds, L1);
  w.place(600, 700, NORTH, 0);
  w.z = w.eyeHeight();
  for (let i = 0; i < 360; i++) w.step(1 / 60, FORWARD);
  return w;
}

describe("a stair rising inside a void", () => {
  it("climbs out at the head when no rail crosses it", () => {
    // The void's rail with the head of the flight left open (x 150..1050 at y 5100).
    const d = voidOverStair([
      run([{ x: 1050, y: 5100 }, { x: 2500, y: 5100 }, { x: 2500, y: 1000 }, { x: 75, y: 1000 }]),
      run([{ x: 75, y: 5100 }, { x: 150, y: 5100 }]),
      run([{ x: 150, y: 5100 }, { x: 150, y: 1200 }, { x: 1050, y: 1200 }, { x: 1050, y: 5100 }], { host_id: STAIR, kind: "stair_well", height_mm: 900 }),
    ]);
    const w = climb(d);
    expect(w.levelId).toBe(L2);
    expect(w.y).toBeGreaterThan(5100);
    expect(w.floorZ).toBe(3000);
  });

  it("stops at a rail the engine runs across the head", () => {
    const d = voidOverStair([run([{ x: 75, y: 1000 }, { x: 2500, y: 1000 }, { x: 2500, y: 5100 }, { x: 75, y: 5100 }])]);
    const w = climb(d);
    // The rail is drawn across the head at y 5100: the walker does not pass it.
    expect(w.y).toBeLessThan(5100);
  });
});
