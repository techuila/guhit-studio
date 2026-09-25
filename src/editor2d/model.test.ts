import { describe, expect, it } from "vitest";
import type { DocState, Element } from "../contract/bindings";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import { gripsFor, hitGrip, jointInset, normalDelta, rotationFromGrip, stretchedWalls, translateElement, walledJointMove } from "./edit";
import { pt, rectFromPoints } from "./geom";
import { hitTest, marqueeSelect } from "./hit";
import { annotationBox, buildIndex, isLocked, layerOf, modelBounds, openingFrame, stairOutline, wallOutline } from "./model";

const doc = fixture as unknown as DocState;
const W = (n: number): string => `00000000-0000-4000-8000-0000000001${String(n).padStart(2, "0")}`;
const opt = { tol: 60, labelHeightMm: 120 };

function withLayers(patch: (key: string) => { visible?: boolean; locked?: boolean }): DocState {
  const d = structuredClone(doc);
  d.project.layers = d.project.layers.map((l) => ({ ...l, ...patch(l.key) }));
  return d;
}

describe("model index", () => {
  it("indexes the fixture", () => {
    const index = buildIndex(doc, null);
    expect(index.visible.length).toBe(doc.project.elements.length);
    expect(index.levelId).toBe(doc.project.levels[0].id);
    const b = modelBounds(index);
    expect(b.minX).toBeLessThanOrEqual(0);
    expect(b.maxX).toBeGreaterThanOrEqual(8000);
  });

  it("hides layers and the openings of hidden walls", () => {
    const index = buildIndex(withLayers((k) => (k === "walls" ? { visible: false } : {})), null);
    expect(index.visible.some((e) => e.kind === "wall")).toBe(false);
    expect(index.visible.some((e) => e.kind === "opening")).toBe(false);
    expect(index.visible.some((e) => e.kind === "room")).toBe(true);
  });

  it("filters by level", () => {
    const index = buildIndex(doc, "some-other-level");
    expect(index.visible.every((e) => e.kind === "camera")).toBe(true);
  });

  it("falls back to a centerline rectangle without derived geometry", () => {
    const wall = doc.project.elements.find((e) => e.kind === "wall");
    if (!wall || wall.kind !== "wall") throw new Error("no wall");
    const o = wallOutline(wall, null);
    expect(o).toHaveLength(4);
    expect(o[0]).toEqual(pt(0, -75));
  });

  it("frames an opening on its host", () => {
    const wall = doc.project.elements.find((e) => e.id === W(1));
    if (!wall || wall.kind !== "wall") throw new Error("no wall");
    const f = openingFrame({ offset_mm: 1500, width_mm: 900 }, wall);
    expect(f.jambA).toEqual(pt(1050, 0));
    expect(f.jambB).toEqual(pt(1950, 0));
    expect(f.normal).toEqual(pt(-0, 1));
  });

  it("follows the contract for stairs and annotations", () => {
    const s = stairOutline(pt(0, 0), 0, 1000, 3000);
    expect(s[2]).toEqual(pt(500, 3000));
    const box = annotationBox(pt(100, 100), "AB\nCD", 200);
    // position is the left end of the first baseline, lines stack below
    expect(Math.min(...box.map((p) => p.x))).toBeCloseTo(100);
    expect(Math.max(...box.map((p) => p.y))).toBeGreaterThan(100);
    expect(Math.min(...box.map((p) => p.y))).toBeLessThan(100 - 200);
  });

  it("maps kinds to layers", () => {
    const cam = doc.project.elements.find((e) => e.kind === "camera");
    if (!cam) throw new Error("no camera");
    expect(layerOf(cam)).toBe("annotations");
  });
});

describe("hit testing", () => {
  const index = buildIndex(doc, null);
  it("picks openings over their wall, walls over rooms", () => {
    expect(hitTest(pt(1500, 10), index, opt)).toBe("00000000-0000-4000-8000-000000000201");
    expect(hitTest(pt(6500, 10), index, opt)).toBe(W(1));
    expect(hitTest(pt(1000, 1000), index, opt)).toBeNull();
  });

  it("picks a room by its label and assets by their footprint", () => {
    expect(hitTest(pt(2512, 3000), index, opt)).toBe("00000000-0000-4000-8000-000000000301");
    expect(hitTest(pt(6900, 4800), index, opt)).toBe("00000000-0000-4000-8000-000000000401");
  });

  it("picks a dimension on its line", () => {
    expect(hitTest(pt(4000, -900), index, opt)).toBe("00000000-0000-4000-8000-000000000501");
  });

  it("skips locked layers", () => {
    const locked = buildIndex(withLayers((k) => (k === "walls" ? { locked: true } : {})), null);
    expect(hitTest(pt(6500, 10), locked, opt)).toBeNull();
    const wall = locked.byId.get(W(1));
    expect(wall && isLocked(wall, locked)).toBe(true);
    expect(marqueeSelect(rectFromPoints(pt(-500, -500), pt(8500, 6500)), false, locked, opt).some((id) => id === W(1))).toBe(false);
  });

  it("window marquee needs full containment, crossing only a touch", () => {
    const rect = rectFromPoints(pt(4000, -500), pt(8500, 6500));
    const win = marqueeSelect(rect, false, index, opt);
    const cross = marqueeSelect(rect, true, index, opt);
    expect(win).toContain(W(2)); // east wall, fully inside
    expect(win).toContain(W(5)); // partition
    expect(win).not.toContain(W(1)); // south wall sticks out to the west
    expect(cross).toContain(W(1));
    expect(cross.length).toBeGreaterThan(win.length);
  });
});

describe("editing helpers", () => {
  const index = buildIndex(doc, null);
  const south = index.byId.get(W(1));
  if (!south || south.kind !== "wall") throw new Error("no wall");

  it("offers wall grips and finds them", () => {
    const grips = gripsFor(south, 10);
    expect(grips.map((g) => g.kind)).toEqual(["wall_start", "wall_end", "wall_mid"]);
    expect(hitGrip(grips, pt(7990, 20), 100)?.kind).toBe("wall_end");
    expect(hitGrip(grips, pt(2000, 2000), 100)).toBeNull();
  });

  it("moves a wall along its normal only", () => {
    const d = normalDelta(south, pt(5555, -1234), 100);
    expect(d.x).toBeCloseTo(0);
    expect(d.y).toBeCloseTo(-1200);
  });

  it("stretches the walls that share a joint", () => {
    const ghost = stretchedWalls(index, new Set([W(1)]), pt(0, -1000));
    const ids = ghost.map((w) => w.id).sort();
    expect(ids).toEqual([W(1), W(2), W(4), W(5)].sort());
    const east = ghost.find((w) => w.id === W(2));
    expect(east?.start).toEqual(pt(8000, -1000));
    expect(east?.end).toEqual(pt(8000, 6000));
  });

  it("moves every wall end at a joint", () => {
    const ghost = walledJointMove(index, pt(8000, 0), pt(9000, 0));
    expect(ghost.map((w) => w.id).sort()).toEqual([W(1), W(2)].sort());
  });

  it("measures the corner inset from joined walls", () => {
    expect(jointInset(index, south, "start")).toBe(75);
  });

  it("rotates from a grip on the local +y axis", () => {
    expect(rotationFromGrip(pt(0, 0), pt(0, 100), 15)).toBe(0);
    expect(rotationFromGrip(pt(0, 0), pt(100, 3), 15)).toBe(270);
    expect(rotationFromGrip(pt(0, 0), pt(-100, 0), 0)).toBeCloseTo(90);
  });

  it("translates elements without touching openings", () => {
    const moved = translateElement(south, pt(10, 20));
    expect(moved.kind === "wall" && moved.start).toEqual(pt(10, 20));
    const door = index.byId.get("00000000-0000-4000-8000-000000000201");
    if (!door) throw new Error("no door");
    expect(translateElement(door, pt(10, 20))).toBe(door);
  });
});

describe("devices on the plan", () => {
  const LEVEL = doc.project.levels[0].id;
  const device = (id: string, key: string, category: "electrical" | "lighting" | "aircon" | "utility", x: number, y: number, rot: number, w: number, d: number): Element => ({
    kind: "asset",
    id,
    level_id: LEVEL,
    catalog_key: key,
    name: key,
    category,
    position: pt(x, y),
    rotation_deg: rot,
    width_mm: w,
    depth_mm: d,
    height_mm: 115,
    elevation_mm: 1143,
    light: null,
    links: [],
    circuit: "",
  });
  // A switch on the inside face of the south wall, a ceiling light, a split unit and a water meter.
  const els = [
    device("sw", "switch-1", "electrical", 2150, 95, 180, 70, 40),
    device("li", "light-ceiling", "lighting", 2500, 3000, 0, 300, 300),
    device("ac", "aircon-indoor-1hp", "aircon", 6500, 5810, 0, 800, 230),
    device("wm", "water-meter", "utility", 1000, 5000, 0, 250, 150),
  ];
  function docWith(patch: (key: string) => { visible?: boolean; locked?: boolean } = () => ({})): DocState {
    const d = withLayers(patch);
    d.project.elements = [...d.project.elements, ...els];
    return d;
  }

  it("puts lights and electrical devices on the electrical layer, aircon units on the aircon layer", () => {
    const index = buildIndex(docWith(), null);
    expect(layerOf(index.byId.get("sw")!)).toBe("electrical");
    expect(layerOf(index.byId.get("li")!)).toBe("electrical");
    expect(layerOf(index.byId.get("ac")!)).toBe("aircon");
    expect(layerOf(index.byId.get("wm")!)).toBe("assets");
  });

  it("hides them with their layer and keeps locked ones from being picked", () => {
    const hidden = buildIndex(docWith((k) => (k === "electrical" ? { visible: false } : {})), null);
    expect(hidden.visibleIds.has("sw")).toBe(false);
    expect(hidden.visibleIds.has("li")).toBe(false);
    expect(hidden.visibleIds.has("ac")).toBe(true);
    const locked = buildIndex(docWith((k) => (k === "aircon" ? { locked: true } : {})), null);
    expect(isLocked(locked.byId.get("ac")!, locked)).toBe(true);
    expect(hitTest(pt(6500, 5810), locked, opt)).not.toBe("ac");
    expect(isLocked(locked.byId.get("sw")!, locked)).toBe(false);
  });

  it("picks a small device by its symbol, not its 70 mm footprint", () => {
    const index = buildIndex(docWith(), null);
    // 120 mm into the room from the switch: outside its footprint, on its "S".
    expect(hitTest(pt(2150, 215), index, { ...opt, tol: 10 })).toBe("sw");
  });
});
