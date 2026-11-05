// Stairs and roofs on the plan (lane L2), drawn from `Derived::stairs` and
// `Derived::roofs` as the sheet export draws them
// (`guhit_export::stairs::{stair_up_items, stair_down_items}`): a stair on
// its own level solid up to the plan's cut 1200 mm over the floor, a break
// line there and what is above dashed, the travel arrow along the walking
// line round the turn and UP; a stair of the level below seen from the
// level it arrives at dashed, with the arrow from the top and DN; the hole
// it makes in the floor kept clear of room fills; a roof dashed as overhead
// on the level it sits on. render.ts calls in here one line per pass.
// A stair the engine has not answered for yet draws its plain outline.

import type { Element, RoofGeometry, StairGeometry } from "../contract/bindings";
import type { RoofEl } from "../contract/roof";
import type { P } from "./geom";
import { add, angleDeg, dist, mul, readableDeg, sub, unit } from "./geom";
import type { ElementStyle, RenderContext } from "./render";
import { path, seg } from "./render";
import type { FlightFrame } from "./roofsStairsGeom";
import { STAIR_CUT_MM, acrossAt, arrowPath, breakLine, flightCut, flightFrame, plainStairOutline, roofGeometryFor, roofLoops, roofPlan, stairGeometryFor, stairPieces } from "./roofsStairsGeom";
import type { StairEl } from "./slab";
import { stairTreadOffsets } from "./slab";
import { stairGhost } from "./stairTool";
import { toScreen } from "./view";

/** Overhead and hidden lines: long dashes for an outline, short for the lines inside it. */
const DASH = [6, 4];
const DASH_ROOF = [9, 4];
const DASH_ROOF_INNER = [3, 3];
/** "UP" and "DN", CSS px. */
const LABEL_PX = 9;

const scaled = (d: readonly number[], k: number): number[] => d.map((v) => v * k);

/** The stairs pass: the level below's stairs arriving here (DN), then this level's (UP). */
export function drawStairs(rc: RenderContext, stairs: readonly StairEl[], styleOf: (id: string) => ElementStyle | null): void {
  for (const s of rc.index.arriving) drawStairFromBelow(rc, s, styleOf(s.id));
  for (const s of stairs) drawStair(rc, s, styleOf(s.id));
}

/** Clips to everything but the holes the stairs arriving on this level make in its floor (L and U holes are not rectangles). */
export function clipOutStairWells(rc: RenderContext): void {
  const { ctx, view } = rc;
  ctx.beginPath();
  ctx.rect(-1, -1, rc.width + 2, rc.height + 2);
  for (const st of rc.index.arriving) {
    const g = stairGeometryFor(st, rc.index);
    const holes = !g ? [plainStairOutline(st)] : g.opening.length >= 3 ? [g.opening] : stairPieces(g);
    for (const hole of holes) {
      hole.forEach((p, i) => {
        const q = toScreen(view, p);
        if (i === 0) ctx.moveTo(q.x, q.y);
        else ctx.lineTo(q.x, q.y);
      });
      ctx.closePath();
    }
  }
  ctx.clip("evenodd");
}

/** A stair on its own level (UP), from the engine's geometry when there is one. */
export function drawStair(rc: RenderContext, s: StairEl, style: ElementStyle | null): void {
  const g = stairGeometryFor(s, rc.index, stairGhost);
  if (g) drawStairUp(rc, g, style);
  else drawPlainStair(rc, s, style);
}

/** A stair of the level below seen from the level it arrives at (DN). */
export function drawStairFromBelow(rc: RenderContext, s: StairEl, style: ElementStyle | null = null): void {
  const g = stairGeometryFor(s, rc.index);
  if (g) drawStairDown(rc, g, style);
  else drawPlainStairFromBelow(rc, s, style);
}

// ---------------------------------------------------------------- from the engine's geometry

/** The pen of one stroke: solid unless the element is drawn dashed (a removed preview). */
interface Pens {
  solid(width: number): void;
  dashed(width: number): void;
}

function pens(rc: RenderContext, style: ElementStyle | null): Pens {
  const { ctx } = rc;
  const k = rc.uiScale;
  return {
    solid: (w) => {
      ctx.setLineDash(style?.dashed ? [5, 4] : []);
      ctx.lineWidth = w * k;
    },
    dashed: (w) => {
      ctx.setLineDash(scaled(DASH, k));
      ctx.lineWidth = w * k;
    },
  };
}

/** A flight's outline from `u0` to `u1` up it: its sides, and the riser lines at the ends that are real ends. */
function flightPart(rc: RenderContext, fr: FlightFrame, u0: number, u1: number, bottom: boolean, top: boolean): void {
  const [l0, r0] = acrossAt(fr, u0);
  const [l1, r1] = acrossAt(fr, u1);
  seg(rc, l0, l1);
  seg(rc, r0, r1);
  if (bottom) seg(rc, l0, r0);
  if (top) seg(rc, l1, r1);
}

/**
 * The travel arrow along `pts`: the line, a dot where it starts, an open
 * head at its end as long as a fifth of the stair's width (at most 260 mm,
 * and never longer than the last stretch).
 */
function travelArrow(rc: RenderContext, pts: readonly P[], widthMm: number): void {
  if (pts.length < 2) return;
  const { ctx } = rc;
  path(rc, pts, false);
  ctx.stroke();
  const end = pts[pts.length - 1];
  const before = pts[pts.length - 2];
  const u = unit(sub(end, before));
  const across = { x: -u.y, y: u.x };
  const head = Math.min(widthMm * 0.22, 260, dist(end, before) * 0.9);
  seg(rc, end, add(end, add(mul(u, -head), mul(across, head * 0.6))));
  seg(rc, end, add(end, add(mul(u, -head), mul(across, -head * 0.6))));
  const sp = toScreen(rc.view, pts[0]);
  ctx.beginPath();
  ctx.arc(sp.x, sp.y, 2.5 * rc.uiScale, 0, Math.PI * 2);
  ctx.fill();
}

/** "UP" or "DN" centered on `at`, readable along `along`, once the stair is wide enough on screen to hold it. */
function stairLabel(rc: RenderContext, fr: FlightFrame, at: (labelMm: number) => P, text: string): void {
  if (fr.width * rc.view.scale <= 40 * rc.uiScale) return;
  const { ctx, palette } = rc;
  const px = LABEL_PX * rc.uiScale;
  const p = toScreen(rc.view, at(px / rc.view.scale));
  const rot = readableDeg(angleDeg(fr.up) - 90);
  ctx.save();
  ctx.translate(p.x, p.y);
  ctx.rotate((-rot * Math.PI) / 180);
  ctx.font = `${px}px ${palette.fontMono}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(text, 0, 0);
  ctx.restore();
}

/**
 * A stair on its own level: below the cut its flights and landings solid
 * with their tread lines; a break line at the cut; above it dashed. The
 * arrow from the first tread round the turn to the last, "UP" before the
 * first riser.
 */
export function drawStairUp(rc: RenderContext, g: StairGeometry, style: ElementStyle | null): void {
  const { ctx, palette } = rc;
  const color = style?.color ?? palette.ink2;
  const pen = pens(rc, style);
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineJoin = "round";
  if (!style?.hollow) {
    ctx.save();
    ctx.globalAlpha *= 0.75;
    ctx.fillStyle = palette.surface;
    for (const piece of stairPieces(g)) {
      path(rc, piece);
      ctx.fill();
    }
    ctx.restore();
  }
  let cutDrawn = false;
  for (const f of g.flights) {
    const c = flightCut(f, STAIR_CUT_MM);
    if (!c) continue;
    const { frame, split, above, going } = c;
    if (split > 0) {
      pen.solid(1.2);
      flightPart(rc, frame, 0, split, true, above === null);
    }
    if (split < frame.run) {
      pen.dashed(0.8);
      flightPart(rc, frame, split, frame.run, split === 0, true);
    }
    // The first nosing is the first riser, drawn with the outline.
    f.nosings.forEach((n, i) => {
      if (i === 0) return;
      if (n.z > STAIR_CUT_MM) pen.dashed(0.7);
      else pen.solid(0.8);
      seg(rc, n.a, n.b);
    });
    if (!cutDrawn && above !== null && above > 0) {
      pen.solid(0.8);
      path(rc, breakLine(frame, split, going), false);
      ctx.stroke();
      cutDrawn = true;
    }
  }
  for (const l of g.landings) {
    if (l.z > STAIR_CUT_MM) pen.dashed(0.8);
    else pen.solid(1.2);
    path(rc, l.polygon);
    ctx.stroke();
  }
  const first = g.flights.length > 0 ? flightFrame(g.flights[0]) : null;
  pen.solid(1.2);
  travelArrow(rc, arrowPath(g), first?.width ?? 1000);
  if (first) stairLabel(rc, first, (th) => sub(first.o, mul(first.up, th * 1.1)), "UP");
  ctx.restore();
}

/**
 * A stair seen from the level it arrives at: every flight and landing
 * dashed with its tread lines, the arrow from the top down the walking
 * line, "DN" just past the top.
 */
export function drawStairDown(rc: RenderContext, g: StairGeometry, style: ElementStyle | null): void {
  const { ctx, palette } = rc;
  const color = style?.color ?? palette.ink2;
  const pen = pens(rc, style);
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineJoin = "round";
  for (const f of g.flights) {
    const fr = flightFrame(f);
    if (!fr) continue;
    pen.dashed(1.1);
    flightPart(rc, fr, 0, fr.run, true, true);
    pen.dashed(0.7);
    for (const n of f.nosings.slice(1)) seg(rc, n.a, n.b);
  }
  pen.dashed(1.1);
  for (const l of g.landings) {
    path(rc, l.polygon);
    ctx.stroke();
  }
  const last = g.flights.length > 0 ? flightFrame(g.flights[g.flights.length - 1]) : null;
  pen.solid(1.1);
  travelArrow(rc, [...arrowPath(g)].reverse(), last?.width ?? 1000);
  if (last) stairLabel(rc, last, (th) => add(last.o, mul(last.up, last.run + th * 1.1)), "DN");
  ctx.restore();
}

// ---------------------------------------------------------------- before the engine answers (Wave 1)

/** A stair with no geometry yet: its width by run outline, Wave 1 tread lines and an UP arrow. */
export function drawPlainStair(rc: RenderContext, s: StairEl, style: ElementStyle | null): void {
  const { ctx, palette } = rc;
  const outline = plainStairOutline(s);
  const run = unit(sub(outline[3], outline[0]));
  const across = unit(sub(outline[1], outline[0]));
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.strokeStyle = style?.color ?? palette.ink2;
  ctx.fillStyle = palette.surface;
  ctx.lineWidth = 1.2 * rc.uiScale;
  if (style?.dashed) ctx.setLineDash([5, 4]);
  path(rc, outline);
  if (!style?.hollow) {
    ctx.save();
    ctx.globalAlpha *= 0.75;
    ctx.fill();
    ctx.restore();
  }
  ctx.stroke();
  ctx.lineWidth = 0.8 * rc.uiScale;
  for (const d of stairTreadOffsets(s.run_mm, s.riser_count)) {
    const a = add(outline[0], mul(run, d));
    seg(rc, a, add(a, mul(across, s.width_mm)));
  }
  const start = add(s.origin, mul(run, s.run_mm * 0.04));
  const end = add(s.origin, mul(run, s.run_mm * 0.94));
  ctx.lineWidth = 1.2 * rc.uiScale;
  ctx.fillStyle = style?.color ?? palette.ink2;
  travelArrow(rc, [start, end], s.width_mm);
  const fr: FlightFrame = { o: s.origin, up: run, across, width: s.width_mm, run: s.run_mm };
  stairLabel(rc, fr, (th) => sub(fr.o, mul(fr.up, th * 1.1)), "UP");
  ctx.restore();
}

/** A stair of the level below with no geometry: its outline and treads dashed, DN down the flight. */
export function drawPlainStairFromBelow(rc: RenderContext, s: StairEl, style: ElementStyle | null = null): void {
  const { ctx, palette } = rc;
  const outline = plainStairOutline(s);
  const run = unit(sub(outline[3], outline[0]));
  const across = unit(sub(outline[1], outline[0]));
  const color = style?.color ?? palette.ink2;
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.setLineDash(scaled(DASH, rc.uiScale));
  ctx.lineWidth = 1.1 * rc.uiScale;
  path(rc, outline);
  ctx.stroke();
  ctx.lineWidth = 0.7 * rc.uiScale;
  for (const d of stairTreadOffsets(s.run_mm, s.riser_count)) {
    const a = add(outline[0], mul(run, d));
    seg(rc, a, add(a, mul(across, s.width_mm)));
  }
  ctx.setLineDash([]);
  ctx.lineWidth = 1.1 * rc.uiScale;
  travelArrow(rc, [add(s.origin, mul(run, s.run_mm * 0.96)), add(s.origin, mul(run, s.run_mm * 0.08))], s.width_mm);
  const fr: FlightFrame = { o: s.origin, up: run, across, width: s.width_mm, run: s.run_mm };
  stairLabel(rc, fr, (th) => add(fr.o, mul(fr.up, fr.run + th * 1.1)), "DN");
  ctx.restore();
}

// ---------------------------------------------------------------- roofs

/** The roofs pass, over everything else on the plan: each roof on the level it sits on. */
export function drawRoofs(rc: RenderContext, roofs: readonly RoofEl[], styleOf: (id: string) => ElementStyle | null): void {
  for (const r of roofs) drawRoof(rc, r, styleOf(r.id));
}

/**
 * One roof, on the level it sits on only (the roof over the top floor
 * belongs to no level, so it reaches every level's pass): its outline at
 * the eaves dashed as overhead, its ridges, hips and valleys thin dashed. A
 * drawn roof the engine has not answered for draws its drawn outline.
 */
export function drawRoof(rc: RenderContext, roof: RoofEl, style: ElementStyle | null): void {
  const g = roofGeometryFor(roof, rc.index);
  if (g) {
    if (g.level_id === rc.index.levelId) drawRoofLines(rc, g, style);
    return;
  }
  if (roof.outline.type !== "polygon" || roof.level_id !== rc.index.levelId || roof.outline.points.length < 2) return;
  const { ctx, palette } = rc;
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.strokeStyle = style?.color ?? palette.ink2;
  ctx.lineWidth = 1.1 * rc.uiScale;
  ctx.setLineDash(scaled(DASH_ROOF, rc.uiScale));
  path(rc, roof.outline.points);
  ctx.stroke();
  ctx.restore();
}

/** A roof's lines: the outline long dashed, the inner lines short dashed and lighter. `weight` thickens both (highlights). */
export function drawRoofLines(rc: RenderContext, g: RoofGeometry, style: ElementStyle | null, weight = 1): void {
  const { ctx, palette } = rc;
  const k = rc.uiScale;
  const plan = roofPlan(g);
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.lineCap = "round";
  ctx.strokeStyle = style?.color ?? palette.ink3;
  ctx.lineWidth = 0.75 * k * weight;
  ctx.setLineDash(scaled(DASH_ROOF_INNER, k));
  for (const e of plan.inner) seg(rc, e.a, e.b);
  ctx.strokeStyle = style?.color ?? palette.ink2;
  ctx.lineWidth = 1.1 * k * weight;
  ctx.setLineDash(scaled(DASH_ROOF, k));
  // Each loop as one path, so the dashes run on round its corners.
  const loops = roofLoops(g);
  if (loops.length > 0) {
    for (const loop of loops) {
      path(rc, loop);
      ctx.stroke();
    }
  } else for (const e of plan.outline) seg(rc, e.a, e.b);
  ctx.restore();
}

// ---------------------------------------------------------------- highlights

/**
 * Selection, hover and a live peer's selection of a stair or a roof: the
 * stair's pieces tinted with its outline, or the roof's area tinted with
 * every line of it in `color`. For render.ts `drawHighlight` and
 * `drawPeerOutline`, which outline other kinds by their plan shape.
 */
export function highlightRoofOrStair(rc: RenderContext, el: Element, strength: "hover" | "selected", alpha = 1, color = rc.palette.selection): void {
  if (alpha <= 0.002 || (el.kind !== "stair" && el.kind !== "roof")) return;
  const { ctx } = rc;
  const sel = strength === "selected";
  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineJoin = "round";
  if (el.kind === "stair") {
    const g = stairGeometryFor(el, rc.index, stairGhost);
    const pieces = g ? stairPieces(g) : [plainStairOutline(el)];
    ctx.globalAlpha = (sel ? 0.16 : 0.1) * alpha;
    for (const p of pieces) {
      path(rc, p);
      ctx.fill();
    }
    ctx.globalAlpha = (sel ? 1 : 0.7) * alpha;
    ctx.lineWidth = sel ? 2 : 1.5;
    for (const p of g && g.opening.length >= 3 ? [g.opening] : pieces) {
      path(rc, p);
      ctx.stroke();
    }
    ctx.restore();
    return;
  }
  const g = roofGeometryFor(el, rc.index);
  if (!g || g.level_id !== rc.index.levelId) {
    ctx.restore();
    if (!g) drawRoof(rc, el, { color, alpha: (sel ? 1 : 0.7) * alpha });
    return;
  }
  ctx.globalAlpha = (sel ? 0.1 : 0.06) * alpha;
  for (const loop of roofLoops(g)) {
    path(rc, loop);
    ctx.fill();
  }
  ctx.restore();
  drawRoofLines(rc, g, { color, alpha: (sel ? 1 : 0.75) * alpha }, sel ? 1.7 : 1.35);
}
