// Service run data shared by the plan, the 3D view and the shell: plumbing,
// storm drainage, electrical conduit and aircon lines all use the pipe model.
// CONTRACT FILE - owned by the orchestrator.
//
// Mirrors guhit-model `defaults::pipe_defaults`, `defaults::drain_min_slope_pct`
// and the size table in docs/CONTRACT.md ("Pipes"). The engine stays the
// authority. These are drawing defaults and menus of the usual nominal sizes,
// never a sizing recommendation: plumbing design is for a registered Master
// Plumber (DECISIONS D19).

import type { LayerKey, PipeMaterial, PipeSystem } from "./bindings";

/** Display order everywhere: tool options, layers, legends, the take-off. */
export const PIPE_SYSTEM_ORDER: PipeSystem[] = [
  "cold_water",
  "hot_water",
  "drainage",
  "vent",
  "storm",
  "conduit",
  "refrigerant",
  "condensate",
];

export type ServiceGroup = "plumbing" | "electrical" | "aircon";

/** Which trade a run belongs to, for tool menus and the licensed professional
 * who signs it: Master Plumber, PEE, PME. */
export const PIPE_GROUP: Record<PipeSystem, ServiceGroup> = {
  cold_water: "plumbing",
  hot_water: "plumbing",
  drainage: "plumbing",
  vent: "plumbing",
  storm: "plumbing",
  conduit: "electrical",
  refrigerant: "aircon",
  condensate: "aircon",
};

export const PIPE_SYSTEM_LABEL: Record<PipeSystem, string> = {
  cold_water: "Cold water",
  hot_water: "Hot water",
  drainage: "Drainage",
  vent: "Vent",
  storm: "Storm drain",
  conduit: "Conduit",
  refrigerant: "Refrigerant line set",
  condensate: "Condensate drain",
};

/** The layer each run is on (Rust `PipeSystem::layer`). */
export const PIPE_LAYER: Record<PipeSystem, LayerKey> = {
  cold_water: "cold_water",
  hot_water: "hot_water",
  drainage: "drainage",
  vent: "vent",
  storm: "storm",
  conduit: "electrical",
  refrigerant: "aircon",
  condensate: "aircon",
};

/** Runs whose flow must fall, first point to last (Rust `PipeSystem::falls`). */
export function pipeFalls(system: PipeSystem): boolean {
  return system === "drainage" || system === "storm" || system === "condensate";
}

/** The CSS variables in src/styles/tokens.css. */
export const PIPE_COLOR_VAR: Record<PipeSystem, string> = {
  cold_water: "--pipe-cold",
  hot_water: "--pipe-hot",
  drainage: "--pipe-drain",
  vent: "--pipe-vent",
  storm: "--pipe-storm",
  conduit: "--pipe-conduit",
  refrigerant: "--pipe-refrigerant",
  condensate: "--pipe-condensate",
};

/** The same colors as hex, for code that cannot read CSS variables. Keep in
 * step with tokens.css and crates/guhit-export/src/pipes.rs. */
export const PIPE_COLOR_HEX: Record<PipeSystem, string> = {
  cold_water: "#2b7bd0",
  hot_water: "#e0563a",
  drainage: "#9b6a35",
  vent: "#3a9a5c",
  storm: "#6f7782",
  conduit: "#d49a1a",
  refrigerant: "#b0428f",
  condensate: "#6c8fb3",
};

export const PIPE_MATERIAL_LABEL: Record<PipeMaterial, string> = {
  ppr: "PPR",
  upvc: "uPVC",
  gi: "GI",
  pe: "PE",
  copper: "Copper",
  pvc: "PVC",
  emt: "EMT",
  imc: "IMC",
  flexible: "Flexible",
};

export interface PipeDefaults {
  material: PipeMaterial;
  diameterMm: number;
  /** Height above the level floor a new run starts at. Negative is below the slab. */
  startHeightMm: number;
}

/** Mirror of `defaults::pipe_defaults`. */
export const PIPE_DEFAULTS: Record<PipeSystem, PipeDefaults> = {
  cold_water: { material: "ppr", diameterMm: 20, startHeightMm: 300 },
  hot_water: { material: "ppr", diameterMm: 20, startHeightMm: 300 },
  drainage: { material: "upvc", diameterMm: 50, startHeightMm: -300 },
  vent: { material: "upvc", diameterMm: 50, startHeightMm: 300 },
  storm: { material: "upvc", diameterMm: 100, startHeightMm: -300 },
  // A one storey level of the default height; `pipeStartHeightMm` for a real level.
  conduit: { material: "pvc", diameterMm: 20, startHeightMm: 3100 },
  refrigerant: { material: "copper", diameterMm: 9.52, startHeightMm: 2400 },
  condensate: { material: "pvc", diameterMm: 20, startHeightMm: 2300 },
};

/** Mirror of `defaults::CONDUIT_ABOVE_CEILING_MM`: a new conduit run starts
 * this far above its level's ceiling (DECISIONS D40). */
export const CONDUIT_ABOVE_CEILING_MM = 100;

/** Slab under an upper level and the default level height, as
 * `ceilingHeightMm` in src/editor2d/mount.ts uses them. */
const UPPER_SLAB_MM = 200;
const DEFAULT_LEVEL_HEIGHT_MM = 3000;

/**
 * Height above the floor of `level` a new run of `system` starts at (Rust
 * `defaults::pipe_start_height_mm`). Conduit starts 100 mm above the
 * level's ceiling, which is its height or the underside of the next level's
 * slab when that is lower (the rule of `ceilingHeightMm` in
 * src/editor2d/mount.ts): inside the slab above, or above the ceiling on the
 * top level. Every other system starts at its `PIPE_DEFAULTS` height.
 */
export function pipeStartHeightMm(
  system: PipeSystem,
  level: { elevation_mm: number; height_mm: number } | undefined,
  levels: readonly { elevation_mm: number }[],
): number {
  if (system !== "conduit") return PIPE_DEFAULTS[system].startHeightMm;
  if (!level) return DEFAULT_LEVEL_HEIGHT_MM + CONDUIT_ABOVE_CEILING_MM;
  let ceiling = level.height_mm;
  for (const l of levels) {
    if (l.elevation_mm > level.elevation_mm + 1) {
      ceiling = Math.min(ceiling, l.elevation_mm - UPPER_SLAB_MM - level.elevation_mm);
    }
  }
  return Math.max(0, ceiling) + CONDUIT_ABOVE_CEILING_MM;
}

export interface SizeGroup {
  material: PipeMaterial;
  /** Nominal sizes in mm, smallest first. */
  sizes: number[];
}

/** The size menu per system and material (docs/CONTRACT.md table). */
export const PIPE_SIZES: Record<PipeSystem, SizeGroup[]> = {
  cold_water: [
    { material: "ppr", sizes: [20, 25, 32, 40, 50, 63] },
    { material: "gi", sizes: [15, 20, 25, 32, 50] },
    { material: "pe", sizes: [20, 25, 32] },
  ],
  hot_water: [
    { material: "ppr", sizes: [20, 25, 32] },
    { material: "copper", sizes: [15, 22, 28] },
  ],
  drainage: [{ material: "upvc", sizes: [32, 50, 75, 100, 150] }],
  vent: [{ material: "upvc", sizes: [32, 50, 75, 100] }],
  storm: [{ material: "upvc", sizes: [75, 100, 150] }],
  conduit: [
    { material: "pvc", sizes: [20, 25, 32, 40, 50] },
    { material: "emt", sizes: [15, 20, 25] },
    { material: "imc", sizes: [20, 25] },
    { material: "flexible", sizes: [15, 20] },
  ],
  // Gas line size. The liquid line is 6.35 mm for every PH split unit.
  refrigerant: [{ material: "copper", sizes: [9.52, 12.7, 15.88] }],
  condensate: [{ material: "pvc", sizes: [20, 25, 32] }],
};

/** Mirror of `defaults::drain_min_slope_pct`: 2 percent, 1 percent from 100 mm
 * up. A review default, not a code statement. */
export function drainMinSlopePct(diameterMm: number): number {
  return diameterMm >= 100 ? 1 : 2;
}

/** Service layers in display order: plumbing, storm, electrical, aircon. */
export const SERVICE_LAYER_ORDER: LayerKey[] = ["cold_water", "hot_water", "drainage", "vent", "storm", "electrical", "aircon"];
const SERVICE_LAYERS = new Set<LayerKey>(SERVICE_LAYER_ORDER);

/** Layers that hold service runs (and, for electrical and aircon, devices). */
export function isServiceLayer(key: LayerKey): boolean {
  return SERVICE_LAYERS.has(key);
}

/** @deprecated Use `isServiceLayer`; kept so older callers compile. */
export const isPipeLayer = isServiceLayer;

/**
 * Where a run joins a fixture (DECISIONS D40): mm above the fixture's
 * underside (`Asset::elevation_mm`), per catalog key and system. A run
 * snapped to a fixture ends at `elevation_mm` plus this, with a riser from
 * the pen height. Systems a fixture does not list keep the pen height.
 * Typical rough-in heights for drawing, not a plumbing design (D19).
 * crates/guhit-core/src/templates.rs places the services demo on the same
 * numbers.
 */
export const FIXTURE_CONNECTION_MM: Record<string, Partial<Record<PipeSystem, number>>> = {
  wc: { cold_water: 200, drainage: 0 },
  lavatory: { cold_water: -100, hot_water: -100, drainage: -200 },
  "kitchen-sink": { cold_water: 500, hot_water: 500, drainage: 450 },
  shower: { cold_water: 1000, hot_water: 1000, drainage: 0 },
  bathtub: { cold_water: 600, hot_water: 600, drainage: 0 },
  "washing-machine": { cold_water: 1000, hot_water: 1000, drainage: 750 },
  "floor-drain": { drainage: 0, storm: 0 },
  "water-heater": { cold_water: 0, hot_water: 400 },
  "water-meter": { cold_water: 75 },
  "water-tank": { cold_water: 150 },
  "septic-tank": { drainage: 1200 },
  "aircon-indoor-1hp": { refrigerant: 50, condensate: 30 },
  "aircon-indoor-2hp": { refrigerant: 50, condensate: 30 },
  "aircon-indoor-3hp": { refrigerant: 50, condensate: 30 },
  "aircon-outdoor-1hp": { refrigerant: 150 },
  "aircon-outdoor-3hp": { refrigerant: 150 },
  "aircon-window": { condensate: 0 },
};

/** Height above the level floor where a run of `system` joins `asset`, or null to keep the pen height. */
export function fixtureConnectionZ(asset: { catalog_key: string; elevation_mm: number }, system: PipeSystem): number | null {
  const off = FIXTURE_CONNECTION_MM[asset.catalog_key]?.[system];
  return off === undefined ? null : asset.elevation_mm + off;
}
