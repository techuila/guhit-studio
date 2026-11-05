// The grid line tool (lane L1): click the two ends of a grid line; a line
// more vertical than horizontal gets the next free number (1, 2, 3 run along
// x), any other the next free letter (A, B, C run along y, no I or O). A
// typed length and Enter place the second end along the pointer, as the
// wall tool does. The ghost line with its bubbles and label follows the
// pointer. Sends `AddElement` with an empty id; the engine names it.

import type { DisplayUnit, Element, GridLine } from "../../contract/bindings";
import type { P } from "../geom";
import { add, dist, mul, sub, unit } from "../geom";
import type { PlanTool, PlanToolHost } from "../planTool";
import type { RenderContext } from "../render";
import { formatLength } from "../typed";
import { toScreen } from "../view";
import { drawGridLine } from "./draw";
import { nextGridLabel } from "./labels";
import { ToolMotion, drawFeedback, drawTyped, exitMs, pill, typeKey, typedMm } from "./toolKit";

/** The engine refuses a grid line shorter than this (`grids::validate`). */
export const GRID_MIN_MM = 100;

interface Draft {
  a: P;
}

const m = new ToolMotion();
let draft: Draft | null = null;
let cursor: P | null = null;
let typed = "";
/** The last draft, fading out after Escape. */
let dropped: GridLine | null = null;

function usedLabels(host: PlanToolHost): string[] {
  const doc = host.index()?.doc;
  if (!doc) return [];
  return doc.project.elements.filter((e): e is Extract<Element, { kind: "grid" }> => e.kind === "grid").map((g) => g.label);
}

function unitOf(host: PlanToolHost): DisplayUnit {
  return host.index()?.doc.project.settings.display_unit ?? "mm";
}

/** Where the second end goes: the pointer, or a typed length along it. */
function endPoint(host: PlanToolHost): P | null {
  if (!draft || !cursor) return null;
  const len = typedMm(typed, unitOf(host));
  if (len === null || len <= 0) return cursor;
  const d = sub(cursor, draft.a);
  const dir = Math.hypot(d.x, d.y) < 1e-6 ? { x: 1, y: 0 } : unit(d);
  return add(draft.a, mul(dir, len));
}

/** The grid line the draft would make. */
export function ghostGrid(host: PlanToolHost): GridLine | null {
  const end = endPoint(host);
  if (!draft || !end) return null;
  return { id: "", label: nextGridLabel(usedLabels(host), draft.a, end), start: draft.a, end };
}

function drop(host: PlanToolHost): void {
  dropped = ghostGrid(host);
  if (dropped) m.to("drop", 0, exitMs("hover"), { from: 1, drop: true });
  draft = null;
  typed = "";
  m.ghostOut();
  m.typedOut();
}

async function commit(host: PlanToolHost, end: P): Promise<void> {
  if (!draft) return;
  if (dist(draft.a, end) < GRID_MIN_MM) {
    m.refuse(`A grid line is at least ${formatLength(GRID_MIN_MM, unitOf(host), true)} long`);
    host.invalidate();
    return;
  }
  const grid: GridLine = { id: "", label: nextGridLabel(usedLabels(host), draft.a, end), start: draft.a, end };
  m.pulse(end);
  draft = null;
  typed = "";
  m.ghostOut();
  m.typedOut();
  host.invalidate();
  await host.run({ type: "add_element", element: { kind: "grid", ...grid } });
}

export const gridTool: PlanTool = {
  tool: "grid",
  enabled: true,
  rail: {
    tool: "grid",
    label: "Grid line",
    phrase: "Draw a grid line",
    icon: "grid",
    key: null,
    group: "annotate",
    keywords: "grid line axis datum column line bubble",
  },

  hint(host) {
    if (!draft) return "Click the two ends of a grid line. Up and down lines get 1, 2, 3; across lines get A, B, C";
    const g = ghostGrid(host);
    return `Click the other end of grid line ${g?.label ?? ""}, or type a length and press Enter. Esc drops it`;
  },

  hover(host, world) {
    cursor = host.snap(world, draft?.a ?? null);
    if (draft) m.ghostIn();
  },

  down(host, world) {
    const p = host.snap(world, draft?.a ?? null);
    cursor = p;
    if (!draft) {
      draft = { a: p };
      dropped = null;
      m.pulse(p);
      m.ghostIn();
      host.invalidate();
      return;
    }
    void commit(host, endPoint(host) ?? p);
  },

  finish(host) {
    if (draft) drop(host);
    host.invalidate();
  },

  key(host, e) {
    if (e.key === "Escape") {
      if (typed !== "") {
        typed = "";
        m.typedOut();
      } else if (draft) drop(host);
      else return false;
      host.invalidate();
      return true;
    }
    if (!draft) return false;
    if (e.key === "Enter") {
      const end = endPoint(host);
      if (end) void commit(host, end);
      return true;
    }
    if (e.key === "Backspace" && typed === "") {
      drop(host);
      host.invalidate();
      return true;
    }
    const next = typeKey(typed, e.key);
    // Mid draft every other key stays here, so a tool letter does not restart the tool.
    if (next === null) return true;
    typed = next;
    if (typed !== "") m.typedIn();
    else m.typedOut();
    host.invalidate();
    return true;
  },

  draw(rc: RenderContext, host) {
    m.frame(host);
    const ghost = m.value("ghost", 1);
    const g = ghostGrid(host);
    if (g && dist(g.start, g.end) > 1) {
      drawGridLine(rc, g, { color: rc.palette.selection, alpha: 0.85 * ghost });
      if (cursor) {
        const s = toScreen(rc.view, g.end);
        if (typed === "") pill(rc, { x: s.x + 18, y: s.y + 26 }, `${g.label}  ${formatLength(dist(g.start, g.end), rc.unit, true)}`, { alpha: ghost });
      }
    } else if (draft) {
      // Only the first end so far: its point.
      const s = toScreen(rc.view, draft.a);
      rc.ctx.save();
      rc.ctx.globalAlpha = ghost;
      rc.ctx.fillStyle = rc.palette.selection;
      rc.ctx.beginPath();
      rc.ctx.arc(s.x, s.y, 3.5, 0, Math.PI * 2);
      rc.ctx.fill();
      rc.ctx.restore();
    }
    const fade = m.value("drop", 0);
    if (dropped && fade > 0.002) drawGridLine(rc, dropped, { color: rc.palette.selection, alpha: 0.85 * fade, hollow: true });
    else dropped = null;
    drawTyped(rc, m, cursor, "Length", typed, rc.unit, "Enter places it");
    drawFeedback(rc, m, cursor);
  },

  reset() {
    draft = null;
    cursor = null;
    typed = "";
    dropped = null;
    m.clear();
  },
};
