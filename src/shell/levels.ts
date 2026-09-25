// Levels: which elements stand on a level, the order to list levels in, and
// the AddLevel and DeleteLevel commands (docs/CONTRACT.md, "Levels"). Pure,
// no React.
import type { Command, Element, Level } from "../contract/bindings";

/** AddLevel with nulls stacks a new level on the highest one. */
export type AddLevelCommand = Extract<Command, { type: "add_level" }>;

/** DeleteLevel removes a level and everything on it, as one undo step. Never the last level. */
export type DeleteLevelCommand = Extract<Command, { type: "delete_level" }>;

export const ADD_LEVEL_ABOVE: AddLevelCommand = { type: "add_level", name: null, elevation_mm: null, height_mm: null };

export function deleteLevelCommand(levelId: string): DeleteLevelCommand {
  return { type: "delete_level", level_id: levelId };
}

/** The level an element stands on: openings follow their wall; cameras stand on none. */
export function levelOfElement(el: Element, byId: Map<string, Element>): string | null {
  if (el.kind === "opening") {
    const host = byId.get(el.wall_id);
    return host && host.kind === "wall" ? host.level_id : null;
  }
  if (el.kind === "camera") return null;
  return el.level_id;
}

/** How many elements go with a level when it is deleted. */
export function elementsOnLevel(elements: Element[], levelId: string): number {
  const byId = new Map(elements.map((e) => [e.id, e]));
  return elements.filter((e) => levelOfElement(e, byId) === levelId).length;
}

/** Top floor first, as a building section reads. Equal elevations keep the project order. */
export function levelsTopDown(levels: Level[]): Level[] {
  return levels
    .map((level, i) => ({ level, i }))
    .sort((a, b) => b.level.elevation_mm - a.level.elevation_mm || a.i - b.i)
    .map(({ level }) => level);
}

/** The level a command added: the one in `after` that `before` did not have. */
export function addedLevel(before: Level[], after: Level[]): Level | null {
  const known = new Set(before.map((l) => l.id));
  return after.find((l) => !known.has(l.id)) ?? null;
}

/** "Delete Level 2 and the 12 elements on it?" */
export function deleteLevelQuestion(level: Pick<Level, "name">, count: number): string {
  if (count === 0) return `Delete ${level.name}? Nothing is drawn on it.`;
  return `Delete ${level.name} and the ${count === 1 ? "1 element" : `${count} elements`} on it?`;
}

/** "+3.00 m", "0.00 m", "-0.45 m": a level's floor, as section drawings mark it. */
export function elevationLabel(mm: number): string {
  const m = mm / 1000;
  if (Math.abs(m) < 0.0005) return "0.00 m";
  return `${m > 0 ? "+" : "-"}${Math.abs(m).toFixed(2)} m`;
}
