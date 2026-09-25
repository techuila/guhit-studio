import * as THREE from "three";
import { describe, expect, it } from "vitest";
import type { DocState } from "../../contract/bindings";
import { signedArea } from "../geom/polygon";
import { buildScene, subtractWells } from "./buildScene";
import { MaterialLibrary } from "./materials";

const rect = (x0: number, y0: number, x1: number, y1: number) => [
  { x: x0, y: y0 },
  { x: x1, y: y0 },
  { x: x1, y: y1 },
  { x: x0, y: y1 },
];

const area = (pieces: { x: number; y: number }[][]) => pieces.reduce((a, p) => a + Math.abs(signedArea(p)), 0);

describe("subtractWells", () => {
  it("cuts a hole inside a floor", () => {
    const pieces = subtractWells(rect(0, 0, 10000, 10000), [rect(2000, 2000, 3000, 5000)]);
    expect(area(pieces)).toBeCloseTo(100e6 - 3e6, -2);
  });

  it("leaves a floor whole when the hole misses it", () => {
    const pieces = subtractWells(rect(0, 0, 4000, 4000), [rect(5000, 5000, 6000, 6000)]);
    expect(pieces).toHaveLength(1);
    expect(area(pieces)).toBeCloseTo(16e6, -2);
  });

  it("cuts only the overlap of a hole across the floor's edge", () => {
    const pieces = subtractWells(rect(0, 0, 4000, 4000), [rect(3000, 1000, 5000, 2000)]);
    expect(area(pieces)).toBeCloseTo(16e6 - 1e6, -2);
  });

  it("cuts two holes", () => {
    const pieces = subtractWells(rect(0, 0, 10000, 10000), [rect(1000, 1000, 2000, 4000), rect(6000, 6000, 8000, 7000)]);
    expect(area(pieces)).toBeCloseTo(100e6 - 3e6 - 2e6, -2);
  });
});

/** Two levels, one building, one flight from the ground floor to the first floor. */
function twoStoreys(): DocState {
  const levels = [
    { id: "L0", name: "Ground", elevation_mm: 0, height_mm: 3000 },
    { id: "L1", name: "First", elevation_mm: 3000, height_mm: 3000 },
  ];
  const fp = rect(0, 0, 8000, 8000);
  return {
    revision: 1,
    project: {
      id: "p",
      name: "Two storeys",
      levels,
      layers: [],
      materials: [],
      roof: { kind: "none" },
      settings: null,
      elements: [
        { kind: "stair", id: "s1", level_id: "L0", origin: { x: 2000, y: 2000 }, rotation_deg: 0, width_mm: 1000, run_mm: 3600, riser_count: 16 },
      ],
    },
    derived: {
      walls: [],
      rooms: [],
      footprints: [
        { level_id: "L0", polygon: fp, area_mm2: 64e6 },
        { level_id: "L1", polygon: fp, area_mm2: 64e6 },
      ],
    },
  } as unknown as DocState;
}

/** First surface a ray straight down at plan (x, y) meets, world meters. */
function firstHitY(root: THREE.Object3D, x: number, y: number): number | null {
  root.updateMatrixWorld(true);
  const ray = new THREE.Raycaster(new THREE.Vector3(x / 1000, 20, -y / 1000), new THREE.Vector3(0, -1, 0));
  const hits = ray.intersectObject(root, true);
  return hits.length ? hits[0].point.y : null;
}

describe("stairwells", () => {
  it("opens the upper floor over a flight and keeps it closed elsewhere", () => {
    const built = buildScene(twoStoreys(), new MaterialLibrary(), { cutaway: false, activeLevelId: null });
    // Beside the stair the first floor's slab is the first thing below.
    expect(firstHitY(built.root, 6000, 6000)).toBeCloseTo(3, 3);
    // Over the flight the ray passes the first floor and lands on a tread.
    const overFlight = firstHitY(built.root, 2000, 4000);
    expect(overFlight).not.toBeNull();
    expect(overFlight as number).toBeLessThan(2.9);
    built.kit.dispose();
  });
});
