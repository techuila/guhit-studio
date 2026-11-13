// The dimension string tool (lane L1): click the witness points in order,
// then Enter, a right click or a double click to place the line; the
// pointer after that sets its offset (a typed number sets it exactly) and a
// click or Enter puts it down. Sends one `Dimension` with a the first point,
// b the last and `through` the ones between, on the level on screen. The
// plain dimension tool (M) stays in the controller.

import type { Dimension, DisplayUnit } from "../../contract/bindings";
import type { P } from "../geom";
import { dist, dot, left, sub, unit } from "../geom";
import type { PlanTool, PlanToolHost } from "../planTool";
import type { RenderContext } from "../render";
import { formatLength } from "../typed";
import { toScreen } from "../view";
import { drawDimensionString } from "./draw";
import { stringGeometry } from "./shapes";
import { ToolMotion, drawFeedback, drawTyped, exitMs, pill, typeKey, typedMm } from "./toolKit";

type Phase = "points" | "offset";

const m = new ToolMotion();
let points: P[] = [];
let phase: Phase = "points";
let cursor: P | null = null;
let typed = "";
let dropped: Dimension | null = null;
/** The offset the line starts at in the offset phase, before the pointer moves. */
const START_OFFSET_PX = 30;
let startOffset: number | null = null;
/** False right after the line is placed: it keeps `startOffset` until the pointer moves. */
let follow = false;

function unitOf(host: PlanToolHost): DisplayUnit {
  return host.index()?.doc.project.settings.display_unit ?? "mm";
}

/** Signed distance of the pointer from the a-b line, along its left normal. */
function offsetAt(a: P, b: P, p: P): number {
  const d = sub(b, a);
  const n = Math.hypot(d.x, d.y) < 1e-9 ? { x: 0, y: 1 } : left(unit(d));
  return dot(sub(p, a), n);
}

/** The dimension the draft would make, or null with fewer than two points. */
export function ghostString(host: PlanToolHost): Dimension | null {
  const pts = phase === "points" && cursor && points.length >= 1 ? [...points, cursor] : points;
  if (pts.length < 2) return null;
  const a = pts[0];
  const b = pts[pts.length - 1];
  if (dist(a, b) < 1) return null;
  let offset = startOffset ?? 0;
  if (phase === "offset") {
    const typedOffset = typedMm(typed, unitOf(host));
    if (typedOffset !== null) offset = typedOffset;
    else if (follow && cursor) offset = offsetAt(a, b, cursor);
  } else {
    offset = START_OFFSET_PX * host.pxMm();
  }
  return {
    id: "",
    level_id: host.levelId() ?? "",
    a,
    b,
    offset_mm: offset,
    text_override: null,
    through: pts.slice(1, -1),
  };
}

function drop(host: PlanToolHost): void {
  dropped = ghostString(host);
  if (dropped) m.to("drop", 0, exitMs("hover"), { from: 1, drop: true });
  points = [];
  phase = "points";
  typed = "";
  startOffset = null;
  m.ghostOut();
  m.typedOut();
}

/** Ends the points: the pointer now sets the offset. */
function place(host: PlanToolHost): void {
  if (points.length < 2) {
    m.refuse(points.length === 0 ? "Click the points to measure first" : "Click at least one more point");
    host.invalidate();
    return;
  }
  if (dist(points[0], points[points.length - 1]) < 1) {
    m.refuse("The first and last points are the same");
    host.invalidate();
    return;
  }
  phase = "offset";
  startOffset = START_OFFSET_PX * host.pxMm();
  follow = false;
  host.invalidate();
}

async function commit(host: PlanToolHost): Promise<void> {
  const d = ghostString(host);
  if (!d) return;
  const levelId = host.levelId();
  if (!levelId) return;
  m.pulse(stringGeometry(d).p2);
  points = [];
  phase = "points";
  typed = "";
  startOffset = null;
  m.ghostOut();
  m.typedOut();
  host.invalidate();
  await host.run({ type: "add_element", element: { kind: "dimension", ...d, level_id: levelId } });
}

export const dimStringTool: PlanTool = {
  tool: "dimension_string",
  enabled: true,
  rail: {
    tool: "dimension_string",
    label: "Dimension string",
    phrase: "Add a dimension string",
    icon: "dimension",
    key: null,
    group: "annotate",
    keywords: "dimension string chain running continuous measure",
  },

  hint() {
    if (phase === "offset") return "Move to set where the line goes, or type the offset, then click or press Enter. Esc drops it";
    if (points.length === 0) return "Click the points to measure, in order. E dimensions this level's outside walls automatically";
    if (points.length === 1) return "Click the next point";
    return "Click the next point, or press Enter (or right click) to place the line";
  },

  hover(host, world) {
    if (phase === "offset") follow = true;
    cursor = phase === "offset" ? world : host.snap(world, points[points.length - 1] ?? null);
    if (points.length > 0) m.ghostIn();
  },

  down(host, world, presses) {
    if (phase === "offset") {
      cursor = world;
      follow = true;
      void commit(host);
      return;
    }
    const p = host.snap(world, points[points.length - 1] ?? null);
    cursor = p;
    // The second press of a double click places the line instead of adding the same point again.
    if (presses >= 2 && points.length >= 2) {
      place(host);
      return;
    }
    const last = points[points.length - 1];
    if (last && dist(last, p) < 1) return;
    points = [...points, p];
    dropped = null;
    m.pulse(p);
    m.ghostIn();
    host.invalidate();
  },

  finish(host) {
    if (phase === "points" && points.length >= 2) place(host);
    else if (points.length > 0) drop(host);
    host.invalidate();
  },

  key(host, e) {
    if (e.key === "Escape") {
      if (typed !== "") {
        typed = "";
        m.typedOut();
      } else if (points.length > 0) drop(host);
      else return false;
      host.invalidate();
      return true;
    }
    if (points.length === 0) {
      // E: automatic exterior dimensions of the level on screen; they follow the model.
      if ((e.key === "e" || e.key === "E") && !e.altKey && host.levelId()) {
        void host.run({ type: "auto_dimension", level_id: host.levelId() ?? "", kinds: ["exterior"] });
        return true;
      }
      return false;
    }
    if (e.key === "Enter") {
      if (phase === "points") place(host);
      else void commit(host);
      return true;
    }
    if (e.key === "Backspace" && typed === "") {
      // Take the last point back; in the offset phase, back to the points.
      if (phase === "offset") phase = "points";
      else points = points.slice(0, -1);
      if (points.length === 0) m.ghostOut();
      host.invalidate();
      return true;
    }
    if (phase === "offset") {
      const next = typeKey(typed, e.key, true);
      if (next !== null) {
        typed = next;
        if (typed !== "") m.typedIn();
        else m.typedOut();
        host.invalidate();
      }
    }
    // Mid draft every other key stays here, so a tool letter does not restart the tool.
    return true;
  },

  draw(rc: RenderContext, host) {
    m.frame(host);
    const ghost = m.value("ghost", 1);
    const d = ghostString(host);
    if (d) {
      drawDimensionString(rc, d, { color: rc.palette.selection, alpha: (phase === "offset" ? 0.95 : 0.7) * ghost });
      if (phase === "points" && cursor) {
        const s = toScreen(rc.view, cursor);
        const total = formatLength(dist(d.a, d.b), rc.unit, true);
        pill(rc, { x: s.x + 18, y: s.y + 24 }, `${points.length + 1} points  ${total}`, { alpha: ghost });
      }
    }
    // The points clicked so far.
    const { ctx, palette, view } = rc;
    ctx.save();
    ctx.globalAlpha = ghost;
    ctx.fillStyle = palette.surface;
    ctx.strokeStyle = palette.selection;
    ctx.lineWidth = 1.5;
    for (const p of points) {
      const s = toScreen(view, p);
      ctx.beginPath();
      ctx.arc(s.x, s.y, 3.5, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
    }
    ctx.restore();
    const fade = m.value("drop", 0);
    if (dropped && fade > 0.002) drawDimensionString(rc, dropped, { color: rc.palette.selection, alpha: 0.7 * fade });
    else dropped = null;
    if (phase === "offset") drawTyped(rc, m, cursor, "Offset", typed, rc.unit, "Enter places it");
    drawFeedback(rc, m, cursor);
  },

  reset() {
    points = [];
    phase = "points";
    cursor = null;
    typed = "";
    dropped = null;
    startOffset = null;
    m.clear();
  },
};
