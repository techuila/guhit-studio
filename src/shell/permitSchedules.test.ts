import { describe, expect, it } from "vitest";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import type { DocState } from "../contract/bindings";
import { buildPermitSchedules, permitSchedulesCsv } from "./permitSchedules";
import { typeCounts, untypedCount } from "./TypesPanel";

const base = () => structuredClone(fixture) as unknown as DocState;

function counted(): DocState {
  const d = base();
  const lv = d.project.levels[0].id;
  const wallMat = d.project.materials[0];
  d.project.opening_types = [
    { id: "t1", mark: "D1", opening_type: "door", style: "swing_single", width_mm: 900, height_mm: 2100, description: "Solid wood panel door", remarks: "" },
  ];
  d.derived.opening_schedule = {
    rows: [
      { mark: "D1", type_id: "t1", opening_type: "door", style: "swing_single", width_mm: 900, height_mm: 2100, sill_mm: 0, material_id: null, description: "", per_level: [{ level_id: lv, count: 2 }], total: 2 },
      { mark: "", type_id: null, opening_type: "window", style: "sliding", width_mm: 1500, height_mm: 1200, sill_mm: 900, material_id: null, description: "", per_level: [{ level_id: lv, count: 3 }], total: 3 },
    ],
    marks: [],
  };
  d.derived.room_schedule = [
    {
      room_id: "r1",
      level_id: lv,
      name: "Living",
      usage: "living",
      net_area_m2: 32.5,
      floor_area_m2: 30.5,
      perimeter_m: 22,
      ceiling_height_mm: 2700,
      floor_material_id: null,
      finishes: { wall_material_id: wallMat.id, ceiling_material_id: null, base_material_id: null, remarks: "Skim coat" },
    },
  ];
  d.derived.level_areas = [{ level_id: lv, net_m2: 45.3375, gross_m2: 50.1225, open_m2: 2, void_m2: 1 }];
  return d;
}

describe("permit schedules", () => {
  it("read every number from Derived, totals from Derived::totals", () => {
    const d = counted();
    const s = buildPermitSchedules(d);
    expect(s.any).toBe(true);
    expect(s.counted).toBe(true);
    expect(s.openings.map((o) => [o.mark, o.size, o.what, o.total, o.perLevel])).toEqual([
      ["D1", "900 x 2100", "Solid wood panel door", 2, [2]],
      ["", "1500 x 1200", "Sliding", 3, [3]],
    ]);
    expect(s.totals).toMatchObject({ doors: 2, windows: 3, floorM2: d.derived.totals.floor_area_m2, grossM2: d.derived.totals.gross_area_m2, openM2: 2, voidM2: 1 });
    expect(s.rooms[0]).toMatchObject({ name: "Living", netM2: 30.5, ceilingMm: 2700, remarks: "Skim coat" });
    expect(s.rooms[0].finishes).toBe(`Walls ${d.project.materials[0].name}`);
    expect(s.levelAreas[0]).toMatchObject({ net: 45.3375, gross: 50.1225 });
  });

  it("says when the engine has not counted yet", () => {
    const d = base();
    d.derived.opening_schedule = { rows: [], marks: [] };
    d.derived.room_schedule = [];
    d.derived.level_areas = [];
    const s = buildPermitSchedules(d);
    expect(s.any).toBe(true);
    expect(s.counted).toBe(false);
  });

  it("reads the regenerated fixture: rows that add up to the project totals", () => {
    const d = base();
    const s = buildPermitSchedules(d);
    expect(s.counted).toBe(true);
    const doors = s.openings.filter((o) => o.kind === "door").reduce((n, o) => n + o.total, 0);
    const windows = s.openings.filter((o) => o.kind === "window").reduce((n, o) => n + o.total, 0);
    expect([doors, windows]).toEqual([s.totals.doors, s.totals.windows]);
    expect(s.rooms.reduce((a, r) => a + r.netM2, 0)).toBeCloseTo(s.totals.floorM2, 6);
    expect(s.levelAreas.reduce((a, l) => a + l.gross, 0)).toBeCloseTo(s.totals.grossM2, 6);
  });

  it("writes CSV with the project totals", () => {
    const csv = permitSchedulesCsv(buildPermitSchedules(counted()));
    expect(csv).toContain("D1,door,900 x 2100,Solid wood panel door,2,2");
    expect(csv).toContain("All doors,,,,,2");
    expect(csv).toContain("Floor area,,45.34");
    expect(csv).toContain("Total,45.34,50.12,2.00,1.00");
  });

  it("counts types and untyped openings from the schedule", () => {
    const d = counted();
    expect(typeCounts(d).get("t1")).toBe(2);
    expect(untypedCount(d)).toBe(3);
    const e = base();
    e.derived.opening_schedule = { rows: [], marks: [] };
    expect(typeCounts(e).size).toBe(0);
    expect(untypedCount(e)).toBe(0);
  });
});
