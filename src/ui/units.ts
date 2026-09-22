// Length and area formatting. Stored values are always millimeters; the
// display unit is a view setting only.
import type { DisplayUnit } from "../contract/bindings";

function trimZeros(s: string): string {
  return s.includes(".") ? s.replace(/0+$/, "").replace(/\.$/, "") : s;
}

/** Value for an input box, without a unit suffix. */
export function lengthToInput(mm: number, unit: DisplayUnit): string {
  if (!Number.isFinite(mm)) return "";
  if (unit === "m") return trimZeros((mm / 1000).toFixed(3));
  return trimZeros(mm.toFixed(1));
}

/** Readable length with its unit, for labels and the status bar. */
export function formatLength(mm: number, unit: DisplayUnit): string {
  if (!Number.isFinite(mm)) return "-";
  if (unit === "m") return `${(mm / 1000).toFixed(2)} m`;
  return `${Math.round(mm).toLocaleString("en-US")} mm`;
}

/**
 * Parses what a person typed into millimeters. A bare number is read in the
 * display unit. An explicit suffix (mm, cm, m) always wins.
 */
export function parseLength(text: string, unit: DisplayUnit): number | null {
  const t = text.trim().toLowerCase().replace(/,/g, "");
  const m = /^(-?\d*\.?\d+)\s*(mm|cm|m)?$/.exec(t);
  if (!m) return null;
  const n = Number(m[1]);
  if (!Number.isFinite(n)) return null;
  const suffix = (m[2] as "mm" | "cm" | "m" | undefined) ?? unit;
  const factor = suffix === "m" ? 1000 : suffix === "cm" ? 10 : 1;
  return n * factor;
}

export function parseNumber(text: string): number | null {
  const t = text.trim().replace(/,/g, "").replace(/(deg|°|%)$/i, "").trim();
  if (t === "" || !/^-?\d*\.?\d+$/.test(t)) return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

export function numberToInput(n: number, decimals = 2): string {
  if (!Number.isFinite(n)) return "";
  return trimZeros(n.toFixed(decimals));
}

export function formatArea(m2: number): string {
  if (!Number.isFinite(m2)) return "-";
  return `${m2.toFixed(2)} m²`;
}

export function formatAreaMm2(mm2: number): string {
  return formatArea(mm2 / 1_000_000);
}

/** "just now", "5 min ago", "3 h ago", "2 days ago", then a date. */
export function relativeTime(iso: string, now: number = Date.now()): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 45) return "just now";
  const min = Math.round(s / 60);
  if (min < 60) return `${min} min ago`;
  const h = Math.round(min / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  if (d === 1) return "yesterday";
  if (d < 14) return `${d} days ago`;
  return new Date(t).toLocaleDateString("en-PH", { year: "numeric", month: "short", day: "numeric" });
}
