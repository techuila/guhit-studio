import { describe, expect, it } from "vitest";
import type { DocState, Element } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";
import { translateElement } from "../edit";
import { pt, rectFromPoints } from "../geom";
import { hitTest, marqueeSelect } from "../hit";
import { buildIndex, elementShape, keyPoints } from "../model";
import { counterClockwise, cornerOut, gridOutline, isDimensionString, lotSides, markGeometry, stringGeometry, windingOf } from "./shapes";

const base = fixture as unknown as DocState;
const level = base.project.levels[0].id;
// 200 mm label height: one screen pixel is 16.7 mm, a bubble about 183 mm.
const opt = { tol: 60, labelHeightMm: 200 };

const GRID = { kind: "grid", id: "g1", label: "A", start: pt(-2000, 12000), end: pt(10000, 12000) };
const MARK = { kind: "view_mark", id: "m1", level_id: level, mark_kind: "section", label: "A", start: pt(-3000, 2000), end: pt(-3000, 5000), flip: false, depth_mm: null };
const ELEV = { kind: "view_mark", id: "m2", level_id: level, mark_kind: "elevation", label: "North", start: pt(0, 9000), end: pt(8000, 9000), flip: true, depth_mm: null };
const LOT = {
  kind: "lot",
  id: "l1",
  corners: [pt(-5000, -5000), pt(15000, -5000), pt(15000, 15000), pt(-5000, 15000)],
  sides: [0, 1, 2, 3].map(() => ({ role: "side", setback_mm: 0, road: "", road_width_mm: null })),
  grade_mm: 0,
};
const STRING = {
  kind: "dimension",
  id: "d9",
  level_id: level,
  a: pt(0, -1000),
  b: pt(8000, -1000),
  offset_mm: -800,
  text_override: null,
  through: [pt(2500, -1000), pt(6000, -1000)],
};

function docWith(...els: unknown[]): DocState {
  const d = structuredClone(base);
  d.project.elements.push(...(els as Element[]));
  return d;
}

describe("permit shapes", () => {
  it("puts grid bubbles one radius past each end", () => {
    const pts = gridOutline({ start: pt(0, 0), end: pt(1000, 0) }, 100);
    expect(pts[0].x).toBeCloseTo(0);
    expect(Math.min(...pts.map((p) => p.x))).toBeCloseTo(-200);
    expect(Math.max(...pts.map((p) => p.x))).toBeCloseTo(1200);
  });

  it("looks left of a section unless flipped", () => {
    expect(markGeometry({ start: pt(0, 0), end: pt(1000, 0), flip: false }, 100).look.y).toBeCloseTo(1);
    expect(markGeometry({ start: pt(0, 0), end: pt(1000, 0), flip: true }, 100).look.y).toBeCloseTo(-1);
  });

  it("projects a string's points onto its line and measures each segment along it", () => {
    const g = stringGeometry({ a: pt(0, 0), b: pt(6000, 0), through: [pt(1000, 500), pt(4000, -300)], offset_mm: 1000 });
    expect(g.onLine.map((p) => p.y)).toEqual([1000, 1000, 1000, 1000]);
    expect(g.segments.map((s) => s.length)).toEqual([1000, 3000, 2000]);
    expect(g.p1).toEqual(pt(0, 1000));
    expect(g.p2).toEqual(pt(6000, 1000));
    expect(isDimensionString({ through: [] })).toBe(false);
    expect(isDimensionString({ through: [pt(1, 1)] })).toBe(true);
  });

  it("winds lots counter-clockwise and points their normals out", () => {
    const cw = [pt(0, 0), pt(0, 1000), pt(1000, 1000), pt(1000, 0)];
    expect(windingOf(cw)).toBe(-1);
    const ccw = counterClockwise(cw);
    expect(windingOf(ccw)).toBe(1);
    const sides = lotSides({ corners: ccw });
    // The first side runs along x = 1000 from y 0 to... whichever way: its normal points away from the middle.
    for (const s of sides) expect((s.mid.x - 500) * s.out.x + (s.mid.y - 500) * s.out.y).toBeGreaterThan(0);
    const o = cornerOut({ corners: ccw }, 0);
    expect((ccw[0].x - 500) * o.x + (ccw[0].y - 500) * o.y).toBeGreaterThan(0);
  });
});

describe("picking the permit annotations", () => {
  const index = buildIndex(docWith(GRID, MARK, ELEV, LOT, STRING), null);

  it("picks a grid line by its line and by its bubbles", () => {
    expect(hitTest(pt(4000, 12010), index, opt)).toBe("g1");
    // Inside the bubble past the start.
    expect(hitTest(pt(-2180, 12000), index, opt)).toBe("g1");
    expect(hitTest(pt(4000, 12500), index, opt)).not.toBe("g1");
  });

  it("picks section heads and elevation targets", () => {
    expect(hitTest(pt(-3000, 3500), index, opt)).toBe("m1");
    // The head one radius past the start.
    expect(hitTest(pt(-3000, 1820), index, opt)).toBe("m1");
    expect(hitTest(pt(4000, 9000), index, opt)).toBe("m2");
  });

  it("picks the lot by its sides, never by its inside", () => {
    expect(hitTest(pt(5000, -5010), index, opt)).toBe("l1");
    expect(hitTest(pt(-4200, -4200), index, opt)).toBeNull();
  });

  it("picks a dimension string along its whole line", () => {
    // Over the second segment, not near a or b.
    expect(hitTest(pt(4500, -1800), index, opt)).toBe("d9");
    expect(hitTest(pt(7900, -1800), index, opt)).toBe("d9");
  });

  it("marquee selects them; a marquee inside the lot leaves the lot", () => {
    const all = marqueeSelect(rectFromPoints(pt(-6000, -6000), pt(16000, 16000)), false, index, opt);
    expect(all).toEqual(expect.arrayContaining(["g1", "m1", "m2", "l1", "d9"]));
    const inside = marqueeSelect(rectFromPoints(pt(-4000, -4000), pt(14000, 14000)), true, index, opt);
    expect(inside).not.toContain("l1");
    expect(inside).toEqual(expect.arrayContaining(["g1", "m1", "m2", "d9"]));
    const window = marqueeSelect(rectFromPoints(pt(-1000, -2000), pt(9000, -500)), false, index, opt);
    expect(window).toContain("d9");
  });

  it("gives shapes and key points to move them by, and moves them whole", () => {
    for (const id of ["g1", "m1", "m2", "l1", "d9"]) {
      const el = index.byId.get(id);
      if (!el) throw new Error(id);
      expect(elementShape(el, index, opt)?.points.length ?? 0).toBeGreaterThan(1);
      expect(keyPoints(el, index).length).toBeGreaterThan(1);
    }
    const lot = index.byId.get("l1");
    if (!lot || lot.kind !== "lot") throw new Error("lot");
    const moved = translateElement(lot, pt(100, 0));
    expect(moved.kind === "lot" && moved.corners[0]).toEqual(pt(-4900, -5000));
  });

  it("drags a dimension string's witness points with it, so its segments keep their lengths (GR3 P0-1)", () => {
    // The reviewer's string: 3000, 2000, 3000 along y = -2000, dragged by (1000, -1000).
    const drawn = { ...STRING, a: pt(0, -2000), b: pt(8000, -2000), through: [pt(3000, -2000), pt(5000, -2000)] } as Element;
    const moved = translateElement(drawn, pt(1000, -1000));
    if (moved.kind !== "dimension") throw new Error("dimension");
    expect(moved.through).toEqual([pt(4000, -3000), pt(6000, -3000)]);
    expect(stringGeometry(moved).segments.map((s) => s.length)).toEqual([3000, 2000, 3000]);
    // A plain dimension has no witness points to move.
    const plain = { kind: "dimension", id: "d1", level_id: level, a: pt(0, 0), b: pt(1000, 0), offset_mm: 500, text_override: null } as Element;
    expect("through" in translateElement(plain, pt(10, 10))).toBe(false);
  });
});
