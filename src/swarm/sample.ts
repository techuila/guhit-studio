// Reading the reveal at one moment, the same way for the plan and the 3D view.

import type { Reveal } from "./reveal";

/** The draft ids that show at `now`: the 3D view's set, the plan's "progress above 0". */
export function revealedIds(reveal: Reveal, ids: Iterable<string>, now: number): Set<string> {
  const out = new Set<string>();
  for (const id of ids) if (reveal.progress(id, now) > 0) out.add(id);
  return out;
}
