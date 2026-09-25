import { describe, expect, it } from "vitest";
import type { CatalogItem, DocState, Element, ScheduleRow } from "../contract/bindings";
import { SCHEDULE_NOTE, buildSchedules, hasScheduledDevices, roomCountsLine, schedulesCsv } from "./schedules";

type Doc = Pick<DocState, "project" | "derived">;

const cat = (key: string, name: string, patch: Partial<CatalogItem>): CatalogItem => ({
  key,
  name,
  category: "electrical",
  width_mm: 100,
  depth_mm: 100,
  height_mm: 100,
  elevation_mm: 0,
  mount: "wall",
  device: null,
  light: null,
  aircon: null,
  ...patch,
});

const CATALOG: CatalogItem[] = [
  cat("light-ceiling", "Ceiling light", { category: "lighting", mount: "ceiling", device: "lighting_outlet", light: { lumens: 900, kelvin: 3000, on: true } }),
  cat("light-table-lamp", "Table lamp", { category: "lighting", mount: "floor", light: { lumens: 400, kelvin: 2700, on: true } }),
  cat("outlet-duplex", "Convenience outlet, duplex", { device: "convenience_receptacle" }),
  cat("outlet-aircon", "Aircon outlet", { device: "special_purpose_outlet" }),
  cat("switch-1", "Switch, one gang", { device: "switch" }),
  cat("aircon-indoor-1hp", "Split aircon indoor unit, 1.0 to 1.5 HP", {
    category: "aircon",
    device: "aircon_indoor",
    aircon: { role: "indoor", hp: 1.5, liquid_mm: 6.35, gas_mm: 9.52, min_line_m: 3, max_line_m: 25, max_rise_m: 10, included_line_m: 3 },
  }),
  cat("aircon-outdoor-1hp", "Aircon outdoor unit, 1.0 to 1.5 HP", {
    category: "aircon",
    mount: "floor",
    device: "aircon_outdoor",
    aircon: { role: "outdoor", hp: 1.5, liquid_mm: 6.35, gas_mm: 9.52, min_line_m: 3, max_line_m: 25, max_rise_m: 10, included_line_m: 3 },
  }),
  cat("sofa-3", "Sofa, 3 seater", { category: "furniture", mount: "floor" }),
];

const room = (id: string, name: string, level = "g"): Element => ({ kind: "room", id, level_id: level, name, usage: "bedroom", seed: { x: 0, y: 0 }, floor_material_id: null, auto_named: false });

const asset = (id: string, key: string, patch: Partial<Extract<Element, { kind: "asset" }>> = {}): Element => ({
  kind: "asset",
  id,
  level_id: "g",
  catalog_key: key,
  name: key,
  category: "electrical",
  position: { x: 0, y: 0 },
  rotation_deg: 0,
  width_mm: 100,
  depth_mm: 100,
  height_mm: 100,
  elevation_mm: 0,
  light: null,
  links: [],
  circuit: "",
  ...patch,
});

const row = (level_id: string, room_id: string | null, catalog_key: string, device: ScheduleRow["device"], count: number, group: ScheduleRow["group"] = "electrical"): ScheduleRow => ({ level_id, room_id, group, catalog_key, device, count });

function makeDoc(elements: Element[], schedule: ScheduleRow[]): Doc {
  return {
    project: {
      levels: [
        { id: "g", name: "Ground floor", elevation_mm: 0, height_mm: 3000 },
        { id: "u", name: "Second floor", elevation_mm: 3000, height_mm: 3000 },
      ],
      elements: [room("living", "Living room"), room("kitchen", "Kitchen"), room("bed", "Bedroom", "u"), ...elements],
      review: [],
    },
    derived: { schedule },
  } as unknown as Doc;
}

const lit = (lumens: number, kelvin: number) => ({ light: { lumens, kelvin, on: true } });

const DOC = makeDoc(
  [
    asset("l1", "light-ceiling", lit(900, 3000)),
    asset("l2", "light-ceiling", lit(900, 3000)),
    asset("l3", "light-ceiling", { level_id: "u", ...lit(1800, 6500) }),
    asset("lamp", "light-table-lamp", lit(400, 2700)),
  ],
  [
    row("g", "living", "light-ceiling", "lighting_outlet", 2),
    row("g", "living", "outlet-duplex", "convenience_receptacle", 3),
    row("g", "living", "switch-1", "switch", 1),
    row("g", "kitchen", "outlet-duplex", "convenience_receptacle", 2),
    row("g", "kitchen", "outlet-aircon", "special_purpose_outlet", 1),
    row("g", null, "outlet-duplex", "convenience_receptacle", 1),
    row("u", "bed", "light-ceiling", "lighting_outlet", 1),
    row("u", "bed", "aircon-indoor-1hp", "aircon_indoor", 1, "aircon"),
    row("g", null, "aircon-outdoor-1hp", "aircon_outdoor", 1, "aircon"),
    row("g", "kitchen", "sofa-3", null, 1, "utility"),
  ],
);

describe("device schedules", () => {
  const s = buildSchedules(DOC, CATALOG);

  it("counts the inspection form rows in form order, per level", () => {
    expect(s.form.map((r) => [r.label, r.total, r.perLevel])).toEqual([
      ["Lighting outlets", 3, [2, 1]],
      ["Convenience receptacles", 6, [6, 0]],
      ["Special purpose outlets (SPO)", 1, [1, 0]],
      ["Switches", 1, [1, 0]],
    ]);
  });

  it("counts per room, rooms in level order, outside last", () => {
    expect(s.rooms.map((r) => [r.levelName, r.roomName, roomCountsLine(r.counts)])).toEqual([
      ["Ground floor", "Kitchen", "2 receptacles, 1 SPO"],
      ["Ground floor", "Living room", "2 lighting outlets, 3 receptacles, 1 switch"],
      ["Ground floor", "Outside the rooms", "1 receptacle"],
      ["Second floor", "Bedroom", "1 lighting outlet"],
    ]);
  });

  it("tables the light fixtures by type, lumens and color, plug-in lamps last", () => {
    expect(s.fixtures.map((f) => [f.name, f.count, f.lumens, f.kelvin, f.plugIn])).toEqual([
      ["Ceiling light", 2, 900, 3000, false],
      ["Ceiling light", 1, 1800, 6500, false],
      ["Table lamp", 1, 400, 2700, true],
    ]);
  });

  it("lists aircon units with their HP and counts only the ones that cool", () => {
    expect(s.aircon.map((a) => [a.levelName, a.roomName, a.role, a.hp, a.count])).toEqual([
      ["Ground floor", "Outside the rooms", "Outdoor", 1.5, 1],
      ["Second floor", "Bedroom", "Indoor", 1.5, 1],
    ]);
    expect(s.coolingUnits).toBe(1);
    expect(s.coolingHp).toBe(1.5);
  });

  it("knows when a project has devices or fixtures", () => {
    expect(hasScheduledDevices(DOC, CATALOG)).toBe(true);
    expect(hasScheduledDevices(makeDoc([asset("s", "sofa-3", { category: "furniture" })], []), CATALOG)).toBe(false);
    expect(hasScheduledDevices(makeDoc([asset("lamp", "light-table-lamp", lit(400, 2700))], []), CATALOG)).toBe(true);
  });

  it("is empty before the engine has counted", () => {
    const empty = buildSchedules(makeDoc([], []), CATALOG);
    expect(empty.form).toEqual([]);
    expect(empty.rooms).toEqual([]);
    expect(empty.aircon).toEqual([]);
  });
});

describe("schedules CSV", () => {
  const lines = schedulesCsv(buildSchedules(DOC, CATALOG)).split("\r\n");

  it("writes the device rows per room, then the totals", () => {
    expect(lines[0]).toBe("Electrical devices");
    expect(lines[1]).toBe("Level,Room,Row,Count");
    expect(lines[2]).toBe("Ground floor,Kitchen,Convenience receptacles,2");
    expect(lines).toContain("Ground floor,Kitchen,Special purpose outlets (SPO),1");
    expect(lines).toContain("All levels,All rooms,Convenience receptacles,6");
  });

  it("writes the fixture table and the aircon units", () => {
    expect(lines).toContain("Type,Count,Lumens each,Color (K),Plug-in");
    expect(lines).toContain("Ceiling light,2,900,3000,no");
    expect(lines).toContain("Table lamp,1,400,2700,yes");
    expect(lines).toContain('Second floor,Bedroom,"Split aircon indoor unit, 1.0 to 1.5 HP",Indoor,1.5,1');
    expect(lines).toContain("Cooling units,,,,1.5,1");
  });

  // The note has commas, so it is one quoted cell.
  const noteCell = `"${SCHEDULE_NOTE}"`;

  it("ends with the note and a line break", () => {
    const csv = schedulesCsv(buildSchedules(DOC, CATALOG));
    expect(SCHEDULE_NOTE).toBe("Counts from the model. Circuits, loads and ratings are for the licensed engineer.");
    expect(lines).toContain(noteCell);
    expect(csv.endsWith(`${noteCell}\r\n`)).toBe(true);
  });

  it("writes only the note when there is nothing to count", () => {
    expect(schedulesCsv(buildSchedules(makeDoc([], []), CATALOG))).toBe(`${noteCell}\r\n`);
  });
});
