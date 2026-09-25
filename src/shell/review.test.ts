import { describe, expect, it } from "vitest";
import type { DocState, Element, Issue, ReviewMark, RoomGeometry } from "../contract/bindings";
import {
  OUTSIDE_ROOMS,
  WHOLE_PROJECT,
  asideRows,
  checkLabel,
  groupReview,
  markCovers,
  neighbourAfterRemoval,
  rememberIssues,
  resolvedTitle,
  targetFor,
  targetKey,
  triageKey,
  type TriageRow,
} from "./review";

type Doc = Pick<DocState, "project" | "derived">;

const square = (x0: number, y0: number, x1: number, y1: number) => [
  { x: x0, y: y0 },
  { x: x1, y: y0 },
  { x: x1, y: y1 },
  { x: x0, y: y1 },
];

const roomGeo = (id: string, x0: number, y0: number, x1: number, y1: number): RoomGeometry => ({
  room_id: id,
  polygon: square(x0 + 75, y0 + 75, x1 - 75, y1 - 75),
  centerline_polygon: square(x0, y0, x1, y1),
  area_mm2: 0,
  perimeter_mm: 0,
  label_point: { x: (x0 + x1) / 2, y: (y0 + y1) / 2 },
  wall_ids: [],
});

const room = (id: string, name: string, level = "g"): Element => ({ kind: "room", id, level_id: level, name, usage: "bedroom", seed: { x: 0, y: 0 }, floor_material_id: null, auto_named: false });
const wall = (id: string, x0: number, y0: number, x1: number, y1: number, level = "g"): Element => ({
  kind: "wall",
  id,
  level_id: level,
  start: { x: x0, y: y0 },
  end: { x: x1, y: y1 },
  thickness_mm: 150,
  height_mm: null,
  material_id: null,
} as Element);
const asset = (id: string, x: number, y: number, level = "g"): Element => ({
  kind: "asset",
  id,
  level_id: level,
  catalog_key: "switch-1",
  name: `Switch ${id}`,
  category: "electrical",
  position: { x, y },
  rotation_deg: 0,
  width_mm: 70,
  depth_mm: 40,
  height_mm: 115,
  elevation_mm: 1143,
  light: null,
  links: [],
  circuit: "",
});

const issue = (id: string, severity: Issue["severity"], code: string, element_ids: string[], patch: Partial<Issue> = {}): Issue => ({
  id,
  severity,
  code,
  message: `${code} message`,
  element_ids,
  location: null,
  status: "open",
  note: "",
  ...patch,
});

// Two rooms side by side on the ground floor (0..4000 and 4000..8000 by
// 0..3000), a bedroom upstairs, walls between.
function makeDoc(issues: Issue[], review: ReviewMark[] = [], resolved: ReviewMark[] = []): Doc {
  const elements: Element[] = [
    room("living", "Living room"),
    room("kitchen", "Kitchen"),
    room("bed", "Bedroom", "u"),
    wall("w-mid", 4000, 0, 4000, 3000),
    wall("w-out", 9000, 0, 9000, 3000),
    asset("sw-kitchen", 6000, 1500),
    asset("sw-living", 1000, 1000),
    asset("sw-up", 1000, 1000, "u"),
    {
      kind: "opening",
      id: "door-mid",
      wall_id: "w-mid",
      opening_type: "door",
      style: "swing_single",
      offset_mm: 1500,
      width_mm: 800,
      height_mm: 2100,
      sill_mm: 0,
      flip_side: true,
      flip_hinge: false,
      material_id: null,
    },
  ];
  return {
    project: {
      levels: [
        { id: "g", name: "Ground floor", elevation_mm: 0, height_mm: 3000 },
        { id: "u", name: "Second floor", elevation_mm: 3000, height_mm: 3000 },
      ],
      elements,
      review,
    },
    derived: {
      rooms: [roomGeo("living", 0, 0, 4000, 3000), roomGeo("kitchen", 4000, 0, 8000, 3000), roomGeo("bed", 0, 0, 4000, 3000)],
      issues,
      review_resolved: resolved,
    },
  } as unknown as Doc;
}

describe("grouping by level and room", () => {
  it("groups items by level, then room, with counts", () => {
    const items = [
      issue("a", "info", "room_small", ["kitchen"]),
      issue("b", "warning", "switch_behind_door", ["sw-kitchen", "door-mid"]),
      issue("c", "info", "switch_no_load", ["sw-living"]),
      issue("d", "warning", "room_no_window", ["bed"]),
      issue("e", "info", "switch_no_load", ["sw-up"]),
    ];
    const groups = groupReview(makeDoc(items), items);
    expect(groups.map((g) => [g.label, g.count])).toEqual([
      ["Ground floor", 3],
      ["Second floor", 2],
    ]);
    expect(groups[0].rooms.map((r) => [r.label, r.items.map((i) => i.id)])).toEqual([
      ["Kitchen", ["b", "a"]],
      ["Living room", ["c"]],
    ]);
    expect(groups[1].rooms.map((r) => [r.label, r.items.map((i) => i.id)])).toEqual([["Bedroom", ["d", "e"]]]);
  });

  it("puts warnings first inside a room and rooms with a warning first", () => {
    const items = [issue("i1", "info", "room_small", ["living"]), issue("i2", "info", "switch_no_load", ["sw-kitchen"]), issue("w1", "warning", "room_no_door", ["kitchen"])];
    const [ground] = groupReview(makeDoc(items), items);
    expect(ground.rooms.map((r) => r.label)).toEqual(["Kitchen", "Living room"]);
    expect(ground.rooms[0].items.map((i) => i.id)).toEqual(["w1", "i2"]);
    expect(ground.rooms[0].worst).toBe("warning");
  });

  it("finds the room of a door on the side its leaf swings to", () => {
    // flip_side true swings to the right of the wall direction (south to north: east), the kitchen.
    const items = [issue("door", "warning", "door_narrow", ["door-mid"])];
    const [ground] = groupReview(makeDoc(items), items);
    expect(ground.rooms[0].label).toBe("Kitchen");
  });

  it("uses the location when the item has one", () => {
    const items = [issue("pipe", "warning", "pipes_cross", ["sw-kitchen"], { location: { x: 1000, y: 2000, z: 300 } })];
    const [ground] = groupReview(makeDoc(items), items);
    expect(ground.rooms[0].label).toBe("Living room");
  });

  it("files items outside every room, and items with no element, apart", () => {
    const items = [issue("out", "warning", "wall_dangling_end", ["w-out"]), issue("none", "info", "pipe_penetrations", [])];
    const groups = groupReview(makeDoc(items), items);
    expect(groups.map((g) => g.label)).toEqual(["Ground floor", WHOLE_PROJECT]);
    expect(groups[0].rooms[0].label).toBe(OUTSIDE_ROOMS);
    expect(groups[1].rooms[0].label).toBe(WHOLE_PROJECT);
  });
});

describe("set aside targets", () => {
  const item = issue("door_narrow:d1", "warning", "door_narrow", ["d1", "w1"]);

  it("targets the item, the whole check, or the check on its first object", () => {
    expect(targetFor("issue", item)).toEqual({ kind: "issue", id: "door_narrow:d1" });
    expect(targetFor("check", item)).toEqual({ kind: "check", code: "door_narrow" });
    expect(targetFor("element", item)).toEqual({ kind: "element", code: "door_narrow", element_id: "d1" });
    expect(targetFor("element", { ...item, element_ids: [] })).toBeNull();
  });

  it("matches marks to items", () => {
    expect(markCovers({ kind: "check", code: "door_narrow" }, item)).toBe(true);
    expect(markCovers({ kind: "element", code: "door_narrow", element_id: "w1" }, item)).toBe(true);
    expect(markCovers({ kind: "element", code: "room_small", element_id: "d1" }, item)).toBe(false);
    expect(markCovers({ kind: "issue", id: "x" }, item)).toBe(false);
    expect(targetKey({ kind: "element", code: "c", element_id: "e" })).toBe("element:c:e");
  });

  it("names checks and never says approved", () => {
    expect(checkLabel("door_narrow")).toBe("Narrow doors");
    expect(checkLabel("some_new_check")).toBe("Some new check");
    for (const code of ["room_no_window", "lineset_long", "unit_near_tv", "switch_behind_door"]) expect(checkLabel(code).toLowerCase()).not.toContain("approv");
  });
});

describe("set aside and resolved rows", () => {
  it("lists marks with their items, leaving resolved ones out", () => {
    const items = [
      issue("room_small:kitchen", "info", "room_small", ["kitchen"], { status: "ignored", note: "Owner wants it small" }),
      issue("switch_no_load:sw-kitchen", "info", "switch_no_load", ["sw-kitchen"], { status: "ignored", note: "Spare" }),
      issue("switch_no_load:sw-living", "info", "switch_no_load", ["sw-living"], { status: "ignored", note: "Spare" }),
    ];
    const marks: ReviewMark[] = [
      { target: { kind: "issue", id: "room_small:kitchen" }, note: "Owner wants it small" },
      { target: { kind: "check", code: "switch_no_load" }, note: "Spare" },
      { target: { kind: "element", code: "room_no_door", element_id: "bed" }, note: "Door comes later" },
      { target: { kind: "issue", id: "door_narrow:gone" }, note: "Checked" },
    ];
    const resolved = [marks[3]];
    const rows = asideRows(makeDoc(items, marks, resolved), (id) => (id === "bed" ? "Bedroom" : null));
    expect(rows.map((r) => [r.title, r.issues.length, r.mark.note])).toEqual([
      ["room_small message", 1, "Owner wants it small"],
      ["Switches that control nothing, everywhere", 2, "Spare"],
      ["Rooms without a door, on Bedroom", 0, "Door comes later"],
    ]);
    expect(rows[1].elementIds.sort()).toEqual(["sw-kitchen", "sw-living"]);
  });

  it("remembers what a resolved item was", () => {
    expect(resolvedTitle("door_narrow:zz")).toBe("Narrow doors: an item the checks no longer find");
    expect(resolvedTitle("zz")).toBe("An item the checks no longer find");
    rememberIssues([issue("door_narrow:zz", "warning", "door_narrow", ["zz"], { message: "This door is 650 mm wide." })]);
    expect(resolvedTitle("door_narrow:zz")).toBe("This door is 650 mm wide.");
  });
});

describe("triage keys", () => {
  const rows: TriageRow[] = [
    { key: "a", kind: "open" },
    { key: "b", kind: "open" },
    { key: "x", kind: "aside" },
  ];
  const k = (key: string, mods: Record<string, boolean> = {}) => ({ key, ...mods });

  it("moves with up and down, and lands on the first row from nothing", () => {
    expect(triageKey(k("ArrowDown"), rows, "a")).toEqual({ type: "move", key: "b" });
    expect(triageKey(k("ArrowDown"), rows, "x")).toEqual({ type: "move", key: "x" });
    expect(triageKey(k("ArrowUp"), rows, "b")).toEqual({ type: "move", key: "a" });
    expect(triageKey(k("ArrowUp"), rows, "a")).toEqual({ type: "move", key: "a" });
    expect(triageKey(k("ArrowDown"), rows, null)).toEqual({ type: "move", key: "a" });
    expect(triageKey(k("End"), rows, "a")).toEqual({ type: "move", key: "x" });
    expect(triageKey(k("Home"), rows, "x")).toEqual({ type: "move", key: "a" });
  });

  it("sets open items aside with S and reopens set-aside rows with O", () => {
    expect(triageKey(k("s"), rows, "a")).toEqual({ type: "set_aside", key: "a" });
    expect(triageKey(k("S"), rows, "b")).toEqual({ type: "set_aside", key: "b" });
    expect(triageKey(k("s"), rows, "x")).toBeNull();
    expect(triageKey(k("o"), rows, "x")).toEqual({ type: "reopen", key: "x" });
    expect(triageKey(k("o"), rows, "a")).toBeNull();
  });

  it("shows the active row with Enter", () => {
    expect(triageKey(k("Enter"), rows, "b")).toEqual({ type: "show", key: "b" });
    expect(triageKey(k("Enter"), rows, null)).toBeNull();
  });

  it("leaves modified keys to the global shortcuts", () => {
    expect(triageKey(k("s", { metaKey: true }), rows, "a")).toBeNull();
    expect(triageKey(k("s", { ctrlKey: true }), rows, "a")).toBeNull();
    expect(triageKey(k("s", { shiftKey: true }), rows, "a")).toBeNull();
    expect(triageKey(k("ArrowDown", { altKey: true }), rows, "a")).toBeNull();
    expect(triageKey(k("w"), rows, "a")).toBeNull();
    expect(triageKey(k("ArrowDown"), [], null)).toBeNull();
  });

  it("lands on the next row when one leaves, else the one before", () => {
    expect(neighbourAfterRemoval(rows, "a")).toBe("b");
    expect(neighbourAfterRemoval(rows, "x")).toBe("b");
    expect(neighbourAfterRemoval([{ key: "only", kind: "open" }], "only")).toBeNull();
  });
});
