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
  conduit: { material: "pvc", diameterMm: 20, startHeightMm: 2800 },
  refrigerant: { material: "copper", diameterMm: 9.52, startHeightMm: 2400 },
  condensate: { material: "pvc", diameterMm: 20, startHeightMm: 2300 },
};

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
