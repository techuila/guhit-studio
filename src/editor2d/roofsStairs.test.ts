// Stairs and roofs on the plan (lane L2): which engine geometry each draws
// with, the cut and the dashes above it, the DN side, the wells, roofs on
// their own level only, and picking. The fixture is what the engine answered
// for a two storey 8 x 6 m box with an L stair (turning right), a U stair
// (turning left), a hip roof over the top floor and a shed carport on the
// ground floor (`S/wave2/l2/scripts/probe.py`).
import { describe, expect, it } from "vitest";
import type { DocState, Element, RoofGeometry, StairGeometry } from "../contract/bindings";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import { clipOutStairWells, drawRoof, drawStair, drawStairFromBelow, highlightRoofOrStair } from "./drawRoofsStairs";
import type { P } from "./geom";
import { pt } from "./geom";
import { hitsElement, hitTest } from "./hit";
import { hitsRoofOrStair } from "./hitRoofsStairs";
import { buildIndex } from "./model";
import type { RenderContext } from "./render";
import { DEFAULT_PALETTE } from "./render";
import probe from "./roofsStairs.fixture.json";
import { STAIR_CUT_MM, arrowPath, moveStairGeometry, roofLoops, roofPlan, roofShape, stairGeometryFor, stairShape } from "./roofsStairsGeom";
import type { StairEl } from "./slab";

type RoofEl = Extract<Element, { kind: "roof" }>;

const [L1, L2] = probe.levels;
const stairs = probe.stairs as unknown as StairGeometry[];
const roofs = probe.roofs as unknown as RoofGeometry[];
const elements = probe.elements as unknown as Element[];
const lStair = elements[0] as StairEl;
const uStair = elements[1] as StairEl;
const hipRoof = elements[2] as RoofEl;
const carport = elements[3] as RoofEl;

function doc(): DocState {
  const d = structuredClone(fixture as unknown as DocState);
  d.project.levels = [
    { id: L1, name: "Ground Floor", elevation_mm: 0, height_mm: 3000 },
    { id: L2, name: "Second Floor", elevation_mm: 3000, height_mm: 3000 },
  ];
  d.project.elements = structuredClone(elements);
  d.derived = { ...d.derived, walls: [], rooms: [], railings: [], stairs: structuredClone(stairs), roofs: structuredClone(roofs) };
  return d;
}

/** One stroke as drawn: its subpaths in model mm (the view is 1 px per mm, y up), dash and width. */
interface Stroke {
  paths: P[][];
  dash: number[];
  width: number;
}

/** A canvas context that records strokes, fills, clips and text, and ignores the rest. */
function recorder() {
  const strokes: Stroke[] = [];
  const fills: P[][][] = [];
  const clips: P[][][] = [];
  const texts: string[] = [];
  let paths: P[][] = [];
  let dash: number[] = [];
  let width = 1;
  const stack: Array<{ dash: number[]; width: number }> = [];
  const m = (x: number, y: number): P => ({ x, y: -y });
  const state: Record<string | symbol, unknown> = {
    save: () => stack.push({ dash, width }),
    restore: () => {
      const s = stack.pop();
      if (s) ({ dash, width } = s);
    },
    beginPath: () => {
      paths = [];
    },
    moveTo: (x: number, y: number) => paths.push([m(x, y)]),
    lineTo: (x: number, y: number) => paths[paths.length - 1]?.push(m(x, y)),
    rect: (x: number, y: number, w: number, h: number) => paths.push([m(x, y), m(x + w, y), m(x + w, y + h), m(x, y + h)]),
    setLineDash: (d: number[]) => {
      dash = [...d];
    },
    stroke: () => strokes.push({ paths: paths.map((p) => [...p]), dash, width }),
    fill: () => fills.push(paths.map((p) => [...p])),
    clip: () => clips.push(paths.map((p) => [...p])),
    fillText: (t: string) => texts.push(t),
    measureText: (t: string) => ({ width: t.length * 6 }),
  };
  const ctx = new Proxy(state, {
    get: (t, k) => (k in t ? t[k] : () => {}),
    set: (t, k, v) => {
      if (k === "lineWidth") width = v as number;
      t[k] = v;
      return true;
    },
  });
  return { ctx: ctx as unknown as CanvasRenderingContext2D, strokes, fills, clips, texts };
}

function rcOn(levelId: string, d: DocState = doc()) {
  const r = recorder();
  const rc: RenderContext = {
    ctx: r.ctx,
    view: { scale: 1, ox: 0, oy: 0 },
    width: 20000,
    height: 20000,
    palette: DEFAULT_PALETTE,
    index: buildIndex(d, levelId),
    unit: "mm",
    images: new Map(),
    uiScale: 1,
  };
  return { rc, ...r };
}

const close = (a: P, b: P) => Math.hypot(a.x - b.x, a.y - b.y) < 0.01;
/** The stroke that drew the segment a to b (either way). */
function strokeOf(strokes: Stroke[], a: P, b: P): Stroke | undefined {
  return strokes.find((s) => s.paths.some((p) => p.length === 2 && ((close(p[0], a) && close(p[1], b)) || (close(p[0], b) && close(p[1], a)))));
}

describe("stair geometry lookup", () => {
  it("draws a stair in the document with the engine's geometry, matched by id", () => {
    const index = buildIndex(doc(), L1);
    const el = index.byId.get(lStair.id) as StairEl;
    expect(stairGeometryFor(el, index)?.stair_id).toBe(lStair.id);
    expect(stairGeometryFor(el, index)?.flights).toHaveLength(2);
  });

  it("moves and turns the geometry with a dragged or turned copy", () => {
    const index = buildIndex(doc(), L1);
    const el = index.byId.get(lStair.id) as StairEl;
    const g = stairGeometryFor(el, index)!;
    const moved = stairGeometryFor({ ...el, origin: pt(el.origin.x + 1000, el.origin.y - 500) }, index)!;
    expect(moved.flights[1].outline[2].x).toBeCloseTo(g.flights[1].outline[2].x + 1000);
    expect(moved.flights[1].outline[2].y).toBeCloseTo(g.flights[1].outline[2].y - 500);
    const turned = stairGeometryFor({ ...el, rotation_deg: 90 }, index)!;
    // A quarter turn about the first riser's middle: up the flight goes from +y to -x.
    expect(turned.flights[0].up.x).toBeCloseTo(-1);
    expect(turned.flights[0].up.y).toBeCloseTo(0);
    expect(turned.landings[0].z).toBe(g.landings[0].z);
  });

  it("asks the ghost for a stair the engine has not worked out, and for a resized copy", () => {
    const index = buildIndex(doc(), L1);
    const asked: StairEl[] = [];
    const ghost = (el: StairEl) => {
      asked.push(el);
      return null;
    };
    expect(stairGeometryFor({ ...lStair, id: "" }, index, ghost)).toBeNull();
    expect(stairGeometryFor({ ...(index.byId.get(lStair.id) as StairEl), width_mm: 1200 }, index, ghost)).toBeNull();
    expect(asked).toHaveLength(2);
  });

  it("follows the walking line round the turn, bends only", () => {
    const g = stairs[0];
    const path = arrowPath(g);
    // Up the first flight, across the landing, along the second flight.
    expect(path.length).toBeGreaterThanOrEqual(3);
    expect(path[0].x).toBeCloseTo(1500);
    expect(path[0].y).toBeCloseTo(800 + 125);
    const last = path[path.length - 1];
    expect(last.y).toBeCloseTo(3300);
    expect(last.x).toBeGreaterThan(2000);
  });

  it("carries a moved geometry's walking line and opening too", () => {
    const g = stairs[1];
    const m = moveStairGeometry(g, uStair, { origin: pt(uStair.origin.x, uStair.origin.y + 100), rotation_deg: uStair.rotation_deg });
    expect(m.opening[0].y).toBeCloseTo(g.opening[0].y + 100);
    expect(m.walk_line[3].z).toBe(g.walk_line[3].z);
  });
});

describe("stairs on the plan", () => {
  it("draws its own level's stair solid under the cut and dashed above it", () => {
    const { rc, strokes, texts } = rcOn(L1);
    drawStair(rc, rc.index.byId.get(lStair.id) as StairEl, null);
    const g = stairs[0];
    let under = 0;
    let over = 0;
    for (const f of g.flights) {
      f.nosings.slice(1).forEach((n) => {
        const s = strokeOf(strokes, n.a, n.b);
        expect(s, `tread at ${n.z}`).toBeDefined();
        if (n.z > STAIR_CUT_MM) {
          expect(s!.dash.length).toBeGreaterThan(0);
          over++;
        } else {
          expect(s!.dash).toEqual([]);
          under++;
        }
      });
    }
    expect(under).toBeGreaterThan(0);
    expect(over).toBeGreaterThan(0);
    // The landing is over the cut: dashed.
    const landing = strokes.find((s) => s.paths.some((p) => p.length === 4 && close(p[0], g.landings[0].polygon[0])));
    expect(landing?.dash.length).toBeGreaterThan(0);
    // One break line (six points), the arrow along the walking line, UP.
    expect(strokes.filter((s) => s.paths.some((p) => p.length === 6))).toHaveLength(1);
    expect(strokes.some((s) => s.paths.some((p) => p.length === arrowPath(g).length && close(p[0], arrowPath(g)[0])))).toBe(true);
    expect(texts).toEqual(["UP"]);
  });

  it("draws the stair of the level below dashed with DN, arrow from the top", () => {
    const { rc, strokes, texts } = rcOn(L2);
    expect(rc.index.arriving.map((s) => s.id).sort()).toEqual([lStair.id, uStair.id].sort());
    drawStairFromBelow(rc, rc.index.arriving.find((s) => s.id === uStair.id)!);
    const g = stairs[1];
    for (const f of g.flights) for (const n of f.nosings.slice(1)) expect(strokeOf(strokes, n.a, n.b)?.dash.length).toBeGreaterThan(0);
    const path = arrowPath(g);
    // Solid and reversed: it starts at the top.
    const arrow = strokes.find((s) => s.paths.some((p) => p.length === path.length && close(p[0], path[path.length - 1]) && close(p[p.length - 1], path[0])));
    expect(arrow?.dash).toEqual([]);
    expect(texts).toEqual(["DN"]);
  });

  it("keeps room fills out of the L and U holes, not their rectangles", () => {
    const { rc, clips } = rcOn(L2);
    clipOutStairWells(rc);
    const holes = clips[0].slice(1);
    expect(holes).toHaveLength(2);
    expect(holes.map((h) => h.length)).toEqual([stairs[0].opening.length, stairs[1].opening.length]);
    expect(stairs[0].opening.length).toBeGreaterThan(4);
  });

  it("draws the plain outline while the engine has no geometry", () => {
    const { rc, strokes, texts } = rcOn(L1);
    drawStair(rc, { ...lStair, id: "", level_id: "nowhere" }, null);
    expect(strokes.some((s) => s.paths.some((p) => p.length === 4))).toBe(true);
    expect(texts).toEqual(["UP"]);
  });

  it("highlights a stair's flights and landing, outlined by its hole", () => {
    const { rc, fills, strokes } = rcOn(L1);
    highlightRoofOrStair(rc, rc.index.byId.get(lStair.id)!, "selected");
    expect(fills).toHaveLength(3);
    expect(strokes[0].paths[0]).toHaveLength(stairs[0].opening.length);
  });

  it("gives the plan an L stair's hole as its shape", () => {
    const index = buildIndex(doc(), L1);
    expect(stairShape(index.byId.get(lStair.id) as StairEl, index).points).toEqual(stairs[0].opening);
  });
});

describe("roofs on the plan", () => {
  it("sorts a hip roof's ridge and hips inside, a shed's high side onto the outline", () => {
    const hip = roofPlan(roofs[0]);
    expect(hip.inner.map((e) => e.kind).sort()).toEqual(["hip", "hip", "hip", "hip", "ridge"]);
    expect(hip.outline.every((e) => e.kind === "eave")).toBe(true);
    const shed = roofPlan(roofs[1]);
    expect(shed.inner).toEqual([]);
    expect(shed.outline.map((e) => e.kind).sort()).toEqual(["eave", "rake", "rake", "ridge"]);
    expect(roofLoops(roofs[1])[0]).toHaveLength(4);
  });

  it("draws the roof over the top floor on the top floor only", () => {
    const top = rcOn(L2);
    drawRoof(top.rc, hipRoof, null);
    // The eave loop long dashed, the ridge and hips short dashed.
    expect(top.strokes.length).toBe(6);
    expect(top.strokes.every((s) => s.dash.length > 0)).toBe(true);
    const ground = rcOn(L1);
    drawRoof(ground.rc, hipRoof, null);
    expect(ground.strokes).toHaveLength(0);
  });

  it("draws a carport on its own level", () => {
    const { rc, strokes } = rcOn(L1);
    drawRoof(rc, carport, null);
    expect(strokes).toHaveLength(1);
    expect(strokes[0].paths[0]).toHaveLength(4);
    expect(roofShape(carport, buildIndex(doc(), L2))).toBeNull();
    expect(roofShape(carport, buildIndex(doc(), L1))?.points).toHaveLength(4);
  });
});

describe("picking stairs and roofs", () => {
  const opt = { tol: 60, labelHeightMm: 200, pxMm: 10 };

  it("picks an L stair in its second flight and on its landing, not in the corner it turns round", () => {
    const index = buildIndex(doc(), L1);
    const el = index.byId.get(lStair.id) as Extract<Element, { kind: "stair" }>;
    // Second flight: x 2000 to 3750, y 2800 to 3800.
    expect(hitsRoofOrStair(pt(3000, 3300), el, index, opt)).toBe(true);
    expect(hitsRoofOrStair(pt(1500, 3300), el, index, opt)).toBe(true);
    // Past the first flight's side, under the second flight: outside the L.
    expect(hitsRoofOrStair(pt(3000, 1500), el, index, opt)).toBe(false);
    expect(hitTest(pt(3000, 3300), index, opt)).toBe(lStair.id);
  });

  it("picks a roof near its ridge on its own level, never by its area", () => {
    const top = buildIndex(doc(), L2);
    const roof = top.byId.get(hipRoof.id)!;
    // The ridge runs y 3000 from x 3000 to 5000.
    expect(hitsElement(pt(4000, 3020), roof, top, opt)).toBe(true);
    // Inside the room under it, away from every line.
    expect(hitsElement(pt(4000, 1500), roof, top, opt)).toBe(false);
    // On the ground floor the roof over the top floor is not there.
    const ground = buildIndex(doc(), L1);
    expect(hitsElement(pt(4000, 3020), ground.byId.get(hipRoof.id)!, ground, opt)).toBe(false);
  });

  it("lets a room's label win over a ridge running through it", () => {
    const bungalow = fixture as unknown as DocState;
    const index = buildIndex(bungalow, null);
    const roof = bungalow.project.elements.find((e) => e.kind === "roof")!;
    const g = bungalow.derived.roofs.find((r) => r.roof_id === roof.id);
    if (!g) return; // A fixture from before Derived::roofs.
    const ridge = g.lines.find((l) => l.kind === "ridge")!;
    const room = bungalow.project.elements.find((e) => e.kind === "room")!;
    const label = index.roomGeo.get(room.id)!.label_point;
    if (Math.abs(label.y - ridge.a.y) < 1) expect(hitTest(label, index, opt)).toBe(room.id);
    // Its rake outside the walls still picks the roof.
    const rake = g.lines.find((l) => l.kind === "rake")!;
    expect(hitsElement(pt((rake.a.x + rake.b.x) / 2, (rake.a.y + rake.b.y) / 2), roof, index, opt)).toBe(true);
  });

  it("picks a carport near its eave", () => {
    const index = buildIndex(doc(), L1);
    const el = index.byId.get(carport.id)!;
    const eave = roofs[1].lines.find((l) => l.kind === "eave")!;
    expect(hitsElement(pt((eave.a.x + eave.b.x) / 2, eave.a.y + 20), el, index, opt)).toBe(true);
    expect(hitsElement(pt(10500, 2500), el, index, opt)).toBe(false);
  });
});
