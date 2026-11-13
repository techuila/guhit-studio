// Walk mode on L and U stairs (DECISIONS D54): the walker climbs by the
// engine's walking line, round the landing, to the floor above. The stairs
// are the engine's own geometry (turningStairs.fixture.json, written by
// `cargo test -p guhit-core --test l2_stairs -- --ignored write_walk_fixture`).

import { describe, expect, it } from "vitest";
import type { DocState, Element, StairGeometry } from "../../contract/bindings";
import { WALKER_RADIUS_MM } from "../geom/collision";
import { WalkState, type WalkInput } from "../engine/walker";
import { holdOnFlight, nearStair, onFlight, rampHeight, stairColliders, stairLocal, stairPieces, stairsOf, stairWells, turning, uAtHeight } from "./stairs";
import { L1, L2, twoLevelDoc } from "./testModel";
import { WalkWorlds } from "./worlds";
import fixture from "./turningStairs.fixture.json";

const EYE = 1600;
const FORWARD: WalkInput = { forward: 1, strafe: 0, up: 0, run: false };

type Turning = { element: Element; geometry: StairGeometry };
const L = (fixture as unknown as Record<string, Turning>).l;
const U = (fixture as unknown as Record<string, Turning>).u;

/** The walk tests' bungalow with its second floor and one turning stair instead of the straight one. */
function docWith(...stairs: Turning[]): DocState {
  const d = twoLevelDoc();
  d.project.elements = d.project.elements.filter((e) => e.kind !== "stair");
  for (const s of stairs) d.project.elements.push(s.element);
  d.derived.stairs = stairs.map((s) => s.geometry);
  return d;
}

function walker(worlds: WalkWorlds, x: number, y: number, yaw: number): WalkState {
  const w = new WalkState();
  w.setNav(worlds, L1);
  w.place(x, y, yaw, 0);
  w.z = w.eyeHeight();
  return w;
}

/**
 * Walks up the stair as a person would, looking 600 mm ahead along its
 * walking line, until the walker is on the floor above; returns the floor
 * heights under it.
 */
function climb(w: WalkState, worlds: WalkWorlds, seconds = 20): number[] {
  const s = worlds.stairs[0];
  const zs: number[] = [];
  for (let i = 0; i < seconds * 60 && w.levelId !== L2; i++) {
    const { u } = stairLocal(s, w);
    const ahead = Math.min(Math.max(u, -s.going / 2) + 600, s.run + s.going);
    const target = pointAlong(s.path, s.pathAt, ahead + s.going / 2);
    w.yaw = Math.atan2(target.y - w.y, target.x - w.x);
    w.step(1 / 60, FORWARD);
    zs.push(w.floorZ);
  }
  // A little on, onto the floor above.
  for (let i = 0; i < 30; i++) w.step(1 / 60, FORWARD);
  return zs;
}

function pointAlong(path: { x: number; y: number }[], at: number[], d: number) {
  for (let i = 0; i + 1 < path.length; i++) {
    if (d <= at[i + 1] || i + 2 === path.length) {
      const t = Math.min(Math.max((d - at[i]) / (at[i + 1] - at[i] || 1), 0), 2);
      return { x: path[i].x + (path[i + 1].x - path[i].x) * t, y: path[i].y + (path[i + 1].y - path[i].y) * t };
    }
  }
  return path[path.length - 1];
}

function smooth(zs: number[], riser: number) {
  for (let i = 1; i < zs.length; i++) {
    expect(zs[i]).toBeGreaterThanOrEqual(zs[i - 1] - 1e-6);
    expect(zs[i] - zs[i - 1]).toBeLessThan(riser);
  }
}

describe("turning stairs from the engine", () => {
  it("read as one stair of two flights, a landing and the walking line", () => {
    const [s] = stairsOf(docWith(L));
    expect(turning(s)).toBe(true);
    expect(s.flights).toHaveLength(2);
    expect(s.landings).toHaveLength(1);
    expect(s.treads).toBe(14);
    expect(s.going).toBeCloseTo(260, 6);
    expect(s.topLevelId).toBe(L2);
    // The walking line climbs from the floor to the floor above, flat on the landing.
    expect(rampHeight(s, -s.going / 2)).toBe(0);
    expect(rampHeight(s, s.run + s.going / 2)).toBeCloseTo(3000, 6);
    const onLanding = stairLocal(s, { x: 1000, y: 3270 });
    expect(rampHeight(s, onLanding.u)).toBeCloseTo(s.landings[0].z, 6);
    expect(uAtHeight(s, s.landings[0].z)).toBeLessThanOrEqual(onLanding.u + 1e-6);
  });

  it("know their flights and landing, not the corner beside them", () => {
    const [s] = stairsOf(docWith(L));
    expect(onFlight(s, { x: 1000, y: 2000 })).toBe(true);
    expect(onFlight(s, { x: 1000, y: 3270 })).toBe(true);
    expect(onFlight(s, { x: 2500, y: 3270 })).toBe(true);
    // Beside the first flight, under the second flight's start: floor.
    expect(onFlight(s, { x: 2500, y: 2000 })).toBe(false);
    expect(nearStair(s, { x: 1000, y: 600 }, WALKER_RADIUS_MM)).toBe(true);
  });

  it("hold a walker inside the flights and the landing, a radius in from the sides", () => {
    const [s] = stairsOf(docWith(L));
    const r = WALKER_RADIUS_MM;
    // Off the outer side of the landing: back inside.
    const held = holdOnFlight(s, { x: 300, y: 3300 }, r);
    expect(held.x).toBeCloseTo(550 + r, 6);
    // From the landing into the second flight: free.
    expect(holdOnFlight(s, { x: 1500, y: 3270 }, r)).toEqual({ x: 1500, y: 3270 });
  });

  it("are closed round everything but the foot below and the head above", () => {
    const [s] = stairsOf(docWith(L));
    const lower = stairColliders(s, "lower");
    const upper = stairColliders(s, "upper");
    expect(lower.length).toBeGreaterThan(4);
    // No strip across the foot below, none across the head above.
    const across = (cs: typeof lower, p: { x: number; y: number }) => cs.some((c) => c.kind === "poly" && c.minX <= p.x && c.maxX >= p.x && c.minY <= p.y && c.maxY >= p.y);
    expect(across(lower, { x: 1000, y: 995 })).toBe(false);
    expect(across(upper, { x: 1000, y: 995 })).toBe(true);
    expect(across(upper, { x: 3275, y: 3270 })).toBe(false);
    expect(across(lower, { x: 3275, y: 3270 })).toBe(true);
  });

  it("cut convex wells out of the floor above: both flights and the landing, and a U's well", () => {
    const wells = stairWells(stairsOf(docWith(L, U)));
    const pieces = wells.get(L2) ?? [];
    // L: two flights and a landing; U: two flights, a landing and the well.
    expect(pieces).toHaveLength(3 + 4);
    expect(stairPieces(stairsOf(docWith(U))[0])).toHaveLength(3);
  });
});

describe("walking up a turning stair", () => {
  for (const [name, st] of [
    ["an L", L],
    ["a U", U],
  ] as const) {
    it(`climbs ${name} round its landing to the floor above, smoothly`, () => {
      const worlds = new WalkWorlds(docWith(st), EYE);
      const s = worlds.stairs[0];
      const foot = s.flights[0].origin;
      const w = walker(worlds, foot.x, foot.y - 500, Math.PI / 2);
      const zs = climb(w, worlds);
      expect(w.levelId).toBe(L2);
      expect(w.floorZ).toBe(3000);
      expect(Math.max(...zs)).toBeLessThanOrEqual(3000 + 1e-6);
      smooth(zs, s.rise / s.risers);
      // It went round: it passed over the landing at the landing's height.
      expect(zs.some((z) => Math.abs(z - s.landings[0].z) < 1e-6)).toBe(true);
    });
  }
});
