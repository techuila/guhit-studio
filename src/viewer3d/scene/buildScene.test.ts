import * as THREE from "three";
import { describe, expect, it } from "vitest";
import type { DocState, Element, Footprint } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";
import { buildScene, groupFootprints, PLINTH_MM } from "./buildScene";
import { MaterialLibrary } from "./materials";

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

describe("objects below grade", () => {
  const base = fixture as unknown as DocState;
  const someAsset = base.project.elements.find((e) => e.kind === "asset") as Extract<Element, { kind: "asset" }>;
  const place = (id: string, x: number, y: number, elevation: number, height = 1300): Element =>
    ({ ...someAsset, id, catalog_key: "septic-tank", category: "utility", light: null, position: { x, y }, elevation_mm: elevation, height_mm: height }) as Element;

  it("sit at their real elevation from the level floor, the datum their pipes use", () => {
    const doc = structuredClone(base);
    doc.project.elements.push(
      place("tank", 20000, -6000, -1670),
      place("bench", 20000, -9000, 0),
      place("box", 3000, 3000, -200, 100),
      place("just-under", 20000, -12000, -1),
      place("meter", 20000, -15000, -225, 150),
    );
    const lib = new MaterialLibrary();
    const built = buildScene(doc, lib, { cutaway: false, activeLevelId: null, packModels: false });
    try {
      const top = (id: string) => built.root.children.find((o) => o.userData.elementId === id && !(o as THREE.Mesh).isMesh)!;
      // Buried outdoors: from the level floor, not the lawn.
      expect(top("tank").position.y).toBeCloseTo(-1.67, 6);
      // On the lawn: stands on the ground, as before.
      expect(top("bench").position.y).toBeCloseTo(-PLINTH_MM / 1000, 6);
      // Under the floor datum but not under the lawn: still on the lawn, never above it.
      expect(top("just-under").position.y).toBeCloseTo(-PLINTH_MM / 1000, 6);
      // Under the lawn: buried at its real height.
      expect(top("meter").position.y).toBeCloseTo(-0.225, 6);
      // Indoors below the floor: where it is.
      expect(top("box").position.y).toBeCloseTo(-0.2, 6);
      // The tank's form starts at its underside and its access covers reach the lawn.
      const box = new THREE.Box3().setFromObject(top("tank"));
      expect(box.min.y).toBeCloseTo(-1.67, 3);
      expect(box.max.y).toBeCloseTo(-PLINTH_MM / 1000 - 0.04 + 0.05, 3);
      // Framing stays above the ground.
      expect(built.bounds.minZ).toBeGreaterThanOrEqual(-PLINTH_MM);
    } finally {
      built.kit.dispose();
      lib.dispose();
    }
  });
});
