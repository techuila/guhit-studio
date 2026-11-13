// Slabs as the 3D view and the walker read them (DECISIONS D50). A slab's
// own kind is `slab_kind` (floor, balcony, deck, void): `kind` is the
// element tag, "slab".

import type { DocState, Element, Slab } from "../../contract/bindings";

/** The slab elements, in element order. */
export function slabsOf(elements: readonly Element[] | null | undefined): Slab[] {
  const out: Slab[] = [];
  for (const e of elements ?? []) if (e.kind === "slab" && Array.isArray(e.polygon)) out.push(e);
  return out;
}

/** `slabsOf` for a document. */
export function docSlabs(doc: DocState | null | undefined): Slab[] {
  return slabsOf(doc?.project.elements);
}

/** A deck this close to the ground on the lowest level is paving: a carport, a driveway, a lanai on grade. */
export const GROUND_DECK_MM = 300;

/**
 * The finish a deck slab with no material of its own shows (G4 problem 11):
 * paving stones on the ground of the lowest level, where carports and driveways
 * are, timber decking when raised or on an upper level.
 */
export function deckFinish(s: Slab, onLowestLevel: boolean): string {
  const top = Number.isFinite(s.top_offset_mm) ? s.top_offset_mm : 0;
  return onLowestLevel && Math.abs(top) <= GROUND_DECK_MM ? "mat-paving-stone" : "mat-wood-cladding";
}

/** A slab element for tests and dev scenes. */
export function slabElement(s: Omit<Slab, "material_id" | "railing"> & Partial<Pick<Slab, "material_id" | "railing">>): Element {
  return { kind: "slab", material_id: null, railing: null, ...s };
}
