// Roof decks: the part of a level that the next level up does not cover gets
// a flat 200 mm concrete deck at its ceiling, the way a Philippine two-storey
// house roofs the ground floor rooms outside the upper floor (DECISIONS D26).
// The top level keeps the project roof. Pure plan math, no scene objects.

import type { Pt } from "../geom/coords";
import { clipHalfPlane, ensureCCW, offsetPolygon, signedArea, triangulate } from "../geom/polygon";

/** Deck thickness, the same as the slab under an upper level. */
export const DECK_MM = 200;
/**
 * Topping over the deck. It lifts the deck's top just above the wall tops,
 * which would otherwise share its plane and flicker against it where they
 * are open to the sky.
 */
export const DECK_TOPPING_MM = 20;

/** Slabs and decks stay this far inside the outer wall faces (buildScene). */
const EDGE_INSET_MM = 8;
/** Pieces narrower than this are slivers from walls that almost line up. */
const MIN_WIDTH_MM = 50;

/** The part of `poly` outside the convex polygon `hole`, as convex-cut pieces. */
function minusConvex(poly: Pt[], hole: Pt[]): Pt[][] {
  const out: Pt[][] = [];
  let rest = poly;
  for (let i = 0; i < hole.length && rest.length >= 3; i++) {
    const a = hole[i];
    const b = hole[(i + 1) % hole.length];
    const len = Math.hypot(b.x - a.x, b.y - a.y);
    if (len < 1e-6) continue;
    // Outward normal of a counter-clockwise edge.
    const nx = (b.y - a.y) / len;
    const ny = -(b.x - a.x) / len;
    const c = nx * a.x + ny * a.y;
    const outside = clipHalfPlane(rest, -nx, -ny, -c);
    if (outside.length >= 3) out.push(outside);
    rest = clipHalfPlane(rest, nx, ny, c);
  }
  // What is left inside every edge is covered.
  return out;
}

/** Roughly how wide a piece is: its area over half its perimeter. */
function width(poly: Pt[]): number {
  let perimeter = 0;
  for (let i = 0; i < poly.length; i++) {
    const a = poly[i];
    const b = poly[(i + 1) % poly.length];
    perimeter += Math.hypot(b.x - a.x, b.y - a.y);
  }
  return perimeter > 0 ? (2 * Math.abs(signedArea(poly))) / perimeter : 0;
}

/**
 * Plan pieces of a level's footprints that the next level's footprints do
 * not cover, where the level needs a deck. Upper footprints may be any simple
 * polygon: they are cut into triangles and taken away one by one.
 */
export function deckPieces(lower: Pt[][], upper: Pt[][]): Pt[][] {
  let pieces = lower
    .map((fp) => ensureCCW(offsetPolygon(fp, -EDGE_INSET_MM)))
    .filter((p) => p.length >= 3 && Math.abs(signedArea(p)) > 1);
  for (const fp of upper) {
    // The upper slab is inset the same, so the deck meets its edge exactly.
    const cover = ensureCCW(offsetPolygon(fp, -EDGE_INSET_MM));
    if (cover.length < 3) continue;
    for (const [i, j, k] of triangulate(cover)) {
      const tri = ensureCCW([cover[i], cover[j], cover[k]]);
      if (Math.abs(signedArea(tri)) < 1) continue;
      pieces = pieces.flatMap((p) => minusConvex(p, tri));
    }
  }
  return pieces.filter((p) => p.length >= 3 && width(p) >= MIN_WIDTH_MM);
}
