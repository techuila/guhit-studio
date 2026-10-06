// Where live session things sit over the plan canvas: remote cursors, their
// labels and the cursor chat bubble. Plan mm go through the same transform
// as the plan itself (editor2d/view.ts). Pure, tested in placement.test.ts.

import type { Point } from "../contract/bindings";
import { toScreen, type View } from "../editor2d/view";

/** A cursor's name label or chat bubble, right and below the arrow tip. */
export const LABEL_OFFSET = { x: 12, y: 16 } as const;

/** Room kept for a label before it flips to the other side of the arrow. */
export const LABEL_ROOM = { w: 200, h: 72 } as const;

/** Plan mm to canvas CSS pixels. */
export function planToCanvas(view: View, p: Point): Point {
  return toScreen(view, p);
}

/** True when a canvas point is on the canvas, grown by `margin` pixels. */
export function onCanvas(p: Point, width: number, height: number, margin = 0): boolean {
  return p.x >= -margin && p.y >= -margin && p.x <= width + margin && p.y <= height + margin;
}

/**
 * Which way a cursor's label opens so it stays readable: to the left of the
 * arrow near the right edge, above it near the bottom edge.
 */
export function labelFlip(p: Point, width: number, height: number, room: { w: number; h: number } = LABEL_ROOM): { x: boolean; y: boolean } {
  return {
    x: width > room.w * 1.5 && p.x > width - room.w,
    y: height > room.h * 2 && p.y > height - room.h,
  };
}

/**
 * The cursor chat bubble's top left corner for a pointer at `p`: beside the
 * pointer like a remote cursor's label, kept on the canvas.
 */
export function bubbleAt(p: Point, size: { w: number; h: number }, width: number, height: number, pad = 6): Point {
  let x = p.x + LABEL_OFFSET.x;
  let y = p.y + LABEL_OFFSET.y;
  if (x + size.w > width - pad) x = Math.max(pad, p.x - LABEL_OFFSET.x - size.w);
  if (y + size.h > height - pad) y = Math.max(pad, p.y - LABEL_OFFSET.y - size.h);
  return { x: Math.max(pad, x), y: Math.max(pad, y) };
}

/** Same transform, to within a hundredth of a pixel: nothing moved on screen. */
export function sameView(a: View | null, b: View): boolean {
  if (!a) return false;
  return Math.abs(a.scale - b.scale) < 1e-9 && Math.abs(a.ox - b.ox) < 0.01 && Math.abs(a.oy - b.oy) < 0.01;
}

/** The view that puts plan point `p` at the middle of a `width` x `height` canvas, at the same zoom. */
export function centeredOn(view: View, p: Point, width: number, height: number): View {
  return { scale: view.scale, ox: width / 2 - p.x * view.scale, oy: height / 2 + p.y * view.scale };
}
