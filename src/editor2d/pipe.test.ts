import { describe, expect, it } from "vitest";
import type { DocState, Element, Vec3 } from "../contract/bindings";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import { gripsFor } from "./edit";
import { pt, rectFromPoints } from "./geom";
import { hitTest, marqueeSelect } from "./hit";
import { buildIndex, isLocked } from "./model";
import type { PipeEl, PipeSnapOptions, PipeSnapScene } from "./pipe";
import {
  PIPE_DEFAULTS,
  addRunPoint,
  canJoin,
  cleanRun,
  drainFallPct,
  fallEnd,
  finishRun,
  fixturePoints,
  formatHeight,
  formatPct,
  hitsPipe,
  isPlumbingFixture,
  isServiceFixture,
  isVertical,
  lineSetPx,
  lineSetSpacingMm,
  movePipeNode,
  pipeNodes,
  pipePlan,
  pipeSpec,
  popRunPoint,
  segmentFallPct,
  snapToPipes,
  toolFallPct,
  withPendingRiser,
} from "./pipe";

const v = (x: number, y: number, z: number): Vec3 => ({ x, y, z });
const LEVEL = "00000000-0000-4000-8000-0000000000a1";

function pipe(id: string, system: PipeEl["system"], points: Vec3[], diameter = 20): PipeEl {
  return { kind: "pipe", id, level_id: LEVEL, system, material: system === "drainage" || system === "vent" ? "upvc" : "ppr", diameter_mm: diameter, points, name: "" };
}

describe("tool defaults", () => {
  it("mirrors the contract table", () => {
    expect(PIPE_DEFAULTS.cold_water).toEqual({ material: "ppr", diameterMm: 20, startHeightMm: 300 });
    expect(PIPE_DEFAULTS.drainage).toEqual({ material: "upvc", diameterMm: 50, startHeightMm: -300 });
    expect(PIPE_DEFAULTS.vent.startHeightMm).toBe(300);
  });

  it("fills null options with the system defaults", () => {
    const spec = pipeSpec({ pipeSystem: "drainage", pipeMaterial: null, pipeDiameterMm: null, pipeElevationMm: null });
    expect(spec).toEqual({ system: "drainage", material: "upvc", diameterMm: 50, startHeightMm: -300 });
    const set = pipeSpec({ pipeSystem: "cold_water", pipeMaterial: "gi", pipeDiameterMm: 25, pipeElevationMm: 1500 });
    expect(set).toEqual({ system: "cold_water", material: "gi", diameterMm: 25, startHeightMm: 1500 });
  });
});

describe("drainage fall", () => {
  it("is 2 percent, 1 percent from 100 mm up", () => {
    expect(drainFallPct(50)).toBe(2);
    expect(drainFallPct(75)).toBe(2);
    expect(drainFallPct(100)).toBe(1);
    expect(drainFallPct(150)).toBe(1);
    expect(toolFallPct({ system: "drainage", diameterMm: 100 })).toBe(1);
    expect(toolFallPct({ system: "cold_water", diameterMm: 20 })).toBeNull();
  });

  it("drops by the fall over the plan length", () => {
    expect(fallEnd(-300, pt(0, 0), pt(3000, 0), 2)).toBe(-360);
    expect(fallEnd(-300, pt(0, 0), pt(3000, 4000), 1)).toBe(-350);
    // Never flatter than asked, never float noise.
    const z = fallEnd(-300, pt(0, 0), pt(1234.5678, 0), 2);
    expect(-300 - z).toBeGreaterThanOrEqual(1234.5678 * 0.02);
    expect(Math.round(z * 1000)).toBe(z * 1000);
  });

  it("measures a segment's fall in percent, null for a riser", () => {
    expect(segmentFallPct(v(0, 0, -300), v(3000, 0, -360))).toBeCloseTo(2);
    expect(segmentFallPct(v(0, 0, -300), v(1000, 0, -290))).toBeCloseTo(-1);
    expect(segmentFallPct(v(0, 0, 0), v(0, 0, -300))).toBeNull();
  });

  it("each new horizontal segment of a drainage run falls from the last point", () => {
    let d = addRunPoint({ points: [], penZ: -300 }, pt(0, 0), { fallPct: null, snapZ: null });
    d = addRunPoint(d, pt(3000, 0), { fallPct: 2, snapZ: null });
    d = addRunPoint(d, pt(3000, 1000), { fallPct: 2, snapZ: null });
    expect(d.points).toEqual([v(0, 0, -300), v(3000, 0, -360), v(3000, 1000, -380)]);
    expect(d.penZ).toBe(-380);
  });

  it("water runs stay level at the pen height", () => {
    let d = addRunPoint({ points: [], penZ: 300 }, pt(0, 0), { fallPct: null, snapZ: null });
    d = addRunPoint(d, pt(2000, 0), { fallPct: null, snapZ: null });
    expect(d.points).toEqual([v(0, 0, 300), v(2000, 0, 300)]);
  });
});

describe("risers", () => {
  it("a height change adds a riser at the last point", () => {
    const d = { points: [v(0, 0, 300), v(2000, 0, 300)], penZ: 1200 };
    expect(withPendingRiser(d)).toEqual([v(0, 0, 300), v(2000, 0, 300), v(2000, 0, 1200)]);
    const next = addRunPoint(d, pt(2000, 1500), { fallPct: null, snapZ: null });
    expect(next.points).toEqual([v(0, 0, 300), v(2000, 0, 300), v(2000, 0, 1200), v(2000, 1500, 1200)]);
    const nodes = pipeNodes(next.points);
    expect(nodes.map((n) => n.count)).toEqual([1, 2, 1]);
    expect(nodes[1]).toMatchObject({ zIn: 300, zOut: 1200 });
  });

  it("drainage keeps falling after a drop", () => {
    const d = { points: [v(0, 0, 0)], penZ: -300 };
    const next = addRunPoint(d, pt(2000, 0), { fallPct: 2, snapZ: null });
    expect(next.points).toEqual([v(0, 0, 0), v(0, 0, -300), v(2000, 0, -340)]);
  });

  it("finishing keeps a riser at the end of the run", () => {
    expect(finishRun({ points: [v(0, 0, 300), v(1000, 0, 300)], penZ: 900 })).toEqual([v(0, 0, 300), v(1000, 0, 300), v(1000, 0, 900)]);
    expect(finishRun({ points: [v(0, 0, 300)], penZ: 300 })).toBeNull();
    // A single point with a riser is a vertical run: two points.
    expect(finishRun({ points: [v(0, 0, 0)], penZ: 2400 })).toEqual([v(0, 0, 0), v(0, 0, 2400)]);
  });

  it("escape drops the pending riser, then the last point, then the run", () => {
    let d = popRunPoint({ points: [v(0, 0, 300), v(1000, 0, 300)], penZ: 900 });
    expect(d).toEqual({ points: [v(0, 0, 300), v(1000, 0, 300)], penZ: 300 });
    d = popRunPoint(d!);
    expect(d).toEqual({ points: [v(0, 0, 300)], penZ: 300 });
    expect(popRunPoint(d!)).toBeNull();
  });

  it("a click on the last point only commits the pending riser", () => {
    const d = addRunPoint({ points: [v(0, 0, 300)], penZ: 800 }, pt(0.4, 0), { fallPct: null, snapZ: null });
    expect(d.points).toEqual([v(0, 0, 300), v(0, 0, 800)]);
    const same = addRunPoint({ points: [v(0, 0, 300)], penZ: 300 }, pt(0, 0), { fallPct: null, snapZ: null });
    expect(same.points).toEqual([v(0, 0, 300)]);
  });

  it("a point snapped to a pipe at another height joins it with a riser", () => {
    const d = addRunPoint({ points: [v(0, 0, 300)], penZ: 300 }, pt(3000, 0), { fallPct: null, snapZ: 1200 });
    expect(d.points).toEqual([v(0, 0, 300), v(3000, 0, 300), v(3000, 0, 1200)]);
    expect(d.penZ).toBe(1200);
    // Within a millimeter it is the same height: no riser.
    const close = addRunPoint({ points: [v(0, 0, 300)], penZ: 300 }, pt(3000, 0), { fallPct: null, snapZ: 300.4 });
    expect(close.points).toEqual([v(0, 0, 300), v(3000, 0, 300.4)]);
    // The first point takes the snapped height directly.
    const first = addRunPoint({ points: [], penZ: 300 }, pt(0, 0), { fallPct: null, snapZ: 1500 });
    expect(first).toEqual({ points: [v(0, 0, 1500)], penZ: 1500 });
  });

  it("a falling run drops into a join below it, and slopes up to one above it instead of climbing a riser", () => {
    const drop = addRunPoint({ points: [v(0, 0, -300)], penZ: -300 }, pt(2000, 0), { fallPct: 2, snapZ: -500 });
    expect(drop.points).toEqual([v(0, 0, -300), v(2000, 0, -340), v(2000, 0, -500)]);
    const up = addRunPoint({ points: [v(0, 0, -300)], penZ: -300 }, pt(2000, 0), { fallPct: 2, snapZ: -320 });
    expect(up.points).toEqual([v(0, 0, -300), v(2000, 0, -320)]);
    expect(segmentFallPct(up.points[0], up.points[1])).toBeCloseTo(1);
  });

  it("cleans points closer than 1 mm", () => {
    expect(cleanRun([v(0, 0, 0), v(0.3, 0, 0.2), v(10, 0, 0)])).toEqual([v(0, 0, 0), v(10, 0, 0)]);
  });
});

describe("the contract riser rule, as the exports apply it", () => {
  // Same cases as guhit-export pipes.rs a_sloped_drain_is_not_a_riser.
  it("is under 1 mm in plan, or at most 50 mm while rising ten times that", () => {
    expect(isVertical(v(0, 0, -300), v(4300, 0, -334))).toBe(false);
    expect(isVertical(v(0, 0, 0), v(0, 0.5, 900))).toBe(true);
    expect(isVertical(v(0, 0, 0), v(20, 0, 900))).toBe(true);
    expect(isVertical(v(0, 0, 0), v(80, 0, 900))).toBe(false);
    // Rising less than ten times its plan length is a sloped segment.
    expect(isVertical(v(0, 0, 0), v(40, 0, 300))).toBe(false);
    expect(segmentFallPct(v(0, 0, 0), v(20, 0, 900))).toBeNull();
  });

  // Same case as guhit-export pipes.rs plan_view_splits_runs_at_risers.
  it("splits plan runs at risers and puts one circle at each", () => {
    const plan = pipePlan([v(600, -1800, 0), v(600, -1800, -300), v(600, 5850, -300), v(2000, 5850, -300), v(2000, 6000, -300), v(2000, 6000, 300)]);
    expect(plan.risers.map((r) => r.point)).toEqual([pt(600, -1800), pt(2000, 6000)]);
    expect(plan.risers.map((r) => r.zTo > r.zFrom)).toEqual([false, true]);
    expect(plan.runs).toHaveLength(1);
    expect(plan.runs[0]).toHaveLength(4);
    // A pipe that only drops is a riser mark alone.
    const drop = pipePlan([v(6780, 5650, 20), v(6780, 5650, -430)]);
    expect(drop.runs).toHaveLength(0);
    expect(drop.risers).toHaveLength(1);
  });

  it("draws a riser that leans a little as a circle at its middle, never as a line", () => {
    const pts = [v(0, 0, 300), v(2000, 0, 300), v(2020, 0, 1200), v(2020, 1500, 1200)];
    const plan = pipePlan(pts);
    expect(plan.runs).toEqual([[pt(0, 0), pt(2000, 0)], [pt(2020, 0), pt(2020, 1500)]]);
    expect(plan.risers).toEqual([{ point: pt(2010, 0), zFrom: 300, zTo: 1200 }]);
    // It is one node for handles, snapping and height labels.
    const nodes = pipeNodes(pts);
    expect(nodes.map((n) => [n.index, n.count])).toEqual([[0, 1], [1, 2], [3, 1]]);
    expect(nodes[1].point).toEqual(pt(2010, 0));
    // Moving it moves both points by the same step: it stays a riser.
    expect(movePipeNode(pts, 1, 2, pt(2510, 100), null)).toEqual([v(0, 0, 300), v(2500, 100, 300), v(2520, 100, 1200), v(2020, 1500, 1200)]);
    // Picked inside its circle, even between the two points.
    expect(hitsPipe(pt(2010, 30), { diameter_mm: 20, points: pts }, 10, 60)).toBe(true);
  });

  it("joins a leaning riser on its centerline at the pen height", () => {
    const leaning = { id: "l", system: "cold_water" as const, points: [v(0, 0, 300), v(2000, 0, 300), v(2020, 0, 1200), v(2020, 1500, 1200)] };
    const r = snapToPipes(pt(2015, 10), { pipes: [leaning], fixtures: [] }, { tol: 60, system: "cold_water", penZ: 750, anchor: null, ortho: false });
    expect(r).toMatchObject({ kind: "pipe_joint", z: 750 });
    expect(r!.point.x).toBeCloseTo(2010);
    expect(r!.point.y).toBeCloseTo(0);
  });
});

describe("pipe snapping", () => {
  const main = pipe("main", "cold_water", [v(0, 0, 1200), v(3000, 0, 1200)]);
  const drain = pipe("drain", "drainage", [v(0, 2000, -300), v(3000, 2000, -360)], 100);
  const riser = pipe("riser", "cold_water", [v(0, 4000, 300), v(2000, 4000, 300), v(2000, 4000, 2400), v(2000, 6000, 2400)]);
  const scene: PipeSnapScene = {
    pipes: [main, drain, riser],
    fixtures: fixturePoints({ position: pt(5000, 0), rotation_deg: 0, depth_mm: 700, name: "Water closet" }),
  };
  const opt = (o: Partial<PipeSnapOptions> = {}): PipeSnapOptions => ({ tol: 60, system: "cold_water", penZ: 300, anchor: null, ortho: false, ...o });

  it("snaps to a pipe end and inherits its height", () => {
    const r = snapToPipes(pt(3020, 25), scene, opt());
    expect(r).toMatchObject({ kind: "pipe_end", point: pt(3000, 0), z: 1200, pipeId: "main" });
  });

  it("snaps to the middle of a segment of the same system as a tee, at that pipe's height", () => {
    const r = snapToPipes(pt(1500, 30), scene, opt());
    expect(r).toMatchObject({ kind: "tee", pipeId: "main", z: 1200 });
    expect(r!.point.x).toBeCloseTo(1500);
    expect(r!.point.y).toBeCloseTo(0);
    // On a falling drain the tee takes the height at that point.
    const t = snapToPipes(pt(1500, 1980), scene, opt({ system: "drainage" }));
    expect(t).toMatchObject({ kind: "tee", pipeId: "drain" });
    expect(t!.z).toBeCloseTo(-330);
  });

  it("joins a riser in the middle of a run at the pen height", () => {
    const r = snapToPipes(pt(2010, 4010), scene, opt({ penZ: 1500 }));
    expect(r).toMatchObject({ kind: "pipe_joint", z: 1500 });
    const low = snapToPipes(pt(2010, 4010), scene, opt({ penZ: -500 }));
    expect(low!.z).toBe(300);
  });

  it("only joins compatible systems: water to itself, drainage and vent together", () => {
    expect(canJoin("cold_water", "hot_water")).toBe(false);
    expect(canJoin("vent", "drainage")).toBe(true);
    // Cold water ignores the drain; a vent snaps to it.
    const cold = snapToPipes(pt(3010, 2010), scene, opt());
    expect(cold?.pipeId).not.toBe("drain");
    const vent = snapToPipes(pt(3010, 2010), scene, opt({ system: "vent" }));
    expect(vent).toMatchObject({ kind: "pipe_end", pipeId: "drain", z: -360 });
  });

  it("prefers pipe points over fixtures over tees", () => {
    const busy: PipeSnapScene = { pipes: [main], fixtures: [{ point: pt(3000, 0), label: "Sink, center" }, { point: pt(1500, 10), label: "Lavatory, back" }] };
    expect(snapToPipes(pt(3005, 5), busy, opt())?.kind).toBe("pipe_end");
    expect(snapToPipes(pt(1505, 5), busy, opt())).toMatchObject({ kind: "fixture", label: "Lavatory, back", z: null });
  });

  it("snaps to a fixture center and the middle of its back edge", () => {
    expect(snapToPipes(pt(5010, 10), scene, opt())).toMatchObject({ kind: "fixture", point: pt(5000, 0), label: "Water closet, center" });
    const back = snapToPipes(pt(5010, 340), scene, opt());
    expect(back?.label).toBe("Water closet, back");
    expect(back?.point.y).toBeCloseTo(350);
    const turned = fixturePoints({ position: pt(0, 0), rotation_deg: 90, depth_mm: 600, name: "Sink" });
    expect(turned[1].point.x).toBeCloseTo(-300);
    expect(turned[1].point.y).toBeCloseTo(0);
    expect(isPlumbingFixture({ category: "sanitary", catalog_key: "wc" })).toBe(true);
    expect(isPlumbingFixture({ category: "kitchen", catalog_key: "kitchen-sink" })).toBe(true);
    expect(isPlumbingFixture({ category: "kitchen", catalog_key: "kitchen-counter" })).toBe(false);
  });

  it("with ortho, a tee lies on the locked ray from the anchor", () => {
    // Drawing north from (1000, -2000): the ray crosses the main at x = 1000.
    const r = snapToPipes(pt(1040, -30), scene, opt({ anchor: pt(1000, -2000), ortho: true }));
    expect(r).toMatchObject({ kind: "tee", pipeId: "main" });
    expect(r!.point.x).toBeCloseTo(1000);
    expect(r!.point.y).toBeCloseTo(0);
  });

  it("a branch drawn nearly straight lands its tee on the straight line", () => {
    // From (1035, -1000) toward (1000, -20): 2 degrees off north, inside the soft lock.
    const r = snapToPipes(pt(1000, -20), scene, opt({ anchor: pt(1035, -1000) }));
    expect(r).toMatchObject({ kind: "tee", pipeId: "main" });
    expect(r!.point.x).toBeCloseTo(1035);
    // Off every locked angle, the tee is the nearest point on the pipe.
    const free = snapToPipes(pt(1000, -20), scene, opt({ anchor: pt(1500, -1000) }));
    expect(free!.point.x).toBeCloseTo(1000);
  });

  it("returns null out of reach", () => {
    expect(snapToPipes(pt(1500, 900), scene, opt())).toBeNull();
  });
});

describe("pipe nodes and edits", () => {
  it("moves a riser as one node so it stays vertical", () => {
    const pts = [v(0, 0, 300), v(2000, 0, 300), v(2000, 0, 1200), v(2000, 1500, 1200)];
    const moved = movePipeNode(pts, 1, 2, pt(2500, 0), null)!;
    expect(moved).toEqual([v(0, 0, 300), v(2500, 0, 300), v(2500, 0, 1200), v(2000, 1500, 1200)]);
  });

  it("takes a snapped height for a single point and merges a node dropped on its neighbour", () => {
    const pts = [v(0, 0, 300), v(1000, 0, 300), v(2000, 0, 300)];
    expect(movePipeNode(pts, 2, 1, pt(2000, 500), 900)).toEqual([v(0, 0, 300), v(1000, 0, 300), v(2000, 500, 900)]);
    expect(movePipeNode(pts, 1, 1, pt(2000, 0), null)).toEqual([v(0, 0, 300), v(2000, 0, 300)]);
    expect(movePipeNode([v(0, 0, 300), v(1000, 0, 300)], 1, 1, pt(0, 0), null)).toBeNull();
  });

  it("gives a selected pipe one grip per plan node", () => {
    const p = pipe("p", "cold_water", [v(0, 0, 300), v(2000, 0, 300), v(2000, 0, 1200), v(2000, 1500, 1200)]);
    const grips = gripsFor(p, 10);
    expect(grips.map((g) => [g.kind, g.index, g.count])).toEqual([
      ["pipe_node", 0, 1],
      ["pipe_node", 1, 2],
      ["pipe_node", 3, 1],
    ]);
  });
});

describe("pipe hit testing", () => {
  function docWith(pipes: PipeEl[], layers: Record<string, { visible?: boolean; locked?: boolean }> = {}): DocState {
    const d = structuredClone(fixture) as unknown as DocState;
    const keys = ["cold_water", "hot_water", "drainage", "vent"] as const;
    d.project.layers = [...d.project.layers, ...keys.map((key) => ({ key, visible: true, locked: false }))].map((l) => ({ ...l, ...(layers[l.key] ?? {}) }));
    d.project.elements = [...d.project.elements, ...(pipes as Element[])];
    return d;
  }
  // Inside the bungalow, clear of walls: rooms are picked by their label only.
  const cold = pipe("cold", "cold_water", [v(1000, 1000, 300), v(4000, 1000, 300)]);
  const drain = pipe("drain", "drainage", [v(1000, 2000, -300), v(4000, 2000, -360)], 100);
  const opt = { tol: 60, labelHeightMm: 120, pxMm: 10 };

  it("picks a pipe near its centerline and misses beside it", () => {
    const index = buildIndex(docWith([cold, drain]), null);
    expect(hitTest(pt(2500, 1015), index, opt)).toBe("cold");
    expect(hitTest(pt(2500, 1300), index, opt)).not.toBe("cold");
    // A 100 mm drain is picked across its drawn width.
    expect(hitTest(pt(2500, 2060), index, opt)).toBe("drain");
    expect(hitsPipe(pt(2500, 1300), cold, 10, 60)).toBe(false);
  });

  it("picks a riser inside its circle", () => {
    const r = pipe("r", "vent", [v(1000, 1000, 300), v(1000, 1000, 2700)]);
    expect(hitsPipe(pt(1030, 1030), r, 10, 60)).toBe(true);
    expect(hitsPipe(pt(1200, 1000), r, 10, 60)).toBe(false);
  });

  it("hidden pipe layers are not drawn or picked, locked ones are drawn but not picked", () => {
    const hidden = buildIndex(docWith([cold, drain], { cold_water: { visible: false } }), null);
    expect(hidden.visibleIds.has("cold")).toBe(false);
    expect(hidden.visibleIds.has("drain")).toBe(true);
    expect(hitTest(pt(2500, 1015), hidden, opt)).not.toBe("cold");
    const locked = buildIndex(docWith([cold, drain], { drainage: { locked: true } }), null);
    expect(locked.visibleIds.has("drain")).toBe(true);
    expect(isLocked(drain, locked)).toBe(true);
    expect(hitTest(pt(2500, 2010), locked, opt)).not.toBe("drain");
    expect(marqueeSelect(rectFromPoints(pt(500, 1500), pt(4500, 2500)), false, locked, opt)).not.toContain("drain");
  });

  it("selects pipes with a window or a crossing marquee", () => {
    const index = buildIndex(docWith([cold, drain]), null);
    const inside = marqueeSelect(rectFromPoints(pt(900, 900), pt(4100, 1100)), false, index, opt);
    expect(inside).toContain("cold");
    expect(inside).not.toContain("drain");
    const crossing = marqueeSelect(rectFromPoints(pt(2400, 800), pt(2600, 2200)), true, index, opt);
    expect(crossing).toEqual(expect.arrayContaining(["cold", "drain"]));
  });
});

describe("service runs", () => {
  it("falls only for drainage, storm and condensate", () => {
    expect(toolFallPct({ system: "storm", diameterMm: 100 })).toBe(1);
    expect(toolFallPct({ system: "condensate", diameterMm: 20 })).toBe(2);
    for (const system of ["cold_water", "hot_water", "vent", "conduit", "refrigerant"] as const) {
      expect(toolFallPct({ system, diameterMm: 20 })).toBeNull();
    }
  });

  it("joins within a system, and drainage with vent, nothing else", () => {
    expect(canJoin("conduit", "conduit")).toBe(true);
    expect(canJoin("refrigerant", "refrigerant")).toBe(true);
    expect(canJoin("condensate", "drainage")).toBe(false);
    expect(canJoin("storm", "drainage")).toBe(false);
    expect(canJoin("refrigerant", "condensate")).toBe(false);
    expect(canJoin("conduit", "cold_water")).toBe(false);
    // A conduit run never snaps onto a water pipe.
    const water = pipe("w", "cold_water", [v(0, 0, 2800), v(2000, 0, 2800)]);
    const scene: PipeSnapScene = { pipes: [water], fixtures: [] };
    expect(snapToPipes(pt(1000, 5), scene, { tol: 50, system: "conduit", penZ: 2800, anchor: null, ortho: false })).toBeNull();
  });

  it("starts runs at the objects of their trade", () => {
    const acu = { category: "aircon" as const, catalog_key: "aircon-indoor-1hp" };
    expect(isServiceFixture(acu, "refrigerant", "aircon_indoor")).toBe(true);
    expect(isServiceFixture(acu, "condensate", "aircon_indoor")).toBe(true);
    expect(isServiceFixture({ category: "aircon", catalog_key: "aircon-outdoor-1hp" }, "condensate", "aircon_outdoor")).toBe(false);
    expect(isServiceFixture({ category: "sanitary", catalog_key: "floor-drain" }, "condensate", null)).toBe(true);
    expect(isServiceFixture({ category: "electrical", catalog_key: "switch-1" }, "conduit", "switch")).toBe(true);
    expect(isServiceFixture({ category: "lighting", catalog_key: "light-ceiling" }, "conduit", "lighting_outlet")).toBe(true);
    expect(isServiceFixture({ category: "sanitary", catalog_key: "wc" }, "conduit", null)).toBe(false);
    expect(isServiceFixture({ category: "sanitary", catalog_key: "wc" }, "drainage", null)).toBe(true);
    expect(isServiceFixture({ category: "sanitary", catalog_key: "wc" }, "storm", null)).toBe(false);
  });

  it("draws a line set as two lines at plan scale that never merge", () => {
    // 9.52 gas and 6.35 liquid, each in 10 mm of foam: about 28 mm apart.
    expect(lineSetSpacingMm(9.52)).toBeCloseTo(27.935, 3);
    const near = lineSetPx(9.52, 1);
    expect(near.sep).toBeCloseTo(27.935, 3);
    expect(near.gasW).toBeCloseTo(9.52);
    expect(near.liquidW).toBeCloseTo(6.35);
    const far = lineSetPx(9.52, 0.05);
    expect(far.sep).toBe(4.5);
    expect(far.gasW).toBeGreaterThan(far.liquidW);
    // Picked across both lines.
    const ls = pipe("ls", "refrigerant", [v(0, 0, 2400), v(3000, 0, 2400)], 9.52);
    expect(hitsPipe(pt(1500, 20), ls, 1, 6)).toBe(true);
    expect(hitsPipe(pt(1500, 20), { ...ls, system: "cold_water" }, 1, 6)).toBe(false);
  });
});

describe("readouts", () => {
  it("formats heights with their sign and falls in percent", () => {
    expect(formatHeight(300, "mm")).toBe("+300");
    expect(formatHeight(-300, "mm")).toBe("-300");
    expect(formatHeight(0.2, "mm")).toBe("0");
    expect(formatHeight(1500, "m", true)).toBe("+1.50 m");
    expect(formatPct(2)).toBe("2%");
    expect(formatPct(1.46)).toBe("1.5%");
  });
});
