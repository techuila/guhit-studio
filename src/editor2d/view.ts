// View transform between model mm (+y north) and canvas CSS pixels (+y down).
// Pure, tested in view.test.ts.

import type { P, Rect } from "./geom";
import { rectIsEmpty } from "./geom";

export interface View {
  /** Pixels per millimeter. */
  scale: number;
  /** Screen position of the model origin, CSS pixels. */
  ox: number;
  oy: number;
}

export const MIN_SCALE = 0.002; // 1 px = 500 mm
export const MAX_SCALE = 4; // 1 mm = 4 px

export function toScreen(v: View, p: P): P {
  return { x: v.ox + p.x * v.scale, y: v.oy - p.y * v.scale };
}

export function toWorld(v: View, s: P): P {
  return { x: (s.x - v.ox) / v.scale, y: (v.oy - s.y) / v.scale };
}

export function clampScale(scale: number): number {
  return Math.max(MIN_SCALE, Math.min(MAX_SCALE, scale));
}

/** Zooms by `factor` keeping the model point under `screen` fixed. */
export function zoomAt(v: View, screen: P, factor: number): View {
  const scale = clampScale(v.scale * factor);
  const w = toWorld(v, screen);
  return { scale, ox: screen.x - w.x * scale, oy: screen.y + w.y * scale };
}

export function panBy(v: View, dx: number, dy: number): View {
  return { scale: v.scale, ox: v.ox + dx, oy: v.oy + dy };
}

/** Fits `bounds` into a width x height viewport with a pixel margin. */
export function fitRect(bounds: Rect, width: number, height: number, margin = 60): View {
  if (rectIsEmpty(bounds) || width <= 0 || height <= 0) {
    return { scale: 0.08, ox: width / 2, oy: height / 2 };
  }
  const bw = Math.max(bounds.maxX - bounds.minX, 1000);
  const bh = Math.max(bounds.maxY - bounds.minY, 1000);
  const aw = Math.max(width - margin * 2, 50);
  const ah = Math.max(height - margin * 2, 50);
  const scale = clampScale(Math.min(aw / bw, ah / bh));
  const cx = (bounds.minX + bounds.maxX) / 2;
  const cy = (bounds.minY + bounds.maxY) / 2;
  return { scale, ox: width / 2 - cx * scale, oy: height / 2 + cy * scale };
}

export function visibleWorldRect(v: View, width: number, height: number): Rect {
  const a = toWorld(v, { x: 0, y: height });
  const b = toWorld(v, { x: width, y: 0 });
  return { minX: a.x, minY: a.y, maxX: b.x, maxY: b.y };
}

/** True when `bounds` fits entirely inside what `v` shows in a `width` x `height` viewport. */
export function boundsVisible(v: View, bounds: Rect, width: number, height: number): boolean {
  if (rectIsEmpty(bounds)) return true;
  const visible = visibleWorldRect(v, width, height);
  return bounds.minX >= visible.minX && bounds.maxX <= visible.maxX && bounds.minY >= visible.minY && bounds.maxY <= visible.maxY;
}

export interface GridSteps {
  /** Minor line spacing in mm. */
  minor: number;
  /** Major line spacing in mm. */
  major: number;
  /** 0 to 1, how strongly to draw minor lines (fades as they get dense). */
  minorAlpha: number;
}

/**
 * Picks grid spacing for the current zoom. Starts from the project grid and
 * multiplies by 1, 5, 10, 50... until minor lines are at least `minPx` apart.
 * When zoomed in far, subdivides by 10 (never below 1 mm).
 */
export function gridSteps(gridMm: number, scale: number, minPx = 9): GridSteps {
  let minor = gridMm > 0 ? gridMm : 100;
  const mults = [5, 2];
  let i = 0;
  while (minor * scale < minPx) {
    minor *= mults[i % 2];
    i++;
    if (i > 40) break;
  }
  while (minor * scale > minPx * 14 && minor / 10 >= 1) minor /= 10;
  const major = minor * (i % 2 === 1 ? 2 : 10);
  const px = minor * scale;
  const minorAlpha = Math.max(0, Math.min(1, (px - minPx) / (minPx * 1.5) + 0.25));
  return { minor, major, minorAlpha };
}

/** Step used by grid snapping: the project grid, or finer when zoomed in far. */
export function snapStep(gridMm: number, scale: number): number {
  const g = gridMm > 0 ? gridMm : 100;
  const s = gridSteps(g, scale);
  return Math.min(g, s.minor);
}
