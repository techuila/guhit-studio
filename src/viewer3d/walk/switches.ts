// Switches in walk mode. Clicking a wall switch while walking or flying flips
// the lights it is linked to (`Asset::links`) in the view only: the model and
// its undo history never change (docs/CONTRACT.md, "Devices, fixtures and
// links"). Two switches on one light (a 3-way) flip the same light.
//
// The lights belong to the 3D view's lamps (docs/CONTRACT.md, "Sun and
// light", light/LightRig.ts). This module decides what a click means and asks
// the engine to flip them (`LampSwitchApi`).

import type { CatalogItem, DocState } from "../../contract/bindings";
import { useApp } from "../../state/store";

/** Catalog keys of switches (device kind `switch`). Until the catalog loads, the built-in `switch-` keys. */
export function switchKeysOf(catalog: CatalogItem[]): Set<string> {
  const keys = new Set(catalog.filter((c) => c.device === "switch").map((c) => c.key));
  if (keys.size === 0) for (const k of ["switch-1", "switch-2", "switch-3"]) keys.add(k);
  return keys;
}

/**
 * The lights a switch controls: the assets it links that give light. Null
 * when `id` is not a switch, so the click selects it as usual.
 */
export function switchLoads(doc: DocState | null, id: string, keys: Set<string>): string[] | null {
  if (!doc) return null;
  const byId = new Map(doc.project.elements.map((e) => [e.id, e]));
  const sw = byId.get(id);
  if (!sw || sw.kind !== "asset" || !keys.has(sw.catalog_key)) return null;
  return sw.links.filter((l) => {
    const e = byId.get(l);
    return e?.kind === "asset" && e.light !== null;
  });
}

/** What the 3D view offers to flip lamps in the view, not in the model (`ViewerEngine.switchLamps`). */
export interface LampSwitchApi {
  /**
   * Flips these fixtures in the view: all on when any of them is off, else
   * all off. Returns false when the view has no lamps to flip.
   */
  switchLamps(ids: string[]): boolean;
}

/**
 * A click on `id` while walking. True when it was a switch (the click is
 * used and the switch flashes), false to select it as usual.
 */
export function walkSwitchClick(lamps: LampSwitchApi, doc: DocState | null, id: string, keys: Set<string>): boolean {
  const loads = switchLoads(doc, id, keys);
  if (loads === null) return false;
  const app = useApp.getState();
  if (loads.length === 0) {
    app.toast("info", "This switch has no lights linked yet. Link them in the plan with the link tool (L).");
    return true;
  }
  // A light the view does not draw (a hidden layer, a level cut away) has no lamp to flip.
  if (!lamps.switchLamps(loads)) app.toast("info", "The lights on this switch are not shown in this view.");
  return true;
}
