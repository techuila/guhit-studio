// Stairs and roofs on the plan, read from the engine (lane L2): which
// `Derived::stairs` or `Derived::roofs` entry an element draws with (moved
// along with a dragged or turned ghost), the pieces a stair stands on, the
// cut through its flights, the arrow along its walking line, a roof's
// outline and its inner lines. No stair or roof math of its own (DECISIONS
// D54): only reading, moving and sorting what the engine answered. Pure, so
// drawing, picking and the plan's shapes share it; tested in
// roofsStairs.test.ts.

import type { DocState, Element, Flight, Point, RoofGeometry, RoofLineKind, StairGeometry } from "../contract/bindings";
import type { RoofEl } from "../contract/roof";
import type { P } from "./geom";
import { add, cross, dist, dot, left, lerp, mul, pointInPolygon, rotate, sub, unit } from "./geom";
import type { DocIndex } from "./model";
import type { StairEl } from "./slab";

/**
 * The plan cuts a level this high above its floor: treads above it draw
 * dashed past a break line (`guhit_export::stairs::STAIR_CUT_MM`).
 */
export const STAIR_CUT_MM = 1200;

/** What the lookups need of the plan's index. */
export type GeomIndex = Pick<DocIndex, "doc" | "byId">;

// ---------------------------------------------------------------- lookups

const stairsByDoc = new WeakMap<DocState, Map<string, StairGeometry>>();
const roofsByDoc = new WeakMap<DocState, Map<string, RoofGeometry>>();

/** The engine's geometry of stair `id` in `doc`. */
export function derivedStair(doc: DocState, id: string): StairGeometry | null {
  let m = stairsByDoc.get(doc);
  if (!m) {
    m = new Map();
    for (const g of doc.derived.stairs ?? []) m.set(g.stair_id, g);
    stairsByDoc.set(doc, m);
  }
  return m.get(id) ?? null;
}

/** The engine's geometry of roof `id` in `doc`. */
export function derivedRoof(doc: DocState, id: string): RoofGeometry | null {
  let m = roofsByDoc.get(doc);
  if (!m) {
    m = new Map();
    for (const g of doc.derived.roofs ?? []) m.set(g.roof_id, g);
    roofsByDoc.set(doc, m);
  }
  return m.get(id) ?? null;
}

/** True when two stairs differ at most in where they stand and which way they face. */
export function sameStairShape(a: StairEl, b: StairEl): boolean {
  return (
    a.level_id === b.level_id &&
    a.width_mm === b.width_mm &&
    a.run_mm === b.run_mm &&
    a.riser_count === b.riser_count &&
    (a.shape ?? "straight") === (b.shape ?? "straight") &&
    (a.turn ?? "left") === (b.turn ?? "left") &&
    (a.first_flight_risers ?? null) === (b.first_flight_risers ?? null) &&
    (a.landing_mm ?? null) === (b.landing_mm ?? null) &&
    (a.well_mm ?? 0) === (b.well_mm ?? 0)
  );
}

/**
 * A stair's geometry carried from where the engine worked it out (`from`)
 * to another origin and rotation (`to`). The engine builds every stair in
 * its first riser's frame, so this is a plain move and turn.
 */
export function moveStairGeometry(g: StairGeometry, from: Pick<StairEl, "origin" | "rotation_deg">, to: Pick<StairEl, "origin" | "rotation_deg">): StairGeometry {
  const turn = to.rotation_deg - from.rotation_deg;
  if (turn === 0 && from.origin.x === to.origin.x && from.origin.y === to.origin.y) return g;
  const at = (p: Point): Point => add(to.origin, rotate(sub(p, from.origin), turn));
  return {
    ...g,
    flights: g.flights.map((f) => ({
      outline: f.outline.map(at),
      nosings: f.nosings.map((n) => ({ a: at(n.a), b: at(n.b), z: n.z })),
      up: rotate(f.up, turn),
    })),
    landings: g.landings.map((l) => ({ polygon: l.polygon.map(at), z: l.z })),
    opening: g.opening.map(at),
    walk_line: g.walk_line.map((w) => ({ ...at(w), z: w.z })),
  };
}

/**
 * The geometry `el` draws with: the engine's, moved along when `el` is a
 * dragged or turned copy of a stair in the document. Anything else (the
 * placement ghost, a stair being resized) asks `ghost`. Null when there is
 * none yet: draw the plain outline.
 */
export function stairGeometryFor(el: StairEl, index: GeomIndex, ghost?: (el: StairEl) => StairGeometry | null): StairGeometry | null {
  if (el.id) {
    const own = index.byId.get(el.id);
    if (own && own.kind === "stair") {
      const g = derivedStair(index.doc, el.id);
      if (own === el) return g;
      if (g && sameStairShape(own, el)) return moveStairGeometry(g, own, el);
    }
  }
  return ghost ? ghost(el) : null;
}

/** How far a dragged copy of a roof moved from the one in the document; null when it changed otherwise. */
function roofShift(own: RoofEl, el: RoofEl): P | null {
  const rest = (r: RoofEl): string => JSON.stringify({ ...r, outline: r.outline.type });
  if (rest(own) !== rest(el)) return null;
  if (own.outline.type !== "polygon" || el.outline.type !== "polygon") return { x: 0, y: 0 };
  const a = own.outline.points;
  const b = el.outline.points;
  if (a.length !== b.length || a.length === 0) return null;
  const d = sub(b[0], a[0]);
  for (let i = 1; i < a.length; i++) if (dist(sub(b[i], a[i]), d) > 1e-6) return null;
  return d;
}

/** A roof's geometry moved by `d` in plan. */
export function shiftRoofGeometry(g: RoofGeometry, d: P): RoofGeometry {
  if (d.x === 0 && d.y === 0) return g;
  const at = <T extends Point>(p: T): T => ({ ...p, x: p.x + d.x, y: p.y + d.y });
  return {
    ...g,
    planes: g.planes.map((pl) => pl.map(at)),
    lines: g.lines.map((l) => ({ kind: l.kind, a: at(l.a), b: at(l.b) })),
    infill: g.infill.map((pl) => pl.map(at)),
    eaves: g.eaves.map((run) => run.map(at)),
  };
}

/** The geometry `el` draws with: the engine's, moved along with a dragged copy. Null for anything else. */
export function roofGeometryFor(el: RoofEl, index: GeomIndex): RoofGeometry | null {
  const g = el.id ? derivedRoof(index.doc, el.id) : null;
  const own = el.id ? index.byId.get(el.id) : undefined;
  if (!g || !own || own.kind !== "roof") return null;
  if (own === el) return g;
  const d = roofShift(own, el);
  return d ? shiftRoofGeometry(g, d) : null;
}

// ---------------------------------------------------------------- stairs

/** A flight's frame from its outline: the middle of its first riser, up it, across it to the right, width and run. */
export interface FlightFrame {
  o: P;
  up: P;
  across: P;
  width: number;
  run: number;
}

/** `guhit_export::stairs::flight_frame`. Null for a degenerate flight. */
export function flightFrame(f: Flight): FlightFrame | null {
  if (f.outline.length !== 4) return null;
  const [b0, b1, , t0] = f.outline;
  const width = dist(b0, b1);
  const run = dot(sub(t0, b0), f.up);
  if (!(width >= 1 && run >= 1)) return null;
  return { o: lerp(b0, b1, 0.5), up: f.up, across: mul(sub(b1, b0), 1 / width), width, run };
}

/** The two plan points where a flight's sides are `u` up from its first riser. */
export function acrossAt(fr: FlightFrame, u: number): [P, P] {
  const hw = fr.width / 2;
  const c = add(fr.o, mul(fr.up, u));
  return [sub(c, mul(fr.across, hw)), add(c, mul(fr.across, hw))];
}

/** Where the plan's cut passes through a flight. */
export interface FlightCut {
  frame: FlightFrame;
  /** Depth of one tread. */
  going: number;
  /** The first tread whose top is over the cut; null when the whole flight is under it. */
  above: number | null;
  /** How far up the flight the solid part reaches. */
  split: number;
}

/** As `guhit_export::stairs::stair_up_items` splits a flight at the cut. */
export function flightCut(f: Flight, cutMm = STAIR_CUT_MM): FlightCut | null {
  const frame = flightFrame(f);
  if (!frame) return null;
  const going = f.nosings.length > 0 ? frame.run / f.nosings.length : frame.run;
  const k = f.nosings.findIndex((n) => n.z > cutMm);
  const above = k < 0 ? null : k;
  const split = above === null ? frame.run : going * above;
  return { frame, going, above, split };
}

/** The break line across a flight at `u`: a slanted line with a small zig in the middle. */
export function breakLine(fr: FlightFrame, u: number, going: number): P[] {
  const hw = fr.width / 2;
  const slant = Math.min(going, 300);
  const a = add(sub(fr.o, mul(fr.across, hw)), mul(fr.up, u - slant / 2));
  const b = add(add(fr.o, mul(fr.across, hw)), mul(fr.up, u + slant / 2));
  const m = lerp(a, b, 0.5);
  const z = unit(sub(b, a));
  const k = Math.min(going, 150) * 0.5;
  const p1 = sub(m, mul(z, k));
  const p2 = add(m, mul(z, k));
  const zig = mul(left(z), k * 0.8);
  return [a, p1, add(add(p1, zig), mul(z, k * 0.5)), sub(sub(p2, zig), mul(z, k * 0.5)), p2, b];
}

/** Every plan piece a stair stands on: its flights, then its landings. */
export function stairPieces(g: StairGeometry): P[][] {
  return [...g.flights.map((f) => f.outline), ...g.landings.map((l) => l.polygon)];
}

/**
 * The walking line in plan from the middle of the first tread to the middle
 * of the last, bends only: what the travel arrow follows round the turn
 * (`guhit_export::stairs::arrow_path`).
 */
export function arrowPath(g: StairGeometry): P[] {
  const w = g.walk_line;
  if (w.length < 3) return w.map((p) => ({ x: p.x, y: p.y }));
  const pts: P[] = [];
  for (const p of w.slice(1, -1)) {
    const q = { x: p.x, y: p.y };
    const last = pts[pts.length - 1];
    if (!last || dist(last, q) > 1e-6) pts.push(q);
  }
  const out: P[] = [];
  pts.forEach((p, i) => {
    if (i === 0 || i === pts.length - 1) {
      out.push(p);
      return;
    }
    const a = sub(pts[i], pts[i - 1]);
    const b = sub(pts[i + 1], pts[i]);
    const c = cross(a, b);
    if (Math.abs(c) > 1e-6 * Math.hypot(a.x, a.y) * Math.hypot(b.x, b.y)) out.push(p);
  });
  return out;
}

/** The plain outline of a stair with no geometry yet, as Wave 1 drew it: width by run from the first riser. */
export function plainStairOutline(s: Pick<StairEl, "origin" | "rotation_deg" | "width_mm" | "run_mm">): P[] {
  const hw = s.width_mm / 2;
  return [
    { x: -hw, y: 0 },
    { x: hw, y: 0 },
    { x: hw, y: s.run_mm },
    { x: -hw, y: s.run_mm },
  ].map((p) => add(s.origin, rotate(p, s.rotation_deg)));
}

/** True when `p` is on the stair: inside a flight or a landing. */
export function stairCovers(g: StairGeometry, p: P): boolean {
  return stairPieces(g).some((poly) => poly.length >= 3 && pointInPolygon(p, poly));
}

/** Convex hull, counter-clockwise (monotone chain). */
export function convexHull(points: readonly P[]): P[] {
  const pts = [...points].sort((a, b) => a.x - b.x || a.y - b.y);
  if (pts.length < 3) return pts;
  const half = (list: P[]): P[] => {
    const h: P[] = [];
    for (const p of list) {
      while (h.length >= 2 && cross(sub(h[h.length - 1], h[h.length - 2]), sub(p, h[h.length - 2])) <= 0) h.pop();
      h.push(p);
    }
    h.pop();
    return h;
  };
  return [...half(pts), ...half([...pts].reverse())];
}

/**
 * A stair's plan shape for marquee selection, bounds and the settle flash:
 * the hole it makes in the floor above (its outline, landings and the U's
 * well included), else its one flight, else the hull of its pieces.
 */
export function stairShape(el: StairEl, index: GeomIndex): { points: P[]; closed: true } {
  const g = stairGeometryFor(el, index);
  if (!g) return { points: plainStairOutline(el), closed: true };
  if (g.opening.length >= 3) return { points: g.opening, closed: true };
  const pieces = stairPieces(g);
  if (pieces.length === 1) return { points: pieces[0], closed: true };
  return { points: convexHull(pieces.flat()), closed: true };
}

// ---------------------------------------------------------------- roofs

export interface RoofEdge {
  kind: RoofLineKind;
  a: P;
  b: P;
}

/** A roof's lines in plan: its outline at the eaves (eaves, rakes, a shed's high side) and the ridges, hips and valleys inside it. */
export interface RoofPlan {
  outline: RoofEdge[];
  inner: RoofEdge[];
}

const roofPlans = new WeakMap<RoofGeometry, RoofPlan>();

/** True when planes lie on both sides of the segment a to b (an inner line), probed just off its middle. */
function planesBothSides(g: RoofGeometry, a: P, b: P): boolean {
  const d = unit(sub(b, a));
  const m = lerp(a, b, 0.5);
  const n = mul(left(d), 5);
  const inside = (p: P): boolean => g.planes.some((pl) => pl.length >= 3 && pointInPolygon(p, pl));
  return inside(add(m, n)) && inside(sub(m, n));
}

/** Sorts a roof's lines into its outline and its inner lines. */
export function roofPlan(g: RoofGeometry): RoofPlan {
  const hit = roofPlans.get(g);
  if (hit) return hit;
  const plan: RoofPlan = { outline: [], inner: [] };
  for (const l of g.lines) {
    const e: RoofEdge = { kind: l.kind, a: { x: l.a.x, y: l.a.y }, b: { x: l.b.x, y: l.b.y } };
    if (dist(e.a, e.b) < 1e-6) continue;
    const onOutline = l.kind === "eave" || l.kind === "rake" || (l.kind === "ridge" && !planesBothSides(g, e.a, e.b));
    (onOutline ? plan.outline : plan.inner).push(e);
  }
  roofPlans.set(g, plan);
  return plan;
}

/** The outline edges chained into closed loops, largest first. */
export function roofLoops(g: RoofGeometry): P[][] {
  const edges = roofPlan(g).outline.map((e) => [e.a, e.b] as [P, P]);
  const used = new Array<boolean>(edges.length).fill(false);
  const loops: P[][] = [];
  const near = (p: P, q: P): boolean => dist(p, q) < 1;
  for (let i = 0; i < edges.length; i++) {
    if (used[i]) continue;
    used[i] = true;
    const loop: P[] = [edges[i][0], edges[i][1]];
    for (;;) {
      const end = loop[loop.length - 1];
      if (loop.length > 2 && near(end, loop[0])) {
        loop.pop();
        break;
      }
      let next = -1;
      let flip = false;
      for (let j = 0; j < edges.length && next < 0; j++) {
        if (used[j]) continue;
        if (near(edges[j][0], end)) next = j;
        else if (near(edges[j][1], end)) {
          next = j;
          flip = true;
        }
      }
      if (next < 0) break;
      used[next] = true;
      loop.push(flip ? edges[next][0] : edges[next][1]);
    }
    if (loop.length >= 3) loops.push(loop);
  }
  const area = (poly: P[]): number => Math.abs(poly.reduce((s, p, i) => s + cross(p, poly[(i + 1) % poly.length]), 0));
  return loops.sort((a, b) => area(b) - area(a));
}

/** True when `p` is within `tol` of one of the roof's lines or eave runs. */
export function nearRoofLine(g: RoofGeometry, p: P, tol: number): boolean {
  const near = (a: P, b: P): boolean => distToSeg(p, a, b) <= tol;
  for (const l of g.lines) if (near(l.a, l.b)) return true;
  for (const run of g.eaves) for (let i = 0; i + 1 < run.length; i++) if (near(run[i], run[i + 1])) return true;
  return false;
}

function distToSeg(p: P, a: P, b: P): number {
  const ab = sub(b, a);
  const l2 = dot(ab, ab);
  const t = l2 > 0 ? Math.max(0, Math.min(1, dot(sub(p, a), ab) / l2)) : 0;
  return dist(p, add(a, mul(ab, t)));
}

/**
 * A roof's plan shape for marquee selection, bounds and the settle flash:
 * its outline at the eaves, on the level it sits on only. A drawn roof the
 * engine has not answered for keeps its drawn outline.
 */
export function roofShape(el: RoofEl, index: GeomIndex & Pick<DocIndex, "levelId">): { points: P[]; closed: true } | null {
  const g = roofGeometryFor(el, index);
  if (g) {
    if (g.level_id !== index.levelId) return null;
    const loop = roofLoops(g)[0];
    return loop ? { points: loop, closed: true } : null;
  }
  if (el.outline.type === "polygon" && el.level_id === index.levelId && el.outline.points.length >= 3) return { points: el.outline.points, closed: true };
  return null;
}

/** Narrows an element to a stair or a roof. */
export function isRoofOrStair(el: Element): el is StairEl | RoofEl {
  return el.kind === "stair" || el.kind === "roof";
}
