import { describe, expect, it } from "vitest";
import { pt } from "../geom";
import { gridSystemOnWalls } from "./gridSystem";

const wall = (ax: number, ay: number, bx: number, by: number) => ({ start: pt(ax, ay), end: pt(bx, by) });

describe("grid system on the walls", () => {
  it("puts lines on the wall centerlines both ways", () => {
    // The sample bungalow: 8 by 6 m with a wall at x 5000.
    const walls = [wall(0, 0, 8000, 0), wall(8000, 0, 8000, 6000), wall(8000, 6000, 0, 6000), wall(0, 6000, 0, 0), wall(5000, 0, 5000, 6000)];
    expect(gridSystemOnWalls(walls)).toEqual({ type: "add_grid_system", origin: pt(0, 0), rotation_deg: 0, x_spacings_mm: [5000, 3000], y_spacings_mm: [6000] });
  });

  it("merges lines closer than 100 mm and skips slanted and short walls", () => {
    const walls = [wall(0, 0, 0, 3000), wall(40, 3000, 40, 6000), wall(4000, 0, 4000, 6000), wall(0, 0, 3000, 3000), wall(9000, 0, 9000, 50)];
    expect(gridSystemOnWalls(walls)).toEqual({ type: "add_grid_system", origin: pt(20, 0), rotation_deg: 0, x_spacings_mm: [3980], y_spacings_mm: [] });
  });

  it("gives nothing without two lines either way", () => {
    expect(gridSystemOnWalls([])).toBeNull();
    expect(gridSystemOnWalls([wall(0, 0, 0, 3000), wall(0, 0, 3000, 0)])).toBeNull();
  });
});
