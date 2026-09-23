// Snapping engine. Pure: takes the cursor, a scene of snap geometry and
// options, returns the snapped point, its type and guide lines to draw.
// Tested in snap.test.ts.

import type { P, Seg } from "./geom";
import {
  add,
  angleDeg,
  angleDiffDeg,
  clamp,
  closestOnLine,
  closestOnSegment,
  dirDeg,
  dist,
  dot,
  lineLineIntersection,
  mid,
  mul,
  projectParam,
  roundTo,
  segSegIntersection,
  sub,
  unit,
  len,
} from "./geom";

export type SnapType =
  | "none"
  | "endpoint"
  | "midpoint"
  | "intersection"
  | "perpendicular"
  | "nearest"
  | "face"
  | "extension"
  | "angle"
  | "grid"
  // Pipe tool targets (pipe.ts): a pipe end or interior point, a tee on a pipe, a fixture.
  | "pipe_end"
  | "pipe_joint"
  | "tee"
  | "fixture";

export interface SnapGuide {
  from: P;
  to: P;
  kind: "extension" | "angle";
}

export interface SnapResult {
  point: P;
  type: SnapType;
  guides: SnapGuide[];
  /** True when the point lies on a locked angle ray from the anchor. */
  angleLocked: boolean;
  /** Pipe snaps: the height of the pipe joined there. */
  z?: number | null;
  /** Replaces the label of the snap type, for example a fixture name. */
  label?: string;
}

export interface SnapScene {
  /** Wall centerlines. */
  centerlines: Seg[];
  /** Wall face edges. Only used when `useFaces` is on. */
  faces: Seg[];
  /** Wall endpoints, column centers. Used for point snaps and alignment guides. */
  points: P[];
  /** Face corners. Point snaps only, and only when `useFaces` is on. */
  facePoints: P[];
  /** Centerline crossings. */
  intersections: P[];
}

export interface SnapOptions {
  enabled: boolean;
  /** Forces 0/90 angle lock from the anchor (Shift or the ortho toggle). */
  ortho: boolean;
  /** Pick radius in mm (pixels divided by scale). */
  tolerance: number;
  /** Grid step in mm. 0 turns grid snapping off. */
  gridStep: number;
  /** Previous point of the operation, for angle lock and perpendicular. */
  anchor: P | null;
  angleStepDeg?: number;
  angleToleranceDeg?: number;
  useFaces?: boolean;
}

export function emptyScene(): SnapScene {
  return { centerlines: [], faces: [], points: [], facePoints: [], intersections: [] };
}

/** Fills `intersections` from the centerlines. Call once after building a scene. */
export function computeIntersections(scene: SnapScene): void {
  const out: P[] = [];
  const c = scene.centerlines;
  for (let i = 0; i < c.length; i++) {
    for (let j = i + 1; j < c.length; j++) {
      const p = segSegIntersection(c[i].a, c[i].b, c[j].a, c[j].b);
      if (p) out.push(p);
    }
  }
  scene.intersections = out;
}

function nearestPoint(cursor: P, points: readonly P[], tol: number): P | null {
  let best: P | null = null;
  let bestD = tol;
  for (const p of points) {
    const d = dist(cursor, p);
    if (d <= bestD) {
      bestD = d;
      best = p;
    }
  }
  return best;
}

function nearestOnSegments(cursor: P, segs: readonly Seg[], tol: number): { point: P; seg: Seg } | null {
  let best: { point: P; seg: Seg } | null = null;
  let bestD = tol;
  for (const s of segs) {
    const c = closestOnSegment(cursor, s.a, s.b);
    if (c.dist <= bestD) {
      bestD = c.dist;
      best = { point: c.point, seg: s };
    }
  }
  return best;
}

function snapGrid(p: P, step: number): P {
  return step > 0 ? { x: roundTo(p.x, step), y: roundTo(p.y, step) } : p;
}

/** Nearest locked angle (multiple of `step`) to the direction anchor -> cursor. */
export function nearestLockAngle(anchor: P, cursor: P, step: number): { angle: number; diff: number } {
  const a = angleDeg(sub(cursor, anchor));
  const angle = roundTo(a, step) % 360;
  return { angle, diff: angleDiffDeg(a, angle) };
}

function rayPoint(anchor: P, d: P, cursor: P): { point: P; t: number } {
  const t = Math.max(0, dot(sub(cursor, anchor), d));
  return { point: add(anchor, mul(d, t)), t };
}

export function snap(cursor: P, scene: SnapScene, opt: SnapOptions): SnapResult {
  const tol = opt.tolerance;
  const anchor = opt.anchor;
  const none: SnapResult = { point: cursor, type: "none", guides: [], angleLocked: false };

  // Angle lock state.
  let lockDir: P | null = null;
  const forced = !!anchor && opt.ortho;
  if (anchor && dist(anchor, cursor) > Math.min(tol, 1)) {
    const step = opt.ortho ? 90 : (opt.angleStepDeg ?? 45);
    const lock = nearestLockAngle(anchor, cursor, step);
    const angTol = opt.angleToleranceDeg ?? 4;
    if (forced || (opt.enabled && lock.diff <= angTol)) lockDir = dirDeg(lock.angle);
  }

  if (!opt.enabled) {
    if (forced && anchor && lockDir) {
      const r = rayPoint(anchor, lockDir, cursor);
      return { point: r.point, type: "angle", guides: [{ from: anchor, to: r.point, kind: "angle" }], angleLocked: true };
    }
    return none;
  }

  // Point snaps, in priority order.
  const pointSets: [SnapType, readonly P[]][] = [
    ["endpoint", scene.points],
    ["intersection", scene.intersections],
    ["midpoint", scene.centerlines.map((s) => mid(s.a, s.b))],
  ];
  if (opt.useFaces) pointSets.splice(1, 0, ["endpoint", scene.facePoints]);
  let objPoint: P | null = null;
  let objType: SnapType = "none";
  for (const [type, pts] of pointSets) {
    const p = nearestPoint(cursor, pts, tol);
    if (p) {
      objPoint = p;
      objType = type;
      break;
    }
  }

  const segs = opt.useFaces ? [...scene.centerlines, ...scene.faces] : scene.centerlines;

  if (anchor && lockDir) {
    const onRay = (p: P): boolean => {
      const v = sub(p, anchor);
      const t = dot(v, lockDir as P);
      return t > 0 && dist(add(anchor, mul(lockDir as P, t)), p) < 0.5;
    };
    const angleGuide = (to: P): SnapGuide => ({ from: anchor, to, kind: "angle" });

    if (objPoint && onRay(objPoint)) {
      return { point: objPoint, type: objType, guides: [angleGuide(objPoint)], angleLocked: true };
    }
    if (objPoint && !forced) {
      return { point: objPoint, type: objType, guides: [], angleLocked: false };
    }
    const base = rayPoint(anchor, lockDir, cursor);
    const far = add(anchor, mul(lockDir, base.t + tol * 4 + 1));

    // Forced ortho with a nearby object point: align with it.
    if (objPoint && forced) {
      const t = dot(sub(objPoint, anchor), lockDir);
      if (t > 0) {
        const p = add(anchor, mul(lockDir, t));
        return {
          point: p,
          type: "extension",
          guides: [angleGuide(p), { from: objPoint, to: p, kind: "extension" }],
          angleLocked: true,
        };
      }
    }

    // Ray crossing a wall line close to the cursor.
    let bestCross: P | null = null;
    let bestCrossD = tol;
    for (const s of segs) {
      const x = segSegIntersection(anchor, far, s.a, s.b);
      if (!x) continue;
      const d = dist(x, base.point);
      if (d <= bestCrossD && dist(x, anchor) > 1) {
        bestCrossD = d;
        bestCross = x;
      }
    }
    if (bestCross) {
      return { point: bestCross, type: "intersection", guides: [angleGuide(bestCross)], angleLocked: true };
    }

    // Extension alignment: the ray meets the horizontal or vertical through a known point.
    let bestAlign: { p: P; q: P } | null = null;
    let bestAlignD = tol;
    for (const q of scene.points) {
      if (dist(q, anchor) < 1) continue;
      const lines: [P, P][] = [
        [q, add(q, { x: 1, y: 0 })],
        [q, add(q, { x: 0, y: 1 })],
      ];
      for (const [la, lb] of lines) {
        const x = lineLineIntersection(anchor, far, la, lb);
        if (!x) continue;
        if (dot(sub(x, anchor), lockDir) <= 1) continue;
        const d = dist(x, base.point);
        if (d < bestAlignD) {
          bestAlignD = d;
          bestAlign = { p: x, q };
        }
      }
    }
    if (bestAlign) {
      return {
        point: bestAlign.p,
        type: "extension",
        guides: [angleGuide(bestAlign.p), { from: bestAlign.q, to: bestAlign.p, kind: "extension" }],
        angleLocked: true,
      };
    }

    // Round the length along the ray to the grid step.
    const t = opt.gridStep > 0 ? roundTo(base.t, opt.gridStep) : base.t;
    const p = add(anchor, mul(lockDir, t));
    return { point: p, type: "angle", guides: [angleGuide(p)], angleLocked: true };
  }

  if (objPoint) return { point: objPoint, type: objType, guides: [], angleLocked: false };

  // Perpendicular from the anchor onto a wall line.
  if (anchor) {
    let best: P | null = null;
    let bestD = tol;
    for (const s of scene.centerlines) {
      const t = projectParam(anchor, s.a, s.b);
      if (t < 0 || t > 1) continue;
      const foot = closestOnLine(anchor, s.a, s.b);
      if (dist(foot, anchor) < 1) continue;
      const d = dist(foot, cursor);
      if (d <= bestD) {
        bestD = d;
        best = foot;
      }
    }
    if (best) return { point: best, type: "perpendicular", guides: [], angleLocked: false };
  }

  // On a wall line. Slide to a round distance from the segment start, or to an alignment crossing.
  const on = nearestOnSegments(cursor, segs, tol);
  if (on) {
    const isFace = scene.faces.includes(on.seg);
    const L = dist(on.seg.a, on.seg.b);
    const d = unit(sub(on.seg.b, on.seg.a));
    for (const q of scene.points) {
      const lines: [P, P][] = [
        [q, add(q, { x: 1, y: 0 })],
        [q, add(q, { x: 0, y: 1 })],
      ];
      for (const [la, lb] of lines) {
        const x = lineLineIntersection(on.seg.a, on.seg.b, la, lb);
        if (!x || dist(x, on.point) > tol * 0.6 || dist(x, q) < 1) continue;
        const t = dot(sub(x, on.seg.a), d);
        if (t < 0 || t > L) continue;
        return { point: x, type: "extension", guides: [{ from: q, to: x, kind: "extension" }], angleLocked: false };
      }
    }
    let t = dot(sub(on.point, on.seg.a), d);
    if (opt.gridStep > 0) t = clamp(roundTo(t, opt.gridStep), 0, L);
    return { point: add(on.seg.a, mul(d, t)), type: isFace ? "face" : "nearest", guides: [], angleLocked: false };
  }

  // Alignment with known points, free cursor.
  let ax: P | null = null;
  let ay: P | null = null;
  let bx = tol * 0.6;
  let by = tol * 0.6;
  for (const q of scene.points) {
    const dx = Math.abs(q.x - cursor.x);
    const dy = Math.abs(q.y - cursor.y);
    if (dx < bx) {
      bx = dx;
      ax = q;
    }
    if (dy < by) {
      by = dy;
      ay = q;
    }
  }
  if (ax || ay) {
    const g = snapGrid(cursor, opt.gridStep);
    const p = { x: ax ? ax.x : g.x, y: ay ? ay.y : g.y };
    const guides: SnapGuide[] = [];
    if (ax) guides.push({ from: ax, to: p, kind: "extension" });
    if (ay) guides.push({ from: ay, to: p, kind: "extension" });
    return { point: p, type: "extension", guides, angleLocked: false };
  }

  if (opt.gridStep > 0) return { point: snapGrid(cursor, opt.gridStep), type: "grid", guides: [], angleLocked: false };
  return none;
}

/** Length and angle of the segment anchor -> p, for readouts. */
export function polar(anchor: P, p: P): { length: number; angle: number } {
  const v = sub(p, anchor);
  return { length: len(v), angle: len(v) < 1e-9 ? 0 : angleDeg(v) };
}
