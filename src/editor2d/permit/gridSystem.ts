// A grid system on the building's walls (lane L1): the centerlines of the
// level's straight walls give the spacings of an `AddGridSystem` (1, 2, 3
// on the walls that run up and down, A, B, C on the ones that run across).
// Pure, tested in gridSystem.test.ts.

import type { Command, Wall } from "../../contract/bindings";
import type { P } from "../geom";

/** Wall lines closer than this share one grid line. The engine's shortest spacing is 100 mm. */
const MERGE_MM = 100;

/** Sorted positions, neighbors closer than `MERGE_MM` merged into their mean. */
function lines(values: number[]): number[] {
  const sorted = [...values].sort((a, b) => a - b);
  const groups: number[][] = [];
  for (const v of sorted) {
    const g = groups[groups.length - 1];
    if (g && v - g[g.length - 1] < MERGE_MM) g.push(v);
    else groups.push([v]);
  }
  return groups.map((g) => Math.round(g.reduce((s, v) => s + v, 0) / g.length));
}

const spacings = (at: number[]): number[] => at.slice(1).map((v, i) => v - at[i]);

/**
 * The grid system on these walls: lines on every distinct x of the walls
 * that run along y and every distinct y of the walls that run along x.
 * Null when the walls give fewer than two lines either way.
 */
export function gridSystemOnWalls(walls: readonly Pick<Wall, "start" | "end">[]): Extract<Command, { type: "add_grid_system" }> | null {
  const xs: number[] = [];
  const ys: number[] = [];
  for (const w of walls) {
    const dx = Math.abs(w.end.x - w.start.x);
    const dy = Math.abs(w.end.y - w.start.y);
    if (dx < 1 && dy >= MERGE_MM) xs.push((w.start.x + w.end.x) / 2);
    else if (dy < 1 && dx >= MERGE_MM) ys.push((w.start.y + w.end.y) / 2);
  }
  const x = lines(xs);
  const y = lines(ys);
  if (x.length < 2 && y.length < 2) return null;
  const origin: P = { x: x[0] ?? 0, y: y[0] ?? 0 };
  return {
    type: "add_grid_system",
    origin,
    rotation_deg: 0,
    x_spacings_mm: x.length >= 2 ? spacings(x) : [],
    y_spacings_mm: y.length >= 2 ? spacings(y) : [],
  };
}
