import { describe, expect, it } from "vitest";
import type { DocState, Pipe } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";
import { buildCollisionWorld, isClear, WALKER_RADIUS_MM } from "./collision";
import { pointInPolygon } from "./polygon";
import { bestViewInRoom, DOOR_STANDOFF_MM, FINDING_STANDOFF_MM, hiddenAt, longestView, pipeMostlyHidden, viewDistance, walkStartPose, walkToPose } from "./walkStart";

const base = fixture as unknown as DocState;
const LEVEL = base.project.levels[0].id;
const R = WALKER_RADIUS_MM;
const living = base.derived.rooms[0].polygon;

describe("walkStartPose", () => {
  const world = buildCollisionWorld(base, LEVEL);

  const edgesOf = (w: typeof world): [{ x: number; y: number }, { x: number; y: number }][] => [
    ...w.wallPieces.flatMap((poly) => poly.map((a, i) => [a, poly[(i + 1) % poly.length]] as [{ x: number; y: number }, { x: number; y: number }])),
    ...base.derived.footprints.flatMap((f) => f.polygon.map((a, i) => [a, f.polygon[(i + 1) % f.polygon.length]] as [{ x: number; y: number }, { x: number; y: number }])),
  ];

  it("starts in the largest room, clear of the walls, looking along its longest open view", () => {
    // The orbit target in the bedroom does not pull the start there: the living room is larger.
    const pose = walkStartPose(base, LEVEL, world, { x: 6500, y: 3000 }, 1.1);
    expect(pose.from).toBe("room");
    expect(pointInPolygon(pose, living)).toBe(true);
    expect(isClear(pose, R + 400, world)).toBe(true);
    const edges = edgesOf(world);
    // Never facing a wall: at least 4.5 m of room ahead in the 4.9 x 5.9 m living room.
    expect(viewDistance(pose, pose.yaw, edges)).toBeGreaterThan(4500);
    // A better view than from the middle of the room.
    const fromLabel = longestView(base.derived.rooms[0].label_point, edges);
    expect(longestView(pose, edges).length).toBeGreaterThanOrEqual(fromLabel.length - 1);
  });

  it("measures a view to the first wall, through door gaps, and stops at the building outline", () => {
    const edges = edgesOf(world);
    // East from the living room: the partition at x = 4950 (no door there at y = 1000).
    expect(viewDistance({ x: 1000, y: 1000 }, 0, edges)).toBeCloseTo(3950, 0);
    // South through the front door gap (x 1050 to 1950): stops at the outline, not 30 m out.
    expect(viewDistance({ x: 1500, y: 3000 }, -Math.PI / 2, edges)).toBeCloseTo(3075, 0);
  });

  it("never starts on a stair", async () => {
    const { twoLevelDoc } = await import("../walk/testModel");
    const d = twoLevelDoc();
    const w = buildCollisionWorld(d, LEVEL);
    const pose = walkStartPose(d, LEVEL, w, null, null);
    expect(pose.from).toBe("room");
    // The flight: x 150 to 1050, y 1200 to 5100, plus a walker's radius.
    const onIt = pose.x > 150 - R && pose.x < 1050 + R && pose.y > 1200 - 250 - R && pose.y < 5100 + R;
    expect(onIt).toBe(false);
  });

  it("picks the best spot in a room on its own", () => {
    const v = bestViewInRoom({ polygon: living, label: base.derived.rooms[0].label_point }, edgesOf(world), world);
    expect(v).not.toBeNull();
    expect(pointInPolygon(v!, living)).toBe(true);
    expect(v!.length).toBeGreaterThan(4500);
  });

  it("with no rooms, stands just outside the first exterior door, facing in", () => {
    const d = structuredClone(base);
    d.project.elements = d.project.elements.filter((e) => e.kind !== "room");
    d.derived.rooms = [];
    const pose = walkStartPose(d, LEVEL, world, { x: 20000, y: 20000 }, 0);
    expect(pose.from).toBe("door");
    // Front door: center (1500, 0) on a 150 mm wall; outside is south.
    expect(pose.x).toBeCloseTo(1500, 3);
    expect(pose.y).toBeCloseTo(-(75 + DOOR_STANDOFF_MM), 3);
    expect(pose.yaw).toBeCloseTo(Math.PI / 2, 6);
  });

  it("with no target at all, still starts in the largest room", () => {
    expect(walkStartPose(base, LEVEL, world, null, null).from).toBe("room");
  });

  it("with neither rooms nor doors, stands south of the model and faces it", () => {
    const d = structuredClone(base);
    d.project.elements = d.project.elements.filter((e) => e.kind !== "opening" && e.kind !== "room");
    d.derived.rooms = [];
    const w = buildCollisionWorld(d, LEVEL);
    const pose = walkStartPose(d, LEVEL, w, null, null);
    expect(pose.from).toBe("outside");
    expect(pose.y).toBeLessThan(-75);
    expect(Math.sin(pose.yaw)).toBeGreaterThan(0.9);
  });
});

describe("walkToPose", () => {
  const world = buildCollisionWorld(base, LEVEL);

  it("stands 1.5 m from a finding inside a wall, on the room side, facing it", () => {
    // A cold water chase in the north wall, 300 mm above the floor.
    const loc = { x: 2000, y: 6000, z: 300 };
    const pose = walkToPose(base, LEVEL, world, loc);
    expect(pointInPolygon(pose, living)).toBe(true);
    expect(isClear(pose, R, world)).toBe(true);
    expect(Math.hypot(pose.x - loc.x, pose.y - loc.y)).toBeCloseTo(FINDING_STANDOFF_MM, -1);
    // Facing it: the view direction points at the finding.
    const dx = loc.x - pose.x;
    const dy = loc.y - pose.y;
    expect(Math.cos(pose.yaw) * dx + Math.sin(pose.yaw) * dy).toBeCloseTo(Math.hypot(dx, dy), 3);
    // Looking down from 1600 mm to 300 mm.
    expect(pose.pitch).toBeCloseTo(Math.atan2(300 - 1600, Math.hypot(dx, dy)), 3);
  });

  it("stands in the same room as a finding in the open", () => {
    const loc = { x: 6500, y: 1200, z: 1500 };
    const pose = walkToPose(base, LEVEL, world, loc);
    expect(pointInPolygon(pose, base.derived.rooms[1].polygon)).toBe(true);
  });
});

describe("hiddenAt", () => {
  it("tells a wall, the floor and the ceiling from the open", () => {
    expect(hiddenAt(base, LEVEL, { x: 2000, y: 6000, z: 300 })).toBe("wall");
    expect(hiddenAt(base, LEVEL, { x: 2000, y: 3000, z: -300 })).toBe("below");
    expect(hiddenAt(base, LEVEL, { x: 2000, y: 3000, z: 3200 })).toBe("above");
    expect(hiddenAt(base, LEVEL, { x: 2000, y: 3000, z: 1000 })).toBeNull();
  });

  it("does not count a door or window hole as hidden", () => {
    // Front door: x 1050 to 1950 on the south wall, 2100 mm high.
    expect(hiddenAt(base, LEVEL, { x: 1500, y: 0, z: 1500 })).toBeNull();
    expect(hiddenAt(base, LEVEL, { x: 1500, y: 0, z: 2500 })).toBe("wall");
    // Window: x 2750 to 4250, sill 900, 1200 high.
    expect(hiddenAt(base, LEVEL, { x: 3500, y: 0, z: 1500 })).toBeNull();
    expect(hiddenAt(base, LEVEL, { x: 3500, y: 0, z: 500 })).toBe("wall");
  });
});

describe("pipeMostlyHidden", () => {
  const pipe = (points: Pipe["points"]): Pipe => ({ id: "p", level_id: LEVEL, system: "cold_water", material: "ppr", diameter_mm: 20, points, name: "" });

  it("is true for a chase in a wall and a run under the slab", () => {
    expect(pipeMostlyHidden(base, pipe([{ x: 500, y: 6000, z: 300 }, { x: 4500, y: 6000, z: 300 }]))).toBe(true);
    expect(pipeMostlyHidden(base, pipe([{ x: 500, y: 3000, z: -300 }, { x: 4500, y: 3000, z: -300 }]))).toBe(true);
  });

  it("is false for a run in the open", () => {
    expect(pipeMostlyHidden(base, pipe([{ x: 500, y: 3000, z: 300 }, { x: 4500, y: 3000, z: 300 }]))).toBe(false);
  });
});
