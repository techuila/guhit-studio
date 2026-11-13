// The swarm draft on the plan (DECISIONS D56, D57). The plan draws the model
// as always, then this pass draws the draft over it, so the model never
// hides: each part's added and changed elements in the part's color, dashed
// and hollow at 70%, and what the draft removes as red dashed ghosts.
//
// It follows the reveal: walls draw from start to end (clipped along the
// wall), a door leaf swings open (`ElementStyle.reveal`), a room's fill
// fades in, anything else fades in where it stands. The plan's one loop
// stays awake only while something is drawing in (`draftBusy`); between
// elements the reveal's ticker wakes it (`watchDraft`). No frame loop here.

import type { Element, Point } from "../contract/bindings";
import { emptyRect, expandRect, rectIsEmpty, type Rect } from "../editor2d/geom";
import { boundsOfIds, buildIndex, type DocIndex } from "../editor2d/model";
import { drawElement, drawOpening, type ElementStyle, type RenderContext } from "../editor2d/render";
import { toScreen } from "../editor2d/view";
import { peerIndex } from "../live/format";
import { onRevealTick, reveal, useSwarm } from "./swarmStore";
import type { SwarmDraftState } from "./types";

/** The draft's strength on the plan. */
export const DRAFT_ALPHA = 0.7;

/** Draw order, like the model's: fills under walls, labels and notes on top. */
const RANK: Partial<Record<Element["kind"], number>> = {
  underlay: 0,
  linework: 0,
  room: 1,
  slab: 2,
  stair: 3,
  asset: 4,
  wall: 5,
  opening: 6,
  column: 7,
  pipe: 8,
  dimension: 9,
  annotation: 9,
  roof: 10,
  grid: 10,
  lot: 10,
  view_mark: 10,
};

let levelCache: { draft: SwarmDraftState; levelId: string | null; index: DocIndex } | null = null;

/** The draft view as the plan indexes a level, built once per draft and level. */
export function draftLevelIndex(draft: SwarmDraftState, levelId: string | null): DocIndex {
  if (levelCache?.draft !== draft || levelCache.levelId !== levelId) levelCache = { draft, levelId, index: buildIndex(draft.state, levelId) };
  return levelCache.index;
}

/** True while an element is drawing in: the plan keeps its loop awake. */
export function draftBusy(now: number = performance.now()): boolean {
  const st = useSwarm.getState();
  return st.draft !== null && st.showDraft && reveal.busy(now);
}

/** Where the draft draws on a level, mm: empty without a draft. */
export function draftBounds(draft: SwarmDraftState | null, levelId: string | null): Rect {
  if (!draft) return emptyRect();
  const index = draftLevelIndex(draft, levelId);
  const ids = draft.parts.flatMap((p) => p.ids).filter((id) => index.visibleIds.has(id));
  const r = boundsOfIds(index, ids);
  return rectIsEmpty(r) ? emptyRect() : r;
}

/** Grows `a` to hold `b`. */
export function unionRect(a: Rect, b: Rect): Rect {
  const r = emptyRect();
  for (const x of [a, b]) {
    if (rectIsEmpty(x)) continue;
    expandRect(r, { x: x.minX, y: x.minY });
    expandRect(r, { x: x.maxX, y: x.maxY });
  }
  return r;
}

/**
 * Redraws the plan when the draft, its toggle or the reveal changes. The
 * ticker fires once per segment boundary, so between elements the plan
 * sleeps. `grew` hears each new draft, so the plan can keep it in view.
 * Returns the unsubscribe.
 */
export function watchDraft(invalidate: () => void, grew?: (draft: SwarmDraftState) => void): () => void {
  const offStore = useSwarm.subscribe((s, p) => {
    if (s.draft !== p.draft || s.showDraft !== p.showDraft || s.status !== p.status) invalidate();
    if (s.draft && s.draft !== p.draft && s.showDraft) grew?.(s.draft);
  });
  const offTick = onRevealTick(() => {
    if (useSwarm.getState().draft) invalidate();
  });
  return () => {
    offStore();
    offTick();
  };
}

/** Resolves a part's `--peer-N` for the canvas, once per color. */
const colors = new Map<number, string>();
const FALLBACK = ["#c92a2a", "#1864ab", "#2b8a3e", "#c2410c", "#c2255c", "#7a5c00", "#3b5bdb", "#495057"];
export function peerColor(color: number): string {
  const i = peerIndex(color);
  let c = colors.get(i);
  if (c) return c;
  c = FALLBACK[i];
  if (typeof document !== "undefined") c = getComputedStyle(document.documentElement).getPropertyValue(`--peer-${i}`).trim() || c;
  colors.set(i, c);
  return c;
}

/** The draft pass: over the model, under selections and tool overlays. `main` is the model's index for this level. */
export function drawDraftPass(rc: RenderContext, main: DocIndex | null, now: number = performance.now()): void {
  const st = useSwarm.getState();
  const draft = st.draft;
  if (!draft || !st.showDraft) return;
  const index = draftLevelIndex(draft, main?.levelId ?? null);
  const local: RenderContext = { ...rc, index };
  const list: Array<{ el: Element; color: string; p: number }> = [];
  for (const part of draft.parts) {
    const color = peerColor(part.color);
    for (const id of part.ids) {
      if (!index.visibleIds.has(id)) continue;
      const el = index.byId.get(id);
      if (!el) continue;
      const p = reveal.progress(id, now);
      if (p > 0) list.push({ el, color, p });
    }
  }
  list.sort((a, b) => (RANK[a.el.kind] ?? 5) - (RANK[b.el.kind] ?? 5));
  for (const { el, color, p } of list) drawOne(local, el, color, p, main);

  // What the draft removes: red dashed ghosts of the model's elements.
  if (main && draft.removed.length > 0) {
    const danger = rc.palette.danger;
    for (const id of draft.removed) {
      if (!main.visibleIds.has(id)) continue;
      const el = main.byId.get(id);
      const p = reveal.progress(id, now);
      if (el && p > 0) drawElement(rc, el, { color: danger, alpha: DRAFT_ALPHA * p, dashed: true, hollow: true }, main);
    }
  }
}

function drawOne(rc: RenderContext, el: Element, color: string, p: number, main: DocIndex | null): void {
  const style: ElementStyle = { color, alpha: DRAFT_ALPHA, dashed: true, hollow: true };
  const line = lineOf(el);
  if (line && p < 1) {
    // Drawn from start to end: clipped along its own direction.
    const { ctx } = rc;
    ctx.save();
    clipAlong(rc, line[0], line[1], p);
    drawElement(rc, el, style, rc.index);
    ctx.restore();
    return;
  }
  if (el.kind === "opening") {
    const host = rc.index.byId.get(el.wall_id);
    if (!host || host.kind !== "wall") return;
    // A door leaf swings open with the reveal; a window fades in.
    const swings = el.opening_type === "door" && el.style !== "sliding";
    const st: ElementStyle = swings ? { ...style, reveal: p } : { ...style, alpha: DRAFT_ALPHA * p };
    // In a wall of the model, the opening cuts its poche so it reads where it goes.
    drawOpening(rc, el, host, st, !!main?.byId.has(el.wall_id));
    return;
  }
  drawElement(rc, el, p < 1 ? { ...style, alpha: DRAFT_ALPHA * p } : style, rc.index);
}

/** Elements that draw in from one end to the other. */
function lineOf(el: Element): [Point, Point] | null {
  switch (el.kind) {
    case "wall":
    case "grid":
    case "view_mark":
      return [el.start, el.end];
    case "dimension":
      return [el.a, el.b];
    default:
      return null;
  }
}

/** Clips to the part of the plane before `p` of the way from `a` to `b`, with room around the line. */
function clipAlong(rc: RenderContext, a: Point, b: Point, p: number): void {
  const s = toScreen(rc.view, a);
  const e = toScreen(rc.view, b);
  const len = Math.hypot(e.x - s.x, e.y - s.y);
  const { ctx } = rc;
  if (len < 1e-6) return;
  const ux = (e.x - s.x) / len;
  const uy = (e.y - s.y) / len;
  const nx = -uy;
  const ny = ux;
  const back = 4000;
  const side = 4000;
  const reach = len * Math.max(0, Math.min(1, p));
  const pts = [
    [s.x - ux * back + nx * side, s.y - uy * back + ny * side],
    [s.x + ux * reach + nx * side, s.y + uy * reach + ny * side],
    [s.x + ux * reach - nx * side, s.y + uy * reach - ny * side],
    [s.x - ux * back - nx * side, s.y - uy * back - ny * side],
  ];
  ctx.beginPath();
  ctx.moveTo(pts[0][0], pts[0][1]);
  for (let i = 1; i < pts.length; i++) ctx.lineTo(pts[i][0], pts[i][1]);
  ctx.closePath();
  ctx.clip();
}
