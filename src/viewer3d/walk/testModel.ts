// Two-level test model for the walk tests (not used by the app): the sample
// bungalow (8 x 6 m, 150 mm outer walls with inner faces at 75 mm, a 100 mm
// wall at x = 5000, front door on the south wall from x 1050 to 1950) with a
// second floor on top and a stair up the west side of the living room.
//
// Stair: origin (600, 1200), climbing north, 900 wide, 3900 run, 16 risers:
// 15 treads of 260 mm (DECISIONS D44: one fewer than the risers), riser
// 187.5 mm, flight x 150..1050, y 1200..5100.
// Upper floor: elevation 3000, the same outer walls, no inner wall.

import type { DocState, Element } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";

export const L1 = "00000000-0000-4000-8000-0000000000a1";
export const L2 = "test-level-2";
export const STAIR = "test-stair";

export const VOID = "test-void";
export const BALCONY = "test-balcony";
export const DECK = "test-deck";

/**
 * `twoLevelDoc` with slabs (DECISIONS D50): on the upper floor a 2 x 2 m
 * void over the living room (x 2000..4000, y 2000..4000) with a bar rail
 * round it, a door in the south wall (x 2050..2950) onto a balcony
 * (x 1000..4000, y -1575..-75) with a glass rail on its three open edges;
 * on the ground floor a deck east of the house, sunk 150 mm
 * (x 8075..10075, y 1000..3000). The rail runs are what the engine puts in
 * `Derived.railings`.
 */
export function slabDoc(): DocState {
  const d = twoLevelDoc();
  const rect = (x0: number, y0: number, x1: number, y1: number) => [
    { x: x0, y: y0 },
    { x: x1, y: y0 },
    { x: x1, y: y1 },
    { x: x0, y: y1 },
  ];
  d.project.elements.push(
    { kind: "slab", id: VOID, level_id: L2, slab_kind: "void", polygon: rect(2000, 2000, 4000, 4000), thickness_mm: 0, top_offset_mm: 0, material_id: null, railing: { height_mm: 1000, style: "bars" } },
    { kind: "slab", id: BALCONY, level_id: L2, slab_kind: "balcony", polygon: rect(1000, -1575, 4000, -75), thickness_mm: 150, top_offset_mm: 0, material_id: null, railing: { height_mm: 1000, style: "glass" } },
    { kind: "slab", id: DECK, level_id: L1, slab_kind: "deck", polygon: rect(8075, 1000, 10075, 3000), thickness_mm: 100, top_offset_mm: -150, material_id: null, railing: null },
    { kind: "opening", id: "test-upper-door", wall_id: "test-upper-wall-0", opening_type: "door", style: "swing_single", offset_mm: 2500, width_mm: 900, height_mm: 2100, sill_mm: 0, flip_side: false, flip_hinge: false, material_id: null },
  );
  d.derived.railings = [
    {
      host_id: VOID,
      level_id: L2,
      kind: "slab_edge",
      points: [...rect(2000, 2000, 4000, 4000), { x: 2000, y: 2000 }].map((p) => ({ ...p, z: 0 })),
      height_mm: 1000,
      style: "bars",
    },
    {
      host_id: BALCONY,
      level_id: L2,
      kind: "slab_edge",
      points: [
        { x: 1000, y: -75, z: 0 },
        { x: 1000, y: -1575, z: 0 },
        { x: 4000, y: -1575, z: 0 },
        { x: 4000, y: -75, z: 0 },
      ],
      height_mm: 1000,
      style: "glass",
    },
  ];
  return d;
}

export function twoLevelDoc(opts: { upper?: boolean } = {}): DocState {
  const d = structuredClone(fixture as unknown as DocState);
  d.project.elements.push({ kind: "stair", id: STAIR, level_id: L1, origin: { x: 600, y: 1200 }, rotation_deg: 0, width_mm: 900, run_mm: 3900, riser_count: 16, railing: null });
  if (opts.upper === false) return d;
  d.project.levels.push({ id: L2, name: "Second Floor", elevation_mm: 3000, height_mm: 2800 });
  const box: [number, number, number, number][] = [
    [0, 0, 8000, 0],
    [8000, 0, 8000, 6000],
    [8000, 6000, 0, 6000],
    [0, 6000, 0, 0],
  ];
  box.forEach(([x0, y0, x1, y1], i) => {
    const wall: Element = {
      kind: "wall",
      id: `test-upper-wall-${i}`,
      level_id: L2,
      start: { x: x0, y: y0 },
      end: { x: x1, y: y1 },
      thickness_mm: 150,
      height_mm: null,
      material_id: null,
    };
    d.project.elements.push(wall);
  });
  d.derived.footprints.push({
    level_id: L2,
    polygon: [
      { x: -75, y: -75 },
      { x: 8075, y: -75 },
      { x: 8075, y: 6075 },
      { x: -75, y: 6075 },
    ],
    area_mm2: 8150 * 6150,
  });
  return d;
}
