// The light model shared by the live view, the path traced renders and the
// shadow study: where the sun is, how strong it is, what color, and how the
// picture is exposed. Pure: no three.js, no DOM.
//
// Units. The scene is lit in "pre-exposed" units: a light's physical value
// (lux, candela, cd/m2) times LUX times the exposure scale. At the reference
// exposure (EV_REF, a sunny day) the noon sun is 3.0, the value the viewer was
// tuned with, so plain white walls still read white and every tint and
// highlight in display units keeps its look. A night view does not turn the
// tone mapping exposure up a thousand times; the lamps get brighter instead,
// so hover tints, pipe colors and outlines look the same by day and by night,
// and half float targets keep their precision.
//
// Exposure is EV100. The live view is on auto: a base value from the sun and
// the lamps (baseEv), which the view settles into over about half a second.
// A saved view, a render and a study lock an offset from that base
// (`Camera::light.exposure_ev`, "relative to the auto value").

import type { Site, SkyKind, ViewLight } from "../../contract/bindings";
import type { LampMode, LiveLight } from "../viewerStore";
import { HORIZON_DEG, sunPosition, sunTimes, sunVector, type SunPosition } from "./sun";

/** Scene units per lux at the reference exposure. The noon sun, 100 000 lux, is 3.0. */
export const LUX = 3e-5;
/** EV100 of a sunny day, where the exposure scale is 1. */
export const EV_REF = 15;
/** Direct sunlight outside the atmosphere, lux. */
const SOLAR_LUX = 128_000;
/** Clear sky extinction per air mass (photopic). */
const EXTINCTION = 0.21;
/** Overcast: the sun is a faint glow through the cloud. */
const CLOUDY_SUN = 0.12;

export const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
export const smoothstep = (a: number, b: number, x: number) => {
  const t = clamp((x - a) / (b - a), 0, 1);
  return t * t * (3 - 2 * t);
};
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

/** Kasten and Young's relative air mass for a sun altitude in degrees. */
export function airMass(altitudeDeg: number): number {
  const h = Math.max(altitudeDeg, -0.5);
  return 1 / (Math.sin((h * Math.PI) / 180) + 0.50572 * Math.pow(h + 6.07995, -1.6364));
}

/** Direct normal illuminance of a clear-sky sun, lux. 0 once it has set. */
export function sunLux(altitudeDeg: number): number {
  if (altitudeDeg <= HORIZON_DEG) return 0;
  const direct = SOLAR_LUX * Math.exp(-EXTINCTION * airMass(altitudeDeg));
  // The last half degree fades out instead of snapping off at the horizon.
  return direct * smoothstep(HORIZON_DEG, 0.5, altitudeDeg);
}

/**
 * Color of direct sunlight, linear RGB with the brightest channel at 1:
 * white-ish at noon, orange near the horizon (Rayleigh extinction).
 */
export function sunColor(altitudeDeg: number): [number, number, number] {
  const m = Math.min(airMass(altitudeDeg), 40);
  const r = Math.exp(-0.035 * m);
  const g = Math.exp(-0.085 * m);
  const b = Math.exp(-0.2 * m);
  const top = Math.max(r, g, b);
  return [r / top, g / top, b / top];
}

/**
 * Auto exposure for a sun altitude and sky, EV100. Tuned against photography
 * rules of thumb: sunny 15, low sun 13 to 14, blue hour 7 to 9, a room lit by
 * one 9 W LED bulb about 4.5 (a 900 lm lamp reads like a bulb, not a
 * floodlight: D5 users report theirs too bright).
 */
export function baseEv(altitudeDeg: number, sky: SkyKind): number {
  const a = altitudeDeg;
  let ev: number;
  if (a >= 30) ev = 15;
  else if (a >= 10) ev = lerp(14.2, 15, (a - 10) / 20);
  else if (a >= 0) ev = lerp(12.6, 14.2, a / 10);
  // Blue hour: about 8.4 three degrees down, 6.7 at the Dusk preset.
  else if (a >= -6) ev = 5.5 + 7.1 * Math.pow((a + 6) / 6, 1.3);
  else if (a >= -12) ev = lerp(4.5, 5.5, (a + 12) / 6);
  else ev = 4.5;
  // Overcast daylight is about a stop and a half darker than sun.
  if (sky === "cloudy") ev -= 1.5 * smoothstep(-6, 5, a);
  return ev;
}

/** Metering target: log2 of the average scene value before tone mapping (light/refine.ts, `meterTexture`). */
export const METER_KEY = -2.1;

/**
 * Stops a path traced render brightens (or darkens) its own image by, from
 * the log2 average it metered. The tracer blocks the sky light that the
 * live view lets through walls, so a room lit by its windows traces four to
 * six stops darker than the view shows it at the same exposure. Only the part
 * of the gap past one stop is closed: an exterior, or a view the user made a
 * stop brighter or darker, keeps its exposure. By day up to six stops
 * brighter; at night half a stop at most, so night stays night.
 */
export function renderExposureShift(log2: number, daylight: number): number {
  const gap = METER_KEY - log2;
  const past = Math.sign(gap) * Math.max(Math.abs(gap) - 1, 0);
  // `|| 0`: no negative zero.
  return clamp(past, -1, daylight >= 0.6 ? 6 : 0.5) || 0;
}

/** Scene scale for an exposure: 1 at EV_REF, doubling per stop darker. */
export function exposureScale(ev: number): number {
  return Math.pow(2, EV_REF - ev);
}

/** Lamps lit: always on, never, or on auto once the sun is down. */
export function lampsLit(mode: LampMode, altitudeDeg: number): boolean {
  if (mode === "on") return true;
  if (mode === "off") return false;
  return altitudeDeg < HORIZON_DEG;
}

/** Everything the scene needs to know about the light at one moment. */
export interface LightFrame {
  sun: SunPosition;
  /** Unit vector towards the sun, plan axes (x east, y north, z up), north applied. */
  sunDir: { x: number; y: number; z: number };
  /** Direct sun on a surface facing it, lux, cloud applied. */
  sunLux: number;
  sunColor: [number, number, number];
  sky: SkyKind;
  /** 1 in daylight, 0 at night, easing through twilight. */
  daylight: number;
  lampsLit: boolean;
  /** Ghost lights in rooms without a fixture: at night, unless the lamps are off. */
  ghostLit: boolean;
  /** The auto exposure for this light, EV100. */
  baseEv: number;
  sunrise: number | null;
  sunset: number | null;
}

/** The year the live light's month and day belong to. */
export function currentYear(): number {
  return new Date(Date.now() + 8 * 3600e3).getUTCFullYear();
}

export function lightFrame(light: LiveLight, site: Site, northAngleDeg: number, year = currentYear()): LightFrame {
  const sun = sunPosition(site, year, light.month, light.day, light.minutes);
  const cloudy = light.sky === "cloudy";
  const alt = sun.altitudeDeg;
  const times = sunTimes(site, year, light.month, light.day);
  return {
    sun,
    sunDir: sunVector(sun, Number.isFinite(northAngleDeg) ? northAngleDeg : 0),
    sunLux: sunLux(alt) * (cloudy ? CLOUDY_SUN : 1),
    sunColor: sunColor(alt),
    sky: light.sky,
    daylight: smoothstep(-8, 3, alt),
    lampsLit: lampsLit(light.lamps, alt),
    ghostLit: light.lamps !== "off" && alt < HORIZON_DEG,
    baseEv: baseEv(alt, light.sky),
    sunrise: times.sunrise,
    sunset: times.sunset,
  };
}

// ------------------------------------------------------------------ lamps

/**
 * Candela of a fixture for its lumens. A point spreads its flux over 4 pi
 * steradians. A spot puts it into its cone, which three.js fades from the
 * full cone's edge to the inner cone (`penumbra`) with a smoothstep: the
 * solid angle is the inner cone plus half the fading ring. A 180 degree
 * spot that fades all the way is the cosine spread of a flat diffuser:
 * pi steradians, flux / pi on its axis.
 */
export function lampCandela(lumens: number, kind: "point" | "spot", coneDeg = 60, penumbra = 0): number {
  const flux = Math.max(lumens, 0);
  if (kind === "point") return flux / (4 * Math.PI);
  const half = (clamp(coneDeg, 5, 180) / 2) * (Math.PI / 180);
  const outer = Math.cos(half);
  const inner = Math.cos(half * (1 - clamp(penumbra, 0, 1)));
  const solid = 2 * Math.PI * ((inner - outer) * 0.5 + (1 - inner));
  return flux / Math.max(solid, 1e-3);
}

/**
 * Linear RGB of a color temperature (Tanner Helland's fit), brightest
 * channel at 1. 2700 K is warm white, 6500 K daylight.
 */
export function kelvinColor(kelvin: number): [number, number, number] {
  const t = clamp(kelvin, 1500, 12000) / 100;
  let r: number;
  let g: number;
  let b: number;
  if (t <= 66) {
    r = 255;
    g = 99.4708025861 * Math.log(t) - 161.1195681661;
    b = t <= 19 ? 0 : 138.5177312231 * Math.log(t - 10) - 305.0447927307;
  } else {
    r = 329.698727446 * Math.pow(t - 60, -0.1332047592);
    g = 288.1221695283 * Math.pow(t - 60, -0.0755148492);
    b = 255;
  }
  // sRGB bytes to linear, then normalized.
  const lin = (v: number) => {
    const c = clamp(v, 0, 255) / 255;
    return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  };
  const out: [number, number, number] = [lin(r), lin(g), lin(b)];
  const top = Math.max(...out, 1e-6);
  return [out[0] / top, out[1] / top, out[2] / top];
}

/**
 * The color a lamp is drawn with: its color temperature, partly adapted the
 * way eyes and a camera's auto white balance adapt indoors. Warm white
 * (2700 to 3000 K) reads warm without turning the room orange.
 */
export function lampTint(kelvin: number): [number, number, number] {
  const [r, g, b] = kelvinColor(kelvin);
  const k = 0.38;
  return [r + (1 - r) * k, g + (1 - g) * k, b + (1 - b) * k];
}

// ------------------------------------------------------------- saved views

/** The live light as it is saved with a view (`Camera::light`). */
export function toViewLight(light: LiveLight, lockedEv: number): ViewLight {
  return {
    month: light.month,
    day: light.day,
    minutes: Math.round(light.minutes),
    sky: light.sky,
    exposure_ev: Math.round(lockedEv * 10) / 10,
    lamps: light.lamps,
  };
}

/** A saved view's light as the live light. */
export function fromViewLight(v: ViewLight): LiveLight {
  return {
    month: clamp(Math.round(v.month), 1, 12),
    day: clamp(Math.round(v.day), 1, 31),
    minutes: clamp(Math.round(v.minutes), 0, 1439),
    sky: v.sky,
    exposureEv: v.exposure_ev,
    lamps: v.lamps ?? "auto",
  };
}

/** Same light, ignoring the exposure. */
export function sameLight(a: LiveLight, b: LiveLight): boolean {
  return a.month === b.month && a.day === b.day && a.minutes === b.minutes && a.sky === b.sky && a.lamps === b.lamps;
}
