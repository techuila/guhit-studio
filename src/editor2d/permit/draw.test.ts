import { describe, expect, it } from "vitest";
import type { DocState, Element } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";
import { pt } from "../geom";
import { buildIndex } from "../model";
import type { RenderContext } from "../render";
import { DEFAULT_PALETTE, dimensionTextBox, drawModel } from "../render";
import { boxesOverlap } from "../tags";
import { drawGridLine, drawLotShape, lotLengthText, openingMarks, sheetOfMark, shortMark, stringTexts } from "./draw";

const base = fixture as unknown as DocState;
const level = base.project.levels[0].id;
const DOOR = "00000000-0000-4000-8000-000000000201";
const WINDOW = "00000000-0000-4000-8000-000000000203";

/** A canvas context that records the text it is asked to draw and ignores the rest. */
function fakeCtx() {
  const texts: string[] = [];
  const at: Array<{ t: string; x: number; y: number }> = [];
  const state: Record<string | symbol, unknown> = {
    measureText: (t: string) => ({ width: t.length * 6.5 }),
    fillText: (t: string, x: number, y: number) => {
      texts.push(t);
      at.push({ t, x, y });
    },
  };
  const ctx = new Proxy(state, {
    get: (t, k) => (k in t ? t[k] : () => {}),
    set: (t, k, v) => {
      t[k] = v;
      return true;
    },
  });
  return { ctx: ctx as unknown as CanvasRenderingContext2D, texts, at };
}

function rcFor(doc: DocState) {
  const { ctx, texts, at } = fakeCtx();
  const rc: RenderContext = {
    ctx,
    view: { scale: 0.08, ox: 300, oy: 700 },
    width: 1200,
    height: 900,
    palette: DEFAULT_PALETTE,
    index: buildIndex(doc, level),
    unit: "mm",
    images: new Map(),
    uiScale: 1,
  };
  return { rc, texts, at };
}

function withPermit(): DocState {
  const d = structuredClone(base);
  d.project.elements.push(
    { kind: "grid", id: "g1", label: "B", start: pt(-1000, 3000), end: pt(9000, 3000) },
    { kind: "view_mark", id: "m1", level_id: level, mark_kind: "section", label: "A", start: pt(2500, -2000), end: pt(2500, 8000), flip: false, depth_mm: null },
    { kind: "view_mark", id: "m2", level_id: level, mark_kind: "elevation", label: "North", start: pt(0, 9000), end: pt(8000, 9000), flip: true, depth_mm: null },
    {
      kind: "lot",
      id: "l1",
      corners: [pt(-3000, -3000), pt(13000, -3000), pt(13000, 12000), pt(-3000, 12000)],
      sides: [0, 1, 2, 3].map(() => ({ role: "side", setback_mm: 0, road: "", road_width_mm: null })),
      grade_mm: 0,
    },
    { kind: "dimension", id: "d9", level_id: level, a: pt(0, 6000), b: pt(8000, 6000), offset_mm: 900, text_override: null, through: [pt(5000, 6000)], auto: { type: "exterior", side: "north", tier: "walls" } },
  );
  d.project.opening_types = [
    { id: "t1", mark: "D1", opening_type: "door", style: "swing_single", width_mm: 900, height_mm: 2100, description: "", remarks: "" },
    { id: "t2", mark: "W1", opening_type: "window", style: "sliding", width_mm: 1500, height_mm: 1200, sill_mm: 900, description: "", remarks: "" },
  ];
  for (const e of d.project.elements) {
    if (e.id === DOOR && e.kind === "opening") e.type_id = "t1";
    if (e.id === WINDOW && e.kind === "opening") e.type_id = "t2";
  }
  d.derived.opening_schedule = {
    rows: [],
    marks: [
      { opening_id: DOOR, mark: "D1" },
      { opening_id: WINDOW, mark: "W1" },
    ],
  };
  return d;
}

describe("permit drawing", () => {
  it("draws every permit element in the model pass", () => {
    const { rc, texts } = rcFor(withPermit());
    drawModel(rc, { styleOf: () => null, hidden: new Set(), activeCameraId: null, showCameras: false });
    // Grid bubbles at both ends, section heads at both ends.
    expect(texts.filter((t) => t === "B")).toHaveLength(2);
    expect(texts.filter((t) => t === "A")).toHaveLength(2); // the two section heads
    // The elevation target shows N, its full name beside it.
    expect(texts).toEqual(expect.arrayContaining(["N", "North"]));
    // Lot: corners 1 to 4, bearings and lengths.
    expect(texts).toEqual(expect.arrayContaining(["1", "2", "3", "4", "16.00 m", "15.00 m", "N 90°00' E", "N 0°00' E"]));
    // The string's two segments, and the plain dimension of the fixture.
    expect(texts).toEqual(expect.arrayContaining(["5000", "3000", "8000"]));
    // Type tags.
    expect(texts).toEqual(expect.arrayContaining(["D1", "W1"]));
  });

  it("takes the engine's bearings and lengths for the lot when it has them", () => {
    const d = withPermit();
    d.derived.site = {
      lot_id: "l1",
      area_m2: 240,
      sides: [0, 1, 2, 3].map((i) => ({ length_mm: 16000 + i, bearing: `S ${i}°00' E`, clearance_mm: null })),
      setback_outline: [],
      coverage_pct: 20,
    };
    const { rc, texts } = rcFor(d);
    const lot = d.project.elements.find((e) => e.id === "l1");
    if (!lot || lot.kind !== "lot") throw new Error("lot");
    drawLotShape(rc, lot, null);
    expect(texts).toEqual(expect.arrayContaining(["S 0°00' E", "S 3°00' E", "16.00 m"]));
  });

  it("shows nothing for a grid line's ghost it is not given", () => {
    const { rc, texts } = rcFor(base);
    drawGridLine(rc, { id: "", label: "7", start: pt(0, 0), end: pt(0, 5000) }, { color: "#0e8a8f", alpha: 0.5 });
    expect(texts).toEqual(["7", "7"]);
  });

  it("stacks short segments' lengths so they do not overlap", () => {
    const { rc } = rcFor(base);
    const t = stringTexts(rc, { a: pt(0, 0), b: pt(600, 0), through: [pt(200, 0), pt(400, 0)], offset_mm: 500, text_override: null });
    expect(t.map((x) => x.text)).toEqual(["200", "200", "200"]);
    for (let i = 0; i + 1 < t.length; i++) expect(boxesOverlap(t[i].box, t[i + 1].box)).toBe(false);
  });

  it("reads marks from the schedule only", () => {
    const d = withPermit();
    expect(openingMarks(d).get(DOOR)).toBe("D1");
    d.derived.opening_schedule = { rows: [], marks: [{ opening_id: WINDOW, mark: "W7" }] };
    expect([...openingMarks(d).entries()]).toEqual([[WINDOW, "W7"]]);
    d.derived.opening_schedule = { rows: [], marks: [] };
    expect(openingMarks(d).size).toBe(0);
  });

  it("names marks and their sheets in short", () => {
    expect(shortMark("North")).toBe("N");
    expect(shortMark("North 2")).toBe("N2");
    expect(shortMark("B")).toBe("B");
    expect(lotLengthText(12345)).toBe("12.35 m");
    const d = withPermit();
    expect(sheetOfMark(d, "m1")).toBeNull();
    d.project.sheet_set.sheets = [
      { id: "s1", number: "A-5", title: "Sections", paper: "a3", orientation: "landscape", viewports: [{ id: "v1", view: { type: "section", mark_id: "m1" }, scale_denominator: null, position: null, title: null }] },
    ] as never;
    expect(sheetOfMark(d, "m1")).toBe("A-5");
  });

  it("keeps type tags off dimension text", () => {
    const d = withPermit();
    const first = rcFor(d);
    drawModel(first.rc, { styleOf: () => null, hidden: new Set(), activeCameraId: null, showCameras: false });
    const tag = first.at.find((x) => x.t === "D1");
    if (!tag) throw new Error("no tag");
    // A short dimension whose text lands right where the tag went.
    const view = first.rc.view;
    const wx = (tag.x - view.ox) / view.scale;
    const wy = (view.oy - (tag.y + 9)) / view.scale;
    const dim = { kind: "dimension", id: "dx", level_id: level, a: pt(wx - 400, wy), b: pt(wx + 400, wy), offset_mm: 0, text_override: null } as Element;
    d.project.elements.push(dim);
    const again = rcFor(d);
    drawModel(again.rc, { styleOf: () => null, hidden: new Set(), activeCameraId: null, showCameras: false });
    const moved = again.at.find((x) => x.t === "D1");
    if (!moved || dim.kind !== "dimension") throw new Error("no tag");
    expect(moved.x !== tag.x || moved.y !== tag.y).toBe(true);
    const box = dimensionTextBox(again.rc, dim);
    if (!box) throw new Error("no box");
    expect(boxesOverlap(box, { x: moved.x - 8, y: moved.y - 8, w: 16, h: 16 })).toBe(false);
  });
});
