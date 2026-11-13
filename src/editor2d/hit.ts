// Hit testing and marquee selection. Pure, tested in hit.test.ts.

import type { Element } from "../contract/bindings";
import type { P, Rect } from "./geom";
import { dist, distToSegment, pointInPolygon, shapeInsideRect, shapeIntersectsRect } from "./geom";
import type { DocIndex } from "./model";
import { dimensionGeometry, elementShape, isLocked } from "./model";
import { hitsRoofOrStair } from "./hitRoofsStairs";
import { hitsDimensionString, hitsPermit } from "./permit/hitPermit";
import { isDimensionString } from "./permit/shapes";
import { hitsPipe } from "./pipe";
import { hitsSlab } from "./slab";

export interface HitOptions {
  /** Pick radius in mm. */
  tol: number;
  /** Model height of room labels at the current zoom, mm. */
  labelHeightMm: number;
  /** Include elements on locked layers (used for hover-free lookups). */
  includeLocked?: boolean;
  /** Size of one screen pixel in mm. Pipes use it for their drawn width. Default: tol / 6. */
  pxMm?: number;
}

const ORDER: Element["kind"][] = [
  // Pipes draw over the building, openings included, and their pick band is
  // thin: they pick before what they run along or through.
  "pipe",
  "opening",
  "dimension",
  "annotation",
  "view_mark",
  "camera",
  "reference_model",
  "column",
  "asset",
  "stair",
  "wall",
  "roof",
  "grid",
  // Slabs sit under the building: their outline, or their inside away from rooms.
  "slab",
  "lot",
  "linework",
  "room",
  "underlay",
];

export function hitsElement(p: P, el: Element, index: DocIndex, opt: HitOptions): boolean {
  if (el.kind === "dimension") {
    // A dimension string (lane L1) picks along its whole line.
    if (isDimensionString(el)) return hitsDimensionString(p, el, opt);
    const g = dimensionGeometry(el.a, el.b, el.offset_mm);
    return distToSegment(p, g.p1, g.p2) <= opt.tol * 1.2;
  }
  if (el.kind === "camera") {
    return dist(p, { x: el.position.x, y: el.position.y }) <= opt.labelHeightMm * 1.2 + opt.tol;
  }
  if (el.kind === "reference_model") {
    return dist(p, el.position) <= opt.labelHeightMm * 1.2 + opt.tol;
  }
  if (el.kind === "stair" || el.kind === "roof") return hitsRoofOrStair(p, el, index, opt);
  if (el.kind === "grid" || el.kind === "view_mark" || el.kind === "lot") return hitsPermit(p, el, opt);
  if (el.kind === "column" && el.shape === "round") {
    return dist(p, el.center) <= el.width_mm / 2 + opt.tol * 0.3;
  }
  if (el.kind === "pipe") {
    // Near the centerline, not by area: a click between two runs misses both.
    return hitsPipe(p, el, opt.pxMm ?? opt.tol / 6, opt.tol);
  }
  if (el.kind === "slab") {
    return hitsSlab(p, el.polygon, opt.tol * 0.6, index.roomPolygons);
  }
  if (el.kind === "linework") {
    // Selectable by clicking near a segment, not by area: each polyline is
    // tested on its own so a click between two disjoint traces misses both.
    for (const pl of el.polylines) {
      for (let i = 0; i + 1 < pl.length; i++) {
        if (distToSegment(p, pl[i], pl[i + 1]) <= opt.tol * 0.6) return true;
      }
    }
    return false;
  }
  const s = elementShape(el, index, { labelHeightMm: opt.labelHeightMm });
  if (!s) return false;
  if (pointInPolygon(p, s.points)) return true;
  if (el.kind === "wall" || el.kind === "opening") {
    // Thin walls at far zoom still need a usable pick band.
    for (let i = 0; i < s.points.length; i++) {
      if (distToSegment(p, s.points[i], s.points[(i + 1) % s.points.length]) <= opt.tol * 0.5) return true;
    }
  }
  return false;
}

/** Topmost selectable element under `p`, or null. */
export function hitTest(p: P, index: DocIndex, opt: HitOptions): string | null {
  for (const kind of ORDER) {
    // Later elements draw on top, so test them first.
    for (let i = index.visible.length - 1; i >= 0; i--) {
      const el = index.visible[i];
      if (el.kind !== kind) continue;
      if (!opt.includeLocked && isLocked(el, index)) continue;
      if (hitsElement(p, el, index, opt)) return el.id;
    }
  }
  return null;
}

/**
 * The room whose floor is under `p`: what a click that hit nothing else
 * selects, as Revit and SketchUp pick a room or a face by its inside. The
 * smallest room wins where rooms nest. Rooms on locked layers are skipped.
 */
export function roomFloorAt(p: P, index: DocIndex, opt: Pick<HitOptions, "includeLocked"> = {}): string | null {
  let best: string | null = null;
  let bestArea = Infinity;
  for (const el of index.visible) {
    if (el.kind !== "room") continue;
    if (!opt.includeLocked && isLocked(el, index)) continue;
    const g = index.roomGeo.get(el.id);
    if (!g || g.polygon.length < 3 || !pointInPolygon(p, g.polygon)) continue;
    if (g.area_mm2 < bestArea) {
      best = el.id;
      bestArea = g.area_mm2;
    }
  }
  return best;
}

/**
 * Marquee selection. `crossing` false: window, elements fully inside.
 * `crossing` true: anything the rectangle touches.
 */
export function marqueeSelect(rect: Rect, crossing: boolean, index: DocIndex, opt: HitOptions): string[] {
  const out: string[] = [];
  for (const el of index.visible) {
    if (isLocked(el, index)) continue;
    if (el.kind === "underlay") continue;
    const s = elementShape(el, index, { labelHeightMm: opt.labelHeightMm });
    if (!s) continue;
    const hit = crossing ? shapeIntersectsRect(s.points, rect, s.closed) : shapeInsideRect(s.points, rect);
    if (hit) out.push(el.id);
  }
  return out;
}
