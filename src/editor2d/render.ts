// Canvas drawing of the model. No interaction state here: tool overlays are
// in overlay.ts. Everything is drawn in CSS pixel space (the caller applies
// the device pixel ratio), model points go through `toScreen`.

import type {
  Annotation,
  Asset,
  Camera,
  Column,
  Dimension,
  DisplayUnit,
  Element,
  Linework,
  Opening,
  PipeSystem,
  ReferenceModel,
  Room,
  Stair,
  Underlay,
  Wall,
} from "../contract/bindings";
import { PIPE_COLOR_HEX, PIPE_COLOR_VAR, PIPE_SYSTEM_ORDER, pipeFalls } from "../contract/pipes";
import type { P } from "./geom";
import { add, angleDeg, dirDeg, dist, lerp, mul, orientedRect, readableDeg, sub, unit } from "./geom";
import type { DocIndex } from "./model";
import { ANNOTATION_LINE, dimensionGeometry, elementShape, openingFrame, openingRect, stairOutline, symbolMmOf, wallOutline } from "./model";
import type { PipePlan, PipeShape } from "./pipe";
import { lineSetPx, pipeBandHalfPx, pipePlan, pipeRiserRadiusPx, pipeWidthPx } from "./pipe";
import { drawAssetSymbol } from "./symbols";
import type { Box } from "./tags";
import { boxAround } from "./tags";
import { formatArea, formatLength } from "./typed";
import type { View } from "./view";
import { gridSteps, toScreen, toWorld } from "./view";

export type Ctx = CanvasRenderingContext2D;

export interface Palette {
  paper: string;
  surface: string;
  wall: string;
  wallFill: string;
  grid: string;
  gridMajor: string;
  roomFill: string;
  dimension: string;
  selection: string;
  preview: string;
  ink: string;
  ink2: string;
  ink3: string;
  danger: string;
  warn: string;
  /** Every service run color, from the tokens in PIPE_COLOR_VAR. */
  pipes: Record<PipeSystem, string>;
  fontUi: string;
  fontMono: string;
}

export const DEFAULT_PALETTE: Palette = {
  paper: "#f7f5f0",
  surface: "#ffffff",
  wall: "#14283f",
  wallFill: "#2b3f57",
  grid: "#e6e2d8",
  gridMajor: "#d5d0c4",
  roomFill: "rgba(14, 138, 143, 0.05)",
  dimension: "#1f5f99",
  selection: "#0e8a8f",
  preview: "#6b4fbb",
  ink: "#14283f",
  ink2: "#44566b",
  ink3: "#7b8896",
  danger: "#c0392b",
  warn: "#c07a12",
  pipes: { ...PIPE_COLOR_HEX },
  fontUi: '"Inter", system-ui, -apple-system, "Segoe UI", Roboto, sans-serif',
  fontMono: '"JetBrains Mono", ui-monospace, "SF Mono", Menlo, Consolas, monospace',
};

/** Reads the drawing tokens from tokens.css, with safe fallbacks. */
export function readPalette(el: HTMLElement): Palette {
  const cs = getComputedStyle(el);
  const v = (name: string, fallback: string): string => cs.getPropertyValue(name).trim() || fallback;
  const d = DEFAULT_PALETTE;
  return {
    paper: v("--paper", d.paper),
    surface: v("--surface", d.surface),
    wall: v("--draw-wall", d.wall),
    wallFill: v("--draw-wall-fill", d.wallFill),
    grid: v("--draw-grid", d.grid),
    gridMajor: v("--draw-grid-major", d.gridMajor),
    roomFill: v("--draw-room-fill", d.roomFill),
    dimension: v("--draw-dimension", d.dimension),
    selection: v("--draw-selection", d.selection),
    preview: v("--draw-preview", d.preview),
    ink: v("--ink", d.ink),
    ink2: v("--ink-2", d.ink2),
    ink3: v("--ink-3", d.ink3),
    danger: v("--danger", d.danger),
    warn: v("--warn", d.warn),
    pipes: Object.fromEntries(PIPE_SYSTEM_ORDER.map((s) => [s, v(PIPE_COLOR_VAR[s], d.pipes[s])])) as Record<PipeSystem, string>,
    fontUi: v("--font-ui", d.fontUi),
    fontMono: v("--font-mono", d.fontMono),
  };
}

/** Per element style override, used for ghosts, AI previews and removed elements. */
export interface ElementStyle {
  /** Replaces every stroke and the wall fill. */
  color?: string;
  alpha?: number;
  dashed?: boolean;
  /** Outline only, no poche. */
  hollow?: boolean;
  /**
   * Openings only. Which side the leaf swings to, -1 or 1, as a continuous
   * value so a flip can animate through the wall plane. Default: `flip_side`.
   */
  swing?: number;
  /** Openings only. Hinge position along the opening, 0 or 1, animatable. */
  hinge?: number;
  /** Rooms only. Cross fade of the area readout while it changes. */
  areaFade?: { prev: string; p: number };
}

export interface RenderContext {
  ctx: Ctx;
  view: View;
  width: number;
  height: number;
  palette: Palette;
  index: DocIndex;
  unit: DisplayUnit;
  images: Map<string, HTMLImageElement>;
  /** Multiplier for constant-size text and strokes. 1 on screen. */
  uiScale: number;
  /** True for capturePlan: no placeholders for things that did not load. */
  forExport?: boolean;
  /** Switches that share a light with another switch: drawn "S3". */
  threeWay?: ReadonlySet<string>;
}

export const LABEL_PX = 12;

export function labelHeightMm(view: View, uiScale = 1): number {
  return (LABEL_PX * uiScale) / view.scale;
}

function path(rc: RenderContext, pts: readonly P[], close = true): void {
  const { ctx, view } = rc;
  ctx.beginPath();
  pts.forEach((p, i) => {
    const s = toScreen(view, p);
    if (i === 0) ctx.moveTo(s.x, s.y);
    else ctx.lineTo(s.x, s.y);
  });
  if (close) ctx.closePath();
}

function seg(rc: RenderContext, a: P, b: P): void {
  const { ctx, view } = rc;
  const sa = toScreen(view, a);
  const sb = toScreen(view, b);
  ctx.beginPath();
  ctx.moveTo(sa.x, sa.y);
  ctx.lineTo(sb.x, sb.y);
  ctx.stroke();
}

export function drawGrid(rc: RenderContext, gridMm: number): void {
  const { ctx, view, width, height, palette } = rc;
  const steps = gridSteps(gridMm, view.scale);
  const a = toWorld(view, { x: 0, y: height });
  const b = toWorld(view, { x: width, y: 0 });
  const lines = (step: number, color: string, alpha: number): void => {
    if (alpha <= 0.02) return;
    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.strokeStyle = color;
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let x = Math.floor(a.x / step) * step; x <= b.x; x += step) {
      const sx = Math.round(view.ox + x * view.scale) + 0.5;
      ctx.moveTo(sx, 0);
      ctx.lineTo(sx, height);
    }
    for (let y = Math.floor(a.y / step) * step; y <= b.y; y += step) {
      const sy = Math.round(view.oy - y * view.scale) + 0.5;
      ctx.moveTo(0, sy);
      ctx.lineTo(width, sy);
    }
    ctx.stroke();
    ctx.restore();
  };
  lines(steps.minor, palette.grid, steps.minorAlpha);
  lines(steps.major, palette.gridMajor, 1);
}

export function drawOrigin(rc: RenderContext): void {
  const { ctx, view, palette } = rc;
  const o = toScreen(view, { x: 0, y: 0 });
  ctx.save();
  ctx.strokeStyle = palette.ink3;
  ctx.fillStyle = palette.ink3;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(o.x - 9, o.y + 0.5);
  ctx.lineTo(o.x + 16, o.y + 0.5);
  ctx.moveTo(o.x + 0.5, o.y + 9);
  ctx.lineTo(o.x + 0.5, o.y - 16);
  ctx.stroke();
  ctx.beginPath();
  ctx.arc(o.x + 0.5, o.y + 0.5, 3, 0, Math.PI * 2);
  ctx.stroke();
  ctx.font = `9px ${palette.fontMono}`;
  ctx.fillText("x", o.x + 19, o.y + 3);
  ctx.fillText("y", o.x - 2, o.y - 20);
  ctx.restore();
}

// ---------------------------------------------------------------- walls

/**
 * Walls as solid poche. All outlines are stroked first, then filled, so the
 * seams between mitred neighbours disappear and only the outer edge shows.
 */
export function drawWalls(rc: RenderContext, walls: readonly Wall[], styleOf: (id: string) => ElementStyle | null): void {
  const { ctx, palette } = rc;
  const groups = new Map<string, { style: ElementStyle | null; walls: Wall[] }>();
  for (const w of walls) {
    const st = styleOf(w.id);
    const key = st ? `${st.color}|${st.alpha}|${st.dashed}|${st.hollow}` : "";
    let g = groups.get(key);
    if (!g) groups.set(key, (g = { style: st, walls: [] }));
    g.walls.push(w);
  }
  for (const g of groups.values()) {
    const st = g.style;
    ctx.save();
    ctx.globalAlpha = st?.alpha ?? 1;
    // Round joins: mitred outlines have 45 degree tips that would spike with miter joins.
    ctx.lineJoin = "round";
    ctx.strokeStyle = st?.color ?? palette.wall;
    ctx.fillStyle = st?.color ?? palette.wallFill;
    if (st?.dashed) ctx.setLineDash([6, 4]);
    if (st?.hollow) {
      ctx.lineWidth = 1.5 * rc.uiScale;
      for (const w of g.walls) {
        path(rc, wallOutline(w, rc.index));
        ctx.stroke();
      }
    } else {
      ctx.lineWidth = 2.4 * rc.uiScale;
      for (const w of g.walls) {
        path(rc, wallOutline(w, rc.index));
        ctx.stroke();
      }
      ctx.setLineDash([]);
      for (const w of g.walls) {
        path(rc, wallOutline(w, rc.index));
        ctx.fill();
      }
    }
    ctx.restore();
  }
}

// ---------------------------------------------------------------- openings

function arcBetween(rc: RenderContext, center: P, from: P, to: P): void {
  const { ctx, view } = rc;
  const c = toScreen(view, center);
  const a = toScreen(view, from);
  const b = toScreen(view, to);
  const a0 = Math.atan2(a.y - c.y, a.x - c.x);
  const a1 = Math.atan2(b.y - c.y, b.x - c.x);
  let d = a1 - a0;
  while (d > Math.PI) d -= Math.PI * 2;
  while (d < -Math.PI) d += Math.PI * 2;
  ctx.beginPath();
  ctx.arc(c.x, c.y, Math.hypot(a.x - c.x, a.y - c.y), a0, a1, d < 0);
  ctx.stroke();
}

/**
 * Convention used by this editor (the contract does not define it):
 * flip_side false = the leaf swings to the LEFT of the wall direction
 * (start to end). flip_hinge false = the hinge is on the jamb nearer the
 * wall start.
 */
export function drawOpening(rc: RenderContext, o: Opening, host: Wall, style: ElementStyle | null, cut = true): void {
  const { ctx, palette, view } = rc;
  const f = openingFrame(o, host);
  const color = style?.color ?? palette.wall;
  const thin = 1 * rc.uiScale;
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  if (cut) {
    // Cut the poche. A hair wider than the wall so the wall stroke goes too.
    ctx.fillStyle = palette.paper;
    path(rc, openingRect(f, 2 / view.scale));
    ctx.fill();
  }
  ctx.strokeStyle = color;
  ctx.lineWidth = thin;
  if (style?.dashed) ctx.setLineDash([5, 4]);
  const half = f.thickness / 2;
  const nA = mul(f.normal, half);
  // jambs
  ctx.lineWidth = 1.6 * rc.uiScale;
  seg(rc, sub(f.jambA, nA), add(f.jambA, nA));
  seg(rc, sub(f.jambB, nA), add(f.jambB, nA));
  ctx.lineWidth = thin;

  // `swing` lets a flip animate: it passes through 0 (the wall plane) instead
  // of jumping between the two sides.
  const side = style?.swing ?? (o.flip_side ? -1 : 1);
  const n = mul(f.normal, side);

  if (o.opening_type === "door") {
    if (o.style === "sliding") {
      const q = f.thickness / 5;
      const over = Math.min(60, f.width * 0.05);
      ctx.lineWidth = 2 * rc.uiScale;
      seg(rc, add(f.jambA, mul(f.normal, q)), add(add(f.center, mul(f.dir, over)), mul(f.normal, q)));
      seg(rc, add(sub(f.center, mul(f.dir, over)), mul(f.normal, -q)), add(f.jambB, mul(f.normal, -q)));
      ctx.lineWidth = thin;
      // threshold
      ctx.globalAlpha *= 0.5;
      seg(rc, sub(f.jambA, nA), sub(f.jambB, nA));
      seg(rc, add(f.jambA, nA), add(f.jambB, nA));
    } else {
      const leaves: { hinge: P; other: P; w: number }[] = [];
      if (o.style === "swing_double") {
        leaves.push({ hinge: f.jambA, other: f.center, w: f.width / 2 });
        leaves.push({ hinge: f.jambB, other: f.center, w: f.width / 2 });
      } else {
        const h = style?.hinge ?? (o.flip_hinge ? 1 : 0);
        leaves.push({ hinge: lerp(f.jambA, f.jambB, h), other: lerp(f.jambB, f.jambA, h), w: f.width });
      }
      for (const l of leaves) {
        const h = add(l.hinge, mul(n, half));
        const tip = add(h, mul(n, l.w));
        const closed = add(l.other, mul(n, half));
        ctx.lineWidth = 2 * rc.uiScale;
        seg(rc, h, tip);
        ctx.lineWidth = thin;
        ctx.save();
        ctx.globalAlpha *= 0.75;
        arcBetween(rc, h, tip, closed);
        ctx.restore();
      }
      // threshold line on the non swing side
      ctx.save();
      ctx.globalAlpha *= 0.4;
      seg(rc, sub(f.jambA, mul(n, half)), sub(f.jambB, mul(n, half)));
      ctx.restore();
    }
  } else {
    // window: sill lines on both faces, then glazing per style
    seg(rc, sub(f.jambA, nA), sub(f.jambB, nA));
    seg(rc, add(f.jambA, nA), add(f.jambB, nA));
    const q = f.thickness / 6;
    switch (o.style) {
      case "sliding": {
        const over = Math.min(80, f.width * 0.06);
        ctx.lineWidth = 1.6 * rc.uiScale;
        seg(rc, add(f.jambA, mul(f.normal, q)), add(add(f.center, mul(f.dir, over)), mul(f.normal, q)));
        seg(rc, add(sub(f.center, mul(f.dir, over)), mul(f.normal, -q)), add(f.jambB, mul(f.normal, -q)));
        break;
      }
      case "casement": {
        seg(rc, add(f.jambA, mul(f.normal, q)), add(f.jambB, mul(f.normal, q)));
        seg(rc, add(f.jambA, mul(f.normal, -q)), add(f.jambB, mul(f.normal, -q)));
        seg(rc, sub(f.center, nA), add(f.center, nA));
        // small open sashes, toward the swing side
        const w2 = f.width / 2;
        const sash = Math.min(w2, 450);
        for (const jamb of [f.jambA, f.jambB]) {
          const h = add(jamb, mul(n, half));
          const toward = unit(sub(f.center, jamb));
          const tip = add(h, add(mul(n, sash * 0.82), mul(toward, sash * 0.57)));
          ctx.save();
          ctx.globalAlpha *= 0.7;
          seg(rc, h, tip);
          ctx.restore();
        }
        break;
      }
      case "jalousie": {
        seg(rc, f.jambA, f.jambB);
        const count = Math.max(3, Math.round(f.width / 110));
        for (let i = 1; i < count; i++) {
          const c = lerp(f.jambA, f.jambB, i / count);
          seg(rc, add(c, mul(f.normal, q * 1.6)), sub(c, mul(f.normal, q * 1.6)));
        }
        break;
      }
      default:
        ctx.lineWidth = 1.6 * rc.uiScale;
        seg(rc, f.jambA, f.jambB);
    }
  }
  ctx.restore();
}

// ---------------------------------------------------------------- rooms

export function drawRoomFill(rc: RenderContext, room: Room, style: ElementStyle | null, strong = false): void {
  const g = rc.index.roomGeo.get(room.id);
  if (!g || g.polygon.length < 3) return;
  const { ctx, palette } = rc;
  const a = style?.alpha ?? 1;
  ctx.save();
  if (style?.color) {
    ctx.globalAlpha = 0.1 * a;
    ctx.fillStyle = style.color;
  } else if (strong) {
    ctx.globalAlpha = 0.12 * a;
    ctx.fillStyle = palette.selection;
  } else {
    ctx.globalAlpha = a;
    ctx.fillStyle = palette.roomFill;
  }
  path(rc, g.polygon);
  ctx.fill();
  ctx.restore();
}

export function drawRoomLabel(rc: RenderContext, room: Room, style: ElementStyle | null): void {
  const g = rc.index.roomGeo.get(room.id);
  const { ctx, palette, view } = rc;
  const at = toScreen(view, g ? g.label_point : room.seed);
  // Hide labels that cannot fit their room at far zoom.
  if (view.scale * 1000 < 4 * rc.uiScale && rc.uiScale === 1) return;
  const px = LABEL_PX * rc.uiScale;
  const base = style?.alpha ?? 1;
  ctx.save();
  ctx.globalAlpha = base;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillStyle = style?.color ?? palette.ink;
  ctx.font = `600 ${px}px ${palette.fontUi}`;
  ctx.fillText(room.name, at.x, at.y - px * 0.62);
  ctx.fillStyle = style?.color ?? palette.ink2;
  ctx.font = `${px * 0.92}px ${palette.fontMono}`;
  const area = g ? formatArea(g.area_mm2) : "open";
  const fade = style?.areaFade;
  const areaY = at.y + px * 0.72;
  if (fade && fade.p < 1) {
    // The old value rises out while the new one comes up from just below.
    ctx.globalAlpha = base * (1 - fade.p);
    ctx.fillText(fade.prev, at.x, areaY - fade.p * px * 0.4);
    ctx.globalAlpha = base * fade.p;
    ctx.fillText(area, at.x, areaY + (1 - fade.p) * px * 0.4);
  } else {
    ctx.fillText(area, at.x, areaY);
  }
  ctx.restore();
}

// ---------------------------------------------------------------- columns, stairs, assets

export function drawColumn(rc: RenderContext, c: Column, style: ElementStyle | null): void {
  const { ctx, palette, view } = rc;
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.strokeStyle = style?.color ?? palette.wall;
  ctx.fillStyle = style?.color ?? palette.wall;
  ctx.lineWidth = 1.5 * rc.uiScale;
  if (style?.dashed) ctx.setLineDash([5, 4]);
  if (c.shape === "round") {
    const s = toScreen(view, c.center);
    ctx.beginPath();
    ctx.arc(s.x, s.y, (c.width_mm / 2) * view.scale, 0, Math.PI * 2);
  } else {
    path(rc, orientedRect(c.center, c.width_mm, c.depth_mm, c.rotation_deg));
  }
  if (!style?.hollow) ctx.fill();
  ctx.stroke();
  ctx.restore();
}

export function drawStair(rc: RenderContext, s: Stair, style: ElementStyle | null): void {
  const { ctx, palette } = rc;
  const outline = stairOutline(s.origin, s.rotation_deg, s.width_mm, s.run_mm);
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
  // Contract: going depth = run_mm / riser_count.
  const goings = Math.max(1, s.riser_count);
  ctx.lineWidth = 0.8 * rc.uiScale;
  for (let i = 1; i < goings; i++) {
    const a = add(outline[0], mul(run, (s.run_mm * i) / goings));
    seg(rc, a, add(a, mul(across, s.width_mm)));
  }
  // walking line with an arrow pointing up the flight
  const start = add(s.origin, mul(run, s.run_mm * 0.04));
  const end = add(s.origin, mul(run, s.run_mm * 0.94));
  ctx.lineWidth = 1.2 * rc.uiScale;
  seg(rc, start, end);
  const head = Math.min(s.width_mm * 0.22, 260);
  seg(rc, end, add(end, add(mul(run, -head), mul(across, head * 0.6))));
  seg(rc, end, add(end, add(mul(run, -head), mul(across, -head * 0.6))));
  const sp = toScreen(rc.view, start);
  ctx.beginPath();
  ctx.arc(sp.x, sp.y, 2.5 * rc.uiScale, 0, Math.PI * 2);
  ctx.fillStyle = style?.color ?? palette.ink2;
  ctx.fill();
  if (s.width_mm * rc.view.scale > 40 * rc.uiScale) {
    const lp = toScreen(rc.view, add(start, mul(across, s.width_mm * 0.28)));
    ctx.font = `${9 * rc.uiScale}px ${palette.fontMono}`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText("UP", lp.x, lp.y);
  }
  ctx.restore();
}

export function drawAsset(rc: RenderContext, a: Asset, style: ElementStyle | null): void {
  const { ctx, palette, view } = rc;
  const c = toScreen(view, a.position);
  const px = 1 / view.scale;
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.translate(c.x, c.y);
  ctx.rotate((-a.rotation_deg * Math.PI) / 180);
  ctx.scale(view.scale, -view.scale);
  ctx.lineWidth = px * rc.uiScale;
  ctx.lineJoin = "round";
  ctx.strokeStyle = style?.color ?? palette.ink2;
  ctx.fillStyle = style?.hollow ? "rgba(255,255,255,0)" : "rgba(255,255,255,0.78)";
  if (style?.dashed) ctx.setLineDash([5 * px, 4 * px]);
  // Devices draw at the symbol size D of the plan scale; a switch sharing a light reads "S3".
  let label = drawAssetSymbol(ctx, a.catalog_key, a.width_mm, a.depth_mm, px, {
    symbolMm: symbolMmOf(rc.index),
    threeWay: rc.threeWay?.has(a.id) ?? false,
    font: palette.fontUi,
  });
  if (label === null) {
    ctx.beginPath();
    ctx.rect(-a.width_mm / 2, -a.depth_mm / 2, a.width_mm, a.depth_mm);
    ctx.fill();
    ctx.stroke();
    label = a.name;
  }
  ctx.restore();
  if (label) {
    const wPx = Math.min(a.width_mm, a.depth_mm) * view.scale;
    const fpx = Math.min(10 * rc.uiScale, wPx * 0.3);
    if (fpx >= 6 * rc.uiScale * 0.8) {
      ctx.save();
      ctx.globalAlpha = style?.alpha ?? 1;
      ctx.fillStyle = style?.color ?? palette.ink2;
      ctx.font = `${fpx}px ${palette.fontUi}`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      const maxW = Math.max(a.width_mm, a.depth_mm) * view.scale - 6;
      let text = label;
      while (text.length > 3 && ctx.measureText(text).width > maxW) text = text.slice(0, -2);
      ctx.fillText(text, c.x, c.y);
      ctx.restore();
    }
  }
}

// ---------------------------------------------------------------- dimensions, text

export function dimensionText(d: Pick<Dimension, "a" | "b" | "text_override">, unitName: DisplayUnit): string {
  return d.text_override && d.text_override.trim() !== "" ? d.text_override : formatLength(dist(d.a, d.b), unitName);
}

export function drawDimension(
  rc: RenderContext,
  d: Pick<Dimension, "a" | "b" | "offset_mm" | "text_override">,
  style: ElementStyle | null,
): void {
  const { ctx, palette, view } = rc;
  if (dist(d.a, d.b) < 1e-6) return;
  const g = dimensionGeometry(d.a, d.b, d.offset_mm);
  const k = rc.uiScale;
  const sign = d.offset_mm >= 0 ? 1 : -1;
  const px = 1 / view.scale;
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.strokeStyle = style?.color ?? palette.dimension;
  ctx.fillStyle = style?.color ?? palette.dimension;
  ctx.lineWidth = 1 * k;
  if (style?.dashed) ctx.setLineDash([5, 4]);
  // extension lines: a gap at the measured point, a short overshoot past the line
  const gap = mul(g.normal, sign * 5 * k * px);
  const over = mul(g.normal, sign * 7 * k * px);
  seg(rc, add(d.a, gap), add(g.p1, over));
  seg(rc, add(d.b, gap), add(g.p2, over));
  // dimension line, a little past both ticks
  const ext = mul(g.dir, 6 * k * px);
  seg(rc, sub(g.p1, ext), add(g.p2, ext));
  // 45 degree ticks
  ctx.lineWidth = 1.8 * k;
  const tick = mul(unit(add(g.dir, g.normal)), 5.5 * k * px);
  seg(rc, sub(g.p1, tick), add(g.p1, tick));
  seg(rc, sub(g.p2, tick), add(g.p2, tick));
  // text above the line, always readable
  const m = toScreen(view, g.mid);
  const rot = readableDeg(angleDeg(g.dir));
  ctx.translate(m.x, m.y);
  ctx.rotate((-rot * Math.PI) / 180);
  ctx.font = `${11 * k}px ${palette.fontMono}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "alphabetic";
  const text = dimensionText(d, rc.unit);
  const tw = ctx.measureText(text).width;
  ctx.save();
  ctx.globalAlpha *= 0.85;
  ctx.fillStyle = palette.paper;
  ctx.fillRect(-tw / 2 - 3 * k, -15 * k, tw + 6 * k, 12 * k);
  ctx.restore();
  ctx.fillText(text, 0, -5 * k);
  ctx.restore();
}

/**
 * The screen box of a dimension's text, as `drawDimension` paints it (its
 * paper backing, rotated with the line). Fall and height tags keep clear of it.
 */
export function dimensionTextBox(rc: RenderContext, d: Pick<Dimension, "a" | "b" | "offset_mm" | "text_override">): Box | null {
  if (dist(d.a, d.b) < 1e-6) return null;
  const { ctx, palette, view } = rc;
  const k = rc.uiScale;
  const g = dimensionGeometry(d.a, d.b, d.offset_mm);
  const m = toScreen(view, g.mid);
  const rot = (-readableDeg(angleDeg(g.dir)) * Math.PI) / 180;
  ctx.save();
  ctx.font = `${11 * k}px ${palette.fontMono}`;
  const tw = ctx.measureText(dimensionText(d, rc.unit)).width;
  ctx.restore();
  const c = Math.cos(rot);
  const sn = Math.sin(rot);
  const corners = [
    { x: -tw / 2 - 3 * k, y: -15 * k },
    { x: tw / 2 + 3 * k, y: -15 * k },
    { x: tw / 2 + 3 * k, y: -3 * k },
    { x: -tw / 2 - 3 * k, y: -3 * k },
  ].map((p) => ({ x: m.x + p.x * c - p.y * sn, y: m.y + p.x * sn + p.y * c }));
  return boxAround(corners);
}

/** The screen box of a room's name and area, as `drawRoomLabel` paints them. Null when hidden at this zoom. */
export function roomLabelTextBox(rc: RenderContext, room: Room): Box | null {
  const { ctx, palette, view } = rc;
  if (view.scale * 1000 < 4 * rc.uiScale && rc.uiScale === 1) return null;
  const g = rc.index.roomGeo.get(room.id);
  const at = toScreen(view, g ? g.label_point : room.seed);
  const px = LABEL_PX * rc.uiScale;
  ctx.save();
  ctx.font = `600 ${px}px ${palette.fontUi}`;
  const nameW = ctx.measureText(room.name).width;
  ctx.font = `${px * 0.92}px ${palette.fontMono}`;
  const areaW = ctx.measureText(g ? formatArea(g.area_mm2) : "open").width;
  ctx.restore();
  const w = Math.max(nameW, areaW);
  return { x: at.x - w / 2, y: at.y - px * 1.2, w, h: px * 2.45 };
}

export function drawAnnotation(rc: RenderContext, a: Annotation, style: ElementStyle | null): void {
  const { ctx, palette, view } = rc;
  const px = a.size_mm * view.scale;
  if (px < 2.5) return;
  const s = toScreen(view, a.position);
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.translate(s.x, s.y);
  ctx.rotate((-a.rotation_deg * Math.PI) / 180);
  ctx.fillStyle = style?.color ?? palette.ink;
  ctx.font = `${px}px ${palette.fontUi}`;
  // Contract: position is the left end of the first baseline, lines stack below.
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  a.text.split("\n").forEach((t, i) => ctx.fillText(t, 0, i * px * ANNOTATION_LINE));
  ctx.restore();
}

// ---------------------------------------------------------------- underlay, camera

export function drawUnderlay(rc: RenderContext, u: Underlay, style: ElementStyle | null): void {
  const { ctx, palette, view } = rc;
  const w = u.width_px * u.mm_per_px * view.scale;
  const h = u.height_px * u.mm_per_px * view.scale;
  const o = toScreen(view, u.position);
  const img = rc.images.get(u.file_name);
  ctx.save();
  ctx.translate(o.x, o.y);
  ctx.rotate((-u.rotation_deg * Math.PI) / 180);
  if (img && img.complete && img.naturalWidth > 0) {
    ctx.globalAlpha = Math.max(0, Math.min(1, u.opacity)) * (style?.alpha ?? 1);
    ctx.drawImage(img, 0, -h, w, h);
  } else if (!rc.forExport) {
    ctx.globalAlpha = 0.6;
    ctx.strokeStyle = style?.color ?? palette.ink3;
    ctx.setLineDash([6, 4]);
    ctx.strokeRect(0, -h, w, h);
    ctx.fillStyle = palette.ink3;
    ctx.font = `11px ${palette.fontUi}`;
    ctx.fillText(u.file_name, 8, -h + 16);
  }
  if (style?.color) {
    ctx.globalAlpha = 1;
    ctx.strokeStyle = style.color;
    ctx.lineWidth = 1.5;
    if (style.dashed) ctx.setLineDash([6, 4]);
    ctx.strokeRect(0, -h, w, h);
  }
  ctx.restore();
}

export function drawCamera(rc: RenderContext, c: Pick<Camera, "position" | "target" | "fov_deg" | "name">, style: ElementStyle | null, active = false): void {
  const { ctx, palette, view } = rc;
  const p = { x: c.position.x, y: c.position.y };
  const t = { x: c.target.x, y: c.target.y };
  const s = toScreen(view, p);
  const color = style?.color ?? (active ? palette.selection : palette.dimension);
  const flat = dist(p, t) < 1;
  const ang = flat ? 90 : angleDeg(sub(t, p));
  const k = rc.uiScale;
  ctx.save();
  ctx.globalAlpha = style?.alpha ?? 1;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = 1 * k;
  if (!flat) {
    // view cone
    const reach = Math.min(Math.max(dist(p, t), 40 * k / view.scale), 90 * k / view.scale);
    const half = Math.max(10, Math.min(60, c.fov_deg / 2));
    const a = add(p, mul(dirDeg(ang + half), reach));
    const b = add(p, mul(dirDeg(ang - half), reach));
    const sa = toScreen(view, a);
    const sb = toScreen(view, b);
    ctx.save();
    ctx.globalAlpha *= 0.1;
    ctx.beginPath();
    ctx.moveTo(s.x, s.y);
    ctx.lineTo(sa.x, sa.y);
    ctx.lineTo(sb.x, sb.y);
    ctx.closePath();
    ctx.fill();
    ctx.restore();
    ctx.save();
    ctx.globalAlpha *= 0.7;
    ctx.setLineDash([4, 3]);
    ctx.beginPath();
    ctx.moveTo(sa.x, sa.y);
    ctx.lineTo(s.x, s.y);
    ctx.lineTo(sb.x, sb.y);
    ctx.stroke();
    ctx.restore();
  }
  // camera body, pointing along the view direction
  ctx.translate(s.x, s.y);
  ctx.rotate((-ang * Math.PI) / 180);
  ctx.beginPath();
  ctx.rect(-9 * k, -5.5 * k, 12 * k, 11 * k);
  ctx.moveTo(3 * k, -2.5 * k);
  ctx.lineTo(9 * k, -6 * k);
  ctx.lineTo(9 * k, 6 * k);
  ctx.lineTo(3 * k, 2.5 * k);
  ctx.closePath();
  ctx.fillStyle = palette.surface;
  ctx.fill();
  ctx.lineWidth = 1.4 * k;
  ctx.stroke();
  ctx.rotate((ang * Math.PI) / 180);
  ctx.fillStyle = color;
  ctx.font = `${10 * k}px ${palette.fontUi}`;
  ctx.textAlign = "center";
  ctx.fillText(c.name, 0, 22 * k);
  ctx.restore();
}

// ---------------------------------------------------------------- linework, reference models

/** Imported tracing linework: thin lines in the source layer's color. */
export function drawLinework(rc: RenderContext, l: Linework, style: ElementStyle | null): void {
  const { ctx } = rc;
  ctx.save();
  ctx.globalAlpha = (style?.alpha ?? 1) * (l.locked ? 0.85 : 1);
  ctx.strokeStyle = style?.color ?? l.color;
  ctx.lineWidth = 1 * rc.uiScale;
  ctx.lineJoin = "round";
  if (style?.dashed) ctx.setLineDash([6, 4]);
  for (const pl of l.polylines) {
    if (pl.length < 2) continue;
    path(rc, pl, false);
    ctx.stroke();
  }
  ctx.restore();
}

/**
 * Small labelled marker for a 3D reference model: its real shape only shows
 * in the 3D view, so the plan just needs a legible placeholder to select,
 * move and rotate.
 */
export function drawReferenceModel(rc: RenderContext, m: ReferenceModel, style: ElementStyle | null): void {
  const { ctx, palette, view } = rc;
  const s = toScreen(view, m.position);
  const r = 9 * rc.uiScale;
  const color = style?.color ?? palette.ink2;
  ctx.save();
  ctx.globalAlpha = (style?.alpha ?? 1) * (m.locked ? 0.85 : 1);
  ctx.translate(s.x, s.y);
  ctx.rotate((-m.rotation_deg * Math.PI) / 180);
  ctx.strokeStyle = color;
  ctx.fillStyle = palette.surface;
  ctx.lineWidth = 1.4 * rc.uiScale;
  if (style?.dashed) ctx.setLineDash([5, 4]);
  ctx.beginPath();
  ctx.moveTo(0, -r);
  ctx.lineTo(r, 0);
  ctx.lineTo(0, r);
  ctx.lineTo(-r, 0);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(0, -r * 0.4);
  ctx.lineTo(0, r * 0.4);
  ctx.moveTo(-r * 0.4, 0);
  ctx.lineTo(r * 0.4, 0);
  ctx.stroke();
  ctx.rotate((m.rotation_deg * Math.PI) / 180);
  ctx.fillStyle = color;
  ctx.font = `${10 * rc.uiScale}px ${palette.fontUi}`;
  ctx.textAlign = "center";
  ctx.fillText(m.name, 0, r + 13 * rc.uiScale);
  ctx.restore();
}

// ---------------------------------------------------------------- pipes

/** The system color from the tokens (PIPE_COLOR_VAR in src/contract/pipes.ts). */
export function pipeColor(palette: Palette, system: PipeSystem): string {
  return palette.pipes[system] ?? PIPE_COLOR_HEX[system];
}

/**
 * Line pattern per system in CSS pixels: water and line sets solid, runs that
 * fall (drainage, storm, condensate) dashed, vent dash-dot, conduit in short
 * dashes so it never reads as a drain.
 */
export function pipeDash(system: PipeSystem, k: number): number[] {
  if (pipeFalls(system)) return [9 * k, 5 * k];
  if (system === "vent") return [11 * k, 4 * k, 2 * k, 4 * k];
  if (system === "conduit") return [4 * k, 3 * k];
  return [];
}

/** Wider than this (CSS pixels) a pipe is drawn as two edges and its centerline. */
const PIPE_DOUBLE_PX = 4;

/**
 * A polyline moved `d` pixels to its left (screen space, y down), with
 * mitred corners. The miter is capped so a sharp turn never spikes.
 */
export function offsetPolyline(pts: readonly P[], d: number): P[] {
  const n = pts.length;
  if (n < 2 || d === 0) return pts.slice();
  const normals: P[] = [];
  for (let i = 0; i + 1 < n; i++) {
    const dx = pts[i + 1].x - pts[i].x;
    const dy = pts[i + 1].y - pts[i].y;
    const L = Math.hypot(dx, dy) || 1;
    normals.push({ x: dy / L, y: -dx / L });
  }
  const out: P[] = [];
  for (let i = 0; i < n; i++) {
    const a = normals[Math.max(0, i - 1)];
    const b = normals[Math.min(normals.length - 1, i)];
    let mx = a.x + b.x;
    let my = a.y + b.y;
    const ml = Math.hypot(mx, my);
    if (ml < 1e-9) {
      mx = b.x;
      my = b.y;
    } else {
      mx /= ml;
      my /= ml;
    }
    const cos = mx * b.x + my * b.y;
    const k = Math.min(4, 1 / Math.max(0.25, cos));
    out.push({ x: pts[i].x + mx * d * k, y: pts[i].y + my * d * k });
  }
  return out;
}

function tracePolylines(ctx: Ctx, runs: readonly (readonly P[])[]): void {
  ctx.beginPath();
  for (const run of runs) run.forEach((q, i) => (i === 0 ? ctx.moveTo(q.x, q.y) : ctx.lineTo(q.x, q.y)));
}

/**
 * Strokes traced runs as one pipe of width `w`: a hollow outline for removed
 * elements, edges with a tinted paper core and the patterned centerline when
 * wide, else a solid patterned line.
 */
function strokeRuns(ctx: Ctx, runs: readonly (readonly P[])[], w: number, color: string, paper: string, dash: number[], k: number, alpha: number, hollow: boolean): void {
  tracePolylines(ctx, runs);
  ctx.strokeStyle = color;
  if (hollow) {
    ctx.globalAlpha = alpha;
    ctx.lineWidth = 1.5 * k;
    ctx.lineCap = "round";
    ctx.setLineDash(dash);
    ctx.stroke();
  } else if (w > PIPE_DOUBLE_PX * k) {
    ctx.lineCap = "round";
    ctx.globalAlpha = alpha * 0.85;
    ctx.lineWidth = w;
    ctx.stroke();
    ctx.globalAlpha = alpha;
    ctx.strokeStyle = paper;
    ctx.lineWidth = w - 2 * k;
    ctx.stroke();
    ctx.globalAlpha = alpha * 0.16;
    ctx.strokeStyle = color;
    ctx.stroke();
    ctx.globalAlpha = alpha;
    ctx.lineWidth = 1.3 * k;
    ctx.lineCap = dash.length > 0 ? "butt" : "round";
    ctx.setLineDash(dash);
    ctx.stroke();
  } else {
    ctx.globalAlpha = alpha;
    ctx.lineWidth = w;
    ctx.lineCap = dash.length > 0 ? "butt" : "round";
    ctx.setLineDash(dash);
    ctx.stroke();
  }
  ctx.setLineDash([]);
}

/**
 * A pipe in plan, split the way the exports split it (pipe.ts `pipePlan`):
 * its stretches at its size at this zoom (never thinner than PIPE_MIN_PX) in
 * the system color, falling runs dashed with flow arrows pointing from the
 * first point to the last, vent dash-dot, conduit short dashes. A refrigerant
 * line set is two lines side by side, gas and liquid, at plan scale. A riser
 * is a circle marked up or down in run order, never a line. A thin paper
 * casing lets a pipe read over walls and breaks the pipe it crosses.
 */
export function drawPipe(rc: RenderContext, pipe: PipeShape, style: ElementStyle | null, parts: "all" | "lines" | "risers" = "all"): void {
  const { ctx, view, palette } = rc;
  const plan = pipePlan(pipe.points);
  if (parts === "risers") {
    drawPipeRisers(rc, pipe, style, plan);
    return;
  }
  const k = rc.uiScale;
  const color = style?.color ?? pipeColor(palette, pipe.system);
  const alpha = style?.alpha ?? 1;
  const hollow = !!style?.hollow;
  const w = pipeWidthPx(pipe.diameter_mm, view.scale, k);
  const runs = plan.runs.map((run) => run.map((q) => toScreen(view, q)));
  const dash = style?.dashed ? [6 * k, 4 * k] : pipeDash(pipe.system, k);
  ctx.save();
  ctx.lineJoin = "round";
  if (runs.length > 0) {
    const band = pipeBandHalfPx(pipe, view.scale, k) * 2;
    if (!hollow) {
      // The paper casing under the whole band.
      tracePolylines(ctx, runs);
      ctx.lineCap = "round";
      ctx.globalAlpha = alpha * 0.85;
      ctx.strokeStyle = palette.paper;
      ctx.lineWidth = band + 2.5 * k;
      ctx.stroke();
    }
    if (pipe.system === "refrigerant") {
      const ls = lineSetPx(pipe.diameter_mm, view.scale, k);
      strokeRuns(ctx, runs.map((r) => offsetPolyline(r, ls.sep / 2)), ls.gasW, color, palette.paper, dash, k, alpha, hollow);
      strokeRuns(ctx, runs.map((r) => offsetPolyline(r, -ls.sep / 2)), ls.liquidW, color, palette.paper, dash, k, alpha, hollow);
    } else {
      strokeRuns(ctx, runs, w, color, palette.paper, dash, k, alpha, hollow);
    }
    if (pipeFalls(pipe.system) && !hollow) for (const run of runs) drawFlowArrows(ctx, run, color, w, k, alpha);
  }
  ctx.restore();
  if (parts === "all") drawPipeRisers(rc, pipe, style, plan);
}

/** The riser circles of a pipe. Drawn after the lines, so no run covers them. */
function drawPipeRisers(rc: RenderContext, pipe: PipeShape, style: ElementStyle | null, plan: PipePlan = pipePlan(pipe.points)): void {
  const k = rc.uiScale;
  const color = style?.color ?? pipeColor(rc.palette, pipe.system);
  const r = pipeRiserRadiusPx(pipe, rc.view.scale, k);
  for (const q of plan.risers) {
    drawRiserMark(rc.ctx, toScreen(rc.view, q.point), r, q.zTo > q.zFrom, color, rc.palette.paper, k, style?.alpha ?? 1, !!style?.hollow);
  }
}

/** Chevrons along each plan segment, pointing downstream (first point to last). */
function drawFlowArrows(ctx: Ctx, pts: readonly P[], color: string, w: number, k: number, alpha: number): void {
  const s = Math.max(3.5 * k, Math.min(w * 0.45 + 2 * k, 7 * k));
  ctx.save();
  ctx.globalAlpha = alpha;
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.6 * k;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  for (let i = 0; i + 1 < pts.length; i++) {
    const a = pts[i];
    const b = pts[i + 1];
    const L = Math.hypot(b.x - a.x, b.y - a.y);
    if (L < 26 * k) continue;
    const n = Math.max(1, Math.floor(L / (120 * k)));
    const dx = (b.x - a.x) / L;
    const dy = (b.y - a.y) / L;
    for (let j = 0; j < n; j++) {
      const t = (j + 0.5) / n;
      const cx = a.x + (b.x - a.x) * t + dx * s * 0.3;
      const cy = a.y + (b.y - a.y) * t + dy * s * 0.3;
      ctx.beginPath();
      ctx.moveTo(cx - dx * s - dy * s * 0.75, cy - dy * s + dx * s * 0.75);
      ctx.lineTo(cx, cy);
      ctx.lineTo(cx - dx * s + dy * s * 0.75, cy - dy * s - dx * s * 0.75);
      ctx.stroke();
    }
  }
  ctx.restore();
}

/** A riser: a circle with a triangle pointing up the screen (rises) or down (drops). */
export function drawRiserMark(ctx: Ctx, c: P, r: number, up: boolean, color: string, paper: string, k: number, alpha: number, hollow = false): void {
  ctx.save();
  ctx.globalAlpha = alpha;
  ctx.setLineDash([]);
  ctx.beginPath();
  ctx.arc(c.x, c.y, r, 0, Math.PI * 2);
  if (!hollow) {
    ctx.fillStyle = paper;
    ctx.fill();
  }
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.5 * k;
  ctx.stroke();
  const t = r * 0.52;
  const dir = up ? -1 : 1;
  ctx.beginPath();
  ctx.moveTo(c.x, c.y + dir * t);
  ctx.lineTo(c.x + t * 0.95, c.y - dir * t * 0.6);
  ctx.lineTo(c.x - t * 0.95, c.y - dir * t * 0.6);
  ctx.closePath();
  ctx.fillStyle = color;
  ctx.fill();
  ctx.restore();
}

/**
 * Tees from `Derived.pipes.fittings` on the active level: a dot where a run
 * ends on another, in the color of the run it joins. Absent until the engine
 * fills the network.
 */
export function drawPipeFittings(rc: RenderContext, styleOf: (id: string) => ElementStyle | null): void {
  const fittings = rc.index.doc.derived.pipes?.fittings ?? [];
  if (fittings.length === 0) return;
  const { ctx, view, palette } = rc;
  const k = rc.uiScale;
  for (const f of fittings) {
    if (f.kind !== "tee" || f.level_id !== rc.index.levelId || !rc.index.visibleIds.has(f.pipe_id)) continue;
    const main = rc.index.byId.get(f.pipe_id);
    if (!main || main.kind !== "pipe") continue;
    const st = styleOf(f.pipe_id);
    const w = pipeWidthPx(f.diameter_mm, view.scale, k);
    const r = Math.max(3.2 * k, w * 0.62);
    const c = toScreen(view, f.position);
    ctx.save();
    ctx.globalAlpha = st?.alpha ?? 1;
    ctx.beginPath();
    ctx.arc(c.x, c.y, r + 1.2 * k, 0, Math.PI * 2);
    ctx.fillStyle = palette.paper;
    ctx.fill();
    ctx.beginPath();
    ctx.arc(c.x, c.y, r, 0, Math.PI * 2);
    ctx.fillStyle = st?.color ?? pipeColor(palette, main.system);
    ctx.fill();
    ctx.restore();
  }
}

// ---------------------------------------------------------------- one element, any kind

/** Draws a single element with a style. Used for ghosts and removed elements. */
export function drawElement(rc: RenderContext, el: Element, style: ElementStyle | null, index: DocIndex = rc.index): void {
  const local: RenderContext = index === rc.index ? rc : { ...rc, index };
  switch (el.kind) {
    case "wall":
      drawWalls(local, [el], () => style);
      break;
    case "opening": {
      const host = index.byId.get(el.wall_id);
      if (host && host.kind === "wall") drawOpening(local, el, host, style, !style?.hollow);
      break;
    }
    case "room":
      drawRoomFill(local, el, style);
      drawRoomLabel(local, el, style);
      break;
    case "column":
      drawColumn(local, el, style);
      break;
    case "stair":
      drawStair(local, el, style);
      break;
    case "asset":
      drawAsset(local, el, style);
      break;
    case "annotation":
      drawAnnotation(local, el, style);
      break;
    case "dimension":
      drawDimension(local, el, style);
      break;
    case "camera":
      drawCamera(local, el, style);
      break;
    case "underlay":
      drawUnderlay(local, el, style);
      break;
    case "linework":
      drawLinework(local, el, style);
      break;
    case "reference_model":
      drawReferenceModel(local, el, style);
      break;
    case "pipe":
      drawPipe(local, el, style);
      break;
  }
}

export interface SceneOptions {
  styleOf: (id: string) => ElementStyle | null;
  /** Elements not drawn at all (for example text being edited inline). */
  hidden: Set<string>;
  activeCameraId: string | null;
  showCameras: boolean;
}

/** Draws every visible element in plan order. */
export function drawModel(rc: RenderContext, opt: SceneOptions): void {
  const els = rc.index.visible.filter((e) => !opt.hidden.has(e.id));
  const of = <K extends Element["kind"]>(kind: K): Extract<Element, { kind: K }>[] =>
    els.filter((e): e is Extract<Element, { kind: K }> => e.kind === kind);

  for (const u of of("underlay")) drawUnderlay(rc, u, opt.styleOf(u.id));
  for (const l of of("linework")) drawLinework(rc, l, opt.styleOf(l.id));
  for (const r of of("room")) drawRoomFill(rc, r, opt.styleOf(r.id));
  for (const s of of("stair")) drawStair(rc, s, opt.styleOf(s.id));
  for (const a of of("asset")) drawAsset(rc, a, opt.styleOf(a.id));
  drawWalls(rc, of("wall"), opt.styleOf);
  for (const o of of("opening")) {
    const host = rc.index.byId.get(o.wall_id);
    if (host && host.kind === "wall") drawOpening(rc, o, host, opt.styleOf(o.id));
  }
  for (const c of of("column")) drawColumn(rc, c, opt.styleOf(c.id));
  // Pipes over the building so runs in walls and under slabs still read.
  // Higher runs draw last: seen from above they cross over the lower ones.
  const pipes = of("pipe").sort((a, b) => meanZ(a.points) - meanZ(b.points));
  for (const p of pipes) drawPipe(rc, p, opt.styleOf(p.id), "lines");
  for (const p of pipes) drawPipe(rc, p, opt.styleOf(p.id), "risers");
  drawPipeFittings(rc, opt.styleOf);
  for (const r of of("room")) drawRoomLabel(rc, r, opt.styleOf(r.id));
  for (const d of of("dimension")) drawDimension(rc, d, opt.styleOf(d.id));
  for (const a of of("annotation")) drawAnnotation(rc, a, opt.styleOf(a.id));
  for (const m of of("reference_model")) drawReferenceModel(rc, m, opt.styleOf(m.id));
  if (opt.showCameras) {
    for (const c of of("camera")) drawCamera(rc, c, opt.styleOf(c.id), c.id === opt.activeCameraId);
  }
}

function meanZ(points: readonly { z: number }[]): number {
  let s = 0;
  for (const v of points) s += v.z;
  return points.length > 0 ? s / points.length : 0;
}

/**
 * Selection or hover outline around an element. `alpha` fades the whole
 * highlight in and out, so hovering and selecting are not instant swaps.
 */
export function drawHighlight(rc: RenderContext, el: Element, strength: "hover" | "selected", alpha = 1): void {
  const { ctx, palette } = rc;
  if (alpha <= 0.002) return;
  ctx.save();
  ctx.strokeStyle = palette.selection;
  ctx.fillStyle = palette.selection;
  ctx.lineJoin = "round";
  const sel = strength === "selected";
  ctx.lineWidth = sel ? 2 : 1.5;
  if (el.kind === "room") {
    drawRoomFill(rc, el, { alpha }, true);
    const g = rc.index.roomGeo.get(el.id);
    if (g && sel) {
      ctx.setLineDash([6, 4]);
      ctx.globalAlpha = 0.8 * alpha;
      path(rc, g.polygon);
      ctx.stroke();
    }
    ctx.restore();
    return;
  }
  if (el.kind === "dimension") {
    ctx.restore();
    drawDimension(rc, el, { color: palette.selection, alpha: (sel ? 1 : 0.8) * alpha });
    return;
  }
  if (el.kind === "column" && el.shape === "round") {
    const s = toScreen(rc.view, el.center);
    ctx.globalAlpha = alpha;
    ctx.beginPath();
    ctx.arc(s.x, s.y, (el.width_mm / 2) * rc.view.scale + 2, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
    return;
  }
  if (el.kind === "pipe") {
    // A halo along the stretches and a ring around each riser, wider than the pipe itself.
    const plan = pipePlan(el.points);
    const w = pipeBandHalfPx(el, rc.view.scale, rc.uiScale) * 2;
    ctx.lineCap = "round";
    ctx.globalAlpha = (sel ? 0.3 : 0.2) * alpha;
    ctx.lineWidth = w + (sel ? 9 : 7);
    for (const run of plan.runs) {
      path(rc, run, false);
      ctx.stroke();
    }
    ctx.globalAlpha = (sel ? 1 : 0.7) * alpha;
    ctx.lineWidth = sel ? 1.8 : 1.3;
    const r = pipeRiserRadiusPx(el, rc.view.scale, rc.uiScale) + 3;
    for (const q of plan.risers) {
      const s = toScreen(rc.view, q.point);
      ctx.beginPath();
      ctx.arc(s.x, s.y, r, 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.restore();
    return;
  }
  if (el.kind === "linework") {
    // Each polyline outlined on its own: the generic single-shape path below
    // would draw a phantom segment bridging disjoint polylines.
    ctx.globalAlpha = (sel ? 1 : 0.7) * alpha;
    for (const pl of el.polylines) {
      if (pl.length < 2) continue;
      path(rc, pl, false);
      ctx.stroke();
    }
    ctx.restore();
    return;
  }
  const shape = elementShape(el, rc.index, { labelHeightMm: labelHeightMm(rc.view) });
  if (shape) {
    path(rc, shape.points, shape.closed);
    ctx.globalAlpha = (sel ? 0.16 : 0.1) * alpha;
    if (shape.closed && el.kind !== "underlay") ctx.fill();
    ctx.globalAlpha = (sel ? 1 : 0.7) * alpha;
    ctx.stroke();
  }
  ctx.restore();
}

/**
 * What another participant of a live session has selected, in their color:
 * a soft low-alpha band with a thin line on it, so it reads on paper and on
 * dark wall poche alike, and stays quieter than this window's own selection
 * (drawn over it). Rooms get a faint tint and their boundary dashed.
 */
export function drawPeerOutline(rc: RenderContext, el: Element, color: string, alpha = 1): void {
  if (alpha <= 0.002) return;
  const { ctx } = rc;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  /** The band, then the line, along whatever path is current. */
  const halo = (): void => {
    ctx.globalAlpha = 0.2 * alpha;
    ctx.lineWidth = 6;
    ctx.stroke();
    ctx.globalAlpha = 0.75 * alpha;
    ctx.lineWidth = 1.25;
    ctx.stroke();
  };
  if (el.kind === "room") {
    const g = rc.index.roomGeo.get(el.id);
    if (g) {
      path(rc, g.polygon);
      ctx.globalAlpha = 0.07 * alpha;
      ctx.fill();
      ctx.setLineDash([6, 4]);
      ctx.globalAlpha = 0.8 * alpha;
      ctx.lineWidth = 1.5;
      ctx.stroke();
    }
  } else if (el.kind === "column" && el.shape === "round") {
    const s = toScreen(rc.view, el.center);
    ctx.beginPath();
    ctx.arc(s.x, s.y, (el.width_mm / 2) * rc.view.scale + 1, 0, Math.PI * 2);
    halo();
  } else if (el.kind === "pipe") {
    const plan = pipePlan(el.points);
    ctx.globalAlpha = 0.2 * alpha;
    ctx.lineWidth = pipeBandHalfPx(el, rc.view.scale, rc.uiScale) * 2 + 7;
    for (const run of plan.runs) {
      path(rc, run, false);
      ctx.stroke();
    }
    ctx.globalAlpha = 0.75 * alpha;
    ctx.lineWidth = 1.25;
    const r = pipeRiserRadiusPx(el, rc.view.scale, rc.uiScale) + 3;
    for (const q of plan.risers) {
      const s = toScreen(rc.view, q.point);
      ctx.beginPath();
      ctx.arc(s.x, s.y, r, 0, Math.PI * 2);
      ctx.stroke();
    }
  } else if (el.kind === "linework") {
    for (const pl of el.polylines) {
      if (pl.length < 2) continue;
      path(rc, pl, false);
      halo();
    }
  } else {
    const shape = elementShape(el, rc.index, { labelHeightMm: labelHeightMm(rc.view) });
    if (shape) {
      path(rc, shape.points, shape.closed);
      if (shape.closed && el.kind !== "underlay") {
        ctx.globalAlpha = 0.08 * alpha;
        ctx.fill();
      }
      halo();
    }
  }
  ctx.restore();
}

/**
 * One element redrawn on top of itself, scaled about its own center and
 * fading out: the settle after a placement and the flash after a change.
 */
export function drawFlash(rc: RenderContext, el: Element, color: string, alpha: number, scale: number, index: DocIndex = rc.index): void {
  if (alpha <= 0.002) return;
  const { ctx } = rc;
  ctx.save();
  if (scale !== 1) {
    const c = flashCenter(el, index, rc.view);
    ctx.translate(c.x, c.y);
    ctx.scale(scale, scale);
    ctx.translate(-c.x, -c.y);
  }
  drawElement(rc, el, { color, alpha, hollow: el.kind === "room" }, index);
  ctx.restore();
}

/** Screen point a flash scales about. */
function flashCenter(el: Element, index: DocIndex, view: View): P {
  const shape = elementShape(el, index, { labelHeightMm: labelHeightMm(view) });
  if (!shape || shape.points.length === 0) return toScreen(view, { x: 0, y: 0 });
  let x = 0;
  let y = 0;
  for (const p of shape.points) {
    x += p.x;
    y += p.y;
  }
  return toScreen(view, { x: x / shape.points.length, y: y / shape.points.length });
}
