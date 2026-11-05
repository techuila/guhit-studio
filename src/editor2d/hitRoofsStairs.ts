// Picking stairs and roofs on the plan (lane L2); hit.ts calls in here in
// one line. A stair picks inside any of its flights or landings (`Derived::
// stairs`), or its plain outline before the engine answered. A roof picks
// near one of its lines or eave runs, on the level it sits on only, never
// by its area: a click inside a room under it still picks the room, and so
// does a click on a room's label a ridge runs through. Pure, tested in
// roofsStairs.test.ts.

import type { Element } from "../contract/bindings";
import type { P } from "./geom";
import { distToSegment, pointInPolygon } from "./geom";
import type { HitOptions } from "./hit";
import type { DocIndex } from "./model";
import { elementShape, isLocked } from "./model";
import { nearRoofLine, plainStairOutline, roofGeometryFor, stairCovers, stairGeometryFor } from "./roofsStairsGeom";

/** A roof picks this close to a line, as a share of the plan's pick radius (linework uses the same). */
const ROOF_PICK = 0.6;

/** True when `p` picks this stair or roof. */
export function hitsRoofOrStair(p: P, el: Extract<Element, { kind: "stair" | "roof" }>, index: DocIndex, opt: HitOptions): boolean {
  if (el.kind === "stair") {
    const g = stairGeometryFor(el, index);
    return g ? stairCovers(g, p) : pointInPolygon(p, plainStairOutline(el));
  }
  const tol = opt.tol * ROOF_PICK;
  const g = roofGeometryFor(el, index);
  if (g) return g.level_id === index.levelId && nearRoofLine(g, p, tol) && !onRoomLabel(p, index, opt);
  // A drawn roof the engine has not answered for picks by its drawn outline.
  if (el.outline.type !== "polygon" || el.level_id !== index.levelId) return false;
  const pts = el.outline.points;
  for (let i = 0; i < pts.length; i++) if (distToSegment(p, pts[i], pts[(i + 1) % pts.length]) <= tol) return !onRoomLabel(p, index, opt);
  return false;
}

/** True on the label of a room that picks: it wins over a roof line crossing it (a ridge often runs through a room's middle). */
function onRoomLabel(p: P, index: DocIndex, opt: HitOptions): boolean {
  for (const el of index.visible) {
    if (el.kind !== "room" || (!opt.includeLocked && isLocked(el, index))) continue;
    const s = elementShape(el, index, { labelHeightMm: opt.labelHeightMm });
    if (s && pointInPolygon(p, s.points)) return true;
  }
  return false;
}
