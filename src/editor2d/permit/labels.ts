// Labels of the permit annotations (lane L1): grid line numbers and letters,
// section letters, elevation names by compass side and quadrant bearings.
// Pure, tested in labels.test.ts.

import type { P } from "../geom";
import { angleDeg, left, mul, normDeg, sub, unit } from "../geom";

/** Grid and section letters: A to Z without I and O, as drawings do, then AA, AB. */
const LETTERS = "ABCDEFGHJKLMNPQRSTUVWXYZ";

/** The i-th letter label, 0 is A: A..Z (no I, O), then AA, AB... */
export function letterAt(i: number): string {
  let n = Math.max(0, Math.floor(i));
  let out = "";
  do {
    out = LETTERS[n % LETTERS.length] + out;
    n = Math.floor(n / LETTERS.length) - 1;
  } while (n >= 0);
  return out;
}

const norm = (s: string): string => s.trim().toUpperCase();

/** The first letter label not in `used`. */
export function nextLetter(used: Iterable<string>): string {
  const taken = new Set([...used].map(norm));
  for (let i = 0; ; i++) {
    const l = letterAt(i);
    if (!taken.has(l)) return l;
  }
}

/** The first whole number label not in `used`, from 1. */
export function nextNumber(used: Iterable<string>): string {
  const taken = new Set([...used].map(norm));
  for (let i = 1; ; i++) if (!taken.has(String(i))) return String(i);
}

/** A grid line more vertical than horizontal: its label is a number (1, 2, 3 run along x). */
export function isVerticalLine(a: P, b: P): boolean {
  return Math.abs(b.y - a.y) > Math.abs(b.x - a.x);
}

/** The next free grid label for a line from a to b: numbers for vertical lines, letters otherwise. */
export function nextGridLabel(used: Iterable<string>, a: P, b: P): string {
  return isVerticalLine(a, b) ? nextNumber(used) : nextLetter(used);
}

// ------------------------------------------------------------------ compass

/** Azimuth of a plan direction: degrees clockwise from true north, 0 to 360. */
export function azimuthDeg(v: P, northAngleDeg: number): number {
  // True north is +y turned counter-clockwise by `north_angle_deg`.
  return normDeg(90 + northAngleDeg - angleDeg(v));
}

export type Compass = "North" | "East" | "South" | "West";
const COMPASS: Compass[] = ["North", "East", "South", "West"];

/** The compass side nearest a plan direction. */
export function compassOf(v: P, northAngleDeg: number): Compass {
  return COMPASS[Math.round(azimuthDeg(v, northAngleDeg) / 90) % 4];
}

/** Where a view mark looks: left of start to end, or right when flipped. */
export function lookDir(start: P, end: P, flip: boolean): P {
  const d = sub(end, start);
  const dir = Math.hypot(d.x, d.y) < 1e-9 ? { x: 1, y: 0 } : unit(d);
  const l = left(dir);
  return flip ? mul(l, -1) : l;
}

/**
 * An elevation's name: the side of the building it looks toward. A mark
 * north of the house looking south sees the north side, the "North"
 * elevation, as drawings name them.
 */
export function elevationName(look: P, northAngleDeg: number): Compass {
  return compassOf(mul(look, -1), northAngleDeg);
}

/** `base`, or "base 2", "base 3" when taken. */
export function numbered(base: string, used: Iterable<string>): string {
  const taken = new Set([...used].map(norm));
  if (!taken.has(norm(base))) return base;
  for (let i = 2; ; i++) if (!taken.has(norm(`${base} ${i}`))) return `${base} ${i}`;
}

/**
 * A quadrant bearing from `from` to `to` as titles write it, to the minute:
 * "N 45°30' E". `northAngleDeg` as `ProjectSettings::north_angle_deg`.
 */
export function bearingText(from: P, to: P, northAngleDeg: number): string {
  const az = azimuthDeg(sub(to, from), northAngleDeg);
  let ns: "N" | "S";
  let ew: "E" | "W";
  let deg: number;
  if (az <= 90) [ns, ew, deg] = ["N", "E", az];
  else if (az <= 180) [ns, ew, deg] = ["S", "E", 180 - az];
  else if (az <= 270) [ns, ew, deg] = ["S", "W", az - 180];
  else [ns, ew, deg] = ["N", "W", 360 - az];
  let minutes = Math.round(deg * 60);
  if (minutes > 90 * 60) minutes = 90 * 60;
  const d = Math.floor(minutes / 60);
  const m = minutes % 60;
  return `${ns} ${d}°${String(m).padStart(2, "0")}' ${ew}`;
}
