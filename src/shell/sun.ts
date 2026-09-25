// Sun presets and time steps for the live light (`useViewer().light`), used
// by the U, I, Shift+U, Shift+I and Shift+N keys, the palette and the 3D
// view's Sun control. Sunset comes from the light module's NOAA equations
// (src/viewer3d/light/sun.ts, pure), so the Sun control's "sunset" and the
// Dusk preset always agree. Pure, no React.
import type { Site } from "../contract/bindings";
import { sunTimes } from "../viewer3d/light/sun";
import type { LampMode } from "../viewer3d/viewerStore";

/**
 * Local sunset at the site on that date, in minutes after local midnight.
 * Null where the sun does not set that day, which never happens in the
 * Philippines.
 */
export function sunsetMinutes(site: Site, year: number, month: number, day: number): number | null {
  return sunTimes(site, year, month, day).sunset;
}

// ---------------------------------------------------------------- presets

export type SunPresetId = "morning" | "noon" | "afternoon" | "dusk" | "night";

export interface SunPreset {
  id: SunPresetId;
  label: string;
  /** Minutes after local midnight. */
  minutes: number;
  /** Lamps the preset sets: Dusk turns them on, the rest leave them to the time of day. */
  lamps: LampMode;
}

/** Dusk is this long after sunset, lamps on (the render research, "Presets"). */
export const DUSK_AFTER_SUNSET_MIN = 20;
/** Used when the sunset cannot be computed. */
const FALLBACK_SUNSET_MIN = 18 * 60;

/** The five presets for the site and date, in time order. */
export function sunPresets(site: Site, year: number, month: number, day: number): SunPreset[] {
  const sunset = sunsetMinutes(site, year, month, day) ?? FALLBACK_SUNSET_MIN;
  return [
    { id: "morning", label: "Morning", minutes: 8 * 60, lamps: "auto" },
    { id: "noon", label: "Noon", minutes: 12 * 60, lamps: "auto" },
    { id: "afternoon", label: "Afternoon", minutes: 15 * 60, lamps: "auto" },
    { id: "dusk", label: "Dusk", minutes: Math.round(sunset + DUSK_AFTER_SUNSET_MIN), lamps: "on" },
    { id: "night", label: "Night", minutes: 20 * 60, lamps: "auto" },
  ].sort((a, b) => a.minutes - b.minutes) as SunPreset[];
}

/**
 * The preset after (dir 1) or before (dir -1) the time `minutes`. A time on a
 * preset moves to its neighbour. Past the last preset it wraps to the first,
 * and back.
 */
export function stepPreset(presets: SunPreset[], minutes: number, dir: 1 | -1): SunPreset {
  if (dir > 0) return presets.find((p) => p.minutes > minutes + 0.5) ?? presets[0];
  for (let i = presets.length - 1; i >= 0; i--) if (presets[i].minutes < minutes - 0.5) return presets[i];
  return presets[presets.length - 1];
}

/** The U and I step. */
export const SUN_STEP_MIN = 15;

/**
 * The time one step earlier (dir -1) or later (dir 1), on the 15 minute grid:
 * 10:07 steps to 10:15 or 10:00. Stays inside the day.
 */
export function stepMinutes(minutes: number, dir: 1 | -1): number {
  const next = dir > 0 ? Math.floor(minutes / SUN_STEP_MIN) * SUN_STEP_MIN + SUN_STEP_MIN : Math.ceil(minutes / SUN_STEP_MIN) * SUN_STEP_MIN - SUN_STEP_MIN;
  return Math.min(1439, Math.max(0, next));
}

/** Shift+N: lamps on, or back to following the time of day. */
export function toggleLamps(lamps: LampMode): LampMode {
  return lamps === "on" ? "auto" : "on";
}

/** "8:00 AM", "12:00 PM", "6:08 PM". */
export function formatClock(minutes: number): string {
  const m = ((Math.round(minutes) % 1440) + 1440) % 1440;
  const h24 = Math.floor(m / 60);
  const h12 = h24 % 12 === 0 ? 12 : h24 % 12;
  return `${h12}:${String(m % 60).padStart(2, "0")} ${h24 < 12 ? "AM" : "PM"}`;
}

/** The preset the time sits on, if any. */
export function presetAtTime(presets: SunPreset[], minutes: number): SunPreset | undefined {
  return presets.find((p) => Math.abs(p.minutes - minutes) < 0.5);
}
