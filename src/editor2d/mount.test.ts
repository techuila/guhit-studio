import { describe, expect, it } from "vitest";
import type { Opening, Wall } from "../contract/bindings";
import { pt, thickSegment } from "./geom";
import { backDir } from "./model";
import {
  ceilingElevation,
  ceilingHeightMm,
  facesOfWall,
  latchGuides,
  latchJambs,
  mountAtGuide,
  mountHeightLabel,
  mountInWindow,
  mountOnFace,
  nearestGuide,
  nearestWindow,
  roomCenterSnap,
  snapToWallFace,
  usableGuides,
} from "./mount";

const wall: Wall = { id: "w1", level_id: "l", start: pt(0, 0), end: pt(4000, 0), thickness_mm: 150, height_mm: null, material_id: null };
const door = (patch: Partial<Opening> = {}): Opening => ({
  id: "d1",
  wall_id: "w1",
  opening_type: "door",
  style: "swing_single",
  offset_mm: 2000,
  width_mm: 900,
  height_mm: 2100,
  sill_mm: 0,
  flip_side: false,
  flip_hinge: false,
  material_id: null,
  ...patch,
});
const outline = thickSegment(wall.start, wall.end, wall.thickness_mm);
const SWITCH = { w: 70, d: 40 };

describe("wall faces", () => {
  it("finds both long faces with the openings as gaps", () => {
    const faces = facesOfWall(wall, outline, [door()]);
    expect(faces).toHaveLength(2);
    const south = faces.find((f) => f.side === -1)!;
    const north = faces.find((f) => f.side === 1)!;
    expect(south.out.y).toBeCloseTo(-1);
    expect(north.out.y).toBeCloseTo(1);
    expect(south.a.y).toBeCloseTo(-75);
    expect(north.a.y).toBeCloseTo(75);
    // The same door spans 1550 to 2450 along both faces, measured from each face start.
    expect(south.gaps[0].from).toBeCloseTo(1550);
    expect(south.gaps[0].to).toBeCloseTo(2450);
    expect(north.gaps[0].from).toBeCloseTo(1550);
    expect(north.gaps[0].to).toBeCloseTo(2450);
  });
});

describe("wall snapping", () => {
  const faces = facesOfWall(wall, outline, [door()]);

  it("puts the back of a switch on the nearest face", () => {
    const m = snapToWallFace(pt(1030, 300), faces, SWITCH.w, SWITCH.d, 500, 100)!;
    expect(m).not.toBeNull();
    expect(m.valid).toBe(true);
    expect(m.position.x).toBeCloseTo(1000); // on the plan grid
    expect(m.position.y).toBeCloseTo(75 + 20);
    // The back (local +y) points into the wall.
    const back = backDir(m.rotationDeg);
    expect(back.x).toBeCloseTo(0);
    expect(back.y).toBeCloseTo(-1);
  });

  it("uses the other face from the other side", () => {
    const m = snapToWallFace(pt(1030, -300), faces, SWITCH.w, SWITCH.d, 500, 0)!;
    expect(m.position.y).toBeCloseTo(-95);
    expect(m.position.x).toBeCloseTo(1030);
    expect(backDir(m.rotationDeg).y).toBeCloseTo(1);
  });

  it("never covers a door: moves to the nearer free side", () => {
    const m = snapToWallFace(pt(1900, 300), faces, SWITCH.w, SWITCH.d, 500, 0)!;
    expect(m.valid).toBe(true);
    expect(m.position.x).toBeCloseTo(1550 - 35);
    const n = snapToWallFace(pt(2300, -300), faces, SWITCH.w, SWITCH.d, 500, 0)!;
    expect(n.position.x).toBeCloseTo(2450 + 35);
  });

  it("stays on the face and flags a face shorter than the object", () => {
    const m = snapToWallFace(pt(-400, 200), faces, 800, 230, 800, 0)!;
    expect(m.position.x).toBeCloseTo(400);
    const short = mountOnFace({ ...faces[0], gaps: [] }, 100, 5000, 200, 0);
    expect(short.valid).toBe(false);
    expect(short.reason).toBe("short");
  });

  it("lands on the plan grid along a face that spans several rooms", () => {
    // The face starts at a corner 75 mm off the grid: the object still lands on x 6500.
    const long: Wall = { ...wall, end: pt(8000, 0) };
    const f = facesOfWall(long, thickSegment(long.start, long.end, 150), []).find((x) => x.side === 1)!;
    const m = snapToWallFace(pt(6520, 400), [{ ...f, a: pt(7925, 75), b: pt(75, 75) }], 800, 230, 900, 100)!;
    expect(m.position.x).toBeCloseTo(6500);
    const n = snapToWallFace(pt(6520, 400), [{ ...f, a: pt(75, 75), b: pt(7925, 75) }], 800, 230, 900, 100)!;
    expect(n.position.x).toBeCloseTo(6500);
    // A slanted face rounds the distance from its start.
    const slanted = { ...f, a: pt(0, 0), b: pt(3000, 3000), out: { x: -Math.SQRT1_2, y: Math.SQRT1_2 } };
    const s = mountOnFace(slanted, 1234, 70, 40, 100);
    expect(s.t).toBeCloseTo(1200);
  });

  it("is null away from every wall", () => {
    expect(snapToWallFace(pt(1000, 2000), faces, SWITCH.w, SWITCH.d, 500, 100)).toBeNull();
  });
});

describe("latch-side switch guide", () => {
  it("sits 200 mm from the latch jamb, never the hinge jamb", () => {
    expect(latchJambs(door())).toEqual(["b"]);
    expect(latchJambs(door({ flip_hinge: true }))).toEqual(["a"]);
    expect(latchJambs(door({ style: "swing_double" }))).toEqual([]);
    expect(latchJambs(door({ style: "sliding" }))).toEqual(["a", "b"]);
    expect(latchJambs(door({ opening_type: "window", style: "sliding" }))).toEqual([]);

    const g = latchGuides(door(), wall);
    expect(g).toHaveLength(2);
    for (const x of g) {
      expect(x.jamb.x).toBeCloseTo(2450);
      expect(x.point.x).toBeCloseTo(2650);
      expect(Math.abs(x.point.y)).toBeCloseTo(75);
    }
    const flipped = latchGuides(door({ flip_hinge: true }), wall);
    expect(flipped.every((x) => Math.abs(x.point.x - 1350) < 1e-6)).toBe(true);
  });

  it("snaps a switch to the guide on the cursor's side", () => {
    const faces = facesOfWall(wall, outline, [door()]);
    const guides = usableGuides(latchGuides(door(), wall), faces, SWITCH.w);
    const near = nearestGuide(pt(2700, 400), guides)!;
    expect(near.guide.side).toBe(1);
    const m = mountAtGuide(near.guide, SWITCH.d);
    expect(m.position.x).toBeCloseTo(2650);
    expect(m.position.y).toBeCloseTo(95);
    expect(backDir(m.rotationDeg).y).toBeCloseTo(-1);
  });

  it("drops a guide that would run off the wall or into another opening", () => {
    const nearEnd = door({ offset_mm: 3450 }); // latch jamb at 3900, guide at 4100
    const faces = facesOfWall(wall, outline, [nearEnd]);
    expect(usableGuides(latchGuides(nearEnd, wall), faces, SWITCH.w)).toHaveLength(0);
    const window: Opening = { ...door({ id: "n1", opening_type: "window", style: "sliding", offset_mm: 3000, width_mm: 600, sill_mm: 900, height_mm: 1200 }) };
    const crowded = facesOfWall(wall, outline, [door(), window]);
    // The window spans 2700 to 3300: a switch at 2650 still fits (its edge is at 2685).
    expect(usableGuides(latchGuides(door(), wall), crowded, SWITCH.w)).toHaveLength(2);
    const wider = { ...window, width_mm: 800 }; // 2600 to 3400
    const blocked = facesOfWall(wall, outline, [door(), wider]);
    expect(usableGuides(latchGuides(door(), wall), blocked, SWITCH.w)).toHaveLength(0);
  });
});

describe("ceiling height", () => {
  it("hangs from the level height", () => {
    const light = { height_mm: 60, elevation_mm: 2940 };
    expect(ceilingElevation(3000, light)).toBe(2940);
    expect(ceilingElevation(2700, light)).toBe(2640);
    expect(ceilingElevation(3200, { height_mm: 50, elevation_mm: 2950 })).toBe(3150);
  });

  it("stops under the slab of the level above", () => {
    const ground = { elevation_mm: 0, height_mm: 3000 };
    const upper = { elevation_mm: 3000, height_mm: 2800 };
    expect(ceilingHeightMm(ground, [ground])).toBe(3000);
    expect(ceilingHeightMm(ground, [ground, upper])).toBe(2800);
    expect(ceilingHeightMm(upper, [ground, upper])).toBe(2800);
    // An upper level drawn 200 mm higher leaves room for its slab.
    expect(ceilingHeightMm(ground, [ground, { elevation_mm: 3200, height_mm: 2800 }])).toBe(3000);
    expect(ceilingHeightMm(undefined, [])).toBe(3000);
    expect(ceilingElevation(ceilingHeightMm(ground, [ground, upper]), { height_mm: 60, elevation_mm: 2940 })).toBe(2740);
  });

  it("keeps a pendant's catalog drop unless the ceiling cuts it", () => {
    const pendant = { height_mm: 400, elevation_mm: 2000 };
    expect(ceilingElevation(3000, pendant)).toBe(2000);
    expect(ceilingElevation(3600, pendant)).toBe(2000);
    expect(ceilingElevation(2200, pendant)).toBe(1800);
  });

  it("offers the middle of the room", () => {
    const rooms = [{ polygon: [pt(0, 0), pt(4000, 0), pt(4000, 3000), pt(0, 3000)], label_point: pt(2000, 1500) }];
    expect(roomCenterSnap(pt(2050, 1480), rooms, 100)).toEqual(pt(2000, 1500));
    expect(roomCenterSnap(pt(2500, 1500), rooms, 100)).toBeNull();
    expect(roomCenterSnap(pt(5000, 1500), rooms, 100)).toBeNull();
  });
});

describe("window aircon", () => {
  const win: Opening = door({ id: "n1", opening_type: "window", style: "sliding", offset_mm: 2000, width_mm: 1200, sill_mm: 900, height_mm: 1200 });
  const host = { opening: win, wall };
  const unitSize = { width: 471, depth: 482, height: 345, elevation: 1200 };

  it("finds the window near the cursor", () => {
    expect(nearestWindow(pt(2100, 200), [host, { opening: door(), wall }], 300)?.opening.id).toBe("n1");
    expect(nearestWindow(pt(2100, 900), [host], 300)).toBeNull();
  });

  it("sits across the wall in the window, back to the outside", () => {
    const m = mountInWindow(pt(2030, 60), host, unitSize, -1, 100);
    expect(m.centered).toBe(true);
    expect(m.position).toEqual(pt(2000, 0));
    expect(backDir(m.rotationDeg).y).toBeCloseTo(-1);
    expect(m.elevation).toBe(1200);
    expect(m.fits).toBe(true);
    const inside = mountInWindow(pt(2030, 60), host, unitSize, 1, 100);
    expect(backDir(inside.rotationDeg).y).toBeCloseTo(1);
  });

  it("slides inside the window and stays in the opening", () => {
    const m = mountInWindow(pt(2500, 60), host, unitSize, -1, 100);
    expect(m.centered).toBe(false);
    expect(m.position.x).toBeCloseTo(2600 - 235.5);
    const high = mountInWindow(pt(2000, 0), { opening: { ...win, sill_mm: 1500 }, wall }, unitSize, -1, 100);
    expect(high.elevation).toBe(1500);
  });

  it("flags a unit that does not fit", () => {
    expect(mountInWindow(pt(2000, 0), { opening: { ...win, width_mm: 400 }, wall }, unitSize, -1, 100).fits).toBe(false);
    const low = mountInWindow(pt(2000, 0), { opening: { ...win, height_mm: 300 }, wall }, unitSize, -1, 100);
    expect(low.fits).toBe(false);
    expect(low.elevation).toBe(900);
  });
});

describe("mounting height readout", () => {
  it("gives wall devices their center, the rest their underside", () => {
    expect(mountHeightLabel("wall", { category: "electrical", elevation_mm: 1143, height_mm: 115 }, "mm")).toBe("Center +1200 mm");
    expect(mountHeightLabel("wall", { category: "electrical", elevation_mm: 243, height_mm: 115 }, "mm")).toBe("Center +300 mm");
    expect(mountHeightLabel("wall", { category: "electrical", elevation_mm: 1500, height_mm: 450 }, "m")).toBe("Center +1.725 m");
    expect(mountHeightLabel("wall", { category: "aircon", elevation_mm: 2300, height_mm: 295 }, "mm")).toBe("Underside +2300 mm");
    expect(mountHeightLabel("ceiling", { category: "lighting", elevation_mm: 2940, height_mm: 60 }, "mm")).toBe("Underside +2940 mm");
    expect(mountHeightLabel("floor", { category: "furniture", elevation_mm: 0, height_mm: 500 }, "mm")).toBeNull();
    expect(mountHeightLabel("floor", { category: "sanitary", elevation_mm: 650, height_mm: 200 }, "mm")).toBe("Underside +650 mm");
  });
});
