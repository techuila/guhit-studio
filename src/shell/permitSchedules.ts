// The permit schedules in the project inspector (lane L1): doors and
// windows by mark with counts per level, rooms with their net areas,
// ceilings and finishes, and each level's areas. Everything comes from
// `Derived` (`opening_schedule`, `room_schedule`, `level_areas`, `totals`):
// the inspector never counts on its own, so its totals are the project's.
// Pure, tested in permitSchedules.test.ts.
import type { DocState, OpeningStyle, OpeningType } from "../contract/bindings";

const STYLE: Record<OpeningStyle, string> = {
  swing_single: "Single swing",
  swing_double: "Double swing",
  sliding: "Sliding",
  fixed: "Fixed glass",
  casement: "Casement",
  jalousie: "Jalousie",
};

export interface LevelRef {
  id: string;
  name: string;
}

export interface OpeningLine {
  key: string;
  /** "D1"; empty for untyped openings. */
  mark: string;
  kind: OpeningType;
  /** "900 x 2100", mm. */
  size: string;
  /** The type's description, else its style. */
  what: string;
  /** Count per level, in level order. */
  perLevel: number[];
  total: number;
}

export interface RoomLine {
  key: string;
  level: string;
  name: string;
  netM2: number;
  ceilingMm: number;
  /** "Floor tiles, walls paint" and so on: the finishes that are set. */
  finishes: string;
  remarks: string;
}

export interface LevelLine {
  key: string;
  name: string;
  net: number;
  gross: number;
  open: number;
  void: number;
}

export interface PermitSchedules {
  levels: LevelRef[];
  openings: OpeningLine[];
  rooms: RoomLine[];
  levelAreas: LevelLine[];
  /** `Derived::totals`: the numbers every schedule adds up to. */
  totals: { doors: number; windows: number; floorM2: number; grossM2: number; openM2: number; voidM2: number };
  /** The project has doors, windows or rooms: the schedules show. */
  any: boolean;
  /** The engine has counted them (Derived is not empty yet while it is not). */
  counted: boolean;
}

export function buildPermitSchedules(doc: DocState): PermitSchedules {
  const p = doc.project;
  const d = doc.derived;
  const levels = p.levels.map((l) => ({ id: l.id, name: l.name }));
  const levelName = new Map(levels.map((l) => [l.id, l.name] as const));
  const materials = new Map(p.materials.map((m) => [m.id, m.name] as const));
  const types = new Map((p.opening_types ?? []).map((t) => [t.id, t] as const));
  const openings: OpeningLine[] = (d.opening_schedule?.rows ?? []).map((r, i) => {
    const type = r.type_id ? types.get(r.type_id) : undefined;
    const per = new Map(r.per_level.map((c) => [c.level_id, c.count] as const));
    return {
      key: r.type_id ?? `untyped-${i}`,
      mark: r.mark,
      kind: r.opening_type,
      size: `${Math.round(r.width_mm)} x ${Math.round(r.height_mm)}`,
      what: (type?.description || r.description || STYLE[r.style]).trim(),
      perLevel: levels.map((l) => per.get(l.id) ?? 0),
      total: r.total,
    };
  });
  const mat = (id: string | null | undefined): string | null => (id ? (materials.get(id) ?? null) : null);
  const rooms: RoomLine[] = (d.room_schedule ?? []).map((r) => {
    const parts: string[] = [];
    const f = r.finishes;
    const add = (label: string, id: string | null | undefined) => {
      const name = mat(id);
      if (name) parts.push(`${label} ${name}`);
    };
    add("Floor", r.floor_material_id);
    add("walls", f.wall_material_id);
    add("ceiling", f.ceiling_material_id);
    add("base", f.base_material_id);
    const finishes = parts.join(", ");
    return {
      key: r.room_id,
      level: levelName.get(r.level_id) ?? "",
      name: r.name,
      // What the schedules count: voids, balconies and decks off, so the rows add up to the floor area total.
      netM2: r.floor_area_m2,
      ceilingMm: r.ceiling_height_mm,
      finishes: finishes ? finishes[0].toUpperCase() + finishes.slice(1) : "",
      remarks: f.remarks,
    };
  });
  const levelAreas: LevelLine[] = (d.level_areas ?? []).map((a) => ({
    key: a.level_id,
    name: levelName.get(a.level_id) ?? "",
    net: a.net_m2,
    gross: a.gross_m2,
    open: a.open_m2,
    void: a.void_m2,
  }));
  const any = p.elements.some((e) => e.kind === "opening" || e.kind === "room");
  return {
    levels,
    openings,
    rooms,
    levelAreas,
    totals: {
      doors: d.totals.door_count,
      windows: d.totals.window_count,
      floorM2: d.totals.floor_area_m2,
      grossM2: d.totals.gross_area_m2,
      openM2: levelAreas.reduce((s, l) => s + l.open, 0),
      voidM2: levelAreas.reduce((s, l) => s + l.void, 0),
    },
    any,
    counted: openings.length > 0 || rooms.length > 0 || levelAreas.length > 0,
  };
}

const m2 = (v: number): string => v.toFixed(2);

function csvCell(v: string | number): string {
  const t = String(v);
  return /[",\n]/.test(t) ? `"${t.replace(/"/g, '""')}"` : t;
}

/** The three schedules as CSV, one table after another. */
export function permitSchedulesCsv(s: PermitSchedules): string {
  const lines: string[] = [];
  const row = (...cells: Array<string | number>) => lines.push(cells.map(csvCell).join(","));
  row("Door and window schedule");
  row("Mark", "Kind", "Size (mm)", "Description", ...s.levels.map((l) => l.name), "Total");
  for (const o of s.openings) row(o.mark || "Untyped", o.kind, o.size, o.what, ...o.perLevel, o.total);
  row("All doors", "", "", "", ...s.levels.map(() => ""), s.totals.doors);
  row("All windows", "", "", "", ...s.levels.map(() => ""), s.totals.windows);
  lines.push("");
  row("Room schedule");
  row("Level", "Room", "Net area (m2)", "Ceiling (mm)", "Finishes", "Remarks");
  for (const r of s.rooms) row(r.level, r.name, m2(r.netM2), Math.round(r.ceilingMm), r.finishes, r.remarks);
  row("Floor area", "", m2(s.totals.floorM2));
  lines.push("");
  row("Level areas");
  row("Level", "Net (m2)", "Gross (m2)", "Open (m2)", "Void (m2)");
  for (const l of s.levelAreas) row(l.name, m2(l.net), m2(l.gross), m2(l.open), m2(l.void));
  row("Total", m2(s.totals.floorM2), m2(s.totals.grossM2), m2(s.totals.openM2), m2(s.totals.voidM2));
  return lines.join("\n");
}
