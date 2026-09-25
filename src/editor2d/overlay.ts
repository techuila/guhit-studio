// Tool overlays: ghosts, grips, snap glyphs, guides, marquee, readouts,
// device links and the fall and height tags of service runs. Drawn after the
// model, in CSS pixel space.

import type { Element, Vec3, Wall } from "../contract/bindings";
import { pipeFalls } from "../contract/pipes";
import { useApp } from "../state/store";
import type { DrawnLink, PlacementGhost, TagDraw } from "./controller";
import { HANDLE_PX, K, type PlanController } from "./controller";
import { sameGrip, stretchedWalls, translateElement, walledJointMove } from "./edit";
import type { P } from "./geom";
import { add, dist, dot, left, lerp, mul, sub, unit } from "./geom";
import { arcPoint, controls, linkArc, linkKey } from "./links";
import type { LatchGuide } from "./mount";
import type { PipeShape } from "./pipe";
import { drainFallPct, formatHeight, formatPct, isRiser, pipeBandHalfPx, pipeNodes, pipePlan, planDist, planOf, segmentFallPct } from "./pipe";
import type { ElementStyle, RenderContext } from "./render";
import { dimensionTextBox, drawCamera, drawDimension, drawElement, drawOpening, drawPipe, drawWalls, pipeColor, roomLabelTextBox } from "./render";
import type { SnapResult, SnapType } from "./snap";
import { polar } from "./snap";
import type { Box, TagCandidate } from "./tags";
import { TAG_PRIORITY, layoutTags, pointSpots, segmentSpots } from "./tags";
import { formatAngle, formatArea, formatLength } from "./typed";
import { toScreen } from "./view";

const SNAP_LABEL: Record<SnapType, string> = {
  none: "",
  endpoint: "Endpoint",
  midpoint: "Midpoint",
  intersection: "Intersection",
  perpendicular: "Perpendicular",
  nearest: "On wall",
  face: "Wall face",
  extension: "Aligned",
  angle: "",
  grid: "",
  pipe_end: "Pipe end",
  pipe_joint: "Pipe joint",
  tee: "Tee",
  fixture: "Fixture",
};

const PILL_H = 18;
const TAG_H = 15;

function pillWidth(rc: RenderContext, text: string): number {
  const { ctx, palette } = rc;
  ctx.save();
  ctx.font = `11px ${palette.fontMono}`;
  const w = ctx.measureText(text).width + 12;
  ctx.restore();
  return w;
}

function pill(rc: RenderContext, at: P, text: string, opts: { bg?: string; fg?: string; align?: "left" | "center"; scale?: number; alpha?: number } = {}): void {
  const { ctx, palette } = rc;
  ctx.save();
  ctx.font = `11px ${palette.fontMono}`;
  const w = ctx.measureText(text).width + 12;
  const h = PILL_H;
  let x = opts.align === "center" ? at.x - w / 2 : at.x;
  let y = at.y - h / 2;
  x = Math.max(4, Math.min(rc.width - w - 4, x));
  y = Math.max(4, Math.min(rc.height - h - 4, y));
  if (opts.scale !== undefined && opts.scale !== 1) {
    // Grows from its left edge, where it is anchored to the cursor.
    ctx.translate(x, y + h / 2);
    ctx.scale(opts.scale, opts.scale);
    ctx.translate(-x, -(y + h / 2));
  }
  const a = opts.alpha ?? 1;
  ctx.fillStyle = opts.bg ?? palette.ink;
  ctx.globalAlpha = 0.92 * a;
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, 4);
  ctx.fill();
  ctx.globalAlpha = a;
  ctx.fillStyle = opts.fg ?? "#ffffff";
  ctx.textBaseline = "middle";
  ctx.textAlign = "left";
  ctx.fillText(text, x + 6, y + h / 2 + 0.5);
  ctx.restore();
}

interface PillPart {
  text: string;
  bg?: string;
  scale?: number;
}

/** Where a row of pills lands, kept inside the canvas. */
function pillRowBox(rc: RenderContext, at: P, parts: readonly PillPart[]): Box {
  const total = parts.reduce((sum, p) => sum + pillWidth(rc, p.text), 0) + 4 * Math.max(0, parts.length - 1);
  const x = Math.max(4, Math.min(rc.width - total - 4, at.x));
  const y = Math.max(4, Math.min(rc.height - PILL_H - 4, at.y - PILL_H / 2));
  return { x, y, w: total, h: PILL_H };
}

/** Pills side by side, kept inside the canvas as one row. */
function pillRow(rc: RenderContext, at: P, parts: readonly PillPart[], alpha = 1): void {
  let x = pillRowBox(rc, at, parts).x;
  for (const p of parts) {
    pill(rc, { x, y: at.y }, p.text, { bg: p.bg, scale: p.scale, alpha });
    x += pillWidth(rc, p.text) + 4;
  }
}

/** Where the snap label sits, next to the snapped point. */
function snapLabelBox(rc: RenderContext, r: SnapResult): Box | null {
  const label = r.label ?? SNAP_LABEL[r.type];
  if (!label) return null;
  const { ctx, palette } = rc;
  ctx.save();
  ctx.font = `10px ${palette.fontUi}`;
  const tw = ctx.measureText(label).width;
  ctx.restore();
  const s = toScreen(rc.view, r.point);
  return { x: s.x + 10, y: s.y - 20, w: tw + 4, h: 14 };
}

/**
 * The snap glyph and its guide lines. `alpha` fades the whole thing in and
 * out, `pop` scales the glyph about the snapped point when a snap engages.
 * Switching between two snap targets leaves both at rest, so the glyph slides
 * to the new point instead of popping again.
 */
function drawSnap(rc: RenderContext, r: SnapResult, alpha: number, pop: number): void {
  const { ctx, palette, view } = rc;
  if (alpha <= 0.002) return;
  ctx.save();
  ctx.globalAlpha = alpha;
  for (const g of r.guides) {
    const a = toScreen(view, g.from);
    const b = toScreen(view, g.to);
    // Run the guide a little past the point so it reads as a construction line.
    const d = unit(sub(b, a));
    ctx.strokeStyle = palette.selection;
    ctx.globalAlpha = (g.kind === "angle" ? 0.55 : 0.8) * alpha;
    ctx.lineWidth = 1;
    ctx.setLineDash(g.kind === "angle" ? [2, 4] : [5, 4]);
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x + d.x * 40, b.y + d.y * 40);
    ctx.stroke();
    if (g.kind === "extension") {
      ctx.setLineDash([]);
      ctx.beginPath();
      ctx.arc(a.x, a.y, 3, 0, Math.PI * 2);
      ctx.stroke();
    }
  }
  ctx.setLineDash([]);
  ctx.globalAlpha = alpha;
  const s = toScreen(view, r.point);
  const k = 6;
  if (pop !== 1) {
    ctx.translate(s.x, s.y);
    ctx.scale(pop, pop);
    ctx.translate(-s.x, -s.y);
  }
  ctx.strokeStyle = palette.selection;
  ctx.fillStyle = palette.surface;
  ctx.lineWidth = 1.8;
  ctx.beginPath();
  switch (r.type) {
    case "endpoint":
      ctx.rect(s.x - k, s.y - k, k * 2, k * 2);
      break;
    case "midpoint":
      ctx.moveTo(s.x, s.y - k - 1);
      ctx.lineTo(s.x + k + 1, s.y + k);
      ctx.lineTo(s.x - k - 1, s.y + k);
      ctx.closePath();
      break;
    case "intersection":
      ctx.moveTo(s.x - k, s.y - k);
      ctx.lineTo(s.x + k, s.y + k);
      ctx.moveTo(s.x + k, s.y - k);
      ctx.lineTo(s.x - k, s.y + k);
      break;
    case "perpendicular":
      ctx.moveTo(s.x - k, s.y - k);
      ctx.lineTo(s.x - k, s.y + k);
      ctx.lineTo(s.x + k, s.y + k);
      ctx.moveTo(s.x - k, s.y);
      ctx.lineTo(s.x, s.y);
      ctx.lineTo(s.x, s.y + k);
      break;
    case "nearest":
    case "face":
      ctx.moveTo(s.x, s.y - k - 1);
      ctx.lineTo(s.x + k + 1, s.y);
      ctx.lineTo(s.x, s.y + k + 1);
      ctx.lineTo(s.x - k - 1, s.y);
      ctx.closePath();
      break;
    case "extension":
      ctx.moveTo(s.x - k, s.y);
      ctx.lineTo(s.x + k, s.y);
      ctx.moveTo(s.x, s.y - k);
      ctx.lineTo(s.x, s.y + k);
      break;
    case "pipe_end":
    case "pipe_joint":
      ctx.rect(s.x - k, s.y - k, k * 2, k * 2);
      break;
    case "tee":
      // A ring with the joint in the middle.
      ctx.arc(s.x, s.y, k, 0, Math.PI * 2);
      ctx.moveTo(s.x + 2.5, s.y);
      ctx.arc(s.x, s.y, 2.5, 0, Math.PI * 2);
      break;
    case "fixture":
      ctx.arc(s.x, s.y, k, 0, Math.PI * 2);
      ctx.moveTo(s.x - k + 2, s.y);
      ctx.lineTo(s.x + k - 2, s.y);
      ctx.moveTo(s.x, s.y - k + 2);
      ctx.lineTo(s.x, s.y + k - 2);
      break;
    case "angle":
      ctx.arc(s.x, s.y, 3, 0, Math.PI * 2);
      break;
    case "grid":
      ctx.globalAlpha = 0.6 * alpha;
      ctx.lineWidth = 1;
      ctx.moveTo(s.x - 4, s.y);
      ctx.lineTo(s.x + 4, s.y);
      ctx.moveTo(s.x, s.y - 4);
      ctx.lineTo(s.x, s.y + 4);
      break;
    default:
      break;
  }
  if (r.type === "endpoint" || r.type === "midpoint" || r.type === "nearest" || r.type === "face" || r.type === "pipe_end" || r.type === "pipe_joint") {
    ctx.globalAlpha = 0.85 * alpha;
    ctx.fill();
    ctx.globalAlpha = alpha;
  }
  ctx.stroke();
  const label = r.label ?? SNAP_LABEL[r.type];
  if (label) {
    ctx.font = `10px ${palette.fontUi}`;
    // A light backing keeps the label readable over walls and pipes.
    const tw = ctx.measureText(label).width;
    ctx.globalAlpha = 0.85 * alpha;
    ctx.fillStyle = palette.paper;
    ctx.fillRect(s.x + 10, s.y - 20, tw + 4, 14);
    ctx.globalAlpha = alpha;
    ctx.fillStyle = palette.selection;
    ctx.textBaseline = "middle";
    ctx.fillText(label, s.x + 12, s.y - 13);
  }
  ctx.restore();
}

function ghostWall(a: P, b: P, thickness: number): Wall {
  return { id: "", level_id: "", start: a, end: b, thickness_mm: thickness, height_mm: null, material_id: null };
}

function noIndexWalls(rc: RenderContext, walls: Wall[], style: ElementStyle): void {
  // Ghost walls have no derived outline, so draw them against an index without wall geometry.
  const bare: RenderContext = { ...rc, index: { ...rc.index, wallGeo: new Map() } };
  drawWalls(bare, walls, () => style);
}

/**
 * What the overlay drew this frame that tags must keep clear of: the text of
 * the temporary dimensions, cursor readouts and snap labels. Reset per frame.
 */
let frameObstacles: Box[] = [];

/** A temporary dimension along a-b, offset to the given side in pixels. Its text is an obstacle for tags. */
function tempDimension(rc: RenderContext, a: P, b: P, offsetPx: number, style: ElementStyle = { color: rc.palette.selection }): void {
  if (dist(a, b) < 1) return;
  const d = { a, b, offset_mm: offsetPx / rc.view.scale, text_override: null };
  drawDimension(rc, d, style);
  const box = dimensionTextBox(rc, d);
  if (box) frameObstacles.push(box);
}

function drawGrips(rc: RenderContext, c: PlanController, op: PlanController["op"]): void {
  const { ctx, palette, view } = rc;
  const grips = c.gripList();
  if (grips.length === 0) return;
  const active = op.kind === "grip" ? op.grip : null;
  ctx.save();
  ctx.lineWidth = 1.5;
  ctx.strokeStyle = palette.selection;
  for (let i = 0; i < grips.length; i++) {
    const g = grips[i];
    if (active && sameGrip(active, g)) continue;
    // Scale in from nothing, one after the other, and grow a little under the pointer.
    const grow = c.anim.value(`${K.grip}${Math.min(i, 7)}`, 1);
    if (grow <= 0.002) continue;
    const scale = grow * (1 + 0.3 * c.anim.value(`${K.gripHover}${i}`, 0));
    const s = toScreen(view, g.pos);
    ctx.save();
    ctx.globalAlpha = Math.min(1, grow);
    if (scale !== 1) {
      ctx.translate(s.x, s.y);
      ctx.scale(scale, scale);
      ctx.translate(-s.x, -s.y);
    }
    ctx.fillStyle = palette.surface;
    ctx.beginPath();
    if (g.kind === "rotate") {
      const el = rc.index.byId.get(g.elementId);
      const pivot =
        el && el.kind === "asset"
          ? el.position
          : el && el.kind === "column"
            ? el.center
            : el && el.kind === "stair"
              ? el.origin
              : el && el.kind === "reference_model"
                ? el.position
                : null;
      if (pivot) {
        const p = toScreen(view, pivot);
        ctx.save();
        ctx.globalAlpha *= 0.6;
        ctx.setLineDash([3, 3]);
        ctx.moveTo(p.x, p.y);
        ctx.lineTo(s.x, s.y);
        ctx.stroke();
        ctx.restore();
        ctx.beginPath();
      }
      ctx.arc(s.x, s.y, 5.5, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(s.x, s.y, 2, 0, Math.PI * 2);
      ctx.fillStyle = palette.selection;
      ctx.fill();
    } else if (g.kind === "pipe_node") {
      ctx.arc(s.x, s.y, 4.5, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
    } else if (g.kind === "wall_mid" || g.kind === "dim_offset") {
      ctx.moveTo(s.x, s.y - 6);
      ctx.lineTo(s.x + 6, s.y);
      ctx.lineTo(s.x, s.y + 6);
      ctx.lineTo(s.x - 6, s.y);
      ctx.closePath();
      ctx.fill();
      ctx.stroke();
    } else {
      ctx.rect(s.x - 4.5, s.y - 4.5, 9, 9);
      ctx.fill();
      ctx.stroke();
    }
    ctx.restore();
  }
  ctx.restore();
}

// ---------------------------------------------------------------- device links

/**
 * A link: a dashed curve from the device to its load with a dot at each end.
 * `upto` below 1 draws only the first part, so a new link draws itself in.
 */
function drawLinkArc(rc: RenderContext, from: P, to: P, bow: number, o: { color: string; alpha: number; width: number; upto?: number }): void {
  if (o.alpha <= 0.002) return;
  const { ctx, view } = rc;
  const arc = linkArc(from, to, bow);
  const a = toScreen(view, from);
  const cp = toScreen(view, arc.control);
  const b = toScreen(view, to);
  const upto = Math.max(0, Math.min(1, o.upto ?? 1));
  ctx.save();
  ctx.globalAlpha = o.alpha;
  ctx.strokeStyle = o.color;
  ctx.fillStyle = o.color;
  ctx.lineWidth = o.width;
  ctx.lineCap = "round";
  ctx.setLineDash([6, 4]);
  ctx.beginPath();
  ctx.moveTo(a.x, a.y);
  if (upto >= 0.999) ctx.quadraticCurveTo(cp.x, cp.y, b.x, b.y);
  else {
    const n = 24;
    for (let i = 1; i <= n; i++) {
      const p = arcPoint(a, cp, b, (i / n) * upto);
      ctx.lineTo(p.x, p.y);
    }
  }
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.beginPath();
  ctx.arc(a.x, a.y, 2.4, 0, Math.PI * 2);
  ctx.fill();
  if (upto >= 0.999) {
    ctx.beginPath();
    ctx.arc(b.x, b.y, 2.4, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.restore();
}

/** The small round handle in the middle of a link: click it to flip the bow. */
function drawBowHandle(rc: RenderContext, at: P, hover: number, color: string, alpha: number): void {
  const { ctx, palette, view } = rc;
  const s = toScreen(view, at);
  const r = HANDLE_PX * (1 + 0.35 * hover);
  ctx.save();
  ctx.globalAlpha = alpha;
  ctx.beginPath();
  ctx.arc(s.x, s.y, r, 0, Math.PI * 2);
  ctx.fillStyle = palette.surface;
  ctx.fill();
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.5;
  ctx.stroke();
  // Two small chevrons across the curve: it can go either way.
  ctx.beginPath();
  ctx.moveTo(s.x - r * 0.45, s.y - r * 0.15);
  ctx.lineTo(s.x, s.y - r * 0.55);
  ctx.lineTo(s.x + r * 0.45, s.y - r * 0.15);
  ctx.moveTo(s.x - r * 0.45, s.y + r * 0.15);
  ctx.lineTo(s.x, s.y + r * 0.55);
  ctx.lineTo(s.x + r * 0.45, s.y + r * 0.15);
  ctx.lineWidth = 1.2;
  ctx.stroke();
  ctx.restore();
}

/**
 * The links of the selected device (or of the one the link tool picked), a
 * faint overview of every other link while the link tool is on, links that
 * a change removed fading out, and in the link tool, the link a click would
 * add (a preview) or remove (drawn in the danger color).
 */
function drawLinks(rc: RenderContext, c: PlanController): void {
  const { palette } = rc;
  const s = useApp.getState();
  const list = c.linkDrawList();
  const preview = s.preview;
  const tinted = preview ? new Set([...preview.diff.added, ...preview.diff.modified]) : null;
  const linkTool = s.tool === "link";
  const source = c.linkSource();
  const { all } = c.devices();
  // The link a click on the hovered device would toggle.
  let pending: { key: string; adding: boolean; from: P; to: P; bow: number } | null = null;
  if (linkTool && source && s.hoverId && s.hoverId !== source && !c.hoverHandle) {
    const a = all.get(source);
    const b = all.get(s.hoverId);
    if (a && b) {
      const pair = controls(a.role, b.role) ? { c: a, l: b } : controls(b.role, a.role) ? { c: b, l: a } : null;
      if (pair) {
        const key = linkKey(pair.c.el.id, pair.l.el.id);
        const drawn = list.find((l) => l.key === key);
        const ends = drawn ?? c.linkEnds(pair.c.el, pair.l.el);
        if (ends) pending = { key, adding: !pair.c.el.links.includes(pair.l.el.id), from: ends.from, to: ends.to, bow: c.linkBow(pair.c.el.id, pair.l.el.id, ends.from, ends.to) };
      }
    }
  }
  const focus = new Set(linkTool ? (source ? [source] : []) : s.selection);
  for (const f of c.fadingLinks) {
    if (!linkTool && !focus.has(f.controllerId) && !focus.has(f.loadId)) continue;
    const v = c.anim.value(`${K.linkGone}${f.key}`, 0);
    drawLinkArc(rc, f.from, f.to, f.bow, { color: palette.selection, alpha: v, width: 1.6 });
  }
  const strongOf = (l: DrawnLink): string => (tinted?.has(l.controllerId) ? palette.preview : palette.selection);
  for (const l of list) {
    if (l.strong) continue;
    drawLinkArc(rc, l.from, l.to, l.bow, { color: palette.ink3, alpha: 0.45, width: 1.1, upto: c.anim.value(`${K.linkGrow}${l.key}`, 1) });
  }
  for (const l of list) {
    if (!l.strong) continue;
    const removing = pending && !pending.adding && pending.key === l.key;
    drawLinkArc(rc, l.from, l.to, l.bow, { color: removing ? palette.danger : strongOf(l), alpha: 1, width: 1.6, upto: c.anim.value(`${K.linkGrow}${l.key}`, 1) });
  }
  if (pending?.adding) drawLinkArc(rc, pending.from, pending.to, pending.bow, { color: palette.selection, alpha: 0.5, width: 1.4 });
  for (const l of list) {
    if (!l.handle) continue;
    drawBowHandle(rc, l.handle, c.anim.value(`${K.bowHover}${l.key}`, 0), strongOf(l), 1);
  }
}

/** Pills for the link tool: what a click on the hovered device does, or why it cannot. */
function linkPills(rc: RenderContext, c: PlanController): PillPart[] {
  const { palette } = rc;
  const s = useApp.getState();
  if (s.tool !== "link" && !c.hoverHandle) return [];
  if (c.hoverHandle) return [{ text: "Flip the curve" }];
  const parts: PillPart[] = [];
  if (c.linkNotice) parts.push({ text: c.linkNotice, bg: palette.danger, scale: 1 + 0.12 * c.anim.value(K.placeRefused, 0) });
  const source = c.linkSource();
  const hover = s.hoverId;
  if (!hover) return parts;
  const { all } = c.devices();
  const h = all.get(hover);
  if (!h) return parts;
  if (!source) {
    parts.push({ text: "Pick" });
    return parts;
  }
  if (hover === source) {
    parts.push({ text: "Done" });
    return parts;
  }
  const a = all.get(source);
  if (!a) return parts;
  const pair = controls(a.role, h.role) ? { ctl: a.el, load: h.el.id } : controls(h.role, a.role) ? { ctl: h.el, load: a.el.id } : null;
  if (!pair) parts.push({ text: "Pick" });
  else if (pair.ctl.links.includes(pair.load)) parts.push({ text: "Unlink", bg: palette.danger });
  else parts.push({ text: "Link", bg: palette.selection });
  return parts;
}

// ---------------------------------------------------------------- mounted objects

/**
 * The latch-side switch guide: ticks at the latch jamb and at the switch
 * point on the wall face, and the 200 mm between them. Strong once the
 * switch snapped to it, faint while it is only offered.
 */
function drawLatchGuide(rc: RenderContext, g: LatchGuide & { snapped: boolean }, alpha: number): void {
  const { ctx, palette, view } = rc;
  const color = g.snapped ? palette.selection : palette.ink3;
  const a = alpha * (g.snapped ? 1 : 0.7);
  if (a <= 0.002) return;
  const tick = (p: P, len: number): void => {
    const s0 = toScreen(view, p);
    const s1 = toScreen(view, add(p, mul(g.out, len / view.scale)));
    ctx.beginPath();
    ctx.moveTo(s0.x, s0.y);
    ctx.lineTo(s1.x, s1.y);
    ctx.stroke();
  };
  ctx.save();
  ctx.globalAlpha = a;
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.5;
  tick(g.jamb, 12);
  ctx.setLineDash([3, 3]);
  tick(g.point, 18);
  ctx.restore();
  if (!g.snapped) {
    // The target, a dashed ring where the switch center would go.
    const p = toScreen(view, add(g.point, mul(g.out, 6 / view.scale)));
    ctx.save();
    ctx.globalAlpha = a;
    ctx.strokeStyle = color;
    ctx.setLineDash([2, 3]);
    ctx.beginPath();
    ctx.arc(p.x, p.y, 7, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
  }
  const side = dot(left(g.away), g.out) >= 0 ? 1 : -1;
  tempDimension(rc, g.jamb, g.point, side * 22, { color, alpha: a });
}

/** Ghost of the asset tool: the object, the face or window it mounts on, the switch guide. */
function drawPlacementGhost(rc: RenderContext, c: PlanController, pg: PlacementGhost, pa: number): void {
  const { ctx, palette, view } = rc;
  const m = pg.mount;
  const invalid = !!m && !m.valid;
  drawElement(rc, pg.element as Element, { color: invalid ? palette.danger : palette.selection, alpha: 0.85 * pa });
  const live = !!c.placementGhost;
  const face = m?.face ?? pg.faceSnap?.face ?? null;
  if (face && live) {
    const a = toScreen(view, face.a);
    const b = toScreen(view, face.b);
    ctx.save();
    ctx.strokeStyle = palette.selection;
    ctx.lineWidth = 2;
    ctx.globalAlpha = 0.7 * pa;
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.stroke();
    ctx.restore();
  }
  if (m?.window && live) {
    const a = toScreen(view, m.window.a);
    const b = toScreen(view, m.window.b);
    ctx.save();
    ctx.strokeStyle = invalid ? palette.danger : palette.selection;
    ctx.lineWidth = 4;
    ctx.lineCap = "round";
    ctx.globalAlpha = 0.35 * pa;
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.stroke();
    ctx.restore();
  }
  if (m?.guide && live) drawLatchGuide(rc, m.guide, pa);
}

/** The mounting height, and why a click would place nothing, next to the cursor. */
function placementPills(rc: RenderContext, c: PlanController, pg: PlacementGhost): PillPart[] {
  const m = pg.mount;
  if (!m || !c.placementGhost) return [];
  const parts: PillPart[] = [];
  if (m.heightLabel) parts.push({ text: m.heightLabel });
  if (!m.valid && m.reason) parts.push({ text: m.reason, bg: rc.palette.danger, scale: 1 + 0.12 * c.anim.value(K.placeRefused, 0) });
  return parts;
}

// ---------------------------------------------------------------- fall and height tags

interface TagSpec {
  cand: TagCandidate;
  text: string;
  color: string;
  filled: boolean;
  alpha: number;
}

function tagWidth(rc: RenderContext, text: string): number {
  rc.ctx.save();
  rc.ctx.font = `10px ${rc.palette.fontMono}`;
  const w = rc.ctx.measureText(text).width + 8;
  rc.ctx.restore();
  return w;
}

/**
 * Height tags of a run's nodes above the floor of its level, as the inspector
 * gives them: "+300", or "+300 to +1200" at a riser in run order.
 */
function heightTags(rc: RenderContext, pipe: Pick<PipeShape, "system" | "points">, idPrefix: string, alpha: number, onlyRisers = false): TagSpec[] {
  const color = pipeColor(rc.palette, pipe.system);
  const nodes = pipeNodes(pipe.points);
  const out: TagSpec[] = [];
  nodes.forEach((n, i) => {
    const riser = isRiser(n);
    if (onlyRisers && !riser) return;
    const text = riser ? `${formatHeight(n.zIn, rc.unit)} to ${formatHeight(n.zOut, rc.unit)}` : formatHeight(n.zIn, rc.unit);
    const q = toScreen(rc.view, n.point);
    const w = tagWidth(rc, text);
    const priority = riser ? TAG_PRIORITY.riser : i === 0 || i === nodes.length - 1 ? TAG_PRIORITY.endHeight : TAG_PRIORITY.height;
    out.push({ cand: { id: `${idPrefix}:h:${n.index}`, priority, w, h: TAG_H, spots: pointSpots(q.x, q.y, w, TAG_H) }, text, color, filled: false, alpha });
  });
  return out;
}

/**
 * The fall of each horizontal segment of a run that falls (drainage, storm,
 * condensate), as a percent of its plan length. Below the default fall it
 * turns to the warning color, flat or uphill to the danger color.
 * Suggestions only: the review tab has the same checks.
 */
function fallTags(rc: RenderContext, pipe: Pick<PipeShape, "system" | "diameter_mm" | "points">, idPrefix: string, alpha: number): TagSpec[] {
  if (!pipeFalls(pipe.system)) return [];
  const { palette, view } = rc;
  const minPct = drainFallPct(pipe.diameter_mm);
  const out: TagSpec[] = [];
  const points: readonly Vec3[] = pipe.points;
  for (let i = 0; i + 1 < points.length; i++) {
    const pct = segmentFallPct(points[i], points[i + 1]);
    if (pct === null) continue;
    const sa = toScreen(view, points[i]);
    const sb = toScreen(view, points[i + 1]);
    if (Math.hypot(sb.x - sa.x, sb.y - sa.y) < 48) continue;
    const ok = pct >= minPct - 0.01;
    // A fall under the default rounds down, so a 1.95% warning never reads "2%".
    const shown = ok ? pct : Math.max(0.1, Math.floor(pct * 10) / 10);
    const text = pct <= 0.005 ? (pct < -0.005 ? `rises ${formatPct(pct)}` : "flat") : `${formatPct(shown)} fall`;
    const color = ok ? pipeColor(palette, pipe.system) : pct > 0.005 ? palette.warn : palette.danger;
    const w = tagWidth(rc, text);
    out.push({
      cand: { id: `${idPrefix}:f:${i}`, priority: ok ? TAG_PRIORITY.fall : TAG_PRIORITY.fallProblem, w, h: TAG_H, spots: segmentSpots(sa, sb, w, TAG_H) },
      text,
      color,
      filled: true,
      alpha,
    });
  }
  return out;
}

/** A small label box. */
function tag(rc: RenderContext, x: number, y: number, text: string, color: string, alpha: number, filled: boolean): void {
  const { ctx, palette } = rc;
  ctx.save();
  ctx.font = `10px ${palette.fontMono}`;
  const w = ctx.measureText(text).width + 8;
  const h = TAG_H;
  ctx.globalAlpha = alpha * (filled ? 0.92 : 0.9);
  ctx.fillStyle = filled ? color : palette.paper;
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, 3);
  ctx.fill();
  if (!filled) {
    ctx.globalAlpha = alpha;
    ctx.strokeStyle = color;
    ctx.lineWidth = 1;
    ctx.stroke();
  }
  ctx.globalAlpha = alpha;
  ctx.fillStyle = filled ? "#ffffff" : palette.ink;
  ctx.textBaseline = "middle";
  ctx.textAlign = "left";
  ctx.fillText(text, x + 4, y + h / 2 + 0.5);
  ctx.restore();
}

/**
 * Lays the tags out clear of each other, of every dimension text and room
 * label on the plan, and of what the overlay drew this frame, dropping the
 * least important when crowded. Placed tags fade in, dropped ones fade out.
 */
function drawTags(rc: RenderContext, c: PlanController, specs: readonly TagSpec[]): void {
  let draws: TagDraw[] = [];
  if (specs.length > 0) {
    const obstacles: Box[] = [...frameObstacles];
    for (const el of rc.index.visible) {
      const box = el.kind === "dimension" ? dimensionTextBox(rc, el) : el.kind === "room" ? roomLabelTextBox(rc, el) : null;
      if (box) obstacles.push(box);
    }
    const byId = new Map(specs.map((t) => [t.cand.id, t] as const));
    const placed = layoutTags(
      specs.map((t) => t.cand),
      obstacles,
      { w: rc.width, h: rc.height },
    );
    c.tagLayout = { asked: specs.length, placed, obstacles };
    draws = placed.map((p) => {
      const t = byId.get(p.id) as TagSpec;
      return { id: p.id, box: p.box, text: t.text, color: t.color, filled: t.filled, alpha: t.alpha };
    });
  } else c.tagLayout = null;
  for (const t of c.syncTags(draws)) {
    const v = c.anim.value(`${K.tagOut}${t.id}`, 0);
    if (v > 0.002) tag(rc, t.box.x, t.box.y, t.text, t.color, t.alpha * v, t.filled);
  }
  for (const t of draws) {
    const a = t.alpha * c.anim.value(`${K.tagIn}${t.id}`, 1);
    if (a > 0.002) tag(rc, t.box.x, t.box.y, t.text, t.color, a, t.filled);
  }
}

// ---------------------------------------------------------------- the overlay

export function drawOverlay(rc: RenderContext, c: PlanController): void {
  const { ctx, palette, view, index } = rc;
  const s = useApp.getState();
  frameObstacles = [];
  // A rejected drag keeps drawing its ghost while it eases back to the origin.
  const returning = c.returningOp();
  const op = returning ?? c.op;
  const unitName = rc.unit;
  // Pick up and drop: the dragged ghost lifts off the sheet and settles back.
  const lift = returning ? 0 : c.anim.value(K.lift, 0);
  const fade = returning ? 0.25 + 0.75 * c.anim.value(K.back, 0) : 1;
  const ghost: ElementStyle = { color: palette.selection, alpha: (0.6 + 0.3 * lift) * fade, hollow: true };
  const solidGhost: ElementStyle = { color: palette.selection, alpha: 0.55 };
  const thickness = s.toolOptions.wallThicknessMm ?? index.doc.project.settings.default_wall_thickness_mm;
  const cursorScreen = c.cursorScreen;
  // The pipe run as drawn so far and the segment the next click adds.
  const pipeDraft = op.kind === "pipe" ? c.pipePreview(op) : null;
  const tags: TagSpec[] = [];

  // Links under the grips and ghosts: they belong to the selection.
  drawLinks(rc, c);
  drawGrips(rc, c, op);

  if (lift > 0.002) {
    ctx.save();
    ctx.shadowColor = `rgba(20, 40, 63, ${0.3 * lift})`;
    ctx.shadowBlur = 14 * lift;
    ctx.shadowOffsetY = 3 * lift;
  }

  switch (op.kind) {
    case "marquee": {
      drawMarquee(rc, op.start, op.current, 1);
      break;
    }
    case "move": {
      const moved = new Set(op.ids);
      const walls = stretchedWalls(index, op.duplicate ? new Set() : moved, op.delta);
      const dupWalls: Wall[] = [];
      for (const id of op.ids) {
        const el = index.byId.get(id);
        if (!el) continue;
        if (el.kind === "wall") {
          if (op.duplicate) dupWalls.push(translateElement(el, op.delta) as Wall);
          continue;
        }
        if (el.kind === "room" || el.kind === "opening") continue;
        drawElement(rc, translateElement(el, op.delta), ghost);
      }
      noIndexWalls(rc, op.duplicate ? dupWalls : walls, ghost);
      if (cursorScreen && !returning) {
        const text = `${op.duplicate ? "Copy  " : ""}dx ${formatLength(op.delta.x, unitName)}  dy ${formatLength(op.delta.y, unitName)}`;
        pill(rc, { x: cursorScreen.x + 16, y: cursorScreen.y + 22 }, text);
      }
      break;
    }
    case "slide": {
      const o = { ...op.opening, offset_mm: op.placement.offset };
      drawOpening(rc, o, op.host, { color: op.placement.valid ? palette.selection : palette.danger, alpha: fade }, false);
      if (op.placement.valid && !returning) {
        drawClearDims(rc, op.host, o.width_mm, op.placement.offset, op.placement.clearStart, op.placement.clearEnd, o.flip_side ? 1 : -1);
      }
      break;
    }
    case "grip": {
      const el = op.element;
      if (el.kind === "wall") {
        let walls: Wall[] = [];
        if (op.grip.kind === "wall_mid") walls = stretchedWalls(index, new Set([el.id]), sub(op.current, op.grip.pos));
        else walls = walledJointMove(index, op.grip.pos, op.current);
        noIndexWalls(rc, walls, ghost);
        const me = walls.find((w) => w.id === el.id);
        if (me) tempDimension(rc, me.start, me.end, 26);
      } else if (el.kind === "pipe") {
        const next = c.gripResult(op);
        if (next && next.kind === "pipe") {
          drawPipe(rc, next, { alpha: 0.55 * fade });
          drawElement(rc, next, ghost);
          // Plan lengths of the segments on each side of the moved node.
          const nodes = pipeNodes(next.points);
          const i = nodes.findIndex((n) => planDist(n.point, op.current) < 1);
          if (!returning && i >= 0) {
            if (i > 0) tempDimension(rc, nodes[i - 1].point, nodes[i].point, 26);
            if (i + 1 < nodes.length) tempDimension(rc, nodes[i].point, nodes[i + 1].point, 26);
          }
          tags.push(...heightTags(rc, next, el.id, fade), ...fallTags(rc, next, el.id, fade));
        }
      } else {
        const next = c.gripResult(op);
        if (next) {
          drawElement(rc, next, el.kind === "dimension" || el.kind === "camera" ? { color: palette.selection, alpha: fade } : ghost);
          if ("rotation_deg" in next && cursorScreen && !returning) {
            pill(rc, { x: cursorScreen.x + 16, y: cursorScreen.y + 22 }, formatAngle(next.rotation_deg));
          }
        }
      }
      break;
    }
    case "wall": {
      const pts = op.points;
      const walls: Wall[] = [];
      for (let i = 0; i + 1 < pts.length; i++) walls.push(ghostWall(pts[i], pts[i + 1], thickness));
      const last = pts[pts.length - 1];
      const typedEnd = op.typed ? c.typedWallPoint(op) : null;
      const end = op.committing ? null : (typedEnd ?? c.snapResult?.point ?? c.cursorWorld);
      if (end && dist(end, last) > 1) walls.push(ghostWall(last, end, thickness));
      noIndexWalls(rc, walls, solidGhost);
      // centerline and nodes
      ctx.save();
      ctx.strokeStyle = palette.selection;
      ctx.fillStyle = palette.surface;
      ctx.lineWidth = 1;
      ctx.setLineDash([6, 3, 1, 3]);
      ctx.beginPath();
      [...pts, ...(end ? [end] : [])].forEach((p, i) => {
        const q = toScreen(view, p);
        if (i === 0) ctx.moveTo(q.x, q.y);
        else ctx.lineTo(q.x, q.y);
      });
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.lineWidth = 1.5;
      for (const p of pts) {
        const q = toScreen(view, p);
        ctx.beginPath();
        ctx.arc(q.x, q.y, 3.5, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      }
      ctx.restore();
      if (end && dist(end, last) > 1) {
        tempDimension(rc, last, end, 30);
        const pr = polar(last, end);
        const closing = pts.length >= 3 && dist(end, pts[0]) < 1;
        if (cursorScreen && !op.typed) {
          pill(rc, { x: cursorScreen.x + 18, y: cursorScreen.y + 24 }, `${formatLength(pr.length, unitName, true)}  ${formatAngle(pr.angle)}${closing ? "  close" : ""}`);
        }
      }
      break;
    }
    case "pipe": {
      const spec = c.pipeSpec();
      const color = pipeColor(palette, spec.system);
      const shape = (points: Vec3[]): PipeShape => ({ system: spec.system, diameter_mm: spec.diameterMm, points });
      const { placed, band } = pipeDraft ?? { placed: [], band: [] };
      drawPipe(rc, shape(placed), { alpha: 0.9 }, "lines");
      if (band.length > 1) {
        // A soft glow marks the segment the next click adds.
        const runs = pipePlan(band).runs;
        if (runs.length > 0) {
          ctx.save();
          ctx.globalAlpha = 0.16;
          ctx.strokeStyle = color;
          ctx.lineWidth = pipeBandHalfPx({ system: spec.system, diameter_mm: spec.diameterMm }, view.scale) * 2 + 8;
          ctx.lineCap = "round";
          ctx.lineJoin = "round";
          ctx.beginPath();
          for (const run of runs) {
            run.forEach((p, i) => {
              const q = toScreen(view, p);
              if (i === 0) ctx.moveTo(q.x, q.y);
              else ctx.lineTo(q.x, q.y);
            });
          }
          ctx.stroke();
          ctx.restore();
        }
        drawPipe(rc, shape(band), { alpha: 0.8 }, "lines");
      }
      // Nodes placed so far.
      ctx.save();
      ctx.strokeStyle = color;
      ctx.fillStyle = palette.surface;
      ctx.lineWidth = 1.5;
      for (const n of pipeNodes(placed)) {
        if (isRiser(n)) continue;
        const q = toScreen(view, n.point);
        ctx.beginPath();
        ctx.arc(q.x, q.y, 3.5, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      }
      ctx.restore();
      const run = [...placed, ...band.slice(1)];
      drawPipe(rc, shape(run), { alpha: 0.95 }, "risers");
      // Falls of the runs that fall, and heights where the run climbs or drops.
      tags.push(...fallTags(rc, shape(run), "draft", 1), ...heightTags(rc, shape(run), "draft", 1, true));
      const start = band[0];
      const end = band[band.length - 1];
      // Length on the left of the direction of travel, like the wall tool; fall tags go on the right.
      if (band.length > 1 && planDist(start, end) > 1) tempDimension(rc, planOf(start), planOf(end), 30);
      break;
    }
    case "rect": {
      const corner = c.rectCommitCorner ?? c.rectCorner(op);
      if (corner && dist(corner, op.origin) > 1) {
        const a = op.origin;
        const b = { x: corner.x, y: a.y };
        const d = { x: a.x, y: corner.y };
        noIndexWalls(rc, [ghostWall(a, b, thickness), ghostWall(b, corner, thickness), ghostWall(corner, d, thickness), ghostWall(d, a, thickness)], solidGhost);
        const below = corner.y >= a.y ? -1 : 1;
        const leftSide = corner.x >= a.x ? 1 : -1;
        tempDimension(rc, a.x <= b.x ? a : b, a.x <= b.x ? b : a, below * 30);
        tempDimension(rc, a.y <= d.y ? a : d, a.y <= d.y ? d : a, leftSide * 30);
        const w = Math.abs(corner.x - a.x);
        const dep = Math.abs(corner.y - a.y);
        const net = Math.max(0, w - thickness) * Math.max(0, dep - thickness);
        const mid = toScreen(view, lerp(a, corner, 0.5));
        pill(rc, mid, formatArea(net), { align: "center", bg: palette.selection });
      }
      break;
    }
    case "dimension": {
      if (!op.b) {
        const end = c.snapResult?.point ?? c.cursorWorld;
        if (end) {
          ctx.save();
          ctx.strokeStyle = palette.dimension;
          ctx.setLineDash([4, 3]);
          const a = toScreen(view, op.a);
          const b = toScreen(view, end);
          ctx.beginPath();
          ctx.moveTo(a.x, a.y);
          ctx.lineTo(b.x, b.y);
          ctx.stroke();
          ctx.restore();
          if (cursorScreen && dist(op.a, end) > 1) pill(rc, { x: cursorScreen.x + 18, y: cursorScreen.y + 24 }, formatLength(dist(op.a, end), unitName, true));
        }
      } else {
        drawDimension(rc, { a: op.a, b: op.b, offset_mm: c.dimensionOffset(op), text_override: null }, { color: palette.selection });
      }
      break;
    }
    case "camera": {
      const t = c.snapResult?.point ?? c.cursorWorld ?? op.position;
      drawCamera(rc, { position: { ...op.position, z: 1600 }, target: { x: t.x, y: t.y, z: 1600 }, fov_deg: 60, name: "" }, { color: palette.selection });
      break;
    }
    default:
      break;
  }

  if (lift > 0.002) ctx.restore();

  // Point heights and falls of the one selected run (a node being dragged shows them on its ghost).
  if (s.selection.length === 1 && !(op.kind === "grip" && op.element.id === s.selection[0]) && op.kind !== "move") {
    const el = index.byId.get(s.selection[0]);
    if (el && el.kind === "pipe" && index.visibleIds.has(el.id)) {
      const a = c.anim.value(`${K.sel}${el.id}`, 1);
      tags.push(...heightTags(rc, el, el.id, a), ...fallTags(rc, el, el.id, a));
    }
  }

  // The ring where the pipe tool just placed a point.
  const pv = c.anim.value(K.pipePulse, 0);
  if (c.pipePulse && pv > 0.002) {
    const q = toScreen(view, c.pipePulse.at);
    ctx.save();
    ctx.globalAlpha = pv * 0.9;
    ctx.strokeStyle = pipeColor(palette, c.pipePulse.system);
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(q.x, q.y, 4 + 12 * (1 - pv), 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
  }

  // Next to the cursor: the length and angle of the segment being drawn, then
  // the height of the next point in the system color. Hidden while a value is typed.
  const cursorAt = cursorScreen ? { x: cursorScreen.x + 18, y: cursorScreen.y + 24 } : null;
  const cursorParts: PillPart[] = [];
  const typedOpen = !!c.heightEntry || (op.kind === "pipe" && !!op.typed);
  if (s.tool === "pipe" && cursorScreen && c.pointerInside && !typedOpen && !("committing" in op && op.committing) && !c.pipeLayer().locked) {
    const band = pipeDraft?.band ?? [];
    if (band.length > 1 && planDist(band[0], band[band.length - 1]) > 1) {
      const pr = polar(planOf(band[0]), planOf(band[band.length - 1]));
      cursorParts.push({ text: `${formatLength(pr.length, unitName, true)}  ${formatAngle(pr.angle)}` });
    }
    const bump = 1 + 0.12 * c.anim.value(K.pipeHeight, 0);
    cursorParts.push({ text: `h ${formatHeight(c.pipeNextZ(), unitName, true)}`, bg: pipeColor(palette, c.pipeSpec().system), scale: bump });
  }
  // The link tool, or a flip handle under the pointer.
  if (cursorScreen && c.pointerInside) cursorParts.push(...linkPills(rc, c));

  // Hover ghosts of the placement tools. They keep their last geometry while
  // they fade out, so losing a target is not a pop.
  const og = c.openingGhost ?? c.lastOpeningGhost;
  const oa = c.anim.value(K.ghostOpening, 0);
  if (og && oa > 0.002) {
    const color = og.placement.valid ? palette.selection : palette.danger;
    const swing = c.anim.value(K.swing, og.opening.flip_side ? -1 : 1);
    const hinge = c.anim.value(K.hinge, og.opening.flip_hinge ? 1 : 0);
    drawOpening(rc, og.opening, og.host, { color, alpha: oa, swing, hinge }, og.placement.valid);
    if (oa > 0.85 && c.openingGhost) {
      if (og.placement.valid) {
        drawClearDims(rc, og.host, og.opening.width_mm, og.placement.offset, og.placement.clearStart, og.placement.clearEnd, swing >= 0 ? -1 : 1);
        if (og.placement.snapped === "center" && cursorScreen) {
          pill(rc, { x: cursorScreen.x + 16, y: cursorScreen.y - 22 }, "Centered", { bg: palette.selection });
        }
      } else if (cursorScreen) {
        pill(rc, { x: cursorScreen.x + 16, y: cursorScreen.y + 22 }, og.placement.reason === "overlap" ? "Overlaps another opening" : "Wall too short", { bg: palette.danger });
      }
    }
  }
  const pg = c.placementGhost ?? c.lastPlacementGhost;
  const pa = c.anim.value(K.ghostPlace, 0);
  let placeParts: PillPart[] = [];
  if (pg && pa > 0.002 && !c.placing) {
    drawPlacementGhost(rc, c, pg, pa);
    if (cursorScreen && c.pointerInside) placeParts = placementPills(rc, c, pg);
  }

  // The marquee keeps its last rectangle while it fades out after the release.
  if (op.kind !== "marquee") {
    const mf = c.marqueeFade;
    const ma = c.anim.value(K.marquee, 0);
    if (mf && ma > 0.002) drawMarquee(rc, mf.start, mf.current, ma);
  }

  // Tags last among the drawings, clear of the readouts drawn on top of them.
  const snapped = c.snapResult ?? c.lastSnap;
  const sa = c.anim.value(K.snapAlpha, 0);
  const showSnap = !!snapped && snapped.type !== "none" && sa > 0.002 && !("committing" in op && op.committing);
  if (showSnap && snapped) {
    const box = snapLabelBox(rc, snapped);
    if (box) frameObstacles.push(box);
  }
  const placeAt = cursorScreen ? { x: cursorScreen.x + 16, y: cursorScreen.y + 22 } : null;
  if (cursorAt && cursorParts.length > 0) frameObstacles.push(pillRowBox(rc, cursorAt, cursorParts));
  if (placeAt && placeParts.length > 0) frameObstacles.push(pillRowBox(rc, placeAt, placeParts));
  drawTags(rc, c, tags);

  if (cursorAt && cursorParts.length > 0) pillRow(rc, cursorAt, cursorParts);
  if (placeAt && placeParts.length > 0) pillRow(rc, placeAt, placeParts, pa);
  if (showSnap && snapped) drawSnap(rc, snapped, sa, c.anim.value(K.snapPop, 1));
}

function drawMarquee(rc: RenderContext, start: P, current: P, alpha: number): void {
  const { ctx, palette, view } = rc;
  const a = toScreen(view, start);
  const b = toScreen(view, current);
  const crossing = current.x < start.x;
  ctx.save();
  ctx.strokeStyle = crossing ? palette.dimension : palette.selection;
  ctx.fillStyle = crossing ? palette.dimension : palette.selection;
  ctx.lineWidth = 1;
  if (crossing) ctx.setLineDash([5, 4]);
  ctx.globalAlpha = 0.09 * alpha;
  ctx.fillRect(a.x, a.y, b.x - a.x, b.y - a.y);
  ctx.globalAlpha = alpha;
  ctx.strokeRect(Math.round(a.x) + 0.5, Math.round(a.y) + 0.5, Math.round(b.x - a.x), Math.round(b.y - a.y));
  ctx.restore();
}

/** Live clear distances on both sides of an opening, drawn on the side away from the swing. */
function drawClearDims(rc: RenderContext, host: Wall, width: number, offset: number, clearStart: number, clearEnd: number, side: 1 | -1): void {
  const d = unit(sub(host.end, host.start));
  const n = { x: -d.y, y: d.x };
  const base = mul(n, side * (host.thickness_mm / 2));
  const jambA = add(add(host.start, mul(d, offset - width / 2)), base);
  const jambB = add(add(host.start, mul(d, offset + width / 2)), base);
  const cornerA = sub(jambA, mul(d, clearStart));
  const cornerB = add(jambB, mul(d, clearEnd));
  const off = side * 22;
  if (clearStart > 1) tempDimension(rc, cornerA, jambA, off);
  tempDimension(rc, jambA, jambB, off);
  if (clearEnd > 1) tempDimension(rc, jambB, cornerB, off);
}
