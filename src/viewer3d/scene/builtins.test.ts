import { describe, expect, it } from "vitest";
import catalogSource from "../../../crates/guhit-model/src/catalog.rs?raw";
import { BUILT_IN_KIND, builtInDefaults, builtInParams, isBuiltInKey } from "./builtins";

describe("built-in options", () => {
  it("knows exactly the catalog's built-ins", () => {
    const keys = catalogSource
      .split("\n")
      .filter((l) => l.includes(".built(B::"))
      .map((l) => l.match(/item\("([a-z0-9-]+)"/)?.[1])
      .filter((k): k is string => !!k);
    expect(keys.sort()).toEqual(Object.keys(BUILT_IN_KIND).sort());
    expect(isBuiltInKey("kitchen-run")).toBe(true);
    expect(isBuiltInKey("sofa-3")).toBe(false);
  });

  it("follows the engine's defaults rule for rule", () => {
    // The same table is in crates/guhit-core/tests/l5_placement.rs.
    const cases: [Parameters<typeof builtInDefaults>[0], number, number | null, number | null, number, boolean][] = [
      ["kitchen_run", 2400, 700, 1800, 5, true],
      ["kitchen_run", 1800, 550, 1450, 4, true],
      ["kitchen_run", 1500, 450, null, 3, true],
      ["kitchen_run", 600, 300, null, 1, true],
      ["kitchen_run", 3600, 1100, 2700, 8, true],
      ["vanity", 900, 450, null, 2, true],
      ["counter", 1500, null, null, 3, false],
      ["wardrobe", 1800, null, null, 4, false],
      ["wardrobe", 600, null, null, 2, false],
      ["shelf", 1200, null, null, 0, false],
    ];
    for (const [kind, w, sink, cook, doors, upper] of cases) {
      const d = builtInDefaults(kind, w);
      expect([d.sink_at_mm, d.cooktop_at_mm, d.doors, d.upper], `${kind} ${w}`).toEqual([sink, cook, doors, upper]);
    }
  });

  it("uses an asset's own options over the defaults", () => {
    const own = { upper: false, sink_at_mm: 1200, cooktop_at_mm: null, doors: 3 };
    expect(builtInParams("kitchen-run", 2400, own)).toBe(own);
    expect(builtInParams("kitchen-run", 2400, null)?.sink_at_mm).toBe(700);
    expect(builtInParams("sofa-3", 2100, null)).toBeNull();
  });
});
