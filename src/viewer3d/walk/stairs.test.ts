import { describe, expect, it } from "vitest";
import { WALKER_RADIUS_MM } from "../geom/collision";
import { NO_INPUT, WalkState, type WalkInput } from "../engine/walker";
import { approachHeight, closedSide, holdOnFlight, onFlight, onFootprint, rampHeight, stairColliders, stairLocal, stairsOf, STAIR_STRIP_MM, uAtHeight } from "./stairs";
import { polyCollider } from "../geom/collision";
import { L1, L2, STAIR, twoLevelDoc } from "./testModel";
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

/** Steps at 60 Hz; returns the eye heights seen. */
function run(w: WalkState, input: WalkInput, seconds: number): number[] {
  const zs: number[] = [];
  for (let i = 0; i < Math.round(seconds * 60); i++) {
    w.step(1 / 60, input);
    zs.push(w.z);
  }
  return zs;
}

describe("stairsOf", () => {
  it("reads the flight and finds the level at its top", () => {
    const [s] = stairsOf(twoLevelDoc());
    expect(s.id).toBe(STAIR);
    expect(s.levelId).toBe(L1);
    expect(s.topLevelId).toBe(L2);
    expect(s.going).toBeCloseTo(243.75, 6);
    expect(s.rise).toBe(3000);
    expect(s.bottomZ).toBe(0);
    // Climbs north: the head is 3900 mm north of the first riser.
    const head = stairLocal(s, { x: 600, y: 5100 });
    expect(head.u).toBeCloseTo(3900, 6);
    expect(head.v).toBeCloseTo(0, 6);
  });

  it("has no top level when no floor sits within 300 mm of its top", () => {
    const d = twoLevelDoc();
    d.project.levels[1].elevation_mm = 3400;
    expect(stairsOf(d)[0].topLevelId).toBeNull();
    d.project.levels[1].elevation_mm = 3250;
    expect(stairsOf(d)[0].topLevelId).toBe(L2);
    expect(stairsOf(twoLevelDoc({ upper: false }))[0].topLevelId).toBeNull();
  });

  it("drops stairs on a hidden stairs layer", () => {
    const d = twoLevelDoc();
    d.project.layers = d.project.layers.map((l) => (l.key === "stairs" ? { ...l, visible: false } : l));
    expect(stairsOf(d)).toHaveLength(0);
  });
});

describe("rampHeight", () => {
  const [s] = stairsOf(twoLevelDoc());
  const riser = s.rise / s.risers;

  it("passes through the middle of every tread at that tread's height", () => {
    for (let i = 1; i <= s.risers; i++) expect(rampHeight(s, (i - 0.5) * s.going)).toBeCloseTo(i * riser, 6);
  });

  it("starts half a going before the first riser and is capped at the level height", () => {
    expect(rampHeight(s, -s.going / 2)).toBe(0);
    expect(rampHeight(s, -1000)).toBe(0);
    expect(rampHeight(s, 0)).toBeCloseTo(riser / 2, 6);
    expect(rampHeight(s, s.run - s.going / 2)).toBeCloseTo(s.rise, 6);
    expect(rampHeight(s, s.run)).toBe(s.rise);
    expect(rampHeight(s, s.run + 5000)).toBe(s.rise);
  });

  it("never differs from the stepped treads by more than half a riser", () => {
    for (let u = 0; u < s.run; u += 7) {
      const tread = (Math.floor(u / s.going) + 1) * riser;
      expect(Math.abs(rampHeight(s, u) - tread)).toBeLessThanOrEqual(riser / 2 + 1e-6);
    }
  });

  it("knows the flight and its approach strip", () => {
    expect(onFlight(s, { x: 600, y: 3000 })).toBe(true);
    expect(onFlight(s, { x: 1100, y: 3000 })).toBe(false);
    expect(onFlight(s, { x: 600, y: 1100 })).toBe(false);
    expect(approachHeight(s, { x: 600, y: 1100 })).toBeCloseTo(riser * (-100 / s.going + 0.5), 6);
    expect(approachHeight(s, { x: 600, y: 900 })).toBeNull();
  });

  it("closes the sides and the head below, the sides and the foot above", () => {
    expect(stairColliders(s, "lower")).toHaveLength(3);
    expect(stairColliders(s, "upper")).toHaveLength(3);
  });

  it("leaves the first and last treads open at the sides where a floor is beside them", () => {
    const open = { low: [true, true] as [boolean, boolean], high: [false, true] as [boolean, boolean] };
    // 450 mm up: 2.4 risers, half a going earlier on the walking line.
    const lowU = uAtHeight(s, 450);
    const highU = uAtHeight(s, s.rise - 450);
    expect(lowU).toBeCloseTo((450 / riser - 0.5) * s.going, 6);
    expect(closedSide(s, open, 0)).toEqual({ from: lowU, to: s.run });
    expect(closedSide(s, open, 1)).toEqual({ from: lowU, to: highU });
    const [noTop] = stairsOf(twoLevelDoc({ upper: false }));
    // Nothing to step off onto at the top of a flight with no level there.
    expect(closedSide(noTop, open, 1).to).toBe(noTop.run);
  });

  it("holds a walker between the sides, funnelling in where a closed side starts", () => {
    const open = { low: [true, true] as [boolean, boolean], high: [true, true] as [boolean, boolean] };
    const lowU = uAtHeight(s, 450);
    const at = (u: number, v: number) => stairLocal(s, holdOnFlight(s, { x: s.origin.x + v, y: s.origin.y + u }, R, open));
    // Open treads: the full width and past it (stepping off at the side).
    expect(at(100, 500).v).toBeCloseTo(500, 6);
    // Closed: within the width less the radius.
    expect(at(2000, 600).v).toBeCloseTo(450 - R, 6);
    expect(at(2000, -600).v).toBeCloseTo(-(450 - R), 6);
    // In between, 45 degrees: never a jump.
    expect(at(lowU - 100, 600).v).toBeCloseTo(450 - R + 100, 6);
    // No level at the top: short of the head.
    const [noTop] = stairsOf(twoLevelDoc({ upper: false }));
    expect(stairLocal(noTop, holdOnFlight(noTop, { x: 600, y: 1200 + 3890 }, R)).u).toBeCloseTo(noTop.run - R, 6);
  });

  it("knows what stands on the footprint", () => {
    const box = (x0: number, y0: number, x1: number, y1: number) =>
      polyCollider(
        [
          { x: x0, y: y0 },
          { x: x1, y: y0 },
          { x: x1, y: y1 },
          { x: x0, y: y1 },
        ],
        "wall",
        "w",
      )!;
    // A wall across the flight.
    expect(onFootprint(s, box(0, 2900, 2000, 3050))).toBe(true);
    // The west wall beside it, 75 mm off its edge.
    expect(onFootprint(s, box(-75, -75, 75, 6075))).toBe(false);
    // A wall flush with its side.
    expect(onFootprint(s, box(1050, 1000, 1200, 6000))).toBe(false);
    // A post in the middle, smaller than the flight.
    expect(onFootprint(s, box(550, 3000, 650, 3100))).toBe(true);
    expect(onFootprint(s, { kind: "circle", c: { x: 600, y: 3000 }, r: 50, source: "column", id: "c", minX: 550, minY: 2950, maxX: 650, maxY: 3050 })).toBe(true);
  });
});

describe("walking a stair", () => {
  const worlds = new WalkWorlds(twoLevelDoc(), EYE);

  it("climbs the flight as a smooth ramp and switches to the upper level at the head", () => {
    const w = walker(worlds, L1, 600, 700, NORTH);
    const zs = run(w, FORWARD, 5);
    expect(w.levelId).toBe(L2);
    expect(w.stair).toBeNull();
    expect(w.floorZ).toBe(3000);
    expect(w.y).toBeGreaterThan(5100);
    // Rising all the way, never a drop, never a jump bigger than a riser in one frame.
    for (let i = 1; i < zs.length; i++) {
      expect(zs[i]).toBeGreaterThanOrEqual(zs[i - 1] - 1e-6);
      expect(zs[i] - zs[i - 1]).toBeLessThan(187.5);
    }
    // Capped: the eye never goes above eye height over the upper floor.
    expect(Math.max(...zs)).toBeLessThanOrEqual(3000 + EYE + 1e-6);
    run(w, NO_INPUT, 1);
    expect(w.z).toBe(3000 + EYE);
  });

  it("stands on the walking line of the tread under it halfway up", () => {
    const w = walker(worlds, L1, 600, 700, NORTH);
    for (let i = 0; i < 600 && w.y < 3150; i++) w.step(1 / 60, FORWARD);
    expect(w.stair?.id).toBe(STAIR);
    const u = w.y - 1200;
    expect(w.floorZ).toBeCloseTo(rampHeight(worlds.stairs[0], u), 6);
    // Still counted on the level it came from until it leaves the flight.
    expect(w.levelId).toBe(L1);
  });

  it("walks down from the upper level and switches back at the foot", () => {
    const w = walker(worlds, L2, 600, 5600, SOUTH);
    expect(w.floorZ).toBe(3000);
    run(w, FORWARD, 5);
    expect(w.levelId).toBe(L1);
    expect(w.stair).toBeNull();
    expect(w.y).toBeLessThan(1200);
    run(w, NO_INPUT, 1);
    expect(w.z).toBe(EYE);
  });

  it("holds the walker between the sides of the flight, at the stair's width", () => {
    const w = walker(worlds, L1, 600, 700, NORTH);
    for (let i = 0; i < 600 && w.y < 3000; i++) w.step(1 / 60, FORWARD);
    run(w, { forward: 0, strafe: 1, up: 0, run: true }, 3);
    expect(w.stair?.id).toBe(STAIR);
    expect(w.x).toBeLessThanOrEqual(1050 - R + 0.5);
    run(w, { forward: 0, strafe: -1, up: 0, run: true }, 3);
    expect(w.x).toBeGreaterThanOrEqual(150 + R - 0.5);
  });

  it("is solid from the side on the lower level and around the stairwell on the upper one", () => {
    for (const level of [L1, L2]) {
      const w = walker(worlds, level, 1800, 3000, Math.PI);
      run(w, { ...FORWARD, run: true }, 3);
      expect(w.x).toBeGreaterThanOrEqual(1050 + STAIR_STRIP_MM + R - 0.5);
      expect(w.stair).toBeNull();
      expect(w.levelId).toBe(level);
    }
  });

  it("is closed behind the head on the lower level", () => {
    const w = walker(worlds, L1, 600, 5700, SOUTH);
    run(w, { ...FORWARD, run: true }, 3);
    expect(w.y).toBeGreaterThanOrEqual(5100 + STAIR_STRIP_MM + R - 0.5);
    expect(w.floorZ).toBe(0);
  });

  it("stops at the top when no level sits above the stair", () => {
    const low = new WalkWorlds(twoLevelDoc({ upper: false }), EYE);
    const w = walker(low, L1, 600, 700, NORTH);
    run(w, { ...FORWARD, run: true }, 6);
    expect(w.levelId).toBe(L1);
    expect(w.stair?.id).toBe(STAIR);
    expect(w.y).toBeLessThanOrEqual(5100 - R + 0.5);
    expect(w.floorZ).toBeLessThanOrEqual(3000);
    expect(w.floorZ).toBeGreaterThan(2800);
  });

  it("flies over a stair without climbing it", () => {
    const w = walker(worlds, L1, 600, 700, NORTH);
    w.mode = "fly";
    const z0 = w.z;
    run(w, FORWARD, 3);
    expect(w.y).toBeGreaterThan(5100);
    expect(w.z).toBe(z0);
    expect(w.levelId).toBe(L1);
    expect(w.stair).toBeNull();
  });

  it("lands on the floor under the feet when flying ends", () => {
    const w = walker(worlds, L1, 3000, 3000, NORTH);
    w.mode = "fly";
    w.z = 3000 + EYE + 200;
    w.mode = "walk";
    w.land();
    expect(w.levelId).toBe(L2);
    run(w, NO_INPUT, 1.5);
    expect(w.z).toBe(3000 + EYE);

    const low = walker(worlds, L2, 3000, 3000, NORTH);
    low.z = 900 + EYE;
    low.land();
    expect(low.levelId).toBe(L1);

    // Outside the building the upper floor is not under the walker: the ground is.
    const out = walker(worlds, L1, 12000, 3000, NORTH);
    out.z = 3000 + EYE;
    out.land();
    expect(out.levelId).toBe(L1);

    // Over the flight near its walking line: on the stair.
    const mid = walker(worlds, L1, 600, 3150, NORTH);
    mid.z = rampHeight(worlds.stairs[0], 1950) + EYE;
    mid.land();
    expect(mid.stair?.id).toBe(STAIR);
  });
});

describe("climbing a stair reliably", () => {
  const worlds = new WalkWorlds(twoLevelDoc(), EYE);
  const s = worlds.stairs[0];

  /** Walks with W held until the walker reaches the upper floor or `seconds` pass; returns the eye heights. */
  function climb(w: WalkState, seconds: number, input: WalkInput = FORWARD): number[] {
    const zs: number[] = [];
    for (let i = 0; i < Math.round(seconds * 60) && w.levelId !== L2; i++) {
      w.step(1 / 60, input);
      zs.push(w.z);
    }
    return zs;
  }

  const smooth = (zs: number[]) => {
    for (let i = 1; i < zs.length; i++) {
      expect(zs[i]).toBeGreaterThanOrEqual(zs[i - 1] - 1e-6);
      expect(zs[i] - zs[i - 1]).toBeLessThan(s.rise / s.risers);
    }
  };

  for (const offDeg of [0, -15, -30, -45, -60]) {
    for (const v of [-300, 0, 300]) {
      it(`walks onto the first tread ${-offDeg} degrees off the flight, ${v} mm off its middle, and climbs to the upper level`, () => {
        // Aimed at a point on the first tread; heading north, turned toward the west.
        const a = NORTH - (offDeg * Math.PI) / 180;
        const target = { x: 600 + v, y: 1200 + 100 };
        const start = { x: target.x - Math.cos(a) * 1100, y: target.y - Math.sin(a) * 1100 };
        const w = walker(worlds, L1, start.x, start.y, a);
        const zs = climb(w, 14);
        expect(w.levelId).toBe(L2);
        expect(w.floorZ).toBe(3000);
        smooth(zs);
      });
    }
  }

  it("climbs at running speed and comes back down to the lower level", () => {
    const w = walker(worlds, L1, 600, 700, NORTH);
    smooth(climb(w, 6, { ...FORWARD, run: true }));
    expect(w.levelId).toBe(L2);
    w.place(w.x, w.y + 300, SOUTH, 0);
    run(w, NO_INPUT, 1);
    const zs: number[] = [];
    for (let i = 0; i < 600 && !(w.levelId === L1 && w.stair === null); i++) {
      w.step(1 / 60, FORWARD);
      zs.push(w.z);
    }
    expect(w.levelId).toBe(L1);
    for (let i = 1; i < zs.length; i++) expect(zs[i]).toBeLessThanOrEqual(zs[i - 1] + 1e-6);
  });

  it("steps off the top sideways when the head meets a wall, and back on", () => {
    const d = twoLevelDoc();
    // An upper wall 75 mm past the head: no room to stand there.
    d.project.elements.push({ kind: "wall", id: "test-head-wall", level_id: L2, start: { x: 0, y: 5250 }, end: { x: 3000, y: 5250 }, thickness_mm: 150, height_mm: null, material_id: null } as never);
    const tight = new WalkWorlds(d, EYE);
    const w = walker(tight, L1, 600, 700, NORTH);
    run(w, { ...FORWARD, run: true }, 6);
    // Stopped short of the wall on the top treads, still on the flight.
    expect(w.stair?.id).toBe(STAIR);
    expect(w.y).toBeLessThanOrEqual(5175 - R + 0.5);
    // Turn east and walk off onto the upper floor.
    w.place(w.x, w.y, 0, 0);
    run(w, FORWARD, 2);
    expect(w.stair).toBeNull();
    expect(w.levelId).toBe(L2);
    expect(w.floorZ).toBe(3000);
    // And back onto the flight from the side, then down.
    w.place(w.x, w.y, Math.PI, 0);
    for (let i = 0; i < 240 && !w.stair; i++) w.step(1 / 60, FORWARD);
    expect(w.stair?.id).toBe(STAIR);
    w.place(w.x, w.y, SOUTH, 0);
    run(w, FORWARD, 6);
    expect(w.levelId).toBe(L1);
    expect(w.stair).toBeNull();
  });

  it("is not stopped by an upper wall standing over the flight", () => {
    const d = twoLevelDoc();
    // A wall of the upper floor across the middle of the flight, as when the stairwell is drawn short.
    d.project.elements.push({ kind: "wall", id: "test-over-wall", level_id: L2, start: { x: 0, y: 3000 }, end: { x: 2000, y: 3000 }, thickness_mm: 150, height_mm: null, material_id: null } as never);
    const over = new WalkWorlds(d, EYE);
    const w = walker(over, L1, 600, 700, NORTH);
    climb(w, 8);
    expect(w.levelId).toBe(L2);
  });

  it("keeps the middle of the flight closed at the sides", () => {
    const w = walker(worlds, L1, 600, 700, NORTH);
    for (let i = 0; i < 600 && w.y < 3000; i++) w.step(1 / 60, FORWARD);
    run(w, { forward: 0, strafe: 1, up: 0, run: true }, 2);
    expect(w.stair?.id).toBe(STAIR);
    expect(w.x).toBeLessThanOrEqual(1050 - R + 0.5);
  });
});
