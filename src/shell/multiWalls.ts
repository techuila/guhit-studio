// Thickness and height for several selected walls at once: what the fields
// show, and the edits they send (one update per wall in one batch, one undo
// step). The engine already handles each wall's thickness and height.
import type { DocState, Element } from "../contract/bindings";

export type WallEl = Extract<Element, { kind: "wall" }>;

export interface Shared {
  /** The value every wall has, or the first wall's when they differ. */
  value: number;
  mixed: boolean;
}

export function sharedNumber(values: number[]): Shared {
  const first = values[0] ?? 0;
  return { value: first, mixed: values.some((v) => Math.abs(v - first) > 1e-6) };
}

/** A wall's height, its level's height when it follows the level. */
export function wallHeight(doc: DocState, wall: WallEl): number {
  return wall.height_mm ?? doc.project.levels.find((l) => l.id === wall.level_id)?.height_mm ?? 3000;
}

export function wallsLabel(what: string, n: number): string {
  return n === 1 ? `Set wall ${what}` : `Set ${what} of ${n} walls`;
}

export function thicknessEdits(walls: WallEl[], thickness_mm: number): Array<{ base: Element; next: Element }> {
  return walls.filter((w) => w.thickness_mm !== thickness_mm).map((w) => ({ base: w, next: { ...w, thickness_mm } }));
}

/** A fixed height for every wall, or null to make them all follow their level. */
export function heightEdits(walls: WallEl[], height_mm: number | null): Array<{ base: Element; next: Element }> {
  return walls.filter((w) => w.height_mm !== height_mm).map((w) => ({ base: w, next: { ...w, height_mm } }));
}

/** Turning "Same as level" off pins each wall at the height it shows now. */
export function unpinEdits(doc: DocState, walls: WallEl[]): Array<{ base: Element; next: Element }> {
  return walls.filter((w) => w.height_mm === null).map((w) => ({ base: w, next: { ...w, height_mm: wallHeight(doc, w) } }));
}
