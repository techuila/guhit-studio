// The section and elevation mark tool (lane L1): click the start and the
// end of the cut line (or the picture plane), then the side it looks to.
// Tab switches between a section and an elevation at any step of a draft, or
// with the pointer over the plan before one (elsewhere it moves focus). Sections take
// the next free letter (A, B, C); elevations the side of the building they
// look toward ("North", from the project's north), numbered when taken.

import type { Element, MarkKind, ViewMark } from "../../contract/bindings";
import type { P } from "../geom";
import { dist, dot, left, sub, unit } from "../geom";
import type { PlanTool, PlanToolHost } from "../planTool";
import type { RenderContext } from "../render";
import { dur, ease } from "../../ui/motion";
import { toScreen } from "../view";
import { drawViewMark } from "./draw";
import { elevationName, lookDir, nextLetter, numbered } from "./labels";
import { ToolMotion, drawFeedback, exitMs, pill } from "./toolKit";

/** Shorter than this the mark has no direction to speak of. */
export const MARK_MIN_MM = 100;

type Phase = "start" | "end" | "side";

const m = new ToolMotion();
let kind: MarkKind = "section";
/** The kind before the last Tab, cross fading out. */
let wasKind: MarkKind | null = null;
let phase: Phase = "start";
let start: P | null = null;
let end: P | null = null;
let cursor: P | null = null;
let dropped: ViewMark | null = null;

type MarkEl = Extract<Element, { kind: "view_mark" }>;

function marks(host: PlanToolHost): MarkEl[] {
  const doc = host.index()?.doc;
  return doc ? doc.project.elements.filter((e): e is MarkEl => e.kind === "view_mark") : [];
}

/** True when `p` lies right of start to end: the mark looks right (flip). */
export function flipFor(a: P, b: P, p: P): boolean {
  const d = sub(b, a);
  if (Math.hypot(d.x, d.y) < 1e-9) return false;
  return dot(sub(p, a), left(unit(d))) < 0;
}

/** The label a new mark of this kind gets. */
export function markLabel(all: readonly Pick<ViewMark, "mark_kind" | "label">[], k: MarkKind, a: P, b: P, flip: boolean, northDeg: number): string {
  const same = all.filter((x) => x.mark_kind === k).map((x) => x.label);
  if (k === "section") return nextLetter(same);
  return numbered(elevationName(lookDir(a, b, flip), northDeg), same);
}

/** The mark the draft would make, as of this pointer. */
export function ghostMark(host: PlanToolHost, as: MarkKind = kind): ViewMark | null {
  if (!start) return null;
  const b = phase === "end" ? cursor : end;
  if (!b || dist(start, b) < 1) return null;
  const flip = phase === "side" && cursor ? flipFor(start, b, cursor) : false;
  const north = host.index()?.doc.project.settings.north_angle_deg ?? 0;
  return {
    id: "",
    level_id: host.levelId() ?? "",
    mark_kind: as,
    label: markLabel(marks(host), as, start, b, flip, north),
    start,
    end: b,
    flip,
    depth_mm: null,
  };
}

function clearDraft(): void {
  phase = "start";
  start = null;
  end = null;
  m.ghostOut();
}

function drop(host: PlanToolHost): void {
  dropped = ghostMark(host);
  if (dropped) m.to("drop", 0, exitMs("hover"), { from: 1, drop: true });
  clearDraft();
}

async function commit(host: PlanToolHost): Promise<void> {
  const mark = ghostMark(host);
  const levelId = host.levelId();
  if (!mark || !levelId) return;
  m.pulse(cursor ?? mark.end);
  clearDraft();
  host.invalidate();
  await host.run({ type: "add_element", element: { kind: "view_mark", ...mark, level_id: levelId } });
}

/** Tab: section to elevation and back, the symbol cross fading. */
function switchKind(host: PlanToolHost): void {
  wasKind = kind;
  kind = kind === "section" ? "elevation" : "section";
  m.anim.clear("kind");
  m.to("kind", 1, dur("base"), { from: 0, easing: ease.out });
  host.invalidate();
}

const KIND_WORD: Record<MarkKind, string> = { section: "section", elevation: "elevation" };

export const viewMarkTool: PlanTool = {
  tool: "view_mark",
  enabled: true,
  rail: {
    tool: "view_mark",
    label: "Section or elevation",
    phrase: "Mark a section or an elevation",
    icon: "eye",
    key: null,
    group: "annotate",
    keywords: "section elevation cut mark view front rear side",
  },

  hint() {
    const other = kind === "section" ? "an elevation" : "a section";
    if (phase === "start") return `Click where the ${kind === "section" ? "cut" : "elevation"} line starts. Tab for ${other}`;
    if (phase === "end") return `Click where the ${KIND_WORD[kind]} line ends. Tab for ${other}`;
    return `Click the side the ${KIND_WORD[kind]} looks at. Tab for ${other}, Esc drops it`;
  },

  hover(host, world) {
    if (phase === "side") cursor = world;
    else cursor = host.snap(world, phase === "end" ? start : null);
    if (start) m.ghostIn();
  },

  down(host, world) {
    if (phase === "side") {
      cursor = world;
      void commit(host);
      return;
    }
    const p = host.snap(world, phase === "end" ? start : null);
    cursor = p;
    if (phase === "start") {
      start = p;
      phase = "end";
      dropped = null;
      m.pulse(p);
      m.ghostIn();
    } else if (start) {
      if (dist(start, p) < MARK_MIN_MM) {
        m.refuse("Make the line a little longer");
      } else {
        end = p;
        phase = "side";
        m.pulse(p);
      }
    }
    host.invalidate();
  },

  finish(host) {
    if (start) drop(host);
    host.invalidate();
  },

  key(host, e) {
    if (e.key === "Tab") {
      // Without a draft Tab is the tool's only over the plan; elsewhere it moves focus.
      if (!start && !host.pointerInside()) return false;
      switchKind(host);
      return true;
    }
    if (e.key === "Escape") {
      if (!start) return false;
      drop(host);
      host.invalidate();
      return true;
    }
    if (!start) return false;
    if (e.key === "Backspace") {
      if (phase === "side") {
        phase = "end";
        end = null;
      } else clearDraft();
      host.invalidate();
      return true;
    }
    if (e.key === "Enter" && phase === "side") {
      void commit(host);
      return true;
    }
    // Mid draft every other key stays here, so a tool letter does not restart the tool.
    return true;
  },

  draw(rc: RenderContext, host) {
    m.frame(host);
    const ghost = m.value("ghost", 1);
    const swap = m.value("kind", 1);
    const style = (a: number) => ({ color: rc.palette.selection, alpha: 0.85 * ghost * a });
    const g = ghostMark(host);
    if (g) {
      // Tab cross fades the old symbol out and the new one in.
      if (wasKind && swap < 1) {
        const old = ghostMark(host, wasKind);
        if (old) drawViewMark(rc, old, style(1 - swap));
      }
      drawViewMark(rc, g, style(swap));
      if (cursor) {
        const s = toScreen(rc.view, cursor);
        pill(rc, { x: s.x + 18, y: s.y + 26 }, `${g.mark_kind === "section" ? "Section" : "Elevation"} ${g.label}`, { alpha: ghost });
      }
    } else if (start) {
      const s = toScreen(rc.view, start);
      rc.ctx.save();
      rc.ctx.globalAlpha = ghost;
      rc.ctx.fillStyle = rc.palette.selection;
      rc.ctx.beginPath();
      rc.ctx.arc(s.x, s.y, 3.5, 0, Math.PI * 2);
      rc.ctx.fill();
      rc.ctx.restore();
    } else if (cursor) {
      // Before the first click: which kind is on. Tab swaps it with a small pop.
      const s = toScreen(rc.view, cursor);
      pill(rc, { x: s.x + 18, y: s.y + 26 }, kind === "section" ? "Section" : "Elevation", { alpha: 0.5 + 0.5 * swap, scale: 0.96 + 0.04 * swap });
    }
    const fade = m.value("drop", 0);
    if (dropped && fade > 0.002) drawViewMark(rc, dropped, { color: rc.palette.selection, alpha: 0.85 * fade, hollow: true });
    else dropped = null;
    drawFeedback(rc, m, cursor);
  },

  reset() {
    clearDraft();
    cursor = null;
    dropped = null;
    wasKind = null;
    m.clear();
  },
};
