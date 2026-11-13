// The lot tool (lane L1): click the lot's corners and close on the first one
// (or press Enter), or paste its title's technical description (Cmd or
// Ctrl V while the tool is on): the legs go to the engine as
// `SetLotFromSurvey`, starting at the last clicked point or the middle of
// the plan on screen. One lot per project: a lot drawn when there is one
// already replaces its corners, keeping its id. Corners go counter-clockwise.

import type { Element, Lot, LotSide, Point } from "../../contract/bindings";
import { useApp } from "../../state/store";
import { isModalOpen } from "../../ui/Dialog";
import type { P } from "../geom";
import { dist, polygonArea } from "../geom";
import { isTypingTarget } from "../keys";
import type { PlanTool, PlanToolHost } from "../planTool";
import type { RenderContext } from "../render";
import { formatArea } from "../typed";
import { toScreen, toWorld } from "../view";
import { drawLotShape } from "./draw";
import { counterClockwise } from "./shapes";
import { parseTct } from "./tct";
import { ToolMotion, drawFeedback, exitMs, isMac, pill } from "./toolKit";

/** How close to the first corner (screen px) a click closes the outline. */
const CLOSE_PX = 10;

type LotEl = Extract<Element, { kind: "lot" }>;

const m = new ToolMotion();
let corners: P[] = [];
let cursor: P | null = null;
let closing = false;
let dropped: Lot | null = null;
/** The last point clicked while the tool was on: where a pasted description starts. */
let lastClick: P | null = null;
/** The middle of the plan on screen, from the last frame. */
let viewCenter: P | null = null;
let lastHost: PlanToolHost | null = null;

export const DEFAULT_SIDE: LotSide = { role: "side", setback_mm: 0, road: "", road_width_mm: null };

function existingLot(host: PlanToolHost): LotEl | null {
  const doc = host.index()?.doc;
  return (doc?.project.elements.find((e): e is LotEl => e.kind === "lot") as LotEl | undefined) ?? null;
}

/**
 * The element a closed outline makes: a new lot, or the project's lot with
 * these corners (its sides kept while the count matches, else side roles with
 * no setback). Corners counter-clockwise.
 */
export function lotFrom(points: readonly P[], existing: Lot | null): Lot {
  const c = counterClockwise(points);
  const sides = existing && existing.sides.length === c.length ? existing.sides : c.map(() => ({ ...DEFAULT_SIDE }));
  return existing ? { ...existing, corners: c, sides } : { id: "", corners: c, sides, grade_mm: 0 };
}

function drop(host: PlanToolHost): void {
  dropped = corners.length >= 2 ? draftLot(host, false) : null;
  if (dropped) m.to("drop", 0, exitMs("hover"), { from: 1, drop: true });
  corners = [];
  closing = false;
  m.ghostOut();
}

/** The outline drawn so far as a lot, with the pointer as its next corner. */
function draftLot(host: PlanToolHost, withCursor = true): Lot {
  const pts = withCursor && cursor && !closing ? [...corners, cursor] : corners;
  return { id: "", corners: pts, sides: pts.map(() => ({ ...DEFAULT_SIDE })), grade_mm: existingLot(host)?.grade_mm ?? 0 };
}

async function close(host: PlanToolHost): Promise<void> {
  if (corners.length < 3) {
    m.refuse("A lot needs at least three corners");
    host.invalidate();
    return;
  }
  if (Math.abs(polygonArea(corners)) < 1e4) {
    m.refuse("The corners do not enclose an area");
    host.invalidate();
    return;
  }
  const existing = existingLot(host);
  const lot = lotFrom(corners, existing);
  m.pulse(corners[0]);
  corners = [];
  closing = false;
  m.ghostOut();
  host.invalidate();
  await host.run(existing ? { type: "update_element", element: { kind: "lot", ...lot } } : { type: "add_element", element: { kind: "lot", ...lot } });
}

/** Where a pasted description starts: the last click, else the plan's middle on screen. */
function surveyStart(): Point {
  const p = lastClick ?? viewCenter ?? { x: 0, y: 0 };
  return { x: p.x, y: p.y };
}

/** Pastes a technical description: parsed here, the lot set by the engine. */
export async function pasteSurvey(host: PlanToolHost, text: string): Promise<boolean> {
  const parsed = parseTct(text);
  const app = useApp.getState();
  if (!parsed.ok) {
    m.refuse("No legs in the pasted text");
    app.toast("error", parsed.error);
    host.invalidate();
    return false;
  }
  const start = surveyStart();
  m.pulse(start);
  corners = [];
  closing = false;
  m.ghostOut();
  host.invalidate();
  const result = await app.dispatch({ type: "set_lot_from_survey", start, legs: parsed.legs });
  if (result) app.toast("success", result.diff.summary || `Lot set from ${parsed.legs.length} legs`);
  return !!result;
}

/** Cmd or Ctrl V reaches no lane tool's `key` (the plan leaves MOD keys to the shell): listen for the paste itself. */
let listening = false;
function listen(host: PlanToolHost): void {
  lastHost = host;
  if (listening || typeof window === "undefined") return;
  listening = true;
  window.addEventListener(
    "paste",
    (e: ClipboardEvent) => {
      if (useApp.getState().tool !== "lot" || !lastHost) return;
      if (isTypingTarget(e.target) || isTypingTarget(document.activeElement) || isModalOpen()) return;
      const text = e.clipboardData?.getData("text/plain") ?? "";
      if (text.trim() === "") return;
      e.preventDefault();
      void pasteSurvey(lastHost, text);
    },
    true,
  );
}

const pasteKey = (): string => (isMac() ? "Cmd V" : "Ctrl V");

export const lotTool: PlanTool = {
  tool: "lot",
  enabled: true,
  rail: {
    tool: "lot",
    label: "Lot",
    phrase: "Draw the lot",
    icon: "pin",
    key: null,
    group: "draw",
    keywords: "lot property boundary title tct technical description bearing setback",
  },

  hint(host) {
    listen(host);
    const replace = existingLot(host) ? " It replaces the lot's corners." : "";
    if (corners.length === 0) return `Click the lot's corners, or paste its technical description from the title (${pasteKey()}).${replace}`;
    if (corners.length < 3) return "Click the next corner. Esc drops the outline";
    return "Click the next corner, or click the first one (or press Enter) to close the lot";
  },

  hover(host, world) {
    listen(host);
    const p = host.snap(world, corners[corners.length - 1] ?? null);
    closing = corners.length >= 3 && dist(p, corners[0]) <= CLOSE_PX * host.pxMm();
    cursor = closing ? corners[0] : p;
    if (corners.length > 0) m.ghostIn();
  },

  down(host, world, presses) {
    listen(host);
    const p = host.snap(world, corners[corners.length - 1] ?? null);
    cursor = p;
    lastClick = p;
    if (corners.length >= 3 && (presses >= 2 || dist(p, corners[0]) <= CLOSE_PX * host.pxMm())) {
      void close(host);
      return;
    }
    const last = corners[corners.length - 1];
    if (last && dist(last, p) < 1) return;
    corners = [...corners, p];
    dropped = null;
    m.pulse(p);
    m.ghostIn();
    host.invalidate();
  },

  finish(host) {
    if (corners.length >= 3) void close(host);
    else if (corners.length > 0) drop(host);
    host.invalidate();
  },

  key(host, e) {
    if (e.key === "Escape") {
      if (corners.length === 0) return false;
      drop(host);
      host.invalidate();
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
    // Mid draft every other key stays here, so a tool letter does not restart the tool.
    return true;
  },

  draw(rc: RenderContext, host) {
    listen(host);
    m.frame(host);
    viewCenter = toWorld(rc.view, { x: rc.width / 2, y: rc.height / 2 });
    const ghost = m.value("ghost", 1);
    const { ctx, palette, view } = rc;
    if (corners.length > 0) {
      const lot = draftLot(host);
      const pts = lot.corners;
      // The closing side stays faint until the pointer reaches the first corner.
      if (pts.length >= 2) {
        ctx.save();
        ctx.globalAlpha = 0.85 * ghost;
        ctx.strokeStyle = palette.selection;
        ctx.lineWidth = 1.4;
        ctx.setLineDash([20, 4, 3, 4, 3, 4]);
        ctx.beginPath();
        pts.forEach((p, i) => {
          const s = toScreen(view, p);
          if (i === 0) ctx.moveTo(s.x, s.y);
          else ctx.lineTo(s.x, s.y);
        });
        ctx.stroke();
        if (pts.length >= 3) {
          ctx.globalAlpha = (closing ? 0.85 : 0.3) * ghost;
          ctx.setLineDash([4, 4]);
          const a = toScreen(view, pts[pts.length - 1]);
          const b = toScreen(view, pts[0]);
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
      if (cursor) {
        const s = toScreen(view, cursor);
        const area = pts.length >= 3 ? `  ${formatArea(Math.abs(polygonArea(pts)))}` : "";
        pill(rc, { x: s.x + 18, y: s.y + 26 }, `${corners.length} corners${area}${closing ? "  close" : ""}`, { alpha: ghost });
      }
    }
    const fade = m.value("drop", 0);
    if (dropped && fade > 0.002) drawLotShape(rc, dropped, { color: palette.selection, alpha: 0.8 * fade, hollow: true });
    else dropped = null;
    drawFeedback(rc, m, cursor);
  },

  reset() {
    corners = [];
    cursor = null;
    closing = false;
    dropped = null;
    lastClick = null;
    m.clear();
  },
};
