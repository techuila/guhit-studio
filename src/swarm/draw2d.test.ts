// The swarm draft pass on the plan (draw2d.ts), against a canvas that records
// what it was asked to draw.
import { afterEach, describe, expect, it } from "vitest";
import type { DocState } from "../contract/bindings";
import { buildIndex } from "../editor2d/model";
import { DEFAULT_PALETTE, type RenderContext } from "../editor2d/render";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import { DRAFT_ALPHA, draftBounds, draftBusy, drawDraftPass, peerColor, unionRect, watchDraft } from "./draw2d";
import { kickReveal, reveal, useSwarm } from "./swarmStore";
import { mockStatus } from "./mock";
import type { SwarmDraftState } from "./types";

const doc = fixture as unknown as DocState;
const wall = doc.project.elements.find((e) => e.kind === "wall")!;
const door = doc.project.elements.find((e) => e.kind === "opening" && e.opening_type === "door")!;

interface Stroke {
  color: string;
  alpha: number;
  dashed: boolean;
  clipped: boolean;
}

/** Records every stroke and fill with the state it was drawn in. */
function recorder(): { ctx: CanvasRenderingContext2D; strokes: Stroke[] } {
  const strokes: Stroke[] = [];
  const stack: Array<{ strokeStyle: string; fillStyle: string; globalAlpha: number; dash: number[]; clipped: boolean }> = [];
  let st = { strokeStyle: "#000", fillStyle: "#000", globalAlpha: 1, dash: [] as number[], clipped: false };
  const record = (color: string) => strokes.push({ color, alpha: st.globalAlpha, dashed: st.dash.length > 0, clipped: st.clipped });
  const ctx = {
    save: () => stack.push({ ...st, dash: [...st.dash] }),
    restore: () => {
      st = stack.pop() ?? st;
    },
    setLineDash: (d: number[]) => {
      st.dash = d;
    },
    clip: () => {
      st.clipped = true;
    },
    stroke: () => record(st.strokeStyle),
    fill: () => record(st.fillStyle),
    fillText: () => record(st.fillStyle),
    measureText: () => ({ width: 10 }),
    beginPath: () => undefined,
    moveTo: () => undefined,
    lineTo: () => undefined,
    closePath: () => undefined,
    arc: () => undefined,
    rect: () => undefined,
    setTransform: () => undefined,
    translate: () => undefined,
    rotate: () => undefined,
    fillRect: () => undefined,
    strokeRect: () => undefined,
    drawImage: () => undefined,
    quadraticCurveTo: () => undefined,
    bezierCurveTo: () => undefined,
    ellipse: () => undefined,
  } as Record<string, unknown>;
  for (const key of ["strokeStyle", "fillStyle", "globalAlpha"] as const) {
    Object.defineProperty(ctx, key, { get: () => st[key], set: (v) => ((st as Record<string, unknown>)[key] = v) });
  }
  for (const key of ["lineWidth", "lineJoin", "lineCap", "font", "textAlign", "textBaseline", "lineDashOffset"]) ctx[key] = 0;
  return { ctx: ctx as unknown as CanvasRenderingContext2D, strokes };
}

function context(ctx: CanvasRenderingContext2D): RenderContext {
  const index = buildIndex(doc, null);
  return { ctx, view: { ox: 400, oy: 300, scale: 0.05 }, width: 800, height: 600, palette: DEFAULT_PALETTE, index, unit: doc.project.settings.display_unit, images: new Map(), uiScale: 1 };
}

function draftWith(ids: string[], removed: string[] = []): SwarmDraftState {
  return { swarm_id: "sw-1", draft_revision: 1, state: doc, parts: [{ part: "shell", color: 0, ids }], removed, steps: [], display_modified: [] };
}

function open(draft: SwarmDraftState, showDraft = true): void {
  reveal.reset();
  useSwarm.setState({ status: mockStatus(), draft, showDraft, watching: true, lastSeq: 0 });
}

afterEach(() => {
  reveal.reset();
  useSwarm.setState({ status: null, draft: null, showDraft: true });
});

describe("the draft pass on the plan", () => {
  it("draws each part's elements in its color, dashed, at 70%", () => {
    open(draftWith([wall.id]));
    const { ctx, strokes } = recorder();
    const rc = context(ctx);
    drawDraftPass(rc, rc.index);
    const mine = strokes.filter((x) => x.color === peerColor(0));
    expect(mine.length).toBeGreaterThan(0);
    for (const x of mine) {
      expect(x.dashed).toBe(true);
      expect(x.alpha).toBeCloseTo(DRAFT_ALPHA, 6);
    }
  });

  it("draws nothing with the draft hidden", () => {
    open(draftWith([wall.id]), false);
    const { ctx, strokes } = recorder();
    const rc = context(ctx);
    drawDraftPass(rc, rc.index);
    expect(strokes).toHaveLength(0);
  });

  it("draws nothing before the reveal reaches an element, then a wall clipped along itself", () => {
    open(draftWith([wall.id]));
    const t0 = performance.now();
    if (wall.kind !== "wall") throw new Error("fixture");
    reveal.arrive("shell", [{ id: "other", part: "shell", kind: "line", from: { x: 0, y: 0 }, to: { x: 9000, y: 0 }, lengthMm: 9000, hosts: [], label: "", levelId: null, removed: false }], t0);
    reveal.arrive("shell", [{ id: wall.id, part: "shell", kind: "line", from: wall.start, to: wall.end, lengthMm: 4000, hosts: [], label: "", levelId: null, removed: false }], t0);
    const before = recorder();
    drawDraftPass(context(before.ctx), context(before.ctx).index, t0 + 10);
    expect(before.strokes.filter((x) => x.color === peerColor(0))).toHaveLength(0);
    // Find a moment while the wall draws in: its strokes are clipped.
    let t = t0;
    while (reveal.progress(wall.id, t) === 0 || reveal.progress(wall.id, t) >= 1) t += 5;
    const mid = recorder();
    drawDraftPass(context(mid.ctx), context(mid.ctx).index, t);
    const strokes = mid.strokes.filter((x) => x.color === peerColor(0));
    expect(strokes.length).toBeGreaterThan(0);
    expect(strokes.every((x) => x.clipped)).toBe(true);
    expect(draftBusy(t)).toBe(true);
    expect(draftBusy(t + 60_000)).toBe(false);
  });

  it("swings a door leaf and fades its arc in, never before its wall", () => {
    if (door.kind !== "opening") throw new Error("fixture");
    open(draftWith([door.id]));
    const t0 = performance.now();
    reveal.arrive("shell", [{ id: door.wall_id, part: "shell", kind: "line", from: { x: 0, y: 0 }, to: { x: 4000, y: 0 }, lengthMm: 4000, hosts: [], label: "", levelId: null, removed: false }], t0);
    reveal.arrive("rooms", [{ id: door.id, part: "rooms", kind: "swing", from: { x: 0, y: 0 }, to: { x: 0, y: 900 }, lengthMm: 1400, hosts: [door.wall_id], label: "", levelId: null, removed: false }], t0);
    let wallEnd = t0;
    while (reveal.progress(door.wall_id, wallEnd) < 1) wallEnd += 5;
    const early = recorder();
    drawDraftPass(context(early.ctx), context(early.ctx).index, wallEnd - 5);
    expect(early.strokes.filter((x) => x.color === peerColor(0))).toHaveLength(0);
    const late = recorder();
    drawDraftPass(context(late.ctx), context(late.ctx).index, wallEnd + 60_000);
    expect(late.strokes.filter((x) => x.color === peerColor(0)).length).toBeGreaterThan(0);
  });

  it("shows what the draft removes as red dashed ghosts", () => {
    open(draftWith([], [wall.id]));
    const { ctx, strokes } = recorder();
    const rc = context(ctx);
    drawDraftPass(rc, rc.index);
    const red = strokes.filter((x) => x.color === DEFAULT_PALETTE.danger);
    expect(red.length).toBeGreaterThan(0);
    expect(red.every((x) => x.dashed)).toBe(true);
  });

  it("wakes the plan on a draft change and at reveal boundaries, and not otherwise", () => {
    let woken = 0;
    const stop = watchDraft(() => woken++);
    open(draftWith([wall.id]));
    expect(woken).toBe(1);
    kickReveal();
    expect(woken).toBe(2);
    useSwarm.setState({ brief: "something else" });
    expect(woken).toBe(2);
    stop();
    kickReveal();
    expect(woken).toBe(2);
  });

  it("knows where the draft draws, so the plan can keep it in view, and says when it grows", () => {
    if (wall.kind !== "wall") throw new Error("fixture");
    const d = draftWith([wall.id]);
    const r = draftBounds(d, null);
    expect(r.minX).toBeLessThanOrEqual(Math.min(wall.start.x, wall.end.x));
    expect(r.maxX).toBeGreaterThanOrEqual(Math.max(wall.start.x, wall.end.x));
    expect(draftBounds(null, null).minX).toBe(Infinity);
    expect(unionRect(r, { minX: -50_000, minY: 0, maxX: -40_000, maxY: 10 }).minX).toBe(-50_000);
    let grew = 0;
    const stop = watchDraft(() => undefined, () => grew++);
    open(d);
    expect(grew).toBe(1);
    useSwarm.setState({ showDraft: false, draft: draftWith([wall.id]) });
    expect(grew).toBe(1);
    stop();
  });
});
