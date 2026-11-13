import { describe, expect, it } from "vitest";
import type { Element } from "./bindings";
import { elementLevel, isView } from "./elements";

const el = (e: Record<string, unknown>) => e as unknown as Element;

const wall = el({ kind: "wall", id: "w1", level_id: "g", start: { x: 0, y: 0 }, end: { x: 1000, y: 0 }, thickness_mm: 150, height_mm: null, material_id: null });
const opening = el({ kind: "opening", id: "d1", wall_id: "w1", opening_type: "door", style: "swing_single", offset_mm: 500, width_mm: 800, height_mm: 2100, sill_mm: 0, flip_side: false, flip_hinge: false, material_id: null });
const camera = el({ kind: "camera", id: "c1", name: "View", preset: "custom", position: { x: 0, y: 0, z: 0 }, target: { x: 1, y: 0, z: 0 }, fov_deg: 60, light: null });
const tour = el({ kind: "tour", id: "t1", name: "Walkthrough", shots: [], fps: 30 });
const grid = el({ kind: "grid", id: "g1", label: "A", start: { x: 0, y: 0 }, end: { x: 0, y: 9000 } });
const lot = el({ kind: "lot", id: "l1", corners: [], sides: [], grade_mm: 0 });
const mark = el({ kind: "view_mark", id: "m1", level_id: "g", mark_kind: "section", label: "A", start: { x: 0, y: 0 }, end: { x: 1, y: 0 }, flip: false, depth_mm: null });
const roof = (level_id: string | null, type: string) => el({ kind: "roof", id: "r1", level_id, outline: { type }, shape: "gable", pitch_deg: 25, ridge_axis: "x", overhang_mm: 600, thickness_mm: 100, material_id: null });

describe("elementLevel (Element::level_id)", () => {
  it("gives the element's own level", () => {
    expect(elementLevel(wall)).toBe("g");
    expect(elementLevel(mark)).toBe("g");
    expect(elementLevel(roof("g", "footprint"))).toBe("g");
  });

  it("gives no level to openings, views, grids, the lot and the roof over the top floor", () => {
    for (const e of [opening, camera, tour, grid, lot, roof(null, "top_footprint")]) expect(elementLevel(e)).toBeNull();
  });
});

describe("isView (Element::is_view)", () => {
  it("is true for cameras and tours only", () => {
    expect(isView(camera)).toBe(true);
    expect(isView(tour)).toBe(true);
    for (const e of [wall, opening, grid, lot, mark, roof(null, "top_footprint")]) expect(isView(e)).toBe(false);
  });
});
