import { describe, expect, it } from "vitest";
import type { DocState, Element, Level } from "../contract/bindings";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import { gripsFor, translateElement } from "./edit";
import { hitTest, marqueeSelect } from "./hit";
import { buildIndex, layerOf } from "./model";
import type { SlabEl, StairEl } from "./slab";
import {
  UNDERLAY_KEY,
  arrivalLevel,
  canCloseSlab,
  cleanOutline,
  clipToPolygon,
  hitsSlab,
  insertVertex,
  labelPoint,
  levelBelow,
  moveVertex,
  newSlab,
  offsetRun,
  popSlabPoint,
  railingPlan,
  railingsOn,
  readUnderlay,
  removeVertex,
  offersRoom,
  roomOutlineAt,
  sameOutline,
  slabAreaMm2,
  slabClick,
  stairTreadOffsets,
  stairsArrivingAt,
  underlayWalls,
  voidCross,
  writeUnderlay,
} from "./slab";

const GROUND = "00000000-0000-4000-8000-0000000000a1";
const UPPER = "level-2";
const LIVING = "00000000-0000-4000-8000-000000000301";
const opt = { tol: 60, labelHeightMm: 120 };

const sq = (x0: number, y0: number, x1: number, y1: number) => [
  { x: x0, y: y0 },
  { x: x1, y: y0 },
  { x: x1, y: y1 },
  { x: x0, y: y1 },
];

function slab(id: string, levelId: string, kind: SlabEl["slab_kind"], polygon: { x: number; y: number }[]): SlabEl {
  return { ...newSlab(levelId, kind, true, polygon), id };
}

function stair(id: string, levelId: string): StairEl {
  return { kind: "stair", id, level_id: levelId, origin: { x: 2000, y: 1000 }, rotation_deg: 0, width_mm: 1000, run_mm: 4000, riser_count: 17, railing: { height_mm: 900, style: "bars" } };
}

/** The bungalow with a second level, the Slabs layer, a balcony and a void upstairs, and a stair below. */
function twoLevels(extra: Element[] = []): DocState {
  const d = structuredClone(fixture as unknown as DocState);
  d.project.levels.push({ id: UPPER, name: "Level 2", elevation_mm: 3000, height_mm: 3000 });
  d.project.layers.push({ key: "slabs", name: "Slabs", visible: true, locked: false } as DocState["project"]["layers"][number]);
  d.project.elements.push(
    slab("sl-balcony", UPPER, "balcony", sq(0, -1500, 4000, -75)),
    slab("sl-void", UPPER, "void", sq(500, 500, 2500, 2500)),
    stair("st-1", GROUND),
    ...extra,
  );
  d.derived.railings = [
    { host_id: "sl-balcony", level_id: UPPER, kind: "slab_edge", points: [{ x: 0, y: -75, z: 0 }, { x: 0, y: -1500, z: 0 }, { x: 4000, y: -1500, z: 0 }, { x: 4000, y: -75, z: 0 }], height_mm: 1000, style: "bars" },
    { host_id: "st-1", level_id: GROUND, kind: "stair_side", points: [{ x: 1500, y: 1000, z: 176 }, { x: 1500, y: 5000, z: 3000 }], height_mm: 900, style: "bars" },
    { host_id: "st-1", level_id: UPPER, kind: "stair_well", points: [{ x: 1500, y: 5000, z: 0 }, { x: 1500, y: 1000, z: 0 }, { x: 2500, y: 1000, z: 0 }, { x: 2500, y: 5000, z: 0 }], height_mm: 900, style: "solid" },
  ];
  return d;
}

function withLayer(d: DocState, key: string, patch: { visible?: boolean; locked?: boolean }): DocState {
  d.project.layers = d.project.layers.map((l) => (l.key === key ? { ...l, ...patch } : l));
  return d;
}

describe("slab tool outline", () => {
  it("adds points, ignores a click on the last one and closes on the first", () => {
    let pts: { x: number; y: number }[] = [];
    for (const p of sq(0, 0, 3000, 2000)) {
      const r = slabClick(pts, p, 50);
      expect(r.kind).toBe("add");
      if (r.kind === "add") pts = r.points;
    }
    expect(pts).toHaveLength(4);
    expect(slabClick(pts, { x: 0, y: 2000.5 }, 50).kind).toBe("none");
    const close = slabClick(pts, { x: 20, y: -10 }, 50);
    expect(close.kind).toBe("close");
    if (close.kind === "close") expect(close.points).toEqual(sq(0, 0, 3000, 2000));
  });

  it("needs three points around some area before it closes", () => {
    const two = [
      { x: 0, y: 0 },
      { x: 1000, y: 0 },
    ];
    expect(canCloseSlab(two)).toBe(false);
    expect(slabClick(two, { x: 0, y: 0 }, 50).kind).toBe("none");
    // Three points in a line enclose nothing.
    expect(canCloseSlab([...two, { x: 2000, y: 0 }])).toBe(false);
    // Under 0.01 m2 is refused by the engine, so the tool does not close it.
    expect(canCloseSlab(sq(0, 0, 90, 90))).toBe(false);
    expect(canCloseSlab(sq(0, 0, 100, 100))).toBe(true);
  });

  it("Backspace takes the last point back, then ends the draft", () => {
    expect(popSlabPoint(sq(0, 0, 1, 1))).toHaveLength(3);
    expect(popSlabPoint([{ x: 0, y: 0 }])).toBeNull();
  });

  it("cleans the outline as the engine keeps it", () => {
    const pts = [{ x: 0, y: 0 }, { x: 0.5, y: 0 }, { x: 1000, y: 0 }, { x: 1000, y: 1000 }, { x: 0.2, y: 0.3 }];
    expect(cleanOutline(pts)).toEqual([{ x: 0, y: 0 }, { x: 1000, y: 0 }, { x: 1000, y: 1000 }]);
  });

  it("builds the element JSON the engine reads (slab_kind, not kind)", () => {
    const el = newSlab(UPPER, "void", true, sq(0, 0, 1000, 1000));
    expect(el.kind).toBe("slab");
    expect(el.slab_kind).toBe("void");
    expect(el).toMatchObject({ id: "", level_id: UPPER, thickness_mm: 150, top_offset_mm: 0, material_id: null, railing: { height_mm: 1000, style: "bars" } });
    expect(newSlab(UPPER, "deck", false, sq(0, 0, 1000, 1000)).railing).toBeNull();
    expect(slabAreaMm2(el.polygon)).toBe(1_000_000);
  });

  it("moves, inserts and removes corners, keeping at least three", () => {
    const p = sq(0, 0, 1000, 1000);
    expect(moveVertex(p, 2, { x: 1500, y: 1500 })[2]).toEqual({ x: 1500, y: 1500 });
    const five = insertVertex(p, 0, { x: 500, y: -200 });
    expect(five).toHaveLength(5);
    expect(five[1]).toEqual({ x: 500, y: -200 });
    expect(removeVertex(five, 1)).toEqual(p);
    expect(removeVertex(p.slice(0, 3), 0)).toBeNull();
  });
});

describe("slab tool room pick", () => {
  it("knows an outline already taken, in any order or winding", () => {
    const a = sq(0, 0, 1000, 1000);
    expect(sameOutline(a, a.slice().reverse())).toBe(true);
    expect(sameOutline(a, [...a.slice(1), a[0]])).toBe(true);
    expect(sameOutline(a, sq(0, 0, 1000, 1200))).toBe(false);
    expect(sameOutline(a, a.slice(0, 3))).toBe(false);
  });

  it("takes a room of the active level first", () => {
    const d = twoLevels();
    const r = roomOutlineAt(d, GROUND, { x: 2000, y: 3000 }, true);
    expect(r?.roomId).toBe(LIVING);
    expect(r?.below).toBe(false);
    expect(r?.polygon).toHaveLength(4);
  });

  it("falls back to the level below, through the underlay only", () => {
    const d = twoLevels();
    const r = roomOutlineAt(d, UPPER, { x: 2000, y: 3000 }, true);
    expect(r?.roomId).toBe(LIVING);
    expect(r?.below).toBe(true);
    expect(r?.levelId).toBe(GROUND);
    expect(roomOutlineAt(d, UPPER, { x: 2000, y: 3000 }, false)).toBeNull();
    expect(roomOutlineAt(d, UPPER, { x: -3000, y: 3000 }, true)).toBeNull();
  });

  it("offers the room only away from snaps, unlocked, and not while Alt is held", () => {
    expect(offersRoom("none", false, false)).toBe(true);
    expect(offersRoom("grid", false, false)).toBe(true);
    // Alt draws a free outline inside the room.
    expect(offersRoom("none", true, false)).toBe(false);
    expect(offersRoom("endpoint", false, false)).toBe(false);
    expect(offersRoom("face", false, false)).toBe(false);
    expect(offersRoom("none", false, true)).toBe(false);
  });
});

describe("levels", () => {
  const levels: Level[] = [
    { id: "a", name: "Ground", elevation_mm: 0, height_mm: 3000 },
    { id: "c", name: "Roof deck", elevation_mm: 6200, height_mm: 2800 },
    { id: "b", name: "Second", elevation_mm: 3200, height_mm: 3000 },
  ];

  it("finds the next lower level by elevation", () => {
    expect(levelBelow(levels, "c")?.id).toBe("b");
    expect(levelBelow(levels, "b")?.id).toBe("a");
    expect(levelBelow(levels, "a")).toBeNull();
  });

  it("finds the level a stair arrives at within 300 mm of its top", () => {
    // Ground's top is 3000: Second at 3200 is within 300 mm.
    expect(arrivalLevel(levels, "a")?.id).toBe("b");
    // Second's top is 6200: the roof deck is exactly there.
    expect(arrivalLevel(levels, "b")?.id).toBe("c");
    expect(arrivalLevel(levels, "c")).toBeNull();
    // 400 mm off is too far.
    expect(arrivalLevel([{ id: "a", name: "", elevation_mm: 0, height_mm: 3000 }, { id: "b", name: "", elevation_mm: 3400, height_mm: 3000 }], "a")).toBeNull();
    // The closest one wins.
    expect(arrivalLevel([{ id: "a", name: "", elevation_mm: 0, height_mm: 3000 }, { id: "x", name: "", elevation_mm: 2800, height_mm: 3000 }, { id: "y", name: "", elevation_mm: 3100, height_mm: 3000 }], "a")?.id).toBe("y");
  });

  it("lists the stairs arriving at a level", () => {
    const d = twoLevels();
    expect(stairsArrivingAt(d, UPPER).map((s) => s.id)).toEqual(["st-1"]);
    expect(stairsArrivingAt(d, GROUND)).toEqual([]);
    expect(buildIndex(d, UPPER).arriving.map((s) => s.id)).toEqual(["st-1"]);
    expect(buildIndex(withLayer(d, "stairs", { visible: false }), UPPER).arriving).toEqual([]);
  });
});

describe("stairs on the plan (D44)", () => {
  it("draws n - 1 treads: n - 2 lines inside the flight", () => {
    const lines = stairTreadOffsets(4000, 17);
    expect(lines).toHaveLength(15);
    // 16 treads of 250 mm.
    expect(lines[0]).toBeCloseTo(250);
    expect(lines[14]).toBeCloseTo(3750);
    expect(stairTreadOffsets(500, 2)).toEqual([]);
    expect(stairTreadOffsets(1000, 3)).toEqual([500]);
  });
});

describe("slab plan drawing", () => {
  it("crosses a four-corner void on its diagonals", () => {
    const p = sq(0, 0, 2000, 1000);
    expect(voidCross(p)).toEqual([
      [p[0], p[2]],
      [p[1], p[3]],
    ]);
  });

  it("clips the box diagonals to an L-shaped void", () => {
    const l = [{ x: 0, y: 0 }, { x: 2000, y: 0 }, { x: 2000, y: 1000 }, { x: 1000, y: 1000 }, { x: 1000, y: 2000 }, { x: 0, y: 2000 }];
    const parts = voidCross(l);
    expect(parts.length).toBeGreaterThanOrEqual(2);
    // The main diagonal of the box leaves the L at the notch corner.
    const main = clipToPolygon({ x: 0, y: 0 }, { x: 2000, y: 2000 }, l);
    expect(main).toHaveLength(1);
    expect(main[0][1].x).toBeCloseTo(1000);
    expect(main[0][1].y).toBeCloseTo(1000);
    const label = labelPoint(l);
    expect(label.x).toBeGreaterThan(0);
    expect(label.y).toBeGreaterThan(0);
  });

  it("draws bars as two lines 50 mm apart and solid as a 100 mm band", () => {
    const run = { points: [{ x: 0, y: 0, z: 0 }, { x: 1000, y: 0, z: 0 }], style: "bars" as const };
    const bars = railingPlan(run);
    expect(bars.band).toBeNull();
    expect(bars.lines.map((l) => l[0].y)).toEqual([25, -25]);
    const solid = railingPlan({ ...run, style: "solid" });
    expect(solid.lines.map((l) => l[0].y)).toEqual([50, -50]);
    expect(solid.band).toHaveLength(4);
  });

  it("mitres a closed rail at its start", () => {
    const ring = [...sq(0, 0, 1000, 1000), { x: 0, y: 0 }];
    // A counter-clockwise ring moved to its left goes inside, the corner mitred both ways.
    const out = offsetRun(ring, 25);
    expect(out).toHaveLength(ring.length);
    expect(out[0].x).toBeCloseTo(25);
    expect(out[0].y).toBeCloseTo(25);
    expect(out[out.length - 1]).toEqual(out[0]);
  });

  it("takes rails of the level, following the layer of their host", () => {
    const d = twoLevels();
    expect(railingsOn(d, UPPER, new Set()).map((r) => r.kind)).toEqual(["slab_edge", "stair_well"]);
    expect(railingsOn(d, UPPER, new Set(["slabs"])).map((r) => r.kind)).toEqual(["stair_well"]);
    expect(railingsOn(d, GROUND, new Set(["stairs"]))).toEqual([]);
  });

  it("has the walls of the level below for the underlay", () => {
    const d = twoLevels();
    const below = underlayWalls(d, UPPER);
    expect(below.length).toBe(d.project.elements.filter((e) => e.kind === "wall").length);
    expect(underlayWalls(d, GROUND)).toEqual([]);
    expect(buildIndex(withLayer(d, "walls", { visible: false }), UPPER).below).toEqual([]);
  });
});

describe("slab picking", () => {
  it("is on the Slabs layer and on its own level", () => {
    const d = twoLevels();
    const el = d.project.elements.find((e) => e.id === "sl-void")!;
    expect(layerOf(el)).toBe("slabs");
    expect(buildIndex(d, UPPER).visibleIds.has("sl-void")).toBe(true);
    expect(buildIndex(d, GROUND).visibleIds.has("sl-void")).toBe(false);
  });

  it("picks a slab inside or on its outline, rooms first on their floor", () => {
    const room = sq(0, 0, 1000, 1000);
    const outline = sq(0, 0, 3000, 3000);
    expect(hitsSlab({ x: 2000, y: 2000 }, outline, 20, [room])).toBe(true);
    expect(hitsSlab({ x: 500, y: 500 }, outline, 20, [room])).toBe(false);
    expect(hitsSlab({ x: 10, y: 500 }, outline, 20, [room])).toBe(true);
    expect(hitsSlab({ x: 4000, y: 500 }, outline, 20, [])).toBe(false);
  });

  it("hit tests through the index: visible, unlocked, not hidden", () => {
    const d = twoLevels();
    expect(hitTest({ x: 1500, y: 1500 }, buildIndex(d, UPPER), opt)).toBe("sl-void");
    expect(hitTest({ x: 2000, y: -800 }, buildIndex(d, UPPER), opt)).toBe("sl-balcony");
    expect(hitTest({ x: 1500, y: 1500 }, buildIndex(withLayer(twoLevels(), "slabs", { locked: true }), UPPER), opt)).toBeNull();
    expect(hitTest({ x: 1500, y: 1500 }, buildIndex(withLayer(twoLevels(), "slabs", { visible: false }), UPPER), opt)).toBeNull();
    const ids = marqueeSelect({ minX: -100, minY: -1600, maxX: 4100, maxY: 0 }, false, buildIndex(d, UPPER), opt);
    expect(ids).toEqual(["sl-balcony"]);
  });

  it("has a grip on each corner and edge middle, and moves as one", () => {
    const el = slab("s", UPPER, "deck", sq(0, 0, 1000, 1000));
    const grips = gripsFor(el, 1);
    expect(grips.filter((g) => g.kind === "slab_vertex")).toHaveLength(4);
    expect(grips.filter((g) => g.kind === "slab_mid").map((g) => g.pos)).toContainEqual({ x: 500, y: 0 });
    const moved = translateElement(el, { x: 100, y: -50 });
    expect(moved.kind === "slab" && moved.polygon[0]).toEqual({ x: 100, y: -50 });
  });
});

describe("underlay switch storage", () => {
  const fake = () => {
    const m = new Map<string, string>();
    return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v), removeItem: (k: string) => void m.delete(k), m };
  };

  it("is on by default and remembered per project", () => {
    const st = fake();
    expect(readUnderlay("p1", st)).toBe(true);
    writeUnderlay("p1", false, st);
    expect(st.m.get(`${UNDERLAY_KEY}p1`)).toBe("0");
    expect(readUnderlay("p1", st)).toBe(false);
    expect(readUnderlay("p2", st)).toBe(true);
    writeUnderlay("p1", true, st);
    expect(readUnderlay("p1", st)).toBe(true);
    expect(UNDERLAY_KEY).toBe("guhit.planUnderlay.");
  });

  it("survives storage that throws", () => {
    const broken = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
      removeItem: () => {
        throw new Error("blocked");
      },
    };
    expect(readUnderlay("p1", broken)).toBe(true);
    expect(() => writeUnderlay("p1", false, broken)).not.toThrow();
    expect(readUnderlay(null, null)).toBe(true);
  });
});
