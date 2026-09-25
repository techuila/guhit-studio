import * as THREE from "three";
import { describe, expect, it } from "vitest";
import type { DocState } from "../../contract/bindings";
import type { Pt } from "../geom/coords";
import { signedArea } from "../geom/polygon";
import { buildScene } from "./buildScene";
import { MaterialLibrary } from "./materials";
import { deckPieces } from "./roofDeck";

const rect = (x0: number, y0: number, x1: number, y1: number): Pt[] => [
  { x: x0, y: y0 },
  { x: x1, y: y0 },
  { x: x1, y: y1 },
  { x: x0, y: y1 },
];
const area = (pieces: Pt[][]) => pieces.reduce((sum, p) => sum + Math.abs(signedArea(p)), 0);
/** Area of an 8000 x 6000 footprint after the 8 mm edge inset. */
const INSET = (8000 - 16) * (6000 - 16);

describe("roof decks", () => {
  it("covers the whole level when nothing is above it", () => {
    expect(area(deckPieces([rect(0, 0, 8000, 6000)], []))).toBeCloseTo(INSET, 0);
  });

  it("leaves nothing when the upper floor covers the level", () => {
    expect(deckPieces([rect(0, 0, 8000, 6000)], [rect(0, 0, 8000, 6000)])).toEqual([]);
    // Upper walls drawn a few millimeters off still cover it, with no slivers.
    expect(deckPieces([rect(0, 0, 8000, 6000)], [rect(4, -3, 8005, 6002)])).toEqual([]);
  });

  it("decks the part outside a smaller upper floor", () => {
    // Upper floor over the west half.
    const pieces = deckPieces([rect(0, 0, 8000, 6000)], [rect(0, 0, 4000, 6000)]);
    // East half, from the upper slab's edge to the lower slab's edge.
    expect(area(pieces)).toBeCloseTo((7992 - 3992) * (6000 - 16), 0);
    for (const p of pieces) for (const v of p) expect(v.x).toBeGreaterThanOrEqual(3992 - 1e-6);
  });

  it("takes away an L-shaped upper floor", () => {
    const upperL: Pt[] = [
      { x: 0, y: 0 },
      { x: 8000, y: 0 },
      { x: 8000, y: 3000 },
      { x: 3000, y: 3000 },
      { x: 3000, y: 6000 },
      { x: 0, y: 6000 },
    ];
    const pieces = deckPieces([rect(0, 0, 8000, 6000)], [upperL]);
    // The open corner, edge to edge with both slabs: 5000 x 3000.
    expect(area(pieces)).toBeCloseTo((7992 - 2992) * (5992 - 2992), 0);
    for (const p of pieces) {
      for (const v of p) {
        expect(v.x).toBeGreaterThanOrEqual(2992 - 1e-6);
        expect(v.y).toBeGreaterThanOrEqual(2992 - 1e-6);
      }
    }
  });

  it("decks each detached building on its own", () => {
    const pieces = deckPieces([rect(0, 0, 4000, 4000), rect(6000, 0, 9000, 3000)], [rect(0, 0, 4000, 4000)]);
    expect(area(pieces)).toBeCloseTo((3000 - 16) * (3000 - 16), 0);
  });
});

/** Ground floor 8 x 6 m; the first floor covers its west half only. */
function halfStorey(upper: Pt[] | null): DocState {
  const levels = [{ id: "L0", name: "Ground", elevation_mm: 0, height_mm: 3000 }];
  const footprints = [{ level_id: "L0", polygon: rect(0, 0, 8000, 6000), area_mm2: 48e6 }];
  if (upper) {
    levels.push({ id: "L1", name: "First", elevation_mm: 3000, height_mm: 3000 });
    footprints.push({ level_id: "L1", polygon: upper, area_mm2: 24e6 });
  }
  return {
    revision: 1,
    project: { id: "p", name: "Deck", levels, layers: [], materials: [], roof: { kind: "none" }, settings: null, elements: [] },
    derived: { walls: [], rooms: [], footprints },
  } as unknown as DocState;
}

/** First surface a ray straight down at plan (x, y) meets, world meters. */
function firstHitY(root: THREE.Object3D, x: number, y: number): number | null {
  root.updateMatrixWorld(true);
  const ray = new THREE.Raycaster(new THREE.Vector3(x / 1000, 20, -y / 1000), new THREE.Vector3(0, -1, 0));
  const hits = ray.intersectObject(root, true);
  return hits.length ? hits[0].point.y : null;
}

describe("roof decks in the scene", () => {
  it("roofs the ground floor outside the upper floor, beside its slab", () => {
    const built = buildScene(halfStorey(rect(0, 0, 4000, 6000)), new MaterialLibrary(), { cutaway: false, activeLevelId: null });
    // East half: the deck on the 2.8 m ceiling of the ground floor, 200 mm
    // thick with its 20 mm topping over the wall tops.
    expect(firstHitY(built.root, 6000, 3000)).toBeCloseTo(3.02, 3);
    // West half: the first floor's own slab, at the same height.
    expect(firstHitY(built.root, 2000, 3000)).toBeCloseTo(3, 3);
    // The deck is part of the roof, so it hides with it.
    expect(built.roofGroup.children.length).toBe(1);
    built.kit.dispose();
  });

  it("adds nothing to a single storey", () => {
    const built = buildScene(halfStorey(null), new MaterialLibrary(), { cutaway: false, activeLevelId: null });
    expect(built.roofGroup.children.length).toBe(0);
    built.kit.dispose();
  });
});
