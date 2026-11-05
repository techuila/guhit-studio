// The roof tool (lane L2): a roof over a drawn outline, for a carport, a
// lanai or a porch (DECISIONS D54). Click the corners (they snap, with the
// ortho and angle locks, so corners come out square), then close on the
// first one, press Enter or double click; two corners close as the
// rectangle with that diagonal. H, G, S and F pick hip, gable, shed or flat
// for the next roof (shed until changed: the usual carport and lanai roof).
// The engine refuses an outline the shape cannot take (`roof_outline`) and
// the shell says why. The roof over the top floor stays in the project
// inspector.

import type { Element, Point, RoofShape } from "../contract/bindings";
import { dur, ease } from "../ui/motion";
import type { P } from "./geom";
import { dist, polygonArea } from "./geom";
import { ToolMotion, drawFeedback, exitMs, pill } from "./permit/toolKit";
import { formatArea } from "./typed";
import type { PlanTool, PlanToolHost } from "./planTool";
import type { RenderContext } from "./render";
import { toScreen } from "./view";

type RoofEl = Extract<Element, { kind: "roof" }>;

/** How close to the first corner (screen px) a click closes the outline. */
const CLOSE_PX = 10;
/** New roofs: 15 degrees for a shed, 25 otherwise (flat ignores it). */
export const SHED_PITCH_DEG = 15;
export const PITCH_DEG = 25;
export const ROOF_OVERHANG_MM = 300;
export const ROOF_THICKNESS_MM = 100;
export const FASCIA_MM = 150;
export const GUTTER_MM = 125;
/** The engine takes no roof under this (0.01 m2), and no side under 100 mm reads as a roof. */
const MIN_AREA_MM2 = 10_000;
const MIN_SIDE_MM = 100;

export const ROOF_SHAPE_NAME: Record<RoofShape, string> = { hip: "Hip", gable: "Gable", shed: "Shed", flat: "Flat" };
const SHAPE_KEYS: Record<string, RoofShape> = { h: "hip", g: "gable", s: "shed", f: "flat" };

const m = new ToolMotion();
let shape: RoofShape = "shed";
let corners: P[] = [];
let cursor: P | null = null;
let closing = false;
/** A dropped draft, fading out. */
let dropped: P[] | null = null;

/** The shape the next roof takes. */
export function roofToolShape(): RoofShape {
  return shape;
}

export function setRoofToolShape(next: RoofShape): void {
  shape = next;
}

/** Counter-clockwise, as the engine keeps outlines. */
function ccw(points: readonly P[]): P[] {
  return polygonArea(points) < 0 ? [...points].reverse() : [...points];
}

/** The outline a draft closes to: the rectangle on a diagonal for two corners, else the corners. Counter-clockwise. */
export function closedOutline(points: readonly P[]): P[] {
  if (points.length === 2) {
    const [a, b] = points;
    return ccw([a, { x: b.x, y: a.y }, b, { x: a.x, y: b.y }]);
  }
  return ccw(points);
}

/** Why an outline cannot close, or null when it can. */
export function outlineProblem(points: readonly P[]): string | null {
  if (points.length < 2) return "Click at least two corners";
  const pts = closedOutline(points);
  for (let i = 0; i < pts.length; i++) if (dist(pts[i], pts[(i + 1) % pts.length]) < MIN_SIDE_MM) return "A side is too short for a roof";
  if (Math.abs(polygonArea(pts)) < MIN_AREA_MM2) return "The corners do not enclose an area";
  return null;
}

/** The roof element an outline closes to, on `levelId`, in the tool's shape. */
export function roofFrom(points: readonly P[], levelId: string, roofShape: RoofShape = shape): RoofEl {
  const outline: Point[] = closedOutline(points).map((p) => ({ x: p.x, y: p.y }));
  return {
    kind: "roof",
    id: "",
    level_id: levelId,
    outline: { type: "polygon", points: outline },
    shape: roofShape,
    pitch_deg: roofShape === "shed" ? SHED_PITCH_DEG : PITCH_DEG,
    ridge_axis: "x",
    overhang_mm: ROOF_OVERHANG_MM,
    thickness_mm: ROOF_THICKNESS_MM,
    eave_mm: null,
    material_id: null,
    fascia: { size_mm: FASCIA_MM, material_id: null },
    gutter: { size_mm: GUTTER_MM, material_id: null },
  };
}

/** What the shape takes, beside the pointer when the draft cannot be it: pitched roofs need square corners. */
function shapeNote(n: number): string | null {
  if (n < 3) return null;
  if ((shape === "shed" || shape === "gable") && n !== 4) return `${ROOF_SHAPE_NAME[shape]} takes 4 corners`;
  if (shape === "hip" && n !== 4 && n !== 6 && n !== 8) return "Hip takes 4, 6 or 8 corners";
  return null;
}

function drop(host: PlanToolHost): void {
  dropped = corners.length >= 2 ? [...corners] : null;
  if (dropped) m.to("drop", 0, exitMs("hover"), { from: 1, drop: true });
  corners = [];
  closing = false;
  m.ghostOut();
  host.invalidate();
}

async function close(host: PlanToolHost): Promise<void> {
  const level = host.levelId();
  const why = level ? outlineProblem(corners) : "Open a level to draw a roof on";
  if (why || !level) {
    m.refuse(why ?? "Open a level to draw a roof on");
    host.invalidate();
    return;
  }
  const roof = roofFrom(corners, level);
  m.pulse(corners[0]);
  corners = [];
  closing = false;
  m.ghostOut();
  host.invalidate();
  // Placed, it settles like every placed element; a refusal is the shell's to say.
  await host.run({ type: "add_element", element: roof });
}

function hint(): string {
  const name = ROOF_SHAPE_NAME[shape].toLowerCase();
  if (corners.length === 0) return `Click the corners of a ${name} roof, or two opposite corners for a rectangle. H hip, G gable, S shed, F flat`;
  if (corners.length === 1) return `Click the next corner of the ${name} roof. Esc drops it`;
  if (corners.length === 2) return "Click the next corner, or double click or press Enter for the rectangle on these two";
  return "Click the next corner, or click the first one (or press Enter) to close the roof. Backspace drops a corner";
}

export const roofTool: PlanTool = {
  tool: "roof",
  enabled: true,
  rail: {
    tool: "roof",
    label: "Roof",
    phrase: "Draw a roof",
    icon: "roof",
    key: null,
    group: "draw",
    keywords: "roof hip gable shed flat lanai carport canopy porch eaves posts",
  },

  hint: () => hint(),

  hover(host, world) {
    const last = corners[corners.length - 1] ?? null;
    const p = host.snap(world, last);
    closing = corners.length >= 2 && dist(p, corners[0]) <= CLOSE_PX * host.pxMm();
    cursor = closing ? corners[0] : p;
    // The ghost (and the shape pill before the first corner) fades in once.
    m.ghostIn();
    host.invalidate();
  },

  down(host, world, presses) {
    const last = corners[corners.length - 1] ?? null;
    const p = host.snap(world, last);
    cursor = p;
    // The second press of a double click closes on the corner the first one placed.
    if (presses >= 2 && corners.length >= 2) {
      void close(host);
      return;
    }
    if (corners.length >= 2 && dist(p, corners[0]) <= CLOSE_PX * host.pxMm()) {
      void close(host);
      return;
    }
    if (last && dist(last, p) < 1) return;
    corners = [...corners, p];
    dropped = null;
    m.pulse(p);
    m.ghostIn();
    host.invalidate();
  },

  finish(host) {
    if (corners.length >= 2) void close(host);
    else if (corners.length > 0) drop(host);
    host.invalidate();
  },

  key(host, e) {
    const shapeKey = e.metaKey || e.ctrlKey || e.altKey ? undefined : SHAPE_KEYS[e.key.toLowerCase()];
    if (shapeKey) {
      shape = shapeKey;
      m.to("shape", 0, dur("press"), { from: 1, easing: ease.out, drop: true });
      host.invalidate();
      return true;
    }
    if (e.key === "Escape") {
      if (corners.length === 0) return false;
      drop(host);
      return true;
    }
    if (corners.length === 0) return false;
    if (e.key === "Enter") {
      void close(host);
      return true;
    }
    if (e.key === "Backspace") {
      corners = corners.slice(0, -1);
      if (corners.length === 0) m.ghostOut();
      host.invalidate();
      return true;
    }
    // Mid draft every other key stays here, so a tool letter does not drop the outline.
    return true;
  },

  draw(rc: RenderContext, host) {
    m.frame(host);
    const ghost = m.value("ghost", 1);
    const { ctx, palette, view } = rc;
    if (corners.length > 0) {
      const draft = cursor && !closing ? [...corners, cursor] : corners;
      // Two corners and the pointer: the rectangle that diagonal makes, faint, as Enter would close it.
      if (corners.length === 1 && cursor && !closing) {
        const rect = closedOutline([corners[0], cursor]);
        ctx.save();
        ctx.globalAlpha = 0.35 * ghost;
        ctx.strokeStyle = palette.selection;
        ctx.lineWidth = 1;
        ctx.setLineDash([4, 4]);
        ctx.beginPath();
        rect.forEach((p, i) => {
          const s = toScreen(view, p);
          if (i === 0) ctx.moveTo(s.x, s.y);
          else ctx.lineTo(s.x, s.y);
        });
        ctx.closePath();
        ctx.stroke();
        ctx.restore();
      }
      if (draft.length >= 2) {
        ctx.save();
        ctx.globalAlpha = 0.85 * ghost;
        ctx.strokeStyle = palette.selection;
        ctx.lineWidth = 1.4;
        ctx.setLineDash([9, 4]);
        ctx.beginPath();
        draft.forEach((p, i) => {
          const s = toScreen(view, p);
          if (i === 0) ctx.moveTo(s.x, s.y);
          else ctx.lineTo(s.x, s.y);
        });
        ctx.stroke();
        // The closing side stays faint until the pointer reaches the first corner.
        if (draft.length >= 3) {
          ctx.globalAlpha = (closing ? 0.85 : 0.3) * ghost;
          ctx.setLineDash([4, 4]);
          const a = toScreen(view, draft[draft.length - 1]);
          const b = toScreen(view, draft[0]);
          ctx.beginPath();
          ctx.moveTo(a.x, a.y);
          ctx.lineTo(b.x, b.y);
          ctx.stroke();
        }
        ctx.restore();
      }
      ctx.save();
      ctx.globalAlpha = ghost;
      ctx.fillStyle = palette.surface;
      ctx.strokeStyle = palette.selection;
      ctx.lineWidth = 1.5;
      corners.forEach((p, i) => {
        const s = toScreen(view, p);
        ctx.beginPath();
        ctx.arc(s.x, s.y, i === 0 && closing ? 6 : 3.5, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      });
      ctx.restore();
    }
    if (cursor) {
      const s = toScreen(view, cursor);
      const n = corners.length + (closing || corners.length === 0 ? 0 : 1);
      const note = shapeNote(n);
      const area = corners.length >= 1 ? Math.abs(polygonArea(closedOutline(corners.length === 1 ? [corners[0], cursor] : closing ? corners : [...corners, cursor]))) : 0;
      const parts = [`${ROOF_SHAPE_NAME[shape]} roof`];
      if (area > 0) parts.push(formatArea(area));
      if (closing) parts.push("close");
      // The shape name bumps when H, G, S or F changes it.
      const bump = 1 + 0.12 * m.value("shape", 0);
      pill(rc, { x: s.x + 18, y: s.y + 26 }, parts.join("  "), { alpha: ghost, scale: bump });
      if (note) pill(rc, { x: s.x + 18, y: s.y + 48 }, note, { bg: palette.warn, alpha: ghost });
    }
    const fade = m.value("drop", 0);
    if (dropped && fade > 0.002) {
      ctx.save();
      ctx.globalAlpha = 0.8 * fade;
      ctx.strokeStyle = palette.selection;
      ctx.lineWidth = 1.4;
      ctx.setLineDash([9, 4]);
      ctx.beginPath();
      dropped.forEach((p, i) => {
        const q = toScreen(view, p);
        if (i === 0) ctx.moveTo(q.x, q.y);
        else ctx.lineTo(q.x, q.y);
      });
      ctx.stroke();
      ctx.restore();
    } else dropped = null;
    drawFeedback(rc, m, cursor);
  },

  reset() {
    corners = [];
    cursor = null;
    closing = false;
    dropped = null;
    m.clear();
  },
};

/** The draft's corners (tests). */
export function roofDraft(): readonly P[] {
  return corners;
}
