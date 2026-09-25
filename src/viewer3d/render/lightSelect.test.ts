import { describe, expect, it } from "vitest";
import type { DocState } from "../../contract/bindings";
import {
  cameraRooms,
  lightCopies,
  MAX_TRACER_LIGHTS,
  OUTSIDE,
  planOf,
  ROOM_WEIGHT,
  roomLinks,
  selectLights,
  storeyAt,
  type LightSpot,
  type OpeningSpot,
  type RoomArea,
  type Storey,
} from "./lightSelect";

// Three rooms in a row on one level, 4 m square each, walls on x = 0, 4000,
// 8000, 12000: A | B | C. Doors A-B and B-C, a window from A to the outside.
const square = (x0: number): RoomArea["polygon"] => [
  { x: x0 + 100, y: 100 },
  { x: x0 + 3900, y: 100 },
  { x: x0 + 3900, y: 3900 },
  { x: x0 + 100, y: 3900 },
];
const rooms: RoomArea[] = [
  { id: "A", levelId: "L0", polygon: square(0) },
  { id: "B", levelId: "L0", polygon: square(4000) },
  { id: "C", levelId: "L0", polygon: square(8000) },
];
const door = (x: number): OpeningSpot => ({ levelId: "L0", center: { x, y: 2000 }, normal: { x: 1, y: 0 }, reach: 350, halfWidth: 450 });
const openings: OpeningSpot[] = [
  door(4000),
  door(8000),
  // A window in A's north wall, y = 4000, to the outside.
  { levelId: "L0", center: { x: 2000, y: 4000 }, normal: { x: 0, y: 1 }, reach: 350, halfWidth: 600 },
];
const storeys: Storey[] = [
  { id: "L0", elevationMm: 0, heightMm: 3000 },
  { id: "L1", elevationMm: 3000, heightMm: 3000 },
];
const plan = { rooms, openings, storeys };
const links = roomLinks(rooms, openings);

const lamp = (key: string, room: string, levelId = "L0"): LightSpot => ({ key, rooms: [room], levelId, position: [0, 0, 0] });
const lamps = [lamp("a", "A"), lamp("b", "B"), lamp("c", "C"), lamp("porch", OUTSIDE)];
const nothingInView = () => false;

describe("rooms and openings", () => {
  it("joins the rooms on either side of each door and window", () => {
    expect([...(links.get("A") ?? [])].sort()).toEqual(["B", OUTSIDE]);
    expect([...(links.get("B") ?? [])].sort()).toEqual(["A", "C"]);
    expect([...(links.get("C") ?? [])]).toEqual(["B"]);
    expect([...(links.get(OUTSIDE) ?? [])]).toEqual(["A"]);
  });

  it("finds the level a height is on, or none above and below them all", () => {
    expect(storeyAt(storeys, 1500)).toBe("L0");
    expect(storeyAt(storeys, 4500)).toBe("L1");
    expect(storeyAt(storeys, 9000)).toBeNull();
    expect(storeyAt(storeys, -500)).toBeNull();
  });

  it("puts a camera in the room under it, in both rooms in a doorway, and outside otherwise", () => {
    expect(cameraRooms(plan, { x: 2000, y: 2000 }, 1500)).toEqual({ levelId: "L0", rooms: ["A"] });
    expect(cameraRooms(plan, { x: 4000, y: 2100 }, 1500).rooms.sort()).toEqual(["A", "B"]);
    expect(cameraRooms(plan, { x: 2000, y: -3000 }, 1500)).toEqual({ levelId: "L0", rooms: [OUTSIDE] });
    expect(cameraRooms(plan, { x: 2000, y: 2000 }, 20000)).toEqual({ levelId: null, rooms: [OUTSIDE] });
  });

  it("reads rooms, walls and openings from a document", () => {
    const doc = {
      project: {
        levels: [{ id: "L0", name: "Ground", elevation_mm: 0, height_mm: 3000 }],
        elements: [
          { kind: "room", id: "R1", level_id: "L0", name: "Room", usage: "living", seed: { x: 1, y: 1 }, floor_material_id: null, auto_named: false },
          { kind: "wall", id: "W1", level_id: "L0", start: { x: 0, y: 0 }, end: { x: 4000, y: 0 }, thickness_mm: 200, height_mm: null, material_id: null },
          { kind: "opening", id: "O1", wall_id: "W1", opening_type: "door", offset_mm: 1500, width_mm: 900, height_mm: 2100, sill_mm: 0 },
        ],
      },
      derived: { rooms: [{ room_id: "R1", polygon: square(0), centerline_polygon: [], area_mm2: 1, perimeter_mm: 1, label_point: { x: 2000, y: 2000 }, wall_ids: ["W1"] }] },
    } as unknown as DocState;
    const p = planOf(doc);
    expect(p.rooms).toEqual([{ id: "R1", levelId: "L0", polygon: square(0) }]);
    expect(p.openings).toHaveLength(1);
    expect(p.openings[0].center).toEqual({ x: 1500, y: 0 });
    expect(p.openings[0].normal.x).toBeCloseTo(0, 9);
    expect(p.openings[0].normal.y).toBeCloseTo(1, 9);
    expect(p.openings[0].reach).toBe(100 + 250);
    expect(p.openings[0].halfWidth).toBe(450);
    // The door joins the room and the outside.
    expect([...(roomLinks(p.rooms, p.openings).get("R1") ?? [])]).toEqual([OUTSIDE]);
  });
});

describe("the lamps a render keeps", () => {
  it("keeps the camera's room, the rooms through its openings and the outdoors, drops a room two doors away", () => {
    const keep = selectLights(lamps, { rooms: ["A"], levelId: "L0" }, links, nothingInView);
    expect([...keep].sort()).toEqual(["a", "b", "porch"]);
  });

  it("keeps no outdoor lamp for a room without an opening to the outside", () => {
    const keep = selectLights(lamps, { rooms: ["C"], levelId: "L0" }, links, nothingInView);
    expect([...keep].sort()).toEqual(["b", "c"]);
  });

  it("keeps any lamp the camera sees, however far", () => {
    const keep = selectLights(lamps, { rooms: ["C"], levelId: "L0" }, links, (p) => p[0] === 1);
    expect(keep.has("a")).toBe(false);
    const seen = [{ ...lamp("a", "A"), position: [1, 0, 0] as [number, number, number] }, ...lamps.slice(1)];
    expect(selectLights(seen, { rooms: ["C"], levelId: "L0" }, links, (p) => p[0] === 1).has("a")).toBe(true);
  });

  it("from outdoors keeps the outdoor lamps and the rooms with an opening to the outside", () => {
    const keep = selectLights(lamps, { rooms: [OUTSIDE], levelId: "L0" }, links, nothingInView);
    expect([...keep].sort()).toEqual(["a", "porch"]);
  });

  it("joins rooms on the camera's level only, but the outdoors on every level", () => {
    const upstairs = [lamp("b-up", "B", "L1"), lamp("porch-up", OUTSIDE, "L1")];
    const keep = selectLights(upstairs, { rooms: ["A"], levelId: "L0" }, links, nothingInView);
    expect([...keep]).toEqual(["porch-up"]);
  });

  it("keeps a lamp whose room is not known", () => {
    const keep = selectLights([{ key: "x", rooms: [], levelId: null, position: [0, 0, 0] }], { rooms: ["C"], levelId: "L0" }, links, nothingInView);
    expect(keep.has("x")).toBe(true);
  });

  it("keeps a light shared by fixtures in several rooms when any of them is reached", () => {
    const shared: LightSpot = { key: "ac", rooms: ["C", "A"], levelId: "L0", position: [0, 0, 0] };
    expect(selectLights([shared], { rooms: ["A"], levelId: "L0" }, links, nothingInView).has("ac")).toBe(true);
  });
});

describe("weighing the camera's room", () => {
  it("gives the lamps in the camera's room more copies, and none to lamps dropped", () => {
    const copies = lightCopies(lamps, { rooms: ["A"], levelId: "L0" }, links, nothingInView);
    expect(copies.get("a")).toBe(ROOM_WEIGHT);
    expect(copies.get("b")).toBe(1);
    expect(copies.get("porch")).toBe(1);
    expect(copies.get("c")).toBe(0);
  });

  it("weighs nothing up from outdoors", () => {
    const copies = lightCopies(lamps, { rooms: [OUTSIDE], levelId: "L0" }, links, nothingInView);
    expect(copies.get("a")).toBe(1);
    expect(copies.get("porch")).toBe(1);
  });

  it("stays within the tracer's light budget", () => {
    const many = Array.from({ length: 10 }, (_, i) => lamp(`a${i}`, "A"));
    const copies = lightCopies([...many, lamp("b", "B")], { rooms: ["A"], levelId: "L0" }, links, nothingInView);
    const total = [...copies.values()].reduce((a, b) => a + b, 0);
    expect(total).toBeLessThanOrEqual(Math.max(MAX_TRACER_LIGHTS, many.length + 1));
    expect(copies.get("a0")).toBeGreaterThanOrEqual(1);
    // A room with more lamps than the budget allows still keeps every one of them once.
    const crowd = Array.from({ length: MAX_TRACER_LIGHTS + 4 }, (_, i) => lamp(`x${i}`, "A"));
    const crowdCopies = lightCopies(crowd, { rooms: ["A"], levelId: "L0" }, links, nothingInView);
    expect([...crowdCopies.values()].every((n) => n === 1)).toBe(true);
  });

  it("matches the keep set of selectLights", () => {
    const copies = lightCopies(lamps, { rooms: ["C"], levelId: "L0" }, links, nothingInView);
    const kept = [...copies].filter(([, n]) => n > 0).map(([k]) => k).sort();
    expect(kept).toEqual([...selectLights(lamps, { rooms: ["C"], levelId: "L0" }, links, nothingInView)].sort());
  });
});
