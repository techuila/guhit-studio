import { describe, expect, it } from "vitest";
import type { Footprint } from "../../contract/bindings";
import { groupFootprints } from "./buildScene";

const rect = (x0: number, y0: number, x1: number, y1: number) => [
  { x: x0, y: y0 },
  { x: x1, y: y0 },
  { x: x1, y: y1 },
  { x: x0, y: y1 },
];

describe("groupFootprints", () => {
  it("returns every detached building on the level, not just the first", () => {
    const footprints: Footprint[] = [
      { level_id: "L1", polygon: rect(0, 0, 10000, 8000), area_mm2: 80_000_000 },
      { level_id: "L1", polygon: rect(20000, 0, 25000, 4000), area_mm2: 20_000_000 },
    ];
    const groups = groupFootprints(footprints, "L1");
    expect(groups).toHaveLength(2);
    expect(groups[0]).toHaveLength(4);
    expect(groups[1]).toHaveLength(4);
  });

  it("keeps largest-first order as given by Derived", () => {
    const footprints: Footprint[] = [
      { level_id: "L1", polygon: rect(0, 0, 10000, 8000), area_mm2: 80_000_000 },
      { level_id: "L1", polygon: rect(20000, 0, 25000, 4000), area_mm2: 20_000_000 },
    ];
    const groups = groupFootprints(footprints, "L1");
    // The larger rect (10000x8000) stays first, matching input order.
    expect(Math.max(...groups[0].map((p) => p.x))).toBe(10000);
    expect(Math.max(...groups[1].map((p) => p.x))).toBe(25000);
  });

  it("drops the empty-polygon entry used when a level has no closed building", () => {
    const footprints: Footprint[] = [{ level_id: "L1", polygon: [], area_mm2: 0 }];
    expect(groupFootprints(footprints, "L1")).toEqual([]);
  });

  it("ignores footprints that belong to a different level", () => {
    const footprints: Footprint[] = [
      { level_id: "L1", polygon: rect(0, 0, 5000, 5000), area_mm2: 25_000_000 },
      { level_id: "L2", polygon: rect(0, 0, 6000, 6000), area_mm2: 36_000_000 },
    ];
    expect(groupFootprints(footprints, "L1")).toHaveLength(1);
    expect(groupFootprints(footprints, "L2")).toHaveLength(1);
    expect(groupFootprints(footprints, "L3")).toEqual([]);
  });

  it("fixes clockwise winding to counter-clockwise", () => {
    const cw = [...rect(0, 0, 4000, 3000)].reverse();
    const footprints: Footprint[] = [{ level_id: "L1", polygon: cw, area_mm2: 12_000_000 }];
    const [poly] = groupFootprints(footprints, "L1");
    // Shoelace signed area is positive for a CCW polygon.
    let area = 0;
    for (let i = 0; i < poly.length; i++) {
      const a = poly[i];
      const b = poly[(i + 1) % poly.length];
      area += a.x * b.y - b.x * a.y;
    }
    expect(area).toBeGreaterThan(0);
  });

  it("handles undefined or missing footprints gracefully", () => {
    expect(groupFootprints(undefined, "L1")).toEqual([]);
    expect(groupFootprints(null, "L1")).toEqual([]);
  });
});
