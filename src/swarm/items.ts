// What the reveal paces for one draft step: each element it added or changed,
// from the draft view, and each one it removed, from main. Pure; tested in
// reveal.test.ts.

import type { Element, Point } from "../contract/bindings";
import { rectIsEmpty } from "../editor2d/geom";
import { boundsOfIds, levelOf, openingFrame, type DocIndex } from "../editor2d/model";
import type { RevealItem, SwarmStep } from "./types";

function dist(a: Point, b: Point): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

const addN = (p: Point, n: Point, k: number): Point => ({ x: p.x + n.x * k, y: p.y + n.y * k });

/**
 * What the cursor says while it draws a step's elements: the step's status
 * line, else what the part said it was doing when the step landed, else the
 * step's undo label ("MCP: add walls").
 */
export function stepLabel(step: Pick<SwarmStep, "status" | "label">, partLine: string | null = null): string {
  return step.status?.trim() || partLine?.trim() || step.label;
}

/** The reveal items of one step, in the step's order: added, then changed, then removed. */
export function stepItems(step: SwarmStep, draft: DocIndex, main: DocIndex | null, partLine: string | null = null): RevealItem[] {
  const label = stepLabel(step, partLine);
  const out: RevealItem[] = [];
  const done = new Set<string>();
  for (const id of [...step.added, ...step.modified]) {
    if (done.has(id)) continue;
    done.add(id);
    const el = draft.byId.get(id);
    if (!el) continue;
    const item = itemFor(el, draft, step.part, label, false);
    if (item) out.push(item);
  }
  for (const id of step.removed) {
    if (done.has(id)) continue;
    done.add(id);
    const el = main?.byId.get(id);
    if (!el || !main) continue;
    const item = itemFor(el, main, step.part, label, true);
    if (item) out.push({ ...item, kind: "settle", to: item.from, hosts: [], lengthMm: 0 });
  }
  return out;
}

/** How one element comes in, or null when it has nowhere to be drawn. */
export function itemFor(el: Element, index: DocIndex, part: string, label: string, removed: boolean): RevealItem | null {
  const levelId = levelOf(el, index.byId);
  const base = { id: el.id, part, label, levelId, removed };
  switch (el.kind) {
    case "wall":
      return { ...base, kind: "line", from: el.start, to: el.end, lengthMm: dist(el.start, el.end), hosts: [] };
    case "grid":
    case "view_mark":
      return { ...base, kind: "line", from: el.start, to: el.end, lengthMm: dist(el.start, el.end), hosts: [] };
    case "dimension":
      return { ...base, kind: "line", from: el.a, to: el.b, lengthMm: dist(el.a, el.b), hosts: [] };
    case "opening": {
      const host = index.byId.get(el.wall_id);
      if (!host || host.kind !== "wall") return null;
      const f = openingFrame(el, host);
      const hosts = [el.wall_id];
      const swings = el.opening_type === "door" && el.style !== "sliding";
      if (!swings) return { ...base, kind: "line", from: f.jambA, to: f.jambB, lengthMm: f.width, hosts };
      // The leaf swings from shut (its tip on the far jamb) to open (along the normal).
      const side = el.flip_side ? -1 : 1;
      const n = { x: f.normal.x * side, y: f.normal.y * side };
      const half = f.thickness / 2;
      const hinge = el.flip_hinge ? f.jambB : f.jambA;
      const other = el.flip_hinge ? f.jambA : f.jambB;
      const leaf = el.style === "swing_double" ? f.width / 2 : f.width;
      const shut = el.style === "swing_double" ? addN(f.center, n, half) : addN(other, n, half);
      const open = addN(addN(hinge, n, half), n, leaf);
      return { ...base, kind: "swing", from: shut, to: open, lengthMm: (leaf * Math.PI) / 2, hosts };
    }
    case "room": {
      const g = index.roomGeo.get(el.id);
      const at = g ? g.label_point : el.seed;
      return { ...base, kind: "fill", from: at, to: at, lengthMm: g ? g.perimeter_mm / 4 : 2000, hosts: g ? [...g.wall_ids] : [] };
    }
    default: {
      const r = boundsOfIds(index, [el.id]);
      if (rectIsEmpty(r)) return null;
      const at = { x: (r.minX + r.maxX) / 2, y: (r.minY + r.maxY) / 2 };
      return { ...base, kind: "settle", from: at, to: at, lengthMm: Math.max(r.maxX - r.minX, r.maxY - r.minY), hosts: [] };
    }
  }
}
