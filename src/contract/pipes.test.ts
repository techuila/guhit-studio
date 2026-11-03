import { describe, expect, it } from "vitest";
import { ceilingHeightMm } from "../editor2d/mount";
import { CONDUIT_ABOVE_CEILING_MM, PIPE_DEFAULTS, pipeStartHeightMm } from "./pipes";

describe("pipeStartHeightMm (DECISIONS D40)", () => {
  const ground = { elevation_mm: 0, height_mm: 3000 };
  const upper = { elevation_mm: 3000, height_mm: 2800 };

  it("starts conduit 100 mm above the ceiling: in the slab above, or over the top ceiling", () => {
    expect(pipeStartHeightMm("conduit", ground, [ground])).toBe(3100);
    expect(pipeStartHeightMm("conduit", ground, [ground, upper])).toBe(2900);
    expect(pipeStartHeightMm("conduit", upper, [ground, upper])).toBe(2900);
    expect(pipeStartHeightMm("conduit", undefined, [])).toBe(PIPE_DEFAULTS.conduit.startHeightMm);
  });

  it("follows the ceiling rule of the placement tools", () => {
    const cases = [
      { level: ground, levels: [ground] },
      { level: ground, levels: [ground, upper] },
      { level: ground, levels: [ground, { elevation_mm: 3200, height_mm: 2800 }] },
      { level: upper, levels: [ground, upper] },
    ];
    for (const c of cases) {
      expect(pipeStartHeightMm("conduit", c.level, c.levels)).toBe(ceilingHeightMm(c.level, c.levels) + CONDUIT_ABOVE_CEILING_MM);
    }
  });

  it("keeps the table height for every other system", () => {
    expect(pipeStartHeightMm("cold_water", ground, [ground, upper])).toBe(300);
    expect(pipeStartHeightMm("drainage", ground, [ground, upper])).toBe(-300);
    expect(pipeStartHeightMm("refrigerant", upper, [ground, upper])).toBe(2400);
  });
});
