// Roofs and stairs in the 3D view from the engine's geometry (lane L2,
// DECISIONS D54): a hip roof's covering and soffit close up with no faces
// along its hips and ridge, trim hangs on the eaves, roof decks drop under
// a roof, an L and a U stair build every flight and landing and stop at the
// cutaway, and the carport deck reads as paving.

import { describe, expect, it } from "vitest";
import type { RoofGeometry, Slab, StairGeometry } from "../../contract/bindings";
import { MeshData } from "../geom/meshData";
import { buildDerivedInfill, buildDerivedRoof, buildRoofTrim, fitPlane } from "../geom/roofMesh";
import { signedArea } from "../geom/polygon";
import fixture from "../../editor2d/roofsStairs.fixture.json";
import { deckPieces, uncovered } from "./roofDeck";
import { roofCover } from "./roofs";
import { deckFinish } from "./slabs";
import { flightProfile, pushStairGeometry, stairPieces } from "./stairs";

const roofs = (fixture as unknown as { roofs: RoofGeometry[] }).roofs;
const stairs = (fixture as unknown as { stairs: StairGeometry[] }).stairs;
const hip = roofs.find((g) => g.planes.length === 4)!;
const shed = roofs.find((g) => g.planes.length === 1)!;

/** Heights in mm of every vertex of a mesh, from its three.js positions. */
function heights(md: MeshData): number[] {
  const out: number[] = [];
  for (let i = 1; i < md.positions.length; i += 3) out.push(md.positions[i] * 1000);
  return out;
}

describe("roofs from Derived.roofs", () => {
  it("fit each plane: the top of every corner is on it", () => {
    for (const plane of hip.planes) {
      const f = fitPlane(plane)!;
      for (const p of plane) expect(f.z0 + f.gx * p.x + f.gy * p.y).toBeCloseTo(p.z, 6);
    }
  });

  it("build a closed hip: covering on top, soffit and eave edges below, nothing along the hips", () => {
    const md = new MeshData();
    const trim = new MeshData();
    const made = buildDerivedRoof(hip, 3000, md, trim);
    // Covering: one triangle per triangle of each plane.
    expect(md.triangleCount).toBe(hip.planes.reduce((n, p) => n + p.length - 2, 0));
    // Soffit triangles plus two per eave edge (four eave edges, no hip or ridge sides).
    const soffit = hip.planes.reduce((n, p) => n + p.length - 2, 0);
    expect(trim.triangleCount).toBe(soffit + 4 * 2);
    const top = Math.max(...hip.planes.flat().map((p) => p.z));
    expect(made.peak).toBeCloseTo(3000 + top, 6);
    expect(made.low).toBeCloseTo(3000 + Math.min(...hip.planes.flat().map((p) => p.z)) - hip.vertical_mm, 6);
    // Together a closed solid of the roof's volume, outward facing.
    const both = new MeshData();
    buildDerivedRoof(hip, 3000, both, both);
    expect(both.signedVolume()).toBeGreaterThan(0);
  });

  it("hang the fascia on every eave and the gutter under it", () => {
    const fascia = new MeshData();
    const gutter = new MeshData();
    buildRoofTrim(hip, 3000, 200, 125, fascia, gutter);
    // A prism per eave edge: 12 triangles each.
    expect(fascia.triangleCount).toBe(4 * 12);
    expect(gutter.triangleCount).toBe(4 * 2 * 12);
    const eave = hip.eaves[0][0].z + 3000;
    expect(Math.max(...heights(fascia))).toBeCloseTo(eave, 3);
    expect(Math.min(...heights(gutter))).toBeLessThan(eave - hip.vertical_mm);
    // A shed's high side is no eave: fascia on its eave and two rakes, and a gutter only on the eave.
    const f2 = new MeshData();
    const g2 = new MeshData();
    buildRoofTrim(shed, 0, 150, 125, f2, g2);
    expect(f2.triangleCount).toBe(3 * 12);
    expect(g2.triangleCount).toBe(2 * 12);
  });

  it("give a gable end a wall's thickness", () => {
    const g: RoofGeometry = {
      ...hip,
      infill: [
        [
          { x: 0, y: 0, z: 3000 },
          { x: 6000, y: 0, z: 3000 },
          { x: 3000, y: 0, z: 4500 },
        ],
      ],
    };
    const md = new MeshData();
    buildDerivedInfill(g, 0, 150, md);
    // Two faces of one triangle each, and the two sloped tops; it sits on
    // the wall, so no face underneath.
    expect(md.triangleCount).toBe(2 + 2 * 2);
    expect(Math.max(...heights(md))).toBeCloseTo(4500, 6);
    // The outer face on the wall line (plan y 0), the inner one 150 mm in.
    const ys = new Set<number>();
    for (let i = 2; i < md.positions.length; i += 3) ys.add(Math.round(-md.positions[i] * 1000));
    expect([...ys].sort((a, b) => a - b)).toEqual([0, 150]);
  });

  it("drop the roof deck where a roof covers the level", () => {
    const lower = [[{ x: 0, y: 0 }, { x: 12000, y: 0 }, { x: 12000, y: 8000 }, { x: 0, y: 8000 }]];
    const upper = [[{ x: 0, y: 0 }, { x: 8000, y: 0 }, { x: 8000, y: 8000 }, { x: 0, y: 8000 }]];
    const open = deckPieces(lower, upper);
    const area = (ps: { x: number; y: number }[][]) => ps.reduce((n, p) => n + Math.abs(signedArea(p)), 0);
    expect(area(open)).toBeGreaterThan(30e6);
    const cover = [[{ x: 7900, y: -600 }, { x: 12600, y: -600 }, { x: 12600, y: 8600 }, { x: 7900, y: 8600 }]];
    expect(uncovered(open, cover)).toEqual([]);
    // Half covered, half a deck.
    const half = uncovered(open, [[{ x: 7900, y: -600 }, { x: 12600, y: -600 }, { x: 12600, y: 4000 }, { x: 7900, y: 4000 }]]);
    expect(area(half)).toBeCloseTo(area(open) / 2, -4);
    expect(roofCover(roofs, hip.level_id)).toHaveLength(hip.planes.length + (shed.level_id === hip.level_id ? 1 : 0));
  });
});

describe("stairs from Derived.stairs", () => {
  it("build every flight and the landing of an L and a U", () => {
    for (const g of stairs) {
      const md = new MeshData();
      pushStairGeometry(md, g, 0, null);
      expect(md.triangleCount).toBeGreaterThan(0);
      const top = Math.max(...heights(md));
      // The last tread is one riser under the floor above.
      const nosings = g.flights[g.flights.length - 1].nosings;
      const lastNosing = nosings[nosings.length - 1].z;
      expect(top).toBeCloseTo(lastNosing, 1);
      expect(Math.min(...heights(md))).toBeGreaterThanOrEqual(-1e-6);
      expect(stairPieces(g)).toHaveLength(3);
      expect(md.signedVolume()).toBeGreaterThan(0);
    }
  });

  it("stop at the cutaway", () => {
    const g = stairs[0];
    const md = new MeshData();
    pushStairGeometry(md, g, 0, 1200);
    expect(Math.max(...heights(md))).toBeLessThanOrEqual(1200 + 1e-6);
  });

  it("draw a flight as risers, treads and a soffit", () => {
    const prof = flightProfile(3, 250, 180);
    expect(prof[0]).toEqual({ x: 0, y: 0 });
    expect(prof[1]).toEqual({ x: 0, y: 180 });
    expect(prof[6]).toEqual({ x: 750, y: 540 });
  });
});

describe("decks", () => {
  const deck = (top: number, levelId = "g"): Slab =>
    ({ id: "d", level_id: levelId, slab_kind: "deck", polygon: [], thickness_mm: 100, top_offset_mm: top, material_id: null, railing: null }) as Slab;
  it("read as paving on the ground (a carport, a driveway) and timber when raised (G4 problem 11)", () => {
    expect(deckFinish(deck(0), true)).toBe("mat-paving-stone");
    expect(deckFinish(deck(-150), true)).toBe("mat-paving-stone");
    expect(deckFinish(deck(600), true)).toBe("mat-wood-cladding");
    expect(deckFinish(deck(0), false)).toBe("mat-wood-cladding");
  });
});
