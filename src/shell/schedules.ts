// Device schedules from `Derived::schedule` (counts per level and room,
// computed by the engine) and the fixtures in the model: the rows of the PH
// electrical inspection form, a light fixture table and the aircon units.
// Counts only. Circuits, loads and ratings are for the licensed engineer.
// Pure data, no React.
import type { CatalogItem, DeviceKind, DocState, Level } from "../contract/bindings";
import { AIRCON_KINDS, AIRCON_ROLE_LABEL, DEVICE_LABEL, FORM_ROWS, catalogItem, type AssetEl } from "./devices";

type Doc = Pick<DocState, "project" | "derived">;

export const SCHEDULE_NOTE = "Counts from the model. Circuits, loads and ratings are for the licensed engineer.";
export const OUTSIDE_ROOMS = "Outside the rooms";

export interface FormRow {
  kind: DeviceKind;
  label: string;
  total: number;
  /** Count per level, in `Schedules.levels` order. */
  perLevel: number[];
}

export interface RoomCounts {
  key: string;
  levelName: string;
  roomName: string;
  counts: Array<{ kind: DeviceKind; count: number }>;
}

export interface FixtureRow {
  key: string;
  name: string;
  count: number;
  lumens: number;
  kelvin: number;
  /** Plug-in lamps give light but are not lighting outlets. */
  plugIn: boolean;
}

export interface AirconRow {
  key: string;
  levelName: string;
  roomName: string;
  name: string;
  role: string;
  hp: number;
  count: number;
}

export interface Schedules {
  levels: Level[];
  form: FormRow[];
  rooms: RoomCounts[];
  fixtures: FixtureRow[];
  aircon: AirconRow[];
  /** Indoor and window units: the ones that cool a room. */
  coolingUnits: number;
  coolingHp: number;
}

/** Objects that belong in the schedules: devices with a form row or an aircon role, and fixtures that give light. */
export function hasScheduledDevices(doc: Doc, catalog: CatalogItem[]): boolean {
  return doc.project.elements.some((e) => e.kind === "asset" && (e.light !== null || !!catalogItem(catalog, e.catalog_key)?.device));
}

export function buildSchedules(doc: Doc, catalog: CatalogItem[]): Schedules {
  const levels = doc.project.levels;
  const levelIndex = new Map(levels.map((l, i) => [l.id, i]));
  const levelName = (id: string) => levels.find((l) => l.id === id)?.name ?? "Level";
  const roomName = (id: string | null) => {
    if (!id) return OUTSIDE_ROOMS;
    const el = doc.project.elements.find((e) => e.id === id);
    return el?.kind === "room" && el.name.trim() ? el.name : "Room";
  };
  const rows = doc.derived.schedule ?? [];

  // Inspection form rows: totals and per level.
  const form: FormRow[] = FORM_ROWS.map((kind) => {
    const perLevel = levels.map(() => 0);
    let total = 0;
    for (const r of rows) {
      if (r.device !== kind) continue;
      total += r.count;
      const i = levelIndex.get(r.level_id);
      if (i !== undefined) perLevel[i] += r.count;
    }
    return { kind, label: DEVICE_LABEL[kind][1], total, perLevel };
  }).filter((r) => r.total > 0);

  // The same rows per room, rooms in level order then by name, outside last.
  const roomMap = new Map<string, { levelId: string; roomId: string | null; counts: Map<DeviceKind, number> }>();
  for (const r of rows) {
    if (!r.device || !FORM_ROWS.includes(r.device)) continue;
    const key = `${r.level_id}/${r.room_id ?? ""}`;
    let room = roomMap.get(key);
    if (!room) roomMap.set(key, (room = { levelId: r.level_id, roomId: r.room_id, counts: new Map() }));
    room.counts.set(r.device, (room.counts.get(r.device) ?? 0) + r.count);
  }
  const rooms: RoomCounts[] = [...roomMap.entries()]
    .map(([key, r]) => ({
      key,
      levelId: r.levelId,
      outside: r.roomId === null,
      levelName: levelName(r.levelId),
      roomName: roomName(r.roomId),
      counts: FORM_ROWS.filter((k) => r.counts.has(k)).map((kind) => ({ kind, count: r.counts.get(kind) ?? 0 })),
    }))
    .sort(
      (a, b) =>
        (levelIndex.get(a.levelId) ?? 0) - (levelIndex.get(b.levelId) ?? 0) ||
        Number(a.outside) - Number(b.outside) ||
        a.roomName.localeCompare(b.roomName),
    )
    .map(({ key, levelName: ln, roomName: rn, counts }) => ({ key, levelName: ln, roomName: rn, counts }));

  // Light fixtures from the model: one row per type, lumens and color.
  const fixtureMap = new Map<string, FixtureRow>();
  for (const e of doc.project.elements) {
    if (e.kind !== "asset" || !e.light) continue;
    const a = e as AssetEl;
    const item = catalogItem(catalog, a.catalog_key);
    const key = `${a.catalog_key}/${a.light!.lumens}/${a.light!.kelvin}`;
    const row = fixtureMap.get(key);
    if (row) row.count += 1;
    else
      fixtureMap.set(key, {
        key,
        name: item?.name ?? a.name,
        count: 1,
        lumens: a.light!.lumens,
        kelvin: a.light!.kelvin,
        plugIn: item ? item.device !== "lighting_outlet" : false,
      });
  }
  const fixtures = [...fixtureMap.values()].sort((a, b) => Number(a.plugIn) - Number(b.plugIn) || a.name.localeCompare(b.name) || a.lumens - b.lumens);

  // Aircon units per room, with the capacity from the catalog.
  const units: Array<{ row: AirconRow; level: number; outside: boolean; roleRank: number }> = [];
  let coolingUnits = 0;
  let coolingHp = 0;
  for (const r of rows) {
    if (!r.device || !AIRCON_KINDS.includes(r.device)) continue;
    const item = catalogItem(catalog, r.catalog_key);
    const hp = item?.aircon?.hp ?? 0;
    const role = item?.aircon?.role ?? (r.device === "aircon_outdoor" ? "outdoor" : r.device === "aircon_window" ? "window" : "indoor");
    units.push({
      row: {
        key: `${r.level_id}/${r.room_id ?? ""}/${r.catalog_key}`,
        levelName: levelName(r.level_id),
        roomName: roomName(r.room_id),
        name: item?.name ?? r.catalog_key,
        role: AIRCON_ROLE_LABEL[role],
        hp,
        count: r.count,
      },
      level: levelIndex.get(r.level_id) ?? 0,
      outside: r.room_id === null,
      roleRank: role === "outdoor" ? 1 : 0,
    });
    if (role !== "outdoor") {
      coolingUnits += r.count;
      coolingHp += hp * r.count;
    }
  }
  const aircon = units
    .sort((a, b) => a.level - b.level || Number(a.outside) - Number(b.outside) || a.row.roomName.localeCompare(b.row.roomName) || a.roleRank - b.roleRank || a.row.name.localeCompare(b.row.name))
    .map((u) => u.row);

  return { levels, form, rooms, fixtures, aircon, coolingUnits, coolingHp };
}

/** "4 lighting outlets, 3 receptacles" for one room. */
export function roomCountsLine(counts: RoomCounts["counts"]): string {
  return counts.map(({ kind, count }) => `${count} ${SHORT[kind][count === 1 ? 0 : 1]}`).join(", ");
}

const SHORT: Record<DeviceKind, [string, string]> = {
  lighting_outlet: ["lighting outlet", "lighting outlets"],
  convenience_receptacle: ["receptacle", "receptacles"],
  special_purpose_outlet: ["SPO", "SPOs"],
  switch: ["switch", "switches"],
  panelboard: ["panelboard", "panelboards"],
  smoke_detector: ["smoke detector", "smoke detectors"],
  buzzer: ["buzzer", "buzzers"],
  push_button: ["push button", "push buttons"],
  aircon_indoor: ["indoor unit", "indoor units"],
  aircon_outdoor: ["outdoor unit", "outdoor units"],
  aircon_window: ["window aircon", "window aircons"],
};

function csvCell(value: string | number): string {
  const text = String(value);
  return /[",\n\r]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

function csvRow(cells: Array<string | number>): string {
  return cells.map(csvCell).join(",");
}

const round2 = (n: number) => String(Math.round(n * 100) / 100);

/** The schedules as CSV: form rows per room and in total, fixtures, aircon units, then the note. */
export function schedulesCsv(s: Schedules): string {
  const lines: string[] = [];
  if (s.form.length > 0) {
    lines.push(csvRow(["Electrical devices"]));
    lines.push(csvRow(["Level", "Room", "Row", "Count"]));
    for (const room of s.rooms) {
      for (const c of room.counts) lines.push(csvRow([room.levelName, room.roomName, DEVICE_LABEL[c.kind][1], c.count]));
    }
    for (const row of s.form) lines.push(csvRow(["All levels", "All rooms", row.label, row.total]));
    lines.push("");
  }
  if (s.fixtures.length > 0) {
    lines.push(csvRow(["Light fixtures"]));
    lines.push(csvRow(["Type", "Count", "Lumens each", "Color (K)", "Plug-in"]));
    for (const f of s.fixtures) lines.push(csvRow([f.name, f.count, round2(f.lumens), round2(f.kelvin), f.plugIn ? "yes" : "no"]));
    lines.push("");
  }
  if (s.aircon.length > 0) {
    lines.push(csvRow(["Aircon units"]));
    lines.push(csvRow(["Level", "Room", "Unit", "Role", "HP each", "Count"]));
    for (const a of s.aircon) lines.push(csvRow([a.levelName, a.roomName, a.name, a.role, round2(a.hp), a.count]));
    lines.push(csvRow(["Cooling units", "", "", "", round2(s.coolingHp), s.coolingUnits]));
    lines.push("");
  }
  lines.push(csvRow([SCHEDULE_NOTE]));
  return `${lines.join("\r\n")}\r\n`;
}
