import { describe, expect, it } from "vitest";
import type { DocState, Element } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";
import { buildCollisionWorld, crossesWall, isClear, moveWithCollision, objectBlocks, pushOut, wallPieces, WALKER_RADIUS_MM } from "./collision";

// The sample bungalow: 8 x 6 m, 150 mm outer walls (inner faces at 75 mm), a
// 100 mm wall at x = 5000 (faces 4950 and 5050). Front door on the south wall
// from x 1050 to 1950, a window from 2750 to 4250 on the same wall, an inner
// door from y 2600 to 3400 at x = 5000, a double bed at (6900, 4800).
const base = fixture as unknown as DocState;
const LEVEL = base.project.levels[0].id;
const R = WALKER_RADIUS_MM;

function withElements(extra: Element[], patch?: (d: DocState) => void): DocState {
  const d = structuredClone(base);
  d.project.elements.push(...extra);
  patch?.(d);
  return d;
}

describe("wallPieces", () => {
  it("cuts a wall outline open at each door and keeps the rest", () => {
    const wall = base.project.elements.find((e) => e.kind === "wall" && e.start.x === 0 && e.start.y === 0 && e.end.x === 8000);
    if (!wall || wall.kind !== "wall") throw new Error("fixture changed");
    const outline = base.derived.walls.find((g) => g.wall_id === wall.id)!.outline;
    const door = base.project.elements.find((e) => e.kind === "opening" && e.wall_id === wall.id && e.opening_type === "door");
    if (!door || door.kind !== "opening") throw new Error("fixture changed");
    const pieces = wallPieces(outline, wall, [door]);
    expect(pieces).toHaveLength(2);
    const xs = pieces.map((p) => [Math.min(...p.map((q) => q.x)), Math.max(...p.map((q) => q.x))]);
    expect(xs[0][1]).toBeCloseTo(1050, 3);
    expect(xs[1][0]).toBeCloseTo(1950, 3);
  });
});

describe("buildCollisionWorld", () => {
  it("has a door gap per door, a window span per window, and the bed as an object", () => {
    const world = buildCollisionWorld(base, LEVEL);
    expect(world.doorGaps).toHaveLength(2);
    expect(world.windows).toHaveLength(3);
    // Five walls, two of them cut by a door: seven pieces.
    expect(world.wallPieces).toHaveLength(7);
    expect(world.colliders.filter((c) => c.source === "object")).toHaveLength(1);
  });

  it("drops what a hidden layer does not draw", () => {
    const d = structuredClone(base);
    d.project.layers = d.project.layers.map((l) => (l.key === "assets" || l.key === "walls" ? { ...l, visible: false } : l));
    const world = buildCollisionWorld(d, LEVEL);
    expect(world.colliders).toHaveLength(0);
  });

  it("keeps a wall closed where its doors are on a hidden openings layer", () => {
    const d = structuredClone(base);
    d.project.layers = d.project.layers.map((l) => (l.key === "openings" ? { ...l, visible: false } : l));
    const world = buildCollisionWorld(d, LEVEL);
    expect(world.doorGaps).toHaveLength(0);
    expect(world.wallPieces).toHaveLength(5);
  });
});

describe("objectBlocks", () => {
  it("blocks objects taller than 300 mm that reach below eye height", () => {
    expect(objectBlocks(500, 0)).toBe(true);
    expect(objectBlocks(300, 0)).toBe(false);
    expect(objectBlocks(250, 0)).toBe(false);
    // A wall cabinet at head height still blocks, a ceiling fan does not.
    expect(objectBlocks(700, 1400)).toBe(true);
    expect(objectBlocks(400, 2400)).toBe(false);
  });
});

describe("moveWithCollision", () => {
  const world = buildCollisionWorld(base, LEVEL);

  it("walks out through the front door", () => {
    const p = moveWithCollision({ x: 1500, y: 1000 }, { x: 0, y: -3000 }, R, world);
    expect(p.y).toBeCloseTo(-2000, 0);
    expect(Math.abs(p.x - 1500)).toBeLessThan(1);
  });

  it("stops at a wall with a window: a window leaves no gap", () => {
    const p = moveWithCollision({ x: 3500, y: 1000 }, { x: 0, y: -3000 }, R, world);
    expect(p.y).toBeGreaterThanOrEqual(75 + R - 0.5);
    expect(isClear(p, R, world)).toBe(true);
  });

  it("never tunnels through a 100 mm wall on a long, slow frame", () => {
    // Five meters in one call: a running walker on a half second stall.
    const p = moveWithCollision({ x: 4000, y: 1000 }, { x: 5000, y: 0 }, R, world);
    expect(p.x).toBeLessThanOrEqual(4950 - R + 0.5);
  });

  it("slides along a wall instead of sticking to it", () => {
    const p = moveWithCollision({ x: 3000, y: 400 }, { x: 1000, y: -1000 }, R, world);
    expect(p.y).toBeGreaterThanOrEqual(75 + R - 0.5);
    expect(p.x).toBeGreaterThan(3900);
  });

  it("passes the inner door and is stopped by the wall beside it", () => {
    const through = moveWithCollision({ x: 4000, y: 3000 }, { x: 2000, y: 0 }, R, world);
    expect(through.x).toBeCloseTo(6000, 0);
    const blocked = moveWithCollision({ x: 4000, y: 1000 }, { x: 2000, y: 0 }, R, world);
    expect(blocked.x).toBeLessThanOrEqual(4950 - R + 0.5);
  });

  it("is stopped by the bed, and not by a low object", () => {
    const blocked = moveWithCollision({ x: 6900, y: 3000 }, { x: 0, y: 2000 }, R, world);
    expect(blocked.y).toBeLessThanOrEqual(4800 - 950 - R + 0.5);
    const low = withElements([
      { kind: "asset", id: "rug", level_id: LEVEL, catalog_key: "rug", name: "Rug", category: "furniture", position: { x: 2500, y: 3000 }, rotation_deg: 0, width_mm: 2000, depth_mm: 1400, height_mm: 20, elevation_mm: 0, light: null, links: [], circuit: "" },
    ]);
    const lowWorld = buildCollisionWorld(low, LEVEL);
    const p = moveWithCollision({ x: 2500, y: 1000 }, { x: 0, y: 3000 }, R, lowWorld);
    expect(p.y).toBeCloseTo(4000, 0);
  });

  it("is stopped by a round column", () => {
    const d = withElements([
      { kind: "column", id: "col", level_id: LEVEL, center: { x: 2500, y: 3000 }, shape: "round", width_mm: 300, depth_mm: 300, rotation_deg: 0, material_id: null },
    ]);
    const w = buildCollisionWorld(d, LEVEL);
    const p = moveWithCollision({ x: 2500, y: 1000 }, { x: 0, y: 3000 }, R, w);
    expect(p.y).toBeLessThanOrEqual(3000 - 150 - R + 0.5);
  });

  it("pushes a walker that starts inside a wall out of it", () => {
    const p = pushOut({ x: 4990, y: 1000 }, R, world);
    expect(isClear(p, R, world)).toBe(true);
  });
});

describe("crossesWall", () => {
  it("sees a wall between two rooms and a clear line through a door", () => {
    const world = buildCollisionWorld(base, LEVEL);
    expect(crossesWall({ x: 2500, y: 1000 }, { x: 6500, y: 1000 }, world)).toBe(true);
    expect(crossesWall({ x: 2500, y: 3000 }, { x: 6500, y: 3000 }, world)).toBe(false);
  });
});
