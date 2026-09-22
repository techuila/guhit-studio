// Pure helpers for editing operations: grips, ghosts of moved elements and
// of walls that stretch with them. Tested in edit.test.ts.

import type { Element, Wall } from "../contract/bindings";
import type { P } from "./geom";
import { add, angleDeg, dist, distToSegment, dot, left, lerp, mul, normDeg, roundTo, sub, unit } from "./geom";
import type { DocIndex } from "./model";
import { backDir, dimensionGeometry, type WallEl } from "./model";

export type GripKind =
  | "wall_start"
  | "wall_end"
  | "wall_mid"
  | "rotate"
  | "dim_offset"
  | "dim_a"
  | "dim_b"
  | "cam_pos"
  | "cam_target";

export interface Grip {
  elementId: string;
  kind: GripKind;
  pos: P;
}

/** Grips of one selected element. `pxMm` is the size of one pixel in mm. */
export function gripsFor(el: Element, pxMm: number): Grip[] {
  const g = (kind: GripKind, pos: P): Grip => ({ elementId: el.id, kind, pos });
  switch (el.kind) {
    case "wall":
      return [g("wall_start", el.start), g("wall_end", el.end), g("wall_mid", lerp(el.start, el.end, 0.5))];
    case "asset":
      return [g("rotate", add(el.position, mul(backDir(el.rotation_deg), el.depth_mm / 2 + 26 * pxMm)))];
    case "column": {
      const d = el.shape === "round" ? el.width_mm : el.depth_mm;
      return [g("rotate", add(el.center, mul(backDir(el.rotation_deg), d / 2 + 26 * pxMm)))];
    }
    case "stair":
      return [g("rotate", add(el.origin, mul(backDir(el.rotation_deg), el.run_mm + 26 * pxMm)))];
    case "reference_model":
      return [g("rotate", add(el.position, mul(backDir(el.rotation_deg), 300 + 26 * pxMm)))];
    case "dimension": {
      const dg = dimensionGeometry(el.a, el.b, el.offset_mm);
      return [g("dim_offset", dg.mid), g("dim_a", el.a), g("dim_b", el.b)];
    }
    case "camera":
      return [
        g("cam_pos", { x: el.position.x, y: el.position.y }),
        g("cam_target", { x: el.target.x, y: el.target.y }),
      ];
    default:
      return [];
  }
}

export function hitGrip(grips: readonly Grip[], p: P, tol: number): Grip | null {
  let best: Grip | null = null;
  let bestD = tol;
  for (const g of grips) {
    const d = dist(g.pos, p);
    if (d <= bestD) {
      bestD = d;
      best = g;
    }
  }
  return best;
}

/** Rotation for a rotate grip dragged to `cursor`. The grip sits on the local +y axis. */
export function rotationFromGrip(pivot: P, cursor: P, stepDeg: number): number {
  const a = angleDeg(sub(cursor, pivot)) - 90;
  return normDeg(stepDeg > 0 ? roundTo(a, stepDeg) : a);
}

/** Translation of a wall along its normal for a midpoint grip dragged to `cursor`. */
export function normalDelta(wall: Pick<Wall, "start" | "end">, cursor: P, step: number): P {
  const n = left(unit(sub(wall.end, wall.start)));
  const m = lerp(wall.start, wall.end, 0.5);
  let t = dot(sub(cursor, m), n);
  if (step > 0) t = roundTo(t, step);
  return mul(n, t);
}

export function translateElement(el: Element, d: P): Element {
  switch (el.kind) {
    case "wall":
      return { ...el, start: add(el.start, d), end: add(el.end, d) };
    case "room":
      return { ...el, seed: add(el.seed, d) };
    case "column":
      return { ...el, center: add(el.center, d) };
    case "stair":
      return { ...el, origin: add(el.origin, d) };
    case "asset":
      return { ...el, position: add(el.position, d) };
    case "annotation":
      return { ...el, position: add(el.position, d) };
    case "dimension":
      return { ...el, a: add(el.a, d), b: add(el.b, d) };
    case "camera":
      return {
        ...el,
        position: { ...el.position, x: el.position.x + d.x, y: el.position.y + d.y },
        target: { ...el.target, x: el.target.x + d.x, y: el.target.y + d.y },
      };
    case "underlay":
      return { ...el, position: add(el.position, d) };
    case "linework":
      return { ...el, polylines: el.polylines.map((pl) => pl.map((p) => add(p, d))) };
    case "reference_model":
      return { ...el, position: add(el.position, d) };
    case "opening":
      return el;
  }
}

const JOIN_TOL = 1;

/**
 * Walls as they will look after `moved` walls translate by `delta` with
 * stretch_connected: moved walls shift, walls that end on a moved wall
 * (corner or T joint) keep the joint and stretch. Matches the engine.
 */
export function stretchedWalls(index: DocIndex, movedIds: ReadonlySet<string>, delta: P): Wall[] {
  const walls = index.visible.filter((e): e is WallEl => e.kind === "wall");
  // An end follows when it sits on a moved wall: at a corner or mid span (T joint).
  const moved = walls.filter((w) => movedIds.has(w.id));
  const follows = (p: P): boolean => moved.some((m) => distToSegment(p, m.start, m.end) <= JOIN_TOL);
  const out: Wall[] = [];
  for (const w of walls) {
    if (movedIds.has(w.id)) {
      out.push({ ...w, start: add(w.start, delta), end: add(w.end, delta) });
      continue;
    }
    const s = follows(w.start);
    const e = follows(w.end);
    if (s || e) out.push({ ...w, start: s ? add(w.start, delta) : w.start, end: e ? add(w.end, delta) : w.end });
  }
  return out;
}

/** Walls as they will look after the joint at `from` moves to `to`. */
export function walledJointMove(index: DocIndex, from: P, to: P): Wall[] {
  const out: Wall[] = [];
  for (const w of index.visible) {
    if (w.kind !== "wall") continue;
    const s = dist(w.start, from) <= JOIN_TOL;
    const e = dist(w.end, from) <= JOIN_TOL;
    if (s || e) out.push({ ...w, start: s ? to : w.start, end: e ? to : w.end });
  }
  return out;
}

/** Half thickness of the thickest other wall meeting `wall` at the given end, or 0. */
export function jointInset(index: DocIndex, wall: Wall, end: "start" | "end"): number {
  const p = end === "start" ? wall.start : wall.end;
  let best = 0;
  for (const w of index.byId.values()) {
    if (w.kind !== "wall" || w.id === wall.id || w.level_id !== wall.level_id) continue;
    if (dist(w.start, p) <= JOIN_TOL || dist(w.end, p) <= JOIN_TOL) best = Math.max(best, w.thickness_mm / 2);
  }
  return best;
}
