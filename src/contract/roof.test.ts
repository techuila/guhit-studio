import { describe, expect, it } from "vitest";
import type { Element, Project, Roof } from "./bindings";
import { topRoof, topRoofArea, withTopRoof, type RoofEl } from "./roof";

const LEGACY: Roof = { kind: "gable", pitch_deg: 25, overhang_mm: 600, thickness_mm: 100, ridge_axis: "x", material_id: null };
const MIGRATED: Roof = { ...LEGACY, kind: "none" };

const area = (id: string, outline: RoofEl["outline"], patch: Partial<RoofEl> = {}): RoofEl => ({
  kind: "roof",
  id,
  level_id: outline.type === "top_footprint" ? null : "g",
  outline,
  shape: "hip",
  pitch_deg: 30,
  ridge_axis: "y",
  overhang_mm: 900,
  thickness_mm: 120,
  material_id: "mat-roof-clay-tile",
  fascia: { width_mm: 200, depth_mm: 25, material_id: null } as unknown as RoofEl["fascia"],
  ...patch,
});

const project = (roof: Roof, elements: Element[]): Project => ({ roof, elements }) as unknown as Project;

describe("topRoof (DECISIONS D54)", () => {
  it("reads the TopFootprint roof element of a migrated project", () => {
    const lanai = area("r-lanai", { type: "footprint" }, { shape: "shed" });
    const top = area("r-top", { type: "top_footprint" });
    const p = project(MIGRATED, [lanai, top]);
    expect(topRoofArea(p)?.id).toBe("r-top");
    expect(topRoof(p)).toEqual({ kind: "hip", pitch_deg: 30, overhang_mm: 900, thickness_mm: 120, ridge_axis: "y", material_id: "mat-roof-clay-tile" });
  });

  it("falls back to the project roof without one: unmigrated, or kind none once migrated", () => {
    expect(topRoof(project(LEGACY, []))).toEqual(LEGACY);
    expect(topRoof(project(MIGRATED, [area("r-lanai", { type: "footprint" })]))).toEqual(MIGRATED);
    expect(topRoofArea(project(MIGRATED, []))).toBeNull();
  });

  it("never builds a delete from a migrated project: a pitch edit keeps the roof", () => {
    const p = project(MIGRATED, [area("r-top", { type: "top_footprint" })]);
    const next = { ...topRoof(p), pitch_deg: 25 };
    expect(next.kind).toBe("hip");
    expect(topRoof(withTopRoof(p, next)).pitch_deg).toBe(25);
  });
});

describe("withTopRoof (SetRoof in the dev mocks)", () => {
  it("updates the TopFootprint element and keeps its outline, fascia and level", () => {
    const top = area("r-top", { type: "top_footprint" });
    const p = withTopRoof(project(MIGRATED, [top]), { ...LEGACY, kind: "flat", overhang_mm: 300 });
    const el = topRoofArea(p)!;
    expect(el.shape).toBe("flat");
    expect(el.overhang_mm).toBe(300);
    expect(el.outline).toEqual({ type: "top_footprint" });
    expect(el.fascia).toEqual(top.fascia);
    expect(el.level_id).toBeNull();
    expect(p.roof).toEqual(MIGRATED);
  });

  it("removes the TopFootprint element for kind none and leaves other roofs", () => {
    const lanai = area("r-lanai", { type: "footprint" });
    const p = withTopRoof(project(MIGRATED, [lanai, area("r-top", { type: "top_footprint" })]), MIGRATED);
    expect(p.elements.map((e) => e.id)).toEqual(["r-lanai"]);
    expect(topRoof(p).kind).toBe("none");
  });

  it("sets the project roof of an unmigrated project, as the mocks always did", () => {
    const p = withTopRoof(project(LEGACY, []), { ...LEGACY, kind: "shed" });
    expect(p.roof.kind).toBe("shed");
    expect(p.elements).toEqual([]);
  });
});
