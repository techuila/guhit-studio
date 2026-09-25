import { describe, expect, it } from "vitest";
import type { Element, Level } from "../contract/bindings";
import { ADD_LEVEL_ABOVE, addedLevel, deleteLevelCommand, deleteLevelQuestion, elementsOnLevel, elevationLabel, levelsTopDown } from "./levels";

const level = (id: string, elevation_mm: number, name = id): Level => ({ id, name, elevation_mm, height_mm: 3000 });

const wall = (id: string, level_id: string): Element =>
  ({ kind: "wall", id, level_id, start: { x: 0, y: 0 }, end: { x: 1000, y: 0 }, thickness_mm: 150, height_mm: null, material_id: null }) as Element;

describe("levels", () => {
  it("counts what goes with a level: its elements and the openings in its walls", () => {
    const elements: Element[] = [
      wall("w1", "g"),
      wall("w2", "u"),
      { kind: "opening", id: "d1", wall_id: "w2", opening_type: "door", style: "swing_single", offset_mm: 500, width_mm: 800, height_mm: 2100, sill_mm: 0, flip_side: false, flip_hinge: false, material_id: null },
      { kind: "room", id: "r1", level_id: "u", name: "Bedroom", usage: "bedroom", seed: { x: 0, y: 0 }, floor_material_id: null, auto_named: false },
      { kind: "camera", id: "c1", name: "View", preset: "custom", position: { x: 0, y: 0, z: 0 }, target: { x: 1, y: 0, z: 0 }, fov_deg: 60, light: null },
    ];
    expect(elementsOnLevel(elements, "u")).toBe(3);
    expect(elementsOnLevel(elements, "g")).toBe(1);
    expect(elementsOnLevel(elements, "x")).toBe(0);
  });

  it("lists the top floor first", () => {
    expect(levelsTopDown([level("g", 0), level("u", 3000), level("b", -2800)]).map((l) => l.id)).toEqual(["u", "g", "b"]);
  });

  it("finds the level a command added", () => {
    expect(addedLevel([level("g", 0)], [level("g", 0), level("n", 3000)])?.id).toBe("n");
    expect(addedLevel([level("g", 0)], [level("g", 0)])).toBeNull();
  });

  it("asks before deleting, with the element count", () => {
    expect(deleteLevelQuestion({ name: "Level 2" }, 12)).toBe("Delete Level 2 and the 12 elements on it?");
    expect(deleteLevelQuestion({ name: "Level 2" }, 1)).toBe("Delete Level 2 and the 1 element on it?");
    expect(deleteLevelQuestion({ name: "Roof deck" }, 0)).toBe("Delete Roof deck? Nothing is drawn on it.");
  });

  it("writes floor elevations as sections mark them", () => {
    expect(elevationLabel(3000)).toBe("+3.00 m");
    expect(elevationLabel(0)).toBe("0.00 m");
    expect(elevationLabel(-450)).toBe("-0.45 m");
  });

  it("builds the level commands of the contract", () => {
    expect(ADD_LEVEL_ABOVE).toEqual({ type: "add_level", name: null, elevation_mm: null, height_mm: null });
    expect(deleteLevelCommand("u")).toEqual({ type: "delete_level", level_id: "u" });
  });
});
