import { describe, expect, it } from "vitest";
import type { Issue, Pipe, PipeNetwork } from "../contract/bindings";
import {
  PIPE_SIZES,
  closestSize,
  drainMinSlopePct,
  fittingsLine,
  inMenu,
  isPipeIssue,
  isPipeLayer,
  lengthBySystem,
  midPoint,
  orderIssues,
  pipeDefaults,
  pipeLength,
  pipeToolSettings,
  reversed,
  segmentFalls,
  switchToolSystem,
  takeoffCsv,
  withMaterial,
  withSystem,
} from "./pipes";

const pipe = (patch: Partial<Pipe> = {}): Pipe => ({
  id: "p1",
  level_id: "l1",
  system: "cold_water",
  material: "ppr",
  diameter_mm: 20,
  points: [
    { x: 0, y: 0, z: 300 },
    { x: 1000, y: 0, z: 300 },
  ],
  name: "",
  ...patch,
});

const issue = (id: string, severity: Issue["severity"], code = "room_no_window"): Issue => ({ id, severity, code, message: id, element_ids: [], location: null });

describe("pipe defaults mirror docs/CONTRACT.md", () => {
  it("has the tool defaults per system", () => {
    expect(pipeDefaults("cold_water")).toEqual({ material: "ppr", diameterMm: 20, elevationMm: 300 });
    expect(pipeDefaults("hot_water")).toEqual({ material: "ppr", diameterMm: 20, elevationMm: 300 });
    expect(pipeDefaults("drainage")).toEqual({ material: "upvc", diameterMm: 50, elevationMm: -300 });
    expect(pipeDefaults("vent")).toEqual({ material: "upvc", diameterMm: 50, elevationMm: 300 });
  });

  it("has the size menus", () => {
    expect(PIPE_SIZES.cold_water).toEqual([
      { material: "ppr", sizes: [20, 25, 32, 40, 50, 63] },
      { material: "gi", sizes: [15, 20, 25, 32, 50] },
      { material: "pe", sizes: [20, 25, 32] },
    ]);
    expect(PIPE_SIZES.hot_water).toEqual([
      { material: "ppr", sizes: [20, 25, 32] },
      { material: "copper", sizes: [15, 22, 28] },
    ]);
    expect(PIPE_SIZES.drainage).toEqual([{ material: "upvc", sizes: [32, 50, 75, 100, 150] }]);
    expect(PIPE_SIZES.vent).toEqual([{ material: "upvc", sizes: [32, 50, 75, 100] }]);
  });

  it("every default is on its own menu", () => {
    for (const system of ["cold_water", "hot_water", "drainage", "vent"] as const) {
      const d = pipeDefaults(system);
      expect(inMenu(system, d.material, d.diameterMm)).toBe(true);
    }
  });

  it("falls 2 percent, 1 percent from 100 mm", () => {
    expect(drainMinSlopePct(50)).toBe(2);
    expect(drainMinSlopePct(75)).toBe(2);
    expect(drainMinSlopePct(100)).toBe(1);
    expect(drainMinSlopePct(150)).toBe(1);
  });

  it("knows the pipe layers", () => {
    expect(isPipeLayer("drainage")).toBe(true);
    expect(isPipeLayer("vent")).toBe(true);
    expect(isPipeLayer("walls")).toBe(false);
  });
});

describe("pipe tool options", () => {
  const none = { pipeSystem: "cold_water" as const, pipeMaterial: null, pipeDiameterMm: null, pipeElevationMm: null };

  it("uses the system defaults while the options are null", () => {
    expect(pipeToolSettings({ ...none, pipeSystem: "drainage" })).toEqual({
      system: "drainage",
      material: "upvc",
      diameterMm: 50,
      elevationMm: -300,
      sizeIsDefault: true,
      elevationIsDefault: true,
    });
  });

  it("uses chosen values", () => {
    const s = pipeToolSettings({ ...none, pipeMaterial: "gi", pipeDiameterMm: 25, pipeElevationMm: 2400 });
    expect(s).toMatchObject({ material: "gi", diameterMm: 25, elevationMm: 2400, sizeIsDefault: false, elevationIsDefault: false });
  });

  it("falls back to the default when a size is not on the system's menu", () => {
    const s = pipeToolSettings({ ...none, pipeSystem: "drainage", pipeMaterial: "ppr", pipeDiameterMm: 20 });
    expect(s).toMatchObject({ material: "upvc", diameterMm: 50 });
  });

  it("keeps a size the next system also has, and the start height", () => {
    const next = switchToolSystem({ ...none, pipeMaterial: "ppr", pipeDiameterMm: 25, pipeElevationMm: 2400 }, "hot_water");
    expect(next).toEqual({ pipeSystem: "hot_water", pipeMaterial: "ppr", pipeDiameterMm: 25, pipeElevationMm: 2400 });
  });

  it("drops a size the next system does not have", () => {
    const next = switchToolSystem({ ...none, pipeMaterial: "ppr", pipeDiameterMm: 63 }, "drainage");
    expect(next).toEqual({ pipeSystem: "drainage", pipeMaterial: null, pipeDiameterMm: null, pipeElevationMm: null });
  });
});

describe("editing one pipe", () => {
  it("keeps material and size when the new system has them", () => {
    expect(withSystem(pipe({ diameter_mm: 25 }), "hot_water")).toMatchObject({ system: "hot_water", material: "ppr", diameter_mm: 25 });
  });

  it("takes the nearest size of the same material", () => {
    expect(withSystem(pipe({ diameter_mm: 63 }), "hot_water")).toMatchObject({ system: "hot_water", material: "ppr", diameter_mm: 32 });
  });

  it("takes the new system's defaults for another material", () => {
    expect(withSystem(pipe(), "drainage")).toMatchObject({ system: "drainage", material: "upvc", diameter_mm: 50 });
  });

  it("keeps the element kind through an edit", () => {
    const el = { kind: "pipe" as const, ...pipe() };
    expect(withSystem(el, "vent").kind).toBe("pipe");
    expect(withMaterial(el, "gi").kind).toBe("pipe");
    expect(reversed(el).kind).toBe("pipe");
  });

  it("picks the nearest size for another material", () => {
    expect(withMaterial(pipe({ diameter_mm: 40 }), "gi").diameter_mm).toBe(32);
    expect(withMaterial(pipe({ diameter_mm: 63 }), "pe").diameter_mm).toBe(32);
    expect(closestSize([15, 22, 28], 20)).toBe(22);
    expect(closestSize([20, 30], 25)).toBe(20);
  });

  it("reverses the points, not the original array", () => {
    const p = pipe({ points: [{ x: 0, y: 0, z: 0 }, { x: 1, y: 0, z: 0 }, { x: 2, y: 0, z: 0 }] });
    const r = reversed(p);
    expect(r.points.map((v) => v.x)).toEqual([2, 1, 0]);
    expect(p.points.map((v) => v.x)).toEqual([0, 1, 2]);
  });

  it("measures the centerline in 3D and finds its middle", () => {
    const points = [
      { x: 0, y: 0, z: 0 },
      { x: 3000, y: 0, z: 0 },
      { x: 3000, y: 0, z: 1000 },
    ];
    expect(pipeLength(points)).toBe(4000);
    expect(midPoint(points)).toEqual({ x: 2000, y: 0, z: 0 });
    expect(midPoint([])).toBeNull();
  });
});

describe("drainage falls", () => {
  const drain = (points: Pipe["points"], diameter_mm = 50) => pipe({ system: "drainage", material: "upvc", diameter_mm, points });

  it("reads the fall of each segment in the flow direction", () => {
    const falls = segmentFalls(
      drain([
        { x: 0, y: 0, z: -300 },
        { x: 2000, y: 0, z: -340 },
        { x: 4000, y: 0, z: -350 },
      ]),
    );
    expect(falls).toHaveLength(2);
    expect(falls[0].pct).toBeCloseTo(2, 9);
    expect(falls[0].low).toBe(false);
    expect(falls[1].pct).toBeCloseTo(0.5, 9);
    expect(falls[1].low).toBe(true);
  });

  it("flags uphill and level runs", () => {
    const [up, level] = segmentFalls(
      drain([
        { x: 0, y: 0, z: -300 },
        { x: 1000, y: 0, z: -290 },
        { x: 2000, y: 0, z: -290 },
      ]),
    );
    expect(up.dropMm).toBe(-10);
    expect(up.low).toBe(true);
    expect(level.pct).toBe(0);
    expect(level.low).toBe(true);
  });

  it("does not judge short, steep or vertical segments", () => {
    const [short, steep, vertical] = segmentFalls(
      drain([
        { x: 0, y: 0, z: 0 },
        { x: 200, y: 0, z: 0 },
        { x: 400, y: 0, z: -500 },
        { x: 400, y: 0, z: -1500 },
      ]),
    );
    expect(short.low).toBe(false);
    expect(steep.steep).toBe(true);
    expect(steep.low).toBe(false);
    expect(vertical.pct).toBeNull();
    expect(vertical.low).toBe(false);
  });

  it("uses 1 percent from 100 mm", () => {
    const [f] = segmentFalls(
      drain(
        [
          { x: 0, y: 0, z: 0 },
          { x: 1000, y: 0, z: -12 },
        ],
        100,
      ),
    );
    expect(f.low).toBe(false);
  });
});

describe("review order", () => {
  it("puts errors, then warnings, then info first, keeping the engine order in each", () => {
    const { items, notes } = orderIssues([issue("a", "info"), issue("b", "warning"), issue("c", "info"), issue("d", "error"), issue("e", "warning")]);
    expect(items.map((i) => i.id)).toEqual(["d", "b", "e", "a", "c"]);
    expect(notes).toEqual([]);
  });

  it("moves the penetration summary to the notes", () => {
    const { items, notes } = orderIssues([issue("s", "info", "pipe_penetrations"), issue("w", "warning", "pipes_cross")]);
    expect(items.map((i) => i.id)).toEqual(["w"]);
    expect(notes.map((i) => i.id)).toEqual(["s"]);
    expect(isPipeIssue(items[0])).toBe(true);
    expect(isPipeIssue(issue("x", "warning"))).toBe(false);
  });
});

describe("take-off", () => {
  const net: PipeNetwork = {
    fittings: [],
    penetrations: [],
    takeoff: [
      { system: "cold_water", material: "ppr", diameter_mm: 20, length_m: 12.345, run_count: 3 },
      { system: "cold_water", material: "gi", diameter_mm: 25, length_m: 2, run_count: 1 },
      { system: "drainage", material: "upvc", diameter_mm: 100, length_m: 6.5, run_count: 2 },
    ],
    total_length_m: 20.845,
    elbow_count: 1,
    tee_count: 2,
    sleeve_count: 5,
  };

  it("sums length per system", () => {
    const by = lengthBySystem(net.takeoff);
    expect(by.cold_water).toBeCloseTo(14.345, 9);
    expect(by.drainage).toBe(6.5);
    expect(by.vent).toBe(0);
  });

  it("writes the counts in words", () => {
    expect(fittingsLine(net)).toBe("1 elbow, 2 tees, 5 sleeves or flashings");
  });

  it("writes CSV rows, totals, fittings and the note", () => {
    const lines = takeoffCsv(net).split("\r\n");
    expect(lines[0]).toBe("System,Material,Size (mm),Length (m),Runs");
    expect(lines[1]).toBe("Cold water,PPR,20,12.345,3");
    expect(lines[2]).toBe("Cold water,GI,25,2.000,1");
    expect(lines[3]).toBe("Drainage,uPVC,100,6.500,2");
    expect(lines[4]).toBe("All pipe,,,20.845,6");
    expect(lines).toContain("Elbows,1");
    expect(lines).toContain("Tees,2");
    expect(lines).toContain("Sleeves or flashings,5");
    expect(lines).toContain("Centerline lengths from the model. Sizing is for a registered Master Plumber.");
    expect(takeoffCsv(net).endsWith("\r\n")).toBe(true);
  });
});
