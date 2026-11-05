// Picking the permit annotations on the plan (lane L1): grid lines by their
// line or bubbles, section and elevation marks by their line or symbols,
// the lot by its sides (never by its inside, so a click on a room still
// picks the room) and dimension strings along their whole line. hit.ts
// calls in here one line per kind. Pure, tested in hitPermit.test.ts.

import type { Dimension, Element } from "../../contract/bindings";
import type { P } from "../geom";
import { dist, distToSegment } from "../geom";
import type { HitOptions } from "../hit";
import { GRID_BUBBLE_PX, gridGeometry, markGeometry, markRadiusMm, mmPerPx, stringGeometry } from "./shapes";

type PermitEl = Extract<Element, { kind: "grid" | "view_mark" | "lot" }>;

function pxOf(opt: HitOptions): number {
  return opt.pxMm ?? mmPerPx(opt.labelHeightMm);
}

/** True when `p` picks this grid line, view mark or lot. */
export function hitsPermit(p: P, el: PermitEl, opt: HitOptions): boolean {
  const px = pxOf(opt);
  const band = opt.tol * 0.6;
  switch (el.kind) {
    case "grid": {
      const r = GRID_BUBBLE_PX * px;
      const g = gridGeometry(el, r);
      if (distToSegment(p, el.start, el.end) <= band) return true;
      return dist(p, g.bubbleA) <= r + band * 0.5 || dist(p, g.bubbleB) <= r + band * 0.5;
    }
    case "view_mark": {
      const r = markRadiusMm(el, px);
      const g = markGeometry(el, r);
      if (distToSegment(p, el.start, el.end) <= band) return true;
      // The arrow sits on the look side of each symbol: count it in.
      const reach = r * 1.6 + band * 0.5;
      if (el.mark_kind === "section") return dist(p, g.headA) <= reach || dist(p, g.headB) <= reach;
      return dist(p, g.mid) <= reach;
    }
    case "lot": {
      const c = el.corners;
      for (let i = 0; i < c.length; i++) if (distToSegment(p, c[i], c[(i + 1) % c.length]) <= band) return true;
      return false;
    }
  }
}

/** True when `p` is on a dimension string's line, from its first point to its last. */
export function hitsDimensionString(p: P, d: Pick<Dimension, "a" | "b" | "through" | "offset_mm">, opt: HitOptions): boolean {
  const g = stringGeometry(d);
  return distToSegment(p, g.p1, g.p2) <= opt.tol * 1.2;
}
