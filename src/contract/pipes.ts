// Pipe data shared by the plan, the 3D view and the shell.
// CONTRACT FILE - owned by the orchestrator.
//
// Mirrors guhit-model `defaults::pipe_defaults`, `defaults::drain_min_slope_pct`
// and the size table in docs/CONTRACT.md ("Pipes"). The engine stays the
// authority. These are drawing defaults and menus of the usual nominal sizes,
// never a sizing recommendation: plumbing design is for a registered Master
// Plumber (DECISIONS D19).

import type { LayerKey, PipeMaterial, PipeSystem } from "./bindings";

/** Display order everywhere: tool options, layers, legends, the take-off. */
export const PIPE_SYSTEM_ORDER: PipeSystem[] = ["cold_water", "hot_water", "drainage", "vent"];

export const PIPE_SYSTEM_LABEL: Record<PipeSystem, string> = {
  cold_water: "Cold water",
  hot_water: "Hot water",
  drainage: "Drainage",
  vent: "Vent",
};

/** The CSS variables in src/styles/tokens.css. */
export const PIPE_COLOR_VAR: Record<PipeSystem, string> = {
  cold_water: "--pipe-cold",
  hot_water: "--pipe-hot",
  drainage: "--pipe-drain",
  vent: "--pipe-vent",
};

/** The same colors as hex, for code that cannot read CSS variables. Keep in
 * step with tokens.css and crates/guhit-export/src/pipes.rs. */
export const PIPE_COLOR_HEX: Record<PipeSystem, string> = {
  cold_water: "#2b7bd0",
  hot_water: "#e0563a",
  drainage: "#9b6a35",
  vent: "#3a9a5c",
};

export const PIPE_MATERIAL_LABEL: Record<PipeMaterial, string> = {
  ppr: "PPR",
  upvc: "uPVC",
  gi: "GI",
  pe: "PE",
  copper: "Copper",
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
};

/** Mirror of `defaults::drain_min_slope_pct`: 2 percent, 1 percent from 100 mm
 * up. A review default, not a code statement. */
export function drainMinSlopePct(diameterMm: number): number {
  return diameterMm >= 100 ? 1 : 2;
}

const PIPE_LAYERS = new Set<LayerKey>(PIPE_SYSTEM_ORDER);

/** Pipe layers are named after their system. */
export function isPipeLayer(key: LayerKey): key is PipeSystem {
  return PIPE_LAYERS.has(key);
}
