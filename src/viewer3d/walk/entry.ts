// Lane L2's entry into the engine's walk wiring (ViewerEngine
// refreshWalkWorld): the world the walker moves through, rebuilt when the
// document or the eye height changes. L2 builds L and U stairs and their
// landings from `Derived::stairs` behind this call; today it is the Wave 1
// world, unchanged.

import type { DocState } from "../../contract/bindings";
import { WalkWorlds } from "./worlds";

/** What blocks the walker, what it stands on (levels, stairs) and the minimap, for `doc` at `eyeMm`. */
export function walkWorldsFor(doc: DocState | null, eyeMm: number): WalkWorlds | null {
  return doc ? new WalkWorlds(doc, eyeMm) : null;
}
