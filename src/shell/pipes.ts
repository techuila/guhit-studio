// Pipe systems, size menus and small derived readings the shell shows: tool
// defaults, drainage falls, review ordering, the take-off as CSV. Pure data,
// no React. The engine stays the authority (docs/CONTRACT.md, "Pipes"): the
// defaults below mirror `defaults::pipe_defaults` and `drain_min_slope_pct`.
//
// Guhit coordinates pipes. It never sizes them: the size menus are the usual
// nominal sizes to draw with, not a recommendation.
import type { Issue, LayerKey, Pipe, PipeMaterial, PipeNetwork, PipeSystem, PipeTakeoffRow, Vec3 } from "../contract/bindings";
import {
  PIPE_COLOR_VAR,
  PIPE_DEFAULTS,
  PIPE_GROUP,
  PIPE_LAYER,
  PIPE_MATERIAL_LABEL,
  PIPE_SIZES,
  PIPE_SYSTEM_LABEL,
  PIPE_SYSTEM_ORDER,
  SERVICE_LAYER_ORDER,
  drainMinSlopePct,
  isPipeLayer,
  isServiceLayer,
  pipeFalls,
  type ServiceGroup,
} from "../contract/pipes";

export { PIPE_MATERIAL_LABEL, PIPE_SIZES, PIPE_SYSTEM_LABEL, SERVICE_LAYER_ORDER, drainMinSlopePct, isPipeLayer, isServiceLayer, pipeFalls };
export type { SizeGroup } from "../contract/pipes";

export interface PipeSystemDef {
  value: PipeSystem;
  label: string;
  /** CSS color from tokens.css, shared by the plan, the 3D view and legends. */
  color: string;
}

export const PIPE_COLOR = Object.fromEntries(
  PIPE_SYSTEM_ORDER.map((s) => [s, `var(${PIPE_COLOR_VAR[s]})`]),
) as Record<PipeSystem, string>;

export const PIPE_SYSTEMS: PipeSystemDef[] = PIPE_SYSTEM_ORDER.map((value) => ({
  value,
  label: PIPE_SYSTEM_LABEL[value],
  color: PIPE_COLOR[value],
}));

export interface ServiceTrade {
  group: ServiceGroup;
  label: string;
  /** Who sizes and signs these runs, as the flyout and notes say it. */
  pro: string;
  systems: PipeSystemDef[];
}

/** The runs grouped by trade, for the services flyout and the notes. */
export const SERVICE_TRADES: ServiceTrade[] = (
  [
    ["plumbing", "Plumbing", "a registered Master Plumber"],
    ["electrical", "Electrical", "the Professional Electrical Engineer"],
    ["aircon", "Aircon", "the Professional Mechanical Engineer"],
  ] as Array<[ServiceGroup, string, string]>
).map(([group, label, pro]) => ({ group, label, pro, systems: PIPE_SYSTEMS.filter((sys) => PIPE_GROUP[sys.value] === group) }));

export function tradeOf(system: PipeSystem): ServiceTrade {
  return SERVICE_TRADES.find((t) => t.group === PIPE_GROUP[system]) ?? SERVICE_TRADES[0];
}

/** What to call one run: "Cold water pipe", "Conduit", "Refrigerant line set". */
export function runLabel(system: PipeSystem): string {
  return PIPE_GROUP[system] === "plumbing" ? `${PIPE_SYSTEM_LABEL[system]} pipe` : PIPE_SYSTEM_LABEL[system];
}

/** Layer swatches: each plumbing layer in its system's color, electrical in the conduit's, aircon in the line set's. */
export const SERVICE_LAYER_COLOR: Partial<Record<LayerKey, string>> = {
  cold_water: PIPE_COLOR.cold_water,
  hot_water: PIPE_COLOR.hot_water,
  drainage: PIPE_COLOR.drainage,
  vent: PIPE_COLOR.vent,
  storm: PIPE_COLOR.storm,
  electrical: PIPE_COLOR.conduit,
  aircon: PIPE_COLOR.refrigerant,
};

/** Length in meters of the runs on each service layer, from the take-off rows. */
export function lengthByLayer(rows: PipeTakeoffRow[]): Partial<Record<LayerKey, number>> {
  const out: Partial<Record<LayerKey, number>> = {};
  for (const r of rows) out[PIPE_LAYER[r.system]] = (out[PIPE_LAYER[r.system]] ?? 0) + r.length_m;
  return out;
}

export interface PipeDefaults {
  material: PipeMaterial;
  diameterMm: number;
  /** Height of a new run above the level floor. Negative is below the slab. */
  elevationMm: number;
}

/** The tool defaults of a system (src/contract/pipes.ts). */
export function pipeDefaults(system: PipeSystem): PipeDefaults {
  const d = PIPE_DEFAULTS[system];
  return { material: d.material, diameterMm: d.diameterMm, elevationMm: d.startHeightMm };
}

/** "PPR 20". */
export function sizeShort(material: PipeMaterial, diameterMm: number): string {
  return `${PIPE_MATERIAL_LABEL[material]} ${formatDiameter(diameterMm)}`;
}

/** "PPR 20 mm". */
export function sizeLabel(material: PipeMaterial, diameterMm: number): string {
  return `${sizeShort(material, diameterMm)} mm`;
}

/** "20", "12.7", "9.52": line set sizes are 3/8, 1/2 and 5/8 inch in mm. */
export function formatDiameter(diameterMm: number): string {
  return String(Math.round(diameterMm * 100) / 100);
}

export function materialsFor(system: PipeSystem): PipeMaterial[] {
  return PIPE_SIZES[system].map((g) => g.material);
}

export function sizesFor(system: PipeSystem, material: PipeMaterial): number[] {
  return PIPE_SIZES[system].find((g) => g.material === material)?.sizes ?? [];
}

export function inMenu(system: PipeSystem, material: PipeMaterial, diameterMm: number): boolean {
  return sizesFor(system, material).some((d) => Math.abs(d - diameterMm) < 1e-6);
}

/** The size in `sizes` nearest to `diameterMm`, the smaller one on a tie. */
export function closestSize(sizes: number[], diameterMm: number): number {
  let best = sizes[0] ?? diameterMm;
  for (const d of sizes) if (Math.abs(d - diameterMm) < Math.abs(best - diameterMm)) best = d;
  return best;
}

// ---------------------------------------------------------------- tool options

export interface PipeToolOptions {
  pipeSystem: PipeSystem;
  pipeMaterial: PipeMaterial | null;
  pipeDiameterMm: number | null;
  pipeElevationMm: number | null;
}

export interface PipeToolSettings {
  system: PipeSystem;
  material: PipeMaterial;
  diameterMm: number;
  elevationMm: number;
  /** True when the size comes from the system default (the options are null). */
  sizeIsDefault: boolean;
  elevationIsDefault: boolean;
}

/**
 * What the pipe tool draws with: the chosen options, or the system defaults
 * while they are null. A material and size that do not belong to the system
 * (left over from another system) fall back to the default.
 */
export function pipeToolSettings(o: PipeToolOptions): PipeToolSettings {
  const d = pipeDefaults(o.pipeSystem);
  const material = o.pipeMaterial ?? d.material;
  const diameter = o.pipeDiameterMm ?? d.diameterMm;
  const valid = inMenu(o.pipeSystem, material, diameter);
  return {
    system: o.pipeSystem,
    material: valid ? material : d.material,
    diameterMm: valid ? diameter : d.diameterMm,
    elevationMm: o.pipeElevationMm ?? d.elevationMm,
    sizeIsDefault: o.pipeMaterial === null && o.pipeDiameterMm === null,
    elevationIsDefault: o.pipeElevationMm === null,
  };
}

/**
 * Tool options after picking another system. A chosen size that is also on
 * the new system's menu is kept, otherwise the new system's default applies.
 * A chosen start height is kept.
 */
export function switchToolSystem(o: PipeToolOptions, system: PipeSystem): PipeToolOptions {
  const keep = o.pipeMaterial !== null && o.pipeDiameterMm !== null && inMenu(system, o.pipeMaterial, o.pipeDiameterMm);
  return {
    pipeSystem: system,
    pipeMaterial: keep ? o.pipeMaterial : null,
    pipeDiameterMm: keep ? o.pipeDiameterMm : null,
    pipeElevationMm: o.pipeElevationMm,
  };
}

// ---------------------------------------------------------------- one pipe

/**
 * A pipe moved to another system. Its material and size stay when the new
 * system's menu has them; a material the new menu has keeps the nearest size;
 * anything else takes the new system's default material and size.
 */
export function withSystem<P extends Pipe>(pipe: P, system: PipeSystem): P {
  if (inMenu(system, pipe.material, pipe.diameter_mm)) return { ...pipe, system };
  const d = pipeDefaults(system);
  const sameMaterial = materialsFor(system).includes(pipe.material);
  if (sameMaterial) return { ...pipe, system, diameter_mm: closestSize(sizesFor(system, pipe.material), pipe.diameter_mm) };
  return { ...pipe, system, material: d.material, diameter_mm: d.diameterMm };
}

/** A pipe given another material: the nearest size that material comes in. */
export function withMaterial<P extends Pipe>(pipe: P, material: PipeMaterial): P {
  const sizes = sizesFor(pipe.system, material);
  return { ...pipe, material, diameter_mm: sizes.length > 0 ? closestSize(sizes, pipe.diameter_mm) : pipe.diameter_mm };
}

/** Drainage flows from the first point to the last. Reversing flips the flow. */
export function reversed<P extends Pipe>(pipe: P): P {
  return { ...pipe, points: [...pipe.points].reverse() };
}

function dist3(a: Vec3, b: Vec3): number {
  return Math.hypot(b.x - a.x, b.y - a.y, b.z - a.z);
}

/** Centerline length in mm, along every segment in 3D. */
export function pipeLength(points: Vec3[]): number {
  let total = 0;
  for (let i = 1; i < points.length; i++) total += dist3(points[i - 1], points[i]);
  return total;
}

/** The point halfway along the centerline. */
export function midPoint(points: Vec3[]): Vec3 | null {
  if (points.length === 0) return null;
  const half = pipeLength(points) / 2;
  let run = 0;
  for (let i = 1; i < points.length; i++) {
    const a = points[i - 1];
    const b = points[i];
    const len = dist3(a, b);
    if (run + len >= half && len > 0) {
      const t = (half - run) / len;
      return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, z: a.z + (b.z - a.z) * t };
    }
    run += len;
  }
  return { ...points[0] };
}

/** Drainage segments shorter than this are not judged for fall (docs/CONTRACT.md, `drain_slope_low`). */
export const FALL_MIN_SEGMENT_MM = 300;
/** Rounding noise, in percent, allowed below the default fall. Same as the engine's check. */
const FALL_EPS_PCT = 1e-9;

export interface SegmentFall {
  /** Segment from point `index` to point `index + 1`. */
  index: number;
  lengthMm: number;
  horizontalMm: number;
  /** Positive when the segment goes down in the flow direction. */
  dropMm: number;
  /** Fall in percent of the horizontal run. Null for a vertical segment (plan length under 1 mm). */
  pct: number | null;
  /** Steeper than 45 degrees: a drop, not a sloped run. */
  steep: boolean;
  /** Flatter than 45 degrees, at least 300 mm long, and falls less than the default or runs uphill. */
  low: boolean;
}

/** Fall of each segment of a run, in the flow direction (first point to last). Drainage, storm and condensate fall. */
export function segmentFalls(pipe: Pipe): SegmentFall[] {
  const min = drainMinSlopePct(pipe.diameter_mm);
  const out: SegmentFall[] = [];
  for (let i = 1; i < pipe.points.length; i++) {
    const a = pipe.points[i - 1];
    const b = pipe.points[i];
    const horizontalMm = Math.hypot(b.x - a.x, b.y - a.y);
    const dropMm = a.z - b.z;
    const lengthMm = Math.hypot(horizontalMm, dropMm);
    const steep = Math.abs(dropMm) >= horizontalMm;
    const pct = horizontalMm < 1 ? null : (dropMm / horizontalMm) * 100;
    const low = !steep && pct !== null && lengthMm >= FALL_MIN_SEGMENT_MM && pct < min - FALL_EPS_PCT;
    out.push({ index: i - 1, lengthMm, horizontalMm, dropMm, pct, steep, low });
  }
  return out;
}

// ---------------------------------------------------------------- review

/** Review codes the pipe checks produce (docs/CONTRACT.md, "Pipes"). */
export const PIPE_ISSUE_CODES = new Set([
  "pipe_through_column",
  "pipe_across_opening",
  "pipes_cross",
  "drain_slope_low",
  "pipe_penetrations",
  "condensate_slope_low",
  "condensate_open_end",
  "lineset_long",
  "lineset_rise",
  "lineset_short",
  "lineset_extra",
]);

/** The one pipe item that is a summary, shown as a note. */
export const PENETRATION_SUMMARY = "pipe_penetrations";

export function isPipeIssue(issue: Issue): boolean {
  return PIPE_ISSUE_CODES.has(issue.code);
}

const SEVERITY_RANK: Record<Issue["severity"], number> = { error: 0, warning: 1, info: 2 };

/**
 * The review list in reading order: errors, then warnings, then the rest,
 * keeping the engine's order inside each. The penetration summary is not a
 * finding, so it goes to `notes`, shown after the list.
 */
export function orderIssues(issues: Issue[]): { items: Issue[]; notes: Issue[] } {
  const items = issues
    .map((issue, i) => ({ issue, i }))
    .filter(({ issue }) => issue.code !== PENETRATION_SUMMARY)
    .sort((a, b) => SEVERITY_RANK[a.issue.severity] - SEVERITY_RANK[b.issue.severity] || a.i - b.i)
    .map(({ issue }) => issue);
  const notes = issues.filter((issue) => issue.code === PENETRATION_SUMMARY);
  return { items, notes };
}

// ---------------------------------------------------------------- take-off

/** Shown under a plumbing take-off and written into the CSV. */
export const TAKEOFF_NOTE = "Centerline lengths from the model. Sizing is for a registered Master Plumber.";

function listWords(words: string[]): string {
  return words.length <= 1 ? (words[0] ?? "") : `${words.slice(0, -1).join(", ")} and ${words[words.length - 1]}`;
}

/** The take-off note naming who sizes the runs it lists: the Master Plumber, the PEE, the PME. */
export function takeoffNote(rows: PipeTakeoffRow[]): string {
  const trades = SERVICE_TRADES.filter((t) => rows.some((r) => PIPE_GROUP[r.system] === t.group));
  if (trades.length === 0 || (trades.length === 1 && trades[0].group === "plumbing")) return TAKEOFF_NOTE;
  return `Centerline lengths from the model. Sizing is for ${listWords(trades.map((t) => t.pro))}.`;
}

/** "Plumbing" while every run is plumbing, else "Services". */
export function takeoffTitle(rows: PipeTakeoffRow[]): string {
  return rows.every((r) => PIPE_GROUP[r.system] === "plumbing") ? "Plumbing" : "Services";
}

export const EMPTY_NETWORK: PipeNetwork = {
  fittings: [],
  penetrations: [],
  takeoff: [],
  total_length_m: 0,
  elbow_count: 0,
  tee_count: 0,
  sleeve_count: 0,
};

/** Length in meters per system, from the take-off rows. */
export function lengthBySystem(rows: PipeTakeoffRow[]): Record<PipeSystem, number> {
  const out = Object.fromEntries(PIPE_SYSTEM_ORDER.map((s) => [s, 0])) as Record<PipeSystem, number>;
  for (const r of rows) out[r.system] += r.length_m;
  return out;
}

export function runCount(rows: PipeTakeoffRow[]): number {
  return rows.reduce((sum, r) => sum + r.run_count, 0);
}

/** "12 elbows, 3 tees, 8 sleeves or flashings". */
export function fittingsLine(net: PipeNetwork): string {
  const n = (count: number, one: string, many: string) => `${count} ${count === 1 ? one : many}`;
  return [n(net.elbow_count, "elbow", "elbows"), n(net.tee_count, "tee", "tees"), n(net.sleeve_count, "sleeve or flashing", "sleeves or flashings")].join(", ");
}

function csvCell(value: string | number): string {
  const text = String(value);
  return /[",\n\r]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

function csvRow(cells: Array<string | number>): string {
  return cells.map(csvCell).join(",");
}

/** The take-off as CSV: one row per system, material and size, totals, fittings and the note. */
export function takeoffCsv(net: PipeNetwork): string {
  const lines = [csvRow(["System", "Material", "Size (mm)", "Length (m)", "Runs"])];
  for (const r of net.takeoff) {
    lines.push(csvRow([PIPE_SYSTEM_LABEL[r.system], PIPE_MATERIAL_LABEL[r.material], formatDiameter(r.diameter_mm), r.length_m.toFixed(3), r.run_count]));
  }
  lines.push(csvRow([takeoffTitle(net.takeoff) === "Plumbing" ? "All pipe" : "All runs", "", "", net.total_length_m.toFixed(3), runCount(net.takeoff)]));
  lines.push("");
  lines.push(csvRow(["Elbows", net.elbow_count]));
  lines.push(csvRow(["Tees", net.tee_count]));
  lines.push(csvRow(["Sleeves or flashings", net.sleeve_count]));
  lines.push("");
  lines.push(csvRow([takeoffNote(net.takeoff)]));
  return `${lines.join("\r\n")}\r\n`;
}
