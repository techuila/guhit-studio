// Roofs in the 3D view (lane L2): every roof of `Derived.roofs` (DECISIONS
// D54) on the level that carries it, the roof over the top floor, lanai and
// carport roofs alike, with gable ends, fascia and gutters; and the roof
// decks (D26) where no roof covers them. All into the roof group, which
// hides with the roof without a rebuild.

import type { Element, Level, RoofArea, RoofGeometry } from "../../contract/bindings";
import type { Pt } from "../geom/coords";
import { MeshData, pushPrism } from "../geom/meshData";
import { buildDerivedInfill, buildDerivedRoof, buildRoofTrim, fitPlane } from "../geom/roofMesh";
import { pointInPolygon } from "../geom/polygon";
import { ceilingHeightMm } from "../../editor2d/mount";
import { PLINTH_MM, subtractWells, type SceneHost } from "./buildScene";
import { DECK_MM, DECK_TOPPING_MM, deckPieces, uncovered } from "./roofDeck";

/** Soffit, fascia and barge boards: plain off-white, the usual painted fibre cement board look. */
const TRIM_COLOR = "#ece9e1";
/** Gutters: pre-painted metal. */
const GUTTER_COLOR = "#8d9196";

type RoofEl = Extract<Element, { kind: "roof" }>;

/** The roof element of a derived roof; null for the legacy roof of an unmigrated project. */
function roofElement(elements: readonly Element[], id: string): RoofEl | null {
  if (!id) return null;
  for (const e of elements) if (e.kind === "roof" && e.id === id) return e;
  return null;
}

/** The plan pieces every roof on `levelId` covers: its planes. */
export function roofCover(roofs: readonly RoofGeometry[] | undefined, levelId: string): Pt[][] {
  const out: Pt[][] = [];
  for (const g of roofs ?? []) {
    if (g.level_id !== levelId) continue;
    for (const plane of g.planes) out.push(plane.map((p) => ({ x: p.x, y: p.y })));
  }
  return out;
}

/**
 * The underside of the roof over plan point `p` on `levelId`, mm above that
 * level's floor: the lowest when roofs overlap. Null under no roof.
 */
export function roofUnderAt(roofs: readonly RoofGeometry[] | undefined, levelId: string, p: Pt): number | null {
  let best: number | null = null;
  for (const g of roofs ?? []) {
    if (g.level_id !== levelId) continue;
    for (const plane of g.planes) {
      if (!pointInPolygon(p, plane.map((q) => ({ x: q.x, y: q.y })))) continue;
      const f = fitPlane(plane);
      if (!f) continue;
      const under = f.z0 + f.gx * p.x + f.gy * p.y - g.vertical_mm;
      if (best === null || under < best) best = under;
    }
  }
  return best;
}

/**
 * Columns stand up to the roof over them (a carport or lanai post under its
 * eaves, `AddRoofPosts`) instead of the full level height: the columns pass
 * built them at the level height, so each one under a roof is stretched or
 * cut to the roof's underside at its center.
 */
function fitPostsToRoofs(host: SceneHost, roofs: readonly RoofGeometry[]): void {
  const { doc, root, levels, lowest, levelCut } = host;
  const columns = new Map<string, Extract<Element, { kind: "column" }>>();
  for (const e of doc.project.elements) if (e.kind === "column") columns.set(e.id, e);
  if (columns.size === 0) return;
  for (const obj of root.children) {
    const id = obj.userData?.elementId as string | undefined;
    const c = id ? columns.get(id) : undefined;
    if (!c) continue;
    const level = levels.get(c.level_id) ?? lowest;
    if (!level) continue;
    const under = roofUnderAt(roofs, level.id, c.center);
    if (under === null) continue;
    const drop = level === lowest ? PLINTH_MM : 0;
    const cut = levelCut(level);
    const was = (cut ?? level.height_mm) + drop;
    const want = Math.min(under, cut ?? Infinity) + drop;
    if (want > 10 && was > 10 && Math.abs(want - was) > 1) obj.scale.y = want / was;
  }
}

/** Every derived roof and the roof decks, into `host.roofGroup`. */
export function buildRoofs(host: SceneHost): void {
  const { lib, kit, opts, roofGroup, sortedLevels, levels, levelShown, footprintsOf, walls, wells, addBounds } = host;
  const project = host.doc.project;
  const roofs = host.doc.derived?.roofs ?? [];
  lib.category = "roof";
  fitPostsToRoofs(host, roofs);
  for (const g of roofs) {
    const level = levels.get(g.level_id);
    if (!level || !levelShown(level)) continue;
    const el = roofElement(project.elements, g.roof_id);
    // A roof that is gone draws nothing, even while `Derived` still has its
    // planes; an empty id is the legacy roof of an unmigrated project.
    if (!el && (g.roof_id !== "" || project.roof?.kind === "none")) continue;
    buildOneRoof(host, g, level, el);
  }
  // Roof decks: the part of a level that the next level up does not cover,
  // at its ceiling (DECISIONS D26), and no roof covers (D54). They hide with
  // the roof.
  for (let i = 0; i + 1 < sortedLevels.length; i++) {
    const level = sortedLevels[i];
    if (!levelShown(level)) continue;
    // The deck is the floor of the level above where that level has no
    // building: its voids and stairwells cut it as they cut that floor.
    const above = sortedLevels[i + 1];
    const open = deckPieces(footprintsOf(level.id), footprintsOf(above.id)).flatMap((p) => subtractWells(p, wells.get(above.id)));
    const pieces = uncovered(open, roofCover(roofs, level.id));
    if (pieces.length === 0) continue;
    const bottom = level.elevation_mm + ceilingHeightMm(level, sortedLevels);
    const top = bottom + DECK_MM + DECK_TOPPING_MM;
    const md = new MeshData();
    for (const piece of pieces) pushPrism(md, piece, { bottom: () => bottom, top: () => top });
    roofGroup.add(kit.mesh(kit.fromMeshData(md), lib.get("mat-floor-concrete", undefined, false)));
    if (!opts.cutaway) for (const piece of pieces) addBounds(piece, bottom, top);
  }

  function buildOneRoof(h: SceneHost, g: RoofGeometry, level: Level, el: RoofEl | null): void {
    const md = new MeshData();
    const trim = new MeshData();
    const made = buildDerivedRoof(g, level.elevation_mm, md, trim);
    if (md.triangleCount === 0) return;
    const legacy = el === null ? h.doc.project.roof : null;
    const material = el?.material_id ?? legacy?.material_id ?? null;
    roofGroup.add(kit.mesh(kit.fromMeshData(md), lib.get(material, "mat-roof-longspan")));
    // A migrated roof keeps the 200 mm fascia Wave 1 drew; the legacy roof too.
    const fasciaMm = el ? (el.fascia?.size_mm ?? null) : 200;
    const gutterMm = el ? (el.gutter?.size_mm ?? null) : null;
    const gutter = new MeshData();
    buildRoofTrim(g, level.elevation_mm, fasciaMm, gutterMm, trim, gutter);
    const fasciaMat = el?.fascia?.material_id;
    const gutterMat = el?.gutter?.material_id;
    if (trim.triangleCount > 0) roofGroup.add(kit.mesh(kit.fromMeshData(trim), fasciaMat ? lib.get(fasciaMat, undefined, false) : lib.plain(TRIM_COLOR, 0.85)));
    if (gutter.triangleCount > 0) roofGroup.add(kit.mesh(kit.fromMeshData(gutter), gutterMat ? lib.get(gutterMat, undefined, false) : lib.plain(GUTTER_COLOR, 0.5, 0.6)));
    if (g.infill.length > 0) {
      const infill = new MeshData();
      buildDerivedInfill(g, level.elevation_mm, project.settings?.default_wall_thickness_mm ?? 150, infill);
      if (infill.triangleCount > 0) {
        const firstWall = [...walls.values()].find((w) => w.level_id === level.id);
        roofGroup.add(kit.mesh(kit.fromMeshData(infill), lib.get(firstWall?.material_id, "mat-chb-painted")));
      }
    }
    const pitched = made.planes.length > 1 || g.lines.some((l) => l.kind !== "eave");
    if (pitched && (el === null || el.outline.type !== "polygon")) {
      // A flat ceiling at the wall tops under a pitched roof over walls, so
      // interiors read as closed rooms.
      const wallTop = level.elevation_mm + (el?.eave_mm ?? level.height_mm);
      const ceiling = new MeshData();
      for (const fp of footprintsOf(level.id)) pushPrism(ceiling, fp, { bottom: () => wallTop, top: () => wallTop + 20 });
      if (ceiling.triangleCount > 0) {
        const m = kit.mesh(kit.fromMeshData(ceiling), lib.plain("#f4f2ec", 0.95));
        m.castShadow = false;
        roofGroup.add(m);
      }
    }
    if (!opts.cutaway) for (const poly of made.planes) addBounds(poly, made.low, made.peak);
  }
}

/** A roof element's level: its own, or (the roof over the top floor) the one its geometry sits on. */
export function roofLevelOf(el: RoofArea, roofs: readonly RoofGeometry[] | undefined): string | null {
  return el.level_id ?? roofs?.find((g) => g.roof_id === el.id)?.level_id ?? null;
}
