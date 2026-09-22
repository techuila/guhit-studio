// Hit testing and marquee selection. Pure, tested in hit.test.ts.

import type { Element } from "../contract/bindings";
import type { P, Rect } from "./geom";
import { dist, distToSegment, pointInPolygon, shapeInsideRect, shapeIntersectsRect } from "./geom";
import type { DocIndex } from "./model";
import { dimensionGeometry, elementShape, isLocked } from "./model";

export interface HitOptions {
  /** Pick radius in mm. */
  tol: number;
  /** Model height of room labels at the current zoom, mm. */
  labelHeightMm: number;
  /** Include elements on locked layers (used for hover-free lookups). */
  includeLocked?: boolean;
}

const ORDER: Element["kind"][] = [
  "opening",
  "dimension",
  "annotation",
  "camera",
  "reference_model",
  "column",
  "asset",
  "stair",
  "wall",
  "linework",
  "room",
  "underlay",
];

export function hitsElement(p: P, el: Element, index: DocIndex, opt: HitOptions): boolean {
  if (el.kind === "dimension") {
    const g = dimensionGeometry(el.a, el.b, el.offset_mm);
    return distToSegment(p, g.p1, g.p2) <= opt.tol * 1.2;
  }
  if (el.kind === "camera") {
    return dist(p, { x: el.position.x, y: el.position.y }) <= opt.labelHeightMm * 1.2 + opt.tol;
  }
  if (el.kind === "reference_model") {
    return dist(p, el.position) <= opt.labelHeightMm * 1.2 + opt.tol;
  }
  if (el.kind === "column" && el.shape === "round") {
    return dist(p, el.center) <= el.width_mm / 2 + opt.tol * 0.3;
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
