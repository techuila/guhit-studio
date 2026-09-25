// Two-level test model for the walk tests (not used by the app): the sample
// bungalow (8 x 6 m, 150 mm outer walls with inner faces at 75 mm, a 100 mm
// wall at x = 5000, front door on the south wall from x 1050 to 1950) with a
// second floor on top and a stair up the west side of the living room.
//
// Stair: origin (600, 1200), climbing north, 900 wide, 3900 run, 16 risers:
// going 243.75 mm, riser 187.5 mm, flight x 150..1050, y 1200..5100.
// Upper floor: elevation 3000, the same outer walls, no inner wall.

import type { DocState, Element } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";

export const L1 = "00000000-0000-4000-8000-0000000000a1";
export const L2 = "test-level-2";
export const STAIR = "test-stair";

export function twoLevelDoc(opts: { upper?: boolean } = {}): DocState {
  const d = structuredClone(fixture as unknown as DocState);
  d.project.elements.push({ kind: "stair", id: STAIR, level_id: L1, origin: { x: 600, y: 1200 }, rotation_deg: 0, width_mm: 900, run_mm: 3900, riser_count: 16 });
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
