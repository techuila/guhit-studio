// Tool overlays: ghosts, grips, snap glyphs, guides, marquee and readouts.
// Drawn after the model, in CSS pixel space.

import type { Element, Vec3, Wall } from "../contract/bindings";
import { useApp } from "../state/store";
import { K, type PlanController } from "./controller";
import { sameGrip, stretchedWalls, translateElement, walledJointMove } from "./edit";
import type { P } from "./geom";
import { add, dist, lerp, mul, sub, unit } from "./geom";
import type { PipeEl, PipeShape } from "./pipe";
import { drainFallPct, formatHeight, formatPct, isRiser, pipeNodes, pipePlan, pipeWidthPx, planDist, planOf, segmentFallPct } from "./pipe";
import type { ElementStyle, RenderContext } from "./render";
import { drawCamera, drawDimension, drawElement, drawOpening, drawPipe, drawWalls, pipeColor } from "./render";
import type { SnapResult, SnapType } from "./snap";
import { polar } from "./snap";
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

function pill(rc: RenderContext, at: P, text: string, opts: { bg?: string; fg?: string; align?: "left" | "center"; scale?: number } = {}): void {
  const { ctx, palette } = rc;
  ctx.save();
  ctx.font = `11px ${palette.fontMono}`;
  const w = ctx.measureText(text).width + 12;
  const h = 18;
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
  ctx.fillStyle = opts.bg ?? palette.ink;
  ctx.globalAlpha = 0.92;
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, 4);
  ctx.fill();
  ctx.globalAlpha = 1;
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

/** Pills side by side, kept inside the canvas as one row. */
function pillRow(rc: RenderContext, at: P, parts: readonly PillPart[]): void {
  const { ctx, palette } = rc;
  ctx.save();
  ctx.font = `11px ${palette.fontMono}`;
  const widths = parts.map((p) => ctx.measureText(p.text).width + 12);
  ctx.restore();
  const total = widths.reduce((a, b) => a + b, 0) + 4 * Math.max(0, parts.length - 1);
  let x = Math.max(4, Math.min(rc.width - total - 4, at.x));
  parts.forEach((p, i) => {
    pill(rc, { x, y: at.y }, p.text, { bg: p.bg, scale: p.scale });
    x += widths[i] + 4;
  });
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

/** A temporary dimension along a-b, offset to the given side in pixels. */
function tempDimension(rc: RenderContext, a: P, b: P, offsetPx: number): void {
  if (dist(a, b) < 1) return;
  drawDimension(rc, { a, b, offset_mm: offsetPx / rc.view.scale, text_override: null }, { color: rc.palette.selection });
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

export function drawOverlay(rc: RenderContext, c: PlanController): void {
  const { ctx, palette, view, index } = rc;
  const s = useApp.getState();
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
          drawPipeHeights(rc, next, fade);
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
          ctx.lineWidth = pipeWidthPx(spec.diameterMm, view.scale) + 8;
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
      if (spec.system === "drainage") drawFallLabels(rc, run, drainFallPct(spec.diameterMm), 1);
      // Heights where the run climbs or drops.
      drawPipeHeights(rc, shape(run), 1, true);
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

  // Point heights of the one selected pipe (a node being dragged shows them on its ghost).
  if (s.selection.length === 1 && !(op.kind === "grip" && op.element.id === s.selection[0]) && op.kind !== "move") {
    const el = index.byId.get(s.selection[0]);
    if (el && el.kind === "pipe" && index.visibleIds.has(el.id)) {
      const a = c.anim.value(`${K.sel}${el.id}`, 1);
      drawPipeHeights(rc, el, a);
      if (el.system === "drainage") drawFallLabels(rc, el.points, drainFallPct(el.diameter_mm), a);
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
  const typedOpen = !!c.heightEntry || (op.kind === "pipe" && !!op.typed);
  if (s.tool === "pipe" && cursorScreen && c.pointerInside && !typedOpen && !("committing" in op && op.committing) && !c.pipeLayer().locked) {
    const parts: PillPart[] = [];
    const band = pipeDraft?.band ?? [];
    if (band.length > 1 && planDist(band[0], band[band.length - 1]) > 1) {
      const pr = polar(planOf(band[0]), planOf(band[band.length - 1]));
      parts.push({ text: `${formatLength(pr.length, unitName, true)}  ${formatAngle(pr.angle)}` });
    }
    const bump = 1 + 0.12 * c.anim.value(K.pipeHeight, 0);
    parts.push({ text: `h ${formatHeight(c.pipeNextZ(), unitName, true)}`, bg: pipeColor(palette, c.pipeSpec().system), scale: bump });
    pillRow(rc, { x: cursorScreen.x + 18, y: cursorScreen.y + 24 }, parts);
  }

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
  if (pg && pa > 0.002 && !c.placing) {
    drawElement(rc, pg.element as Element, { color: palette.selection, alpha: 0.85 * pa });
    if (pg.faceSnap && c.placementGhost) {
      const a = toScreen(view, pg.faceSnap.face.a);
      const b = toScreen(view, pg.faceSnap.face.b);
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
  }

  // The marquee keeps its last rectangle while it fades out after the release.
  if (op.kind !== "marquee") {
    const mf = c.marqueeFade;
    const ma = c.anim.value(K.marquee, 0);
    if (mf && ma > 0.002) drawMarquee(rc, mf.start, mf.current, ma);
  }

  const snapped = c.snapResult ?? c.lastSnap;
  const sa = c.anim.value(K.snapAlpha, 0);
  if (snapped && snapped.type !== "none" && sa > 0.002 && !("committing" in op && op.committing)) {
    drawSnap(rc, snapped, sa, c.anim.value(K.snapPop, 1));
  }
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

/** A small label box. Returns its rectangle so callers can keep labels apart. */
function tag(rc: RenderContext, x: number, y: number, text: string, color: string, alpha: number, filled: boolean): { x: number; y: number; w: number; h: number } {
  const { ctx, palette } = rc;
  ctx.save();
  ctx.font = `10px ${palette.fontMono}`;
  const w = ctx.measureText(text).width + 8;
  const h = 15;
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
  return { x, y, w, h };
}

type Box = { x: number; y: number; w: number; h: number };
const overlaps = (a: Box, b: Box): boolean => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

/**
 * The height of every node of a pipe, above the floor of its level, as the
 * inspector would give it: "+300", or "+300 to +1200" for a riser in run
 * order. Labels that would overlap an earlier one are left out.
 */
function drawPipeHeights(rc: RenderContext, pipe: Pick<PipeEl, "system" | "points">, alpha: number, onlyRisers = false): void {
  if (alpha <= 0.002) return;
  const color = pipeColor(rc.palette, pipe.system);
  const placed: Box[] = [];
  for (const n of pipeNodes(pipe.points)) {
    if (onlyRisers && !isRiser(n)) continue;
    const text = isRiser(n) ? `${formatHeight(n.zIn, rc.unit)} to ${formatHeight(n.zOut, rc.unit)}` : formatHeight(n.zIn, rc.unit);
    const q = toScreen(rc.view, n.point);
    rc.ctx.save();
    rc.ctx.font = `10px ${rc.palette.fontMono}`;
    const w = rc.ctx.measureText(text).width + 8;
    rc.ctx.restore();
    // Up and to the left of the node: snap labels take the right, cursor pills the lower right.
    const box: Box = { x: q.x - 9 - w, y: q.y - 22, w, h: 15 };
    if (placed.some((b) => overlaps(b, box))) continue;
    placed.push(tag(rc, box.x, box.y, text, color, alpha, false));
  }
}

/**
 * The fall of each horizontal segment of a drainage run, as a percent of its
 * plan length, at the segment middle. Below the default fall it turns to the
 * warning color, flat or uphill to the danger color. Suggestions only, the
 * review tab has the same check (drain_slope_low).
 */
function drawFallLabels(rc: RenderContext, points: readonly Vec3[], minPct: number, alpha: number): void {
  if (alpha <= 0.002) return;
  const { palette, view } = rc;
  for (let i = 0; i + 1 < points.length; i++) {
    const a = points[i];
    const b = points[i + 1];
    const pct = segmentFallPct(a, b);
    if (pct === null) continue;
    const sa = toScreen(view, a);
    const sb = toScreen(view, b);
    const L = Math.hypot(sb.x - sa.x, sb.y - sa.y);
    if (L < 48) continue;
    const text = pct <= 0.005 ? (pct < -0.005 ? `rises ${formatPct(pct)}` : "flat") : `${formatPct(pct)} fall`;
    const color = pct >= minPct - 0.01 ? palette.pipeDrain : pct > 0.005 ? palette.warn : palette.danger;
    // Beside the middle of the segment, on the right of the direction of flow.
    const nx = -(sb.y - sa.y) / L;
    const ny = (sb.x - sa.x) / L;
    rc.ctx.save();
    rc.ctx.font = `10px ${palette.fontMono}`;
    const w = rc.ctx.measureText(text).width + 8;
    rc.ctx.restore();
    const mx = (sa.x + sb.x) / 2 + nx * 14;
    const my = (sa.y + sb.y) / 2 + ny * 14;
    tag(rc, mx - w / 2, my - 7.5, text, color, alpha, true);
  }
}
