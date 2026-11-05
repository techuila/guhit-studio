// Permit drawing on the plan (lane L1): grid lines with their bubbles,
// section and elevation marks, the lot with its bearings, door and window
// type tags (D1, W1, DECISIONS D53) and dimension strings. render.ts calls
// each pass once, after its own passes, and `drawElement` the one-element
// versions for ghosts and removed elements. Plain dimensions keep drawing in
// render.ts; a dimension string here matches their look segment by segment.
//
// Every pass takes the element's `ElementStyle` (preview tint, placed
// element fading up, ghost, removed and fading): `color` replaces every
// stroke, `alpha` scales the whole element, `dashed` dashes it and `hollow`
// leaves the symbols unfilled.

import type { Dimension, DocState, GridLine, Lot, Opening, ViewMark } from "../../contract/bindings";
import type { P } from "../geom";
import { add, angleDeg, dist, mul, readableDeg, sub, unit } from "../geom";
import { openingFrame } from "../model";
import type { ElementStyle, RenderContext } from "../render";
import { dimensionTextBox, path, seg } from "../render";
import type { Box } from "../tags";
import { boxAround, boxesOverlap } from "../tags";
import { formatLength } from "../typed";
import { toScreen } from "../view";
import { bearingText } from "./labels";
import {
  ELEVATION_TARGET_PX,
  GRID_BUBBLE_PX,
  SECTION_HEAD_PX,
  cornerOut,
  gridGeometry,
  isDimensionString,
  lotSides,
  markGeometry,
  stringGeometry,
} from "./shapes";

export { isDimensionString } from "./shapes";

type StyleOf = (id: string) => ElementStyle | null;

/** A plan direction on screen (y points down there). */
const screenDir = (v: P): P => ({ x: v.x, y: -v.y });

/** Text that fits a circle of radius `r` px: shrinks the font down to 70% before it lets the text spill. */
function fitText(ctx: CanvasRenderingContext2D, text: string, weight: string, sizePx: number, family: string, maxW: number): void {
  let size = sizePx;
  ctx.font = `${weight} ${size}px ${family}`;
  while (size > sizePx * 0.7 && ctx.measureText(text).width > maxW) {
    size -= 0.5;
    ctx.font = `${weight} ${size}px ${family}`;
  }
}

/** A filled arrow head from `c` toward `look`, its base `half` px either side, `reach` px long. */
function arrowHead(ctx: CanvasRenderingContext2D, c: P, look: P, half: number, reach: number): void {
  const l = screenDir(look);
  const side = { x: -l.y, y: l.x };
  ctx.beginPath();
  ctx.moveTo(c.x + l.x * reach, c.y + l.y * reach);
  ctx.lineTo(c.x + side.x * half, c.y + side.y * half);
  ctx.lineTo(c.x - side.x * half, c.y - side.y * half);
  ctx.closePath();
  ctx.fill();
}

// ------------------------------------------------------------------ grids

/** Grid lines, on every level. */
export function drawGrids(rc: RenderContext, grids: readonly GridLine[], styleOf: StyleOf): void {
  for (const g of grids) drawGridLine(rc, g, styleOf(g.id));
}

/** One grid line: a thin dash-dot line with a bubble and its label at both ends. */
export function drawGridLine(rc: RenderContext, grid: GridLine, style: ElementStyle | null): void {
  const { ctx, palette, view } = rc;
  const k = rc.uiScale;
  const rPx = GRID_BUBBLE_PX * k;
  const geo = gridGeometry(grid, rPx / view.scale);
  const color = style?.color ?? palette.ink2;
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = 0.9 * k;
  ctx.setLineDash(style?.dashed ? [5 * k, 4 * k] : [16 * k, 3 * k, 2 * k, 3 * k]);
  ctx.save();
  ctx.globalAlpha *= 0.8;
  seg(rc, grid.start, grid.end);
  ctx.restore();
  ctx.setLineDash(style?.dashed ? [3 * k, 2 * k] : []);
  for (const c of [geo.bubbleA, geo.bubbleB]) {
    const s = toScreen(view, c);
    ctx.beginPath();
    ctx.arc(s.x, s.y, rPx, 0, Math.PI * 2);
    if (!style?.hollow) {
      ctx.fillStyle = palette.paper;
      ctx.fill();
    }
    ctx.lineWidth = 1.2 * k;
    ctx.stroke();
    ctx.fillStyle = color;
    fitText(ctx, grid.label, "600", 10.5 * k, palette.fontUi, rPx * 1.6);
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(grid.label, s.x, s.y + 0.5 * k);
  }
  ctx.restore();
}

// ------------------------------------------------------------------ section and elevation marks

/** Section and elevation marks on this level. */
export function drawViewMarks(rc: RenderContext, marks: readonly ViewMark[], styleOf: StyleOf): void {
  for (const m of marks) drawViewMark(rc, m, styleOf(m.id));
}

/** The sheet a mark's view sits on ("A-5"), for the lower half of its head. */
export function sheetOfMark(doc: DocState, markId: string): string | null {
  for (const sh of doc.project.sheet_set?.sheets ?? []) {
    for (const vp of sh.viewports) {
      const v = vp.view;
      if ((v.type === "section" || v.type === "elevation") && v.mark_id === markId) return sh.number;
    }
  }
  return null;
}

/** "North" reads N, "North 2" N2, "A" stays A: what fits inside an elevation target. */
export function shortMark(label: string): string {
  const t = label.trim();
  if (t.length <= 2) return t;
  const words = t.split(/\s+/);
  const num = words.slice(1).find((w) => /^\d+$/.test(w)) ?? "";
  return (words[0][0] ?? "").toUpperCase() + num;
}

/**
 * One section or elevation mark. A section: the cut as a heavy dash-dot line,
 * a head at both ends (circle with its label, and the sheet it is drawn on
 * when it is on one) and a filled arrow the way it looks. An elevation: its
 * plane faint, a target at the middle with the arrow, and its name beside.
 */
export function drawViewMark(rc: RenderContext, mark: ViewMark, style: ElementStyle | null): void {
  const { ctx, palette, view } = rc;
  const k = rc.uiScale;
  const color = style?.color ?? palette.ink;
  const section = mark.mark_kind === "section";
  const rPx = (section ? SECTION_HEAD_PX : ELEVATION_TARGET_PX) * k;
  const geo = markGeometry(mark, rPx / view.scale);
  const sheet = sheetOfMark(rc.index.doc, mark.id);
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  const head = (c: P, text: string, lower: string | null): void => {
    const s = toScreen(view, c);
    ctx.fillStyle = color;
    arrowHead(ctx, s, geo.look, rPx * 0.98, rPx * 1.75);
    ctx.beginPath();
    ctx.arc(s.x, s.y, rPx, 0, Math.PI * 2);
    if (!style?.hollow) {
      ctx.fillStyle = palette.paper;
      ctx.fill();
    }
    ctx.lineWidth = 1.2 * k;
    ctx.stroke();
    ctx.fillStyle = color;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    if (lower) {
      ctx.beginPath();
      ctx.moveTo(s.x - rPx, s.y);
      ctx.lineTo(s.x + rPx, s.y);
      ctx.lineWidth = 0.8 * k;
      ctx.stroke();
      fitText(ctx, text, "600", 8.5 * k, palette.fontUi, rPx * 1.5);
      ctx.fillText(text, s.x, s.y - rPx * 0.45);
      fitText(ctx, lower, "500", 7 * k, palette.fontUi, rPx * 1.5);
      ctx.fillText(lower, s.x, s.y + rPx * 0.47);
    } else {
      fitText(ctx, text, "600", 10 * k, palette.fontUi, rPx * 1.6);
      ctx.fillText(text, s.x, s.y + 0.5 * k);
    }
  };
  if (section) {
    // The cut: dash-dot through, heavy for a short stretch at each end.
    ctx.lineWidth = 1 * k;
    ctx.setLineDash(style?.dashed ? [5 * k, 4 * k] : [18 * k, 3 * k, 3 * k, 3 * k]);
    seg(rc, mark.start, mark.end);
    ctx.setLineDash([]);
    const len = dist(mark.start, mark.end);
    const heavy = Math.min(len / 2, (16 * k) / view.scale);
    if (heavy > 0 && !style?.dashed) {
      ctx.lineWidth = 2.4 * k;
      seg(rc, mark.start, add(mark.start, mul(geo.dir, heavy)));
      seg(rc, mark.end, sub(mark.end, mul(geo.dir, heavy)));
    }
    head(geo.headA, mark.label, sheet);
    head(geo.headB, mark.label, sheet);
  } else {
    ctx.save();
    ctx.globalAlpha *= 0.55;
    ctx.lineWidth = 1 * k;
    ctx.setLineDash([6 * k, 3 * k]);
    seg(rc, mark.start, mark.end);
    ctx.restore();
    head(geo.mid, shortMark(mark.label), sheet);
    // The full name behind the target, away from the arrow.
    if (mark.label.trim().length > 2) {
      const s = toScreen(view, geo.mid);
      const back = screenDir(mul(geo.look, -1));
      ctx.fillStyle = color;
      ctx.font = `600 ${9.5 * k}px ${palette.fontUi}`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      const tw = ctx.measureText(mark.label).width;
      const reach = rPx + 6 * k + Math.abs(back.x) * (tw / 2) + Math.abs(back.y) * 6 * k;
      ctx.fillText(mark.label, s.x + back.x * reach, s.y + back.y * reach);
    }
  }
  ctx.restore();
}

// ------------------------------------------------------------------ the lot

/** A side's length as titles write it, in meters to the centimeter. */
export function lotLengthText(mm: number): string {
  return `${(mm / 1000).toFixed(2)} m`;
}

/**
 * The lot, on every level (at most one): its boundary, the setback outline
 * from `Derived::site` and the bearings and distances along each side.
 */
export function drawLot(rc: RenderContext, lots: readonly Lot[], styleOf: StyleOf): void {
  const site = rc.index.doc.derived.site;
  for (const lot of lots) {
    const style = styleOf(lot.id);
    if (site && site.lot_id === lot.id && site.setback_outline.length >= 3) {
      const { ctx, palette } = rc;
      const k = rc.uiScale;
      ctx.save();
      ctx.globalAlpha = (style?.alpha ?? 1) * 0.65;
      ctx.strokeStyle = style?.color ?? palette.ink3;
      ctx.lineWidth = 0.9 * k;
      ctx.setLineDash([6 * k, 4 * k]);
      path(rc, site.setback_outline);
      ctx.stroke();
      ctx.restore();
    }
    drawLotShape(rc, lot, style);
  }
}

/** The lot alone: boundary in a long dash line, corners numbered 1, 2, 3, each side's bearing and distance. */
export function drawLotShape(rc: RenderContext, lot: Lot, style: ElementStyle | null): void {
  const { ctx, palette, view } = rc;
  if (lot.corners.length < 2) return;
  const k = rc.uiScale;
  const color = style?.color ?? palette.ink;
  const site = rc.index.doc.derived.site;
  // The engine's bearings (`Derived::site`) for the stored lot, and a moved
  // copy of it; only a ghost with other corners (a draft) works its own out.
  const stored = rc.index.byId.get(lot.id);
  const same = !!stored && stored.kind === "lot" && stored.corners.length === lot.corners.length;
  const derived = site && same && site.lot_id === lot.id && site.sides.length === lot.corners.length ? site.sides : null;
  const north = rc.index.doc.project.settings.north_angle_deg;
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = 1.4 * k;
  ctx.setLineDash(style?.dashed ? [5 * k, 4 * k] : [20 * k, 4 * k, 3 * k, 4 * k, 3 * k, 4 * k]);
  path(rc, lot.corners);
  ctx.stroke();
  ctx.setLineDash([]);
  const sides = lotSides(lot);
  // Bearing outside each side, distance inside, both along it and upright.
  ctx.textAlign = "center";
  sides.forEach((s, i) => {
    const a = toScreen(view, s.from);
    const b = toScreen(view, s.to);
    const lenPx = Math.hypot(b.x - a.x, b.y - a.y);
    const bearing = derived?.[i]?.bearing ?? bearingText(s.from, s.to, north);
    const length = lotLengthText(derived?.[i]?.length_mm ?? s.length);
    ctx.font = `${9.5 * k}px ${palette.fontMono}`;
    if (Math.max(ctx.measureText(bearing).width, ctx.measureText(length).width) + 16 * k > lenPx) return;
    const m = toScreen(view, s.mid);
    const rot = readableDeg(angleDeg(s.dir));
    const out = screenDir(s.out);
    for (const [text, sign] of [
      [bearing, 1],
      [length, -1],
    ] as const) {
      const d = 9 * k * sign;
      ctx.save();
      ctx.translate(m.x + out.x * d, m.y + out.y * d);
      ctx.rotate((-rot * Math.PI) / 180);
      ctx.textBaseline = "middle";
      ctx.fillText(text, 0, 0);
      ctx.restore();
    }
  });
  // Corner numbers just outside each corner.
  ctx.font = `600 ${10 * k}px ${palette.fontUi}`;
  ctx.textBaseline = "middle";
  lot.corners.forEach((c, i) => {
    const s = toScreen(view, c);
    const o = screenDir(cornerOut(lot, i));
    ctx.beginPath();
    ctx.arc(s.x, s.y, 2.2 * k, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillText(String(i + 1), s.x + o.x * 11 * k, s.y + o.y * 11 * k);
  });
  ctx.restore();
}

// ------------------------------------------------------------------ dimension strings

interface StringText {
  text: string;
  /** Screen point the text centers on, rotation in degrees and its box. */
  at: P;
  rot: number;
  box: Box;
}

const TEXT_PX = 11;

/**
 * Where each segment's length goes: above the line at its middle, like a
 * plain dimension. A segment too short for its text sends it up a row, and
 * the next short one down below the line, so neighbours never overlap.
 */
export function stringTexts(rc: RenderContext, d: Pick<Dimension, "a" | "b" | "through" | "offset_mm" | "text_override">): StringText[] {
  const { ctx, palette, view } = rc;
  const k = rc.uiScale;
  const g = stringGeometry(d);
  const rot = readableDeg(angleDeg(g.dir));
  const r = (-rot * Math.PI) / 180;
  const cos = Math.cos(r);
  const sin = Math.sin(r);
  ctx.save();
  ctx.font = `${TEXT_PX * k}px ${palette.fontMono}`;
  const out: StringText[] = [];
  let narrowRow = 0;
  const place = (text: string, mid: P, row: number): void => {
    const tw = ctx.measureText(text).width;
    const m = toScreen(view, mid);
    // Text space: x along the line, y down from it. Row 0 sits just above.
    const top = row >= 0 ? -15 * k - row * 13 * k : 3 * k;
    const corners = [
      { x: -tw / 2 - 3 * k, y: top },
      { x: tw / 2 + 3 * k, y: top },
      { x: tw / 2 + 3 * k, y: top + 12 * k },
      { x: -tw / 2 - 3 * k, y: top + 12 * k },
    ].map((p) => ({ x: m.x + p.x * cos - p.y * sin, y: m.y + p.x * sin + p.y * cos }));
    const c = { x: 0, y: top + 10 * k };
    out.push({ text, at: { x: m.x + c.x * cos - c.y * sin, y: m.y + c.x * sin + c.y * cos }, rot, box: boxAround(corners) });
  };
  for (const s of g.segments) {
    if (s.length < 0.5) continue;
    const text = formatLength(s.length, rc.unit);
    const fits = ctx.measureText(text).width + 6 * k <= s.length * view.scale;
    if (fits) {
      narrowRow = 0;
      place(text, s.mid, 0);
    } else {
      narrowRow = narrowRow === 1 ? -1 : 1;
      place(text, s.mid, narrowRow);
    }
  }
  const override = d.text_override?.trim();
  if (override) {
    // A replacement text names the whole string: under the line, at its middle.
    place(override, { x: (g.p1.x + g.p2.x) / 2, y: (g.p1.y + g.p2.y) / 2 }, -1);
  }
  ctx.restore();
  return out;
}

/** Dimension strings on this level. */
export function drawDimensionStrings(rc: RenderContext, dims: readonly Dimension[], styleOf: StyleOf): void {
  for (const d of dims) drawDimensionString(rc, d, styleOf(d.id));
}

/**
 * One dimension string, as `drawDimension` draws one segment: extension lines
 * from each point with a gap, the line a little past both ends, a 45 degree
 * tick at every point and each segment's length above it. An automatic one
 * (it follows the model) has lighter, thinner ticks; the inspector says what
 * it measures. An "A" badge was tried and read as a grid letter next to the
 * grid bubbles, so the plan keeps to the ticks.
 */
export function drawDimensionString(rc: RenderContext, d: Dimension, style: ElementStyle | null): void {
  const { ctx, palette, view } = rc;
  const g = stringGeometry(d);
  if (dist(g.p1, g.p2) < 1e-6) return;
  const k = rc.uiScale;
  const px = 1 / view.scale;
  const auto = !!d.auto;
  const color = style?.color ?? palette.dimension;
  const pts = [d.a, ...(d.through ?? []), d.b];
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = 1 * k;
  if (style?.dashed) ctx.setLineDash([5, 4]);
  // Extension lines: a gap at the measured point, a short overshoot past the line.
  pts.forEach((p, i) => {
    const q = g.onLine[i];
    const away = sub(q, p);
    const along = away.x * g.normal.x + away.y * g.normal.y;
    if (Math.abs(along) * view.scale < 6 * k) return;
    const sign = along >= 0 ? 1 : -1;
    seg(rc, add(p, mul(g.normal, sign * 5 * k * px)), add(q, mul(g.normal, sign * 7 * k * px)));
  });
  const ext = mul(g.dir, 6 * k * px);
  seg(rc, sub(g.p1, ext), add(g.p2, ext));
  ctx.save();
  ctx.lineWidth = (auto ? 1.4 : 1.8) * k;
  if (auto) ctx.globalAlpha *= 0.75;
  const tick = mul(unit(add(g.dir, g.normal)), 5.5 * k * px);
  for (const q of g.onLine) seg(rc, sub(q, tick), add(q, tick));
  ctx.restore();
  // Lengths, with a paper backing so lines under them do not cut the digits.
  ctx.font = `${TEXT_PX * k}px ${palette.fontMono}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "alphabetic";
  for (const t of stringTexts(rc, d)) {
    ctx.save();
    ctx.translate(t.at.x, t.at.y);
    ctx.rotate((-t.rot * Math.PI) / 180);
    const tw = ctx.measureText(t.text).width;
    ctx.save();
    ctx.globalAlpha *= 0.85;
    ctx.fillStyle = palette.paper;
    ctx.fillRect(-tw / 2 - 3 * k, -10 * k, tw + 6 * k, 12 * k);
    ctx.restore();
    ctx.fillText(t.text, 0, 0);
    ctx.restore();
  }
  ctx.restore();
}

// ------------------------------------------------------------------ type tags

/** Every typed opening's mark, from `Derived::opening_schedule.marks`. */
export function openingMarks(doc: DocState): Map<string, string> {
  return new Map((doc.derived.opening_schedule?.marks ?? []).map((m) => [m.opening_id, m.mark] as const));
}

/** The text boxes of the dimensions on this level: tags keep clear of them. */
function dimensionBoxes(rc: RenderContext): Box[] {
  const out: Box[] = [];
  for (const e of rc.index.visible) {
    if (e.kind !== "dimension") continue;
    if (isDimensionString(e)) for (const t of stringTexts(rc, e)) out.push(t.box);
    else {
      const b = dimensionTextBox(rc, e);
      if (b) out.push(b);
    }
  }
  return out;
}

/** Traces a hexagon of radius r (corner to center), flat top and bottom. */
function hexagon(ctx: CanvasRenderingContext2D, c: P, r: number): void {
  ctx.beginPath();
  for (let i = 0; i < 6; i++) {
    const a = (i / 6) * Math.PI * 2;
    const x = c.x + r * Math.cos(a);
    const y = c.y + r * Math.sin(a);
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.closePath();
}

/**
 * Type tags of the typed openings on this level, from
 * `Derived::opening_schedule.marks`: doors in a circle, windows in a hexagon,
 * beside the opening on the side away from the swing, moved along the wall
 * or further out when that spot is on a dimension's text or another tag.
 */
export function drawTypeTags(rc: RenderContext, openings: readonly Opening[], styleOf: StyleOf): void {
  const marks = openingMarks(rc.index.doc);
  if (marks.size === 0) return;
  const { ctx, palette, view } = rc;
  const k = rc.uiScale;
  const taken = dimensionBoxes(rc);
  ctx.save();
  ctx.font = `600 ${9 * k}px ${palette.fontUi}`;
  for (const o of openings) {
    const mark = marks.get(o.id);
    const host = rc.index.byId.get(o.wall_id);
    if (!mark || !host || host.kind !== "wall") continue;
    const style = styleOf(o.id);
    const f = openingFrame(o, host);
    const door = o.opening_type === "door";
    const tw = ctx.measureText(mark).width;
    const r = Math.max(door ? 8 * k : 8.5 * k, tw / 2 + (door ? 3.5 : 4.5) * k);
    // Away from the swing: the leaf and its arc stay clear.
    const away = mul(f.normal, o.flip_side ? 1 : -1);
    const awayS = screenDir(away);
    const alongS = screenDir(f.dir);
    const face = toScreen(view, add(f.center, mul(away, f.thickness / 2)));
    const halfW = (f.width / 2) * view.scale;
    const near = r + 5 * k;
    const spots: P[] = [
      { x: face.x + awayS.x * near, y: face.y + awayS.y * near },
      { x: face.x + awayS.x * near + alongS.x * (halfW + r), y: face.y + awayS.y * near + alongS.y * (halfW + r) },
      { x: face.x + awayS.x * near - alongS.x * (halfW + r), y: face.y + awayS.y * near - alongS.y * (halfW + r) },
      { x: face.x + awayS.x * (near + 2 * r + 4 * k), y: face.y + awayS.y * (near + 2 * r + 4 * k) },
    ];
    const boxAt = (c: P): Box => ({ x: c.x - r, y: c.y - r, w: 2 * r, h: 2 * r });
    const at = spots.find((c) => !taken.some((b) => boxesOverlap(b, boxAt(c), 1))) ?? spots[0];
    taken.push(boxAt(at));
    const color = style?.color ?? palette.ink2;
    ctx.save();
    ctx.globalAlpha = style?.alpha ?? 1;
    ctx.strokeStyle = color;
    ctx.lineWidth = 1 * k;
    if (style?.dashed) ctx.setLineDash([3 * k, 2 * k]);
    if (door) {
      ctx.beginPath();
      ctx.arc(at.x, at.y, r, 0, Math.PI * 2);
    } else hexagon(ctx, at, r * 1.05);
    if (!style?.hollow) {
      ctx.fillStyle = palette.paper;
      ctx.fill();
    }
    ctx.stroke();
    ctx.fillStyle = color;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(mark, at.x, at.y + 0.5 * k);
    ctx.restore();
  }
  ctx.restore();
}
