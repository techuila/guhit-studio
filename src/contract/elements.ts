// Which level an element is on, and which elements are views.
// CONTRACT FILE - owned by the orchestrator.
//
// Mirrors guhit-model `Element::level_id` and `Element::is_view` (DECISIONS
// D54). Every per-element level lookup and every camera special case in the
// window goes through these two, so tours, grids, the lot and the roof over
// the top floor behave the same everywhere.

import type { Element } from "./bindings";

/**
 * The element's own level, or null for elements on no level of their own:
 * openings (they are on their wall: look up the wall), cameras and tours
 * (views), grid lines and the lot (they hold on every level), and the roof
 * over the top floor (`level_id` null, outline `top_footprint`).
 */
export function elementLevel(el: Element): string | null {
  switch (el.kind) {
    case "opening":
    case "camera":
    case "tour":
    case "grid":
    case "lot":
      return null;
    default:
      // A roof's `level_id` is null exactly for the `TopFootprint` roof.
      return el.level_id;
  }
}

/** Cameras and tours: views of the model, not parts of it. On no level and no layer, always allowed. */
export function isView(el: Pick<Element, "kind">): boolean {
  return el.kind === "camera" || el.kind === "tour";
}
