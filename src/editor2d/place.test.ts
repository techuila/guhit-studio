import { describe, expect, it } from "vitest";
import type { RoomGeometry, Wall } from "../contract/bindings";
import { pt } from "./geom";
import { doorSwingSide, findHostWall, placeOnWall, roomSideOfWall, snapToFace } from "./place";

const wall: Wall = {
  id: "w1",
  level_id: "l",
  start: pt(0, 0),
  end: pt(8000, 0),
  thickness_mm: 150,
  height_mm: null,
  material_id: null,
};

describe("opening placement", () => {
  it("finds the host wall under the cursor", () => {
    expect(findHostWall(pt(3000, 60), [wall], 50)?.id).toBe("w1");
    expect(findHostWall(pt(3000, 600), [wall], 50)).toBeNull();
  });

  it("snaps to the wall center", () => {
    const p = placeOnWall(pt(4030, 20), wall, 900, 75, 75, 100, 80, true);
    expect(p.snapped).toBe("center");
    expect(p.offset).toBeCloseTo(4000);
    expect(p.clearStart).toBeCloseTo(p.clearEnd);
  });

  it("rounds the clear distance from the nearer corner", () => {
    const p = placeOnWall(pt(1333, 20), wall, 900, 75, 75, 100, 80, true);
    expect(p.snapped).toBe("clear");
    expect(p.clearStart).toBeCloseTo(800);
    expect(p.offset).toBeCloseTo(75 + 800 + 450);
    const q = placeOnWall(pt(7000, -20), wall, 900, 75, 75, 100, 80, true);
    expect(q.clearEnd).toBeCloseTo(500);
    expect(q.side).toBe(-1);
  });

  it("clamps inside the wall and flags short walls", () => {
    const p = placeOnWall(pt(-500, 0), wall, 900, 75, 75, 100, 80, true);
    expect(p.offset).toBeCloseTo(525);
    expect(p.clearStart).toBeCloseTo(0);
    const short: Wall = { ...wall, end: pt(800, 0) };
    expect(placeOnWall(pt(400, 0), short, 900, 0, 0, 100, 80, true).valid).toBe(false);
  });

  it("bumps against another opening instead of overlapping it", () => {
    const others = [{ offset: 3500, width: 1500 }]; // spans 2750 to 4250
    const p = placeOnWall(pt(2600, 0), wall, 900, 75, 75, 100, 80, true, others);
    expect(p.valid).toBe(true);
    expect(p.offset).toBeCloseTo(2750 - 450);
    const q = placeOnWall(pt(3900, 0), wall, 900, 75, 75, 100, 80, true, others);
    expect(q.offset).toBeCloseTo(4250 + 450);
    // no room on either side
    const tight: Wall = { ...wall, end: pt(2400, 0) };
    const r = placeOnWall(pt(1200, 0), tight, 900, 0, 0, 100, 80, true, [{ offset: 1200, width: 1000 }]);
    expect(r.valid).toBe(false);
    expect(r.reason).toBe("overlap");
  });

  it("does not round when snapping is off", () => {
    const p = placeOnWall(pt(1333, 20), wall, 900, 75, 75, 100, 80, false);
    expect(p.offset).toBeCloseTo(1333);
    expect(p.snapped).toBeNull();
  });
});

describe("door swing default", () => {
  // South exterior wall, room to the north (+y).
  const southWall: Wall = { ...wall, thickness_mm: 150 };
  const roomToNorth: Pick<RoomGeometry, "wall_ids" | "label_point">[] = [
    { wall_ids: ["w1"], label_point: pt(4000, 3000) },
  ];

  it("finds which side of the wall the room is on", () => {
    expect(roomSideOfWall(southWall, roomToNorth)).toBe(1);
    expect(roomSideOfWall(southWall, [{ wall_ids: ["other"], label_point: pt(4000, 3000) }])).toBeNull();
  });

  it("defaults into the room within half the wall thickness of the centerline, on an exterior wall", () => {
    const roomSide = roomSideOfWall(southWall, roomToNorth);
    // Cursor just south of the centerline (would cursor-side to -1) but inside the 75mm band.
    expect(doorSwingSide(pt(4000, -10), southWall, true, roomSide)).toBe(1);
    // Cursor just north of the centerline, inside the band: still the room side.
    expect(doorSwingSide(pt(4000, 10), southWall, true, roomSide)).toBe(1);
  });

  it("follows the cursor outside the band", () => {
    const roomSide = roomSideOfWall(southWall, roomToNorth);
    expect(doorSwingSide(pt(4000, 100), southWall, true, roomSide)).toBe(1);
    expect(doorSwingSide(pt(4000, -100), southWall, true, roomSide)).toBe(-1);
  });

  it("keeps following the cursor near the centerline on an interior wall", () => {
    expect(doorSwingSide(pt(4000, -10), southWall, false, null)).toBe(-1);
    expect(doorSwingSide(pt(4000, 10), southWall, false, null)).toBe(1);
  });

  it("follows the cursor near the centerline on an exterior wall with no known room side", () => {
    expect(doorSwingSide(pt(4000, -10), southWall, true, null)).toBe(-1);
  });
});

describe("asset face snap", () => {
  const face = { a: pt(75, 5925), b: pt(4925, 5925) }; // inner face of a north wall
  it("puts the back against the face", () => {
    const s = snapToFace(pt(2030, 5100), [face], 1370, 1900, 0, 100, 100)!;
    expect(s).not.toBeNull();
    expect(s.position.y).toBeCloseTo(5925 - 950);
    expect(s.position.x).toBeCloseTo(2075); // round distance along the face from its start
    expect(s.rotationDeg).toBeCloseTo(0);
  });
  it("uses the width when turned", () => {
    const s = snapToFace(pt(2030, 5300), [face], 1370, 1900, 1, 100, 100)!;
    expect(s.position.y).toBeCloseTo(5925 - 685);
    expect(s.rotationDeg).toBeCloseTo(90);
  });
  it("returns null when far", () => {
    expect(snapToFace(pt(2000, 2000), [face], 1370, 1900, 0, 100, 100)).toBeNull();
  });
});
