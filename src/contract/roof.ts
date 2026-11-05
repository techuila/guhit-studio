// The roof over the top floor, read the way the engine reads it.
// CONTRACT FILE - owned by the orchestrator.
//
// Mirrors guhit-model `Project::top_roof_area` and `Project::top_roof`
// (DECISIONS D54). After the schema 6 migration the old `project.roof` has
// kind none and the real roof is a `TopFootprint` roof element, so nothing
// reads `project.roof` directly: an edit built from it would send kind none
// and delete the roof. Read `topRoof(project)`, and send `set_roof` with a
// whole `Roof` built from it.

import type { Element, Project, Roof, RoofShape } from "./bindings";

export type RoofEl = Extract<Element, { kind: "roof" }>;

/** The roof element over the top floor: the first roof whose outline is `top_footprint`. At most one. */
export function topRoofArea(project: Pick<Project, "elements">): RoofEl | null {
  // Partial documents (tests, a mirror still loading) may have no elements yet.
  for (const e of project.elements ?? []) if (e.kind === "roof" && e.outline.type === "top_footprint") return e;
  return null;
}

/** A roof element as one `Roof` value (guhit-model `RoofArea::as_roof`). */
export function roofOfArea(area: RoofEl): Roof {
  return {
    kind: area.shape,
    pitch_deg: area.pitch_deg,
    overhang_mm: area.overhang_mm,
    thickness_mm: area.thickness_mm,
    ridge_axis: area.ridge_axis,
    material_id: area.material_id,
  };
}

/**
 * The roof over the top floor: from the `TopFootprint` roof element when
 * there is one, else `project.roof` (an unmigrated project, or kind none once
 * migrated without a roof).
 */
export function topRoof(project: Pick<Project, "elements" | "roof">): Roof {
  const area = topRoofArea(project);
  return area ? roofOfArea(area) : project.roof;
}

/**
 * The project as `SetRoof { roof }` leaves it, for the dev mocks that answer
 * commands without the engine. Kind none removes the `TopFootprint` roof
 * element; any other kind updates it, keeping its outline, eaves, fascia,
 * gutter and gable sides. Without one, `project.roof` takes the value, as
 * the mocks always did (they load unmigrated fixtures). The engine also
 * creates the element there; the mocks do not.
 */
export function withTopRoof(project: Project, roof: Roof): Project {
  const area = topRoofArea(project);
  if (!area) return { ...project, roof };
  if (roof.kind === "none") return { ...project, elements: project.elements.filter((e) => e !== area) };
  const next: RoofEl = {
    ...area,
    shape: roof.kind as RoofShape,
    pitch_deg: roof.pitch_deg,
    overhang_mm: roof.overhang_mm,
    thickness_mm: roof.thickness_mm,
    ridge_axis: roof.ridge_axis,
    material_id: roof.material_id,
  };
  return { ...project, elements: project.elements.map((e) => (e === area ? next : e)) };
}
