import { describe, expect, it } from "vitest";
import type { Vec3 } from "../contract/bindings";
import { pt } from "./geom";
import type { PipeEl } from "./pipe";
import { addRunPoint, snapToPipes } from "./pipe";
import {
  endStretch,
  followers,
  heightIndices,
  heightSetCommand,
  heightStepCommand,
  insertBendCommand,
  isLevelRun,
  lengthCommand,
  movedPoints,
  nodeAt,
  nodeOf,
  pickAt,
  planIndices,
  pointHeightCommand,
  pointPlanCommand,
  rangeSlopePct,
  refZ,
  removePointCommand,
  runEndHeightCommand,
  segmentAt,
  slopeCommand,
  typedDragDelta,
  validPick,
} from "./pipeEdit";
import { emptyTyped, typedKey } from "./typed";

const v = (x: number, y: number, z: number): Vec3 => ({ x, y, z });

function pipe(id: string, points: Vec3[], system: PipeEl["system"] = "cold_water", diameter_mm = 20): PipeEl {
  return { kind: "pipe", id, level_id: "L1", system, material: "ppr", diameter_mm, points, name: "" };
}

function typed(text: string) {
  let t = emptyTyped("polar");
  for (const k of text) t = typedKey(t, k) ?? emptyTyped("polar");
  return t;
}

// An L shaped cold water run with a riser at its second bend:
// 0 (0,0,300) - 1 (2000,0,300) - 2 (2000,0,1200) riser - 3 (2000,1500,1200)
const riserRun = pipe("a", [v(0, 0, 300), v(2000, 0, 300), v(2000, 0, 1200), v(2000, 1500, 1200)]);
const level = pipe("b", [v(0, 0, 300), v(1000, 0, 300), v(1000, 1000, 300)]);
const drain = pipe("d", [v(0, 0, -300), v(2000, 0, -340), v(2000, 1000, -360)], "drainage", 50);

describe("picking a part of the selected run", () => {
  it("groups the points a riser joins into one node", () => {
    expect(nodeOf(riserRun.points, 2)).toEqual({ index: 1, count: 2 });
    expect(nodeOf(riserRun.points, 1)).toEqual({ index: 1, count: 2 });
    expect(nodeOf(riserRun.points, 3)).toEqual({ index: 3, count: 1 });
  });

  it("picks a point handle before the segment under it", () => {
    expect(pickAt(level, pt(1003, 4), 40, 20)).toEqual({ pipeId: "b", kind: "point", index: 1 });
    expect(pickAt(level, pt(500, 8), 40, 20)).toEqual({ pipeId: "b", kind: "segment", index: 0 });
    expect(pickAt(level, pt(500, 300), 40, 20)).toBeNull();
  });

  it("never picks a riser as a segment, and picks its node at the riser circle", () => {
    expect(segmentAt(riserRun.points, pt(2005, 0), 20)).not.toBe(1);
    expect(nodeAt(riserRun.points, pt(2000, 2), 30)).toBe(1);
  });

  it("keeps a pick that still fits the run and snaps a point to its node", () => {
    expect(validPick(riserRun.points, { pipeId: "a", kind: "point", index: 2 })).toEqual({ pipeId: "a", kind: "point", index: 1 });
    expect(validPick(level.points, { pipeId: "b", kind: "segment", index: 2 })).toBeNull();
    expect(validPick(level.points, null)).toBeNull();
  });
});

describe("which points an edit moves", () => {
  it("moves whole nodes in plan, so a riser stays vertical", () => {
    expect(planIndices(riserRun.points, null)).toEqual([]);
    expect(planIndices(riserRun.points, { pipeId: "a", kind: "point", index: 1 })).toEqual([1, 2]);
    expect(planIndices(riserRun.points, { pipeId: "a", kind: "segment", index: 0 })).toEqual([0, 1, 2]);
  });

  it("raises a segment by its own two points, so the risers at its ends stretch", () => {
    expect(heightIndices(riserRun.points, { pipeId: "a", kind: "segment", index: 2 })).toEqual([2, 3]);
    expect(heightIndices(riserRun.points, { pipeId: "a", kind: "point", index: 1 })).toEqual([1, 2]);
    expect(heightIndices(riserRun.points, null)).toEqual([]);
  });

  it("reads the height of the part from its first point", () => {
    expect(refZ(riserRun.points, null)).toBe(300);
    expect(refZ(riserRun.points, { pipeId: "a", kind: "segment", index: 2 })).toBe(1200);
    expect(refZ(riserRun.points, { pipeId: "a", kind: "point", index: 2 })).toBe(300);
  });

  it("finds the points level with each end", () => {
    expect(endStretch(riserRun.points, "start")).toEqual([0, 1]);
    expect(endStretch(riserRun.points, "end")).toEqual([2, 3]);
    expect(isLevelRun(level.points)).toBe(true);
    expect(isLevelRun(riserRun.points)).toBe(false);
  });
});

describe("the command each edit sends", () => {
  it("PageUp on the whole run moves every point, joins kept", () => {
    expect(heightStepCommand(level, null, 100)).toEqual({
      type: "move_pipe_points",
      targets: [{ pipe_id: "b", indices: [] }],
      delta: { x: 0, y: 0, z: 100 },
      keep_joins: true,
    });
    expect(heightStepCommand(level, null, 0)).toBeNull();
  });

  it("h then a number moves the picked segment so its height reads the value", () => {
    expect(heightSetCommand(riserRun, { pipeId: "a", kind: "segment", index: 2 }, 1500)).toMatchObject({
      type: "move_pipe_points",
      targets: [{ pipe_id: "a", indices: [2, 3] }],
      delta: { x: 0, y: 0, z: 300 },
    });
  });

  it("a point height moves its node, or one end of a riser alone", () => {
    expect(pointHeightCommand(riserRun, 1, 500)).toMatchObject({ targets: [{ pipe_id: "a", indices: [1, 2] }], delta: { z: 200 } });
    expect(pointHeightCommand(riserRun, 2, 1500, true)).toMatchObject({ targets: [{ pipe_id: "a", indices: [2] }], delta: { z: 300 } });
    expect(pointHeightCommand(riserRun, 2, 1200, true)).toBeNull();
  });

  it("a typed x or y moves the point's node in plan", () => {
    expect(pointPlanCommand(riserRun, 1, { x: 2500, y: 0 })).toMatchObject({ targets: [{ pipe_id: "a", indices: [1, 2] }], delta: { x: 500, y: 0, z: 0 } });
    expect(pointPlanCommand(riserRun, 3, { x: 2000, y: 1500 })).toBeNull();
  });

  it("sets a segment length holding the chosen end", () => {
    expect(lengthCommand(level, 0, 1500, "start")).toEqual({ type: "set_pipe_segment_length", pipe_id: "b", segment: 0, length_mm: 1500, hold: "start", keep_joins: true });
    expect(lengthCommand(level, 0, 1000, "end")).toBeNull();
    expect(lengthCommand(level, 0, Number.POSITIVE_INFINITY, "end")).toBeNull();
    // Under the engine's 10 mm shortest segment it is still sent: the refusal says why.
    expect(lengthCommand(level, 0, 5, "end")).toMatchObject({ type: "set_pipe_segment_length", length_mm: 5 });
  });

  it("sets a slope over a range", () => {
    expect(slopeCommand(drain, 0, 1, 2, "end")).toEqual({ type: "set_pipe_slope", pipe_id: "d", from_segment: 0, to_segment: 1, slope_pct: 2, hold: "end", keep_joins: true });
    // Past the engine's 100 percent it is still sent, so its refusal shows.
    expect(slopeCommand(drain, 0, 0, 150, "end")).toMatchObject({ slope_pct: 150 });
    expect(slopeCommand(drain, 0, 0, Number.NaN, "end")).toBeNull();
  });

  it("a falling run's end height keeps the other end and takes one fall", () => {
    // 3000 mm of plan, start -300: an end at -360 is a 2% fall.
    const c = runEndHeightCommand(drain, "end", -360);
    expect(c).toBeNull();
    const lower = runEndHeightCommand(drain, "end", -390);
    expect(lower).toMatchObject({ type: "set_pipe_slope", from_segment: 0, to_segment: 1, hold: "start" });
    expect((lower as { slope_pct: number }).slope_pct).toBeCloseTo(3, 6);
    const start = runEndHeightCommand(drain, "start", -270);
    expect(start).toMatchObject({ hold: "end" });
    expect((start as { slope_pct: number }).slope_pct).toBeCloseTo(3, 6);
  });

  it("any other run moves the end and the points level with it", () => {
    expect(runEndHeightCommand(riserRun, "end", 1500)).toMatchObject({ type: "move_pipe_points", targets: [{ pipe_id: "a", indices: [2, 3] }], delta: { z: 300 } });
  });

  it("adds a bend in the middle of a segment and removes points while two remain", () => {
    expect(insertBendCommand(level, 0)).toEqual({ type: "insert_pipe_point", pipe_id: "b", segment: 0, at: { x: 500, y: 0, z: 300 } });
    expect(removePointCommand(level, 1)).toEqual({ type: "remove_pipe_point", pipe_id: "b", index: 1 });
    expect(removePointCommand(pipe("c", [v(0, 0, 0), v(1, 0, 0)]), 0)).toBeNull();
  });
});

describe("slopes", () => {
  it("averages a range and leaves risers out", () => {
    expect(rangeSlopePct(drain.points, 0, 0)).toBeCloseTo(2, 6);
    expect(rangeSlopePct(drain.points)).toBeCloseTo(2, 6);
    const withDrop = [v(0, 0, 600), v(0, 0, -300), v(1000, 0, -320)];
    expect(rangeSlopePct(withDrop)).toBeCloseTo(2, 6);
  });
});

describe("the move preview", () => {
  it("moves the chosen points, or all of them", () => {
    expect(movedPoints(level.points, [1], v(0, 0, 50))[1]).toEqual(v(1000, 0, 350));
    expect(movedPoints(level.points, [], v(10, 0, 0)).map((q) => q.x)).toEqual([10, 1010, 1010]);
  });

  it("keeps a tee branch in place along a main that only gets longer", () => {
    const main = pipe("m", [v(0, 0, 300), v(2000, 0, 300)]);
    const tee = pipe("t", [v(500, 0, 300), v(500, -800, 300)]);
    expect(followers(main, [1], v(1000, 0, 0), [tee])).toEqual([]);
    // Moved across it, the branch follows the part of the move across.
    const across = followers(main, [0, 1], v(300, 200, 0), [tee]);
    expect(across[0].points[0].x).toBeCloseTo(500, 6);
    expect(across[0].points[0].y).toBeCloseTo(200, 6);
  });

  it("follows chains: what follows carries what rests on it", () => {
    const a = pipe("a", [v(0, 0, 300), v(1000, 0, 300)]);
    // b starts at a's end (an elbow), c is a branch on b's body.
    const b = pipe("b", [v(1000, 0, 300), v(1000, 2000, 300)]);
    const c = pipe("c", [v(1000, 1000, 300), v(1800, 1000, 300)]);
    const moved = followers(a, [1], v(0, 0, 200), [b, c]);
    expect(moved.map((f) => f.id)).toEqual(["b", "c"]);
    expect(moved[0].points[0].z).toBeCloseTo(500, 6);
    expect(moved[1].points[0].z).toBeCloseTo(400, 6);
  });

  it("at three or more ends the branches follow the through run, never the other way", () => {
    // a and b run through (they line up), c joins them from the side.
    const a = pipe("a", [v(0, 0, 300), v(1000, 0, 300)]);
    const b = pipe("b", [v(1000, 0, 300), v(2000, 0, 300)]);
    const c = pipe("c", [v(1000, 0, 300), v(1000, 900, 300)]);
    expect(followers(c, [0], v(0, 0, 100), [a, b])).toEqual([]);
    const fromMain = followers(a, [1], v(0, 0, 100), [b, c]);
    expect(fromMain.map((f) => f.id).sort()).toEqual(["b", "c"]);
  });

  it("joins runs on other levels by absolute height", () => {
    const riser = { ...pipe("r", [v(0, 0, 300), v(0, 0, 3300)]), level_id: "L1" };
    const above = { ...pipe("u", [v(0, 0, 300), v(1000, 0, 300)]), level_id: "L2" };
    const floor = (id: string | undefined) => (id === "L2" ? 3000 : 0);
    expect(followers(riser, [1], v(0, 0, 50), [above], floor).map((f) => f.id)).toEqual(["u"]);
    expect(followers(riser, [1], v(0, 0, 50), [above])).toEqual([]);
  });

  it("carries an end resting on a moved point, and one on a segment in proportion", () => {
    const main = pipe("m", [v(0, 0, 300), v(2000, 0, 300)]);
    const elbow = pipe("e", [v(2000, 0, 300), v(2000, 1000, 300)]);
    const tee = pipe("t", [v(500, 0, 300), v(500, -800, 300)]);
    const hot = pipe("h", [v(2000, 0, 300), v(2000, -900, 300)], "hot_water");
    const moved = followers(main, [1], v(0, 0, 100), [elbow, tee, hot]);
    expect(moved.map((f) => f.id)).toEqual(["e", "t"]);
    expect(moved[0].points[0]).toEqual(v(2000, 0, 400));
    expect(moved[0].points[1]).toEqual(v(2000, 1000, 300));
    expect(moved[1].points[0].z).toBeCloseTo(325, 6);
  });
});

describe("a distance typed while dragging", () => {
  it("goes along the drag, or along a typed angle", () => {
    expect(typedDragDelta(pt(30, 40), typed("500"), "mm")).toEqual({ x: 300, y: 400 });
    // Close to a 45 degree step: exactly along it.
    const near = typedDragDelta(pt(100, 6), typed("500"), "mm");
    expect(near?.x).toBeCloseTo(500, 6);
    expect(near?.y).toBeCloseTo(0, 6);
    expect(typedDragDelta(pt(0, 0), typed("2"), "m")).toEqual({ x: 2000, y: 0 });
    const at = typedDragDelta(pt(1, 0), typed("600<90"), "mm");
    expect(at?.x).toBeCloseTo(0, 6);
    expect(at?.y).toBeCloseTo(600, 6);
    expect(typedDragDelta(pt(1, 0), emptyTyped("polar"), "mm")).toBeNull();
  });
});

describe("a run snapped to a fixture (DECISIONS D40)", () => {
  const lav = { point: pt(3000, 0), label: "Lavatory, back", z: 550 };

  it("joins at the fixture's connection height", () => {
    const hit = snapToPipes(pt(3005, 3), { pipes: [], fixtures: [lav] }, { tol: 30, system: "cold_water", penZ: 300, anchor: null, ortho: false });
    expect(hit).toMatchObject({ kind: "fixture", z: 550 });
  });

  it("ends there with a riser from the pen height", () => {
    const d = addRunPoint({ points: [v(0, 0, 300)], penZ: 300 }, pt(3000, 0), { fallPct: null, snapZ: 550, fixture: true });
    expect(d.points).toEqual([v(0, 0, 300), v(3000, 0, 300), v(3000, 0, 550)]);
  });

  it("meets a fixture with a riser even when a falling run arrives below it", () => {
    const d = addRunPoint({ points: [v(0, 0, -300)], penZ: -300 }, pt(2000, 0), { fallPct: 2, snapZ: 1200, fixture: true });
    expect(d.points).toEqual([v(0, 0, -300), v(2000, 0, -340), v(2000, 0, 1200)]);
    // Another run's end still takes the slope up, so the review shows it.
    const j = addRunPoint({ points: [v(0, 0, -300)], penZ: -300 }, pt(2000, 0), { fallPct: 2, snapZ: -200 });
    expect(j.points).toEqual([v(0, 0, -300), v(2000, 0, -200)]);
  });

  it("starts there and keeps the pen height for a riser", () => {
    const d = addRunPoint({ points: [], penZ: -300 }, pt(3000, 0), { fallPct: null, snapZ: 450, fixture: true });
    expect(d).toEqual({ points: [v(3000, 0, 450)], penZ: -300 });
    const next = addRunPoint(d, pt(3000, 2000), { fallPct: 2, snapZ: null });
    expect(next.points).toEqual([v(3000, 0, 450), v(3000, 0, -300), v(3000, 2000, -340)]);
  });
});
