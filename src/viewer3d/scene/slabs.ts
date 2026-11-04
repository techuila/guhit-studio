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

/** A slab element for tests and dev scenes. */
export function slabElement(s: Omit<Slab, "material_id" | "railing"> & Partial<Pick<Slab, "material_id" | "railing">>): Element {
  return { kind: "slab", material_id: null, railing: null, ...s };
}
