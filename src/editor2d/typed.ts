// Type-to-precise buffer and length formatting. Pure, tested in typed.test.ts.
//
// The buffer is driven by key events, not by a DOM input, so typing works
// the instant a digit is pressed while drawing.

import type { DisplayUnit } from "../contract/bindings";

export type TypedMode = "polar" | "rect";

export interface TypedState {
  mode: TypedMode;
  /** polar: [length, angle]. rect: [width, depth]. Raw text as typed. */
  fields: [string, string];
  active: 0 | 1;
}

export function emptyTyped(mode: TypedMode): TypedState {
  return { mode, fields: ["", ""], active: 0 };
}

export function typedIsEmpty(t: TypedState | null): boolean {
  return !t || (t.fields[0] === "" && t.fields[1] === "");
}

/** True when `key` would start or continue a typed value. */
export function isTypedKey(key: string): boolean {
  return /^[0-9]$/.test(key) || key === "." || key === "-" || key === "<" || key === ",";
}

/**
 * Applies one key to the buffer. Returns the new state, or null when the
 * buffer became empty and should close. Keys that do not apply return the
 * state unchanged.
 */
export function typedKey(t: TypedState, key: string): TypedState | null {
  const fields: [string, string] = [t.fields[0], t.fields[1]];
  let active = t.active;
  if (/^[0-9]$/.test(key)) {
    fields[active] += key;
  } else if (key === ".") {
    if (!fields[active].includes(".")) fields[active] += fields[active] === "" || fields[active] === "-" ? "0." : ".";
  } else if (key === "-") {
    if (fields[active] === "") fields[active] = "-";
  } else if ((key === "<" && t.mode === "polar") || (key === "," && t.mode === "rect")) {
    active = 1;
    fields[1] = "";
  } else if (key === "Tab") {
    active = active === 0 ? 1 : 0;
  } else if (key === "Backspace") {
    if (fields[active] !== "") fields[active] = fields[active].slice(0, -1);
    else if (active === 1) active = 0;
    if (fields[0] === "" && fields[1] === "") return null;
  } else {
    return t;
  }
  return { mode: t.mode, fields, active };
}

function num(s: string): number | null {
  if (s === "" || s === "-" || s === ".") return null;
  const v = Number(s);
  return Number.isFinite(v) ? v : null;
}

/** Typed values are in the display unit. Returns mm (and degrees for the polar angle). */
export function typedValues(t: TypedState, unit: DisplayUnit): { a: number | null; b: number | null } {
  const k = unit === "m" ? 1000 : 1;
  const a = num(t.fields[0]);
  const b = num(t.fields[1]);
  return {
    a: a === null ? null : a * k,
    b: b === null ? null : t.mode === "polar" ? b : b * k,
  };
}

/** Parses "3000<45", "3000", "<45". Lengths in the display unit. */
export function parsePolar(text: string, unit: DisplayUnit): { length: number | null; angle: number | null } | null {
  const m = /^\s*(-?\d*\.?\d*)\s*(?:<\s*(-?\d*\.?\d*))?\s*$/.exec(text);
  if (!m) return null;
  const k = unit === "m" ? 1000 : 1;
  const l = num(m[1] ?? "");
  const a = num(m[2] ?? "");
  if (l === null && a === null) return null;
  return { length: l === null ? null : l * k, angle: a };
}

/** Parses "4000,3000" or "4000 x 3000". Lengths in the display unit. */
export function parseSize(text: string, unit: DisplayUnit): { width: number; depth: number } | null {
  const m = /^\s*(\d*\.?\d+)\s*[,xX]\s*(\d*\.?\d+)\s*$/.exec(text);
  if (!m) return null;
  const k = unit === "m" ? 1000 : 1;
  return { width: Number(m[1]) * k, depth: Number(m[2]) * k };
}

/** Formats a length in the display unit. mm: whole numbers. m: 2 to 3 decimals. */
export function formatLength(mm: number, unit: DisplayUnit, withUnit = false): string {
  if (unit === "m") {
    const m = mm / 1000;
    let s = m.toFixed(3);
    if (s.endsWith("0")) s = s.slice(0, -1);
    return withUnit ? `${s} m` : s;
  }
  const s = String(Math.round(mm));
  return withUnit ? `${s} mm` : s;
}

export function formatArea(mm2: number): string {
  return `${(mm2 / 1e6).toFixed(2)} m²`;
}

export function formatAngle(deg: number): string {
  const r = Math.round(deg * 10) / 10;
  return `${Number.isInteger(r) ? r.toFixed(0) : r.toFixed(1)}°`;
}
