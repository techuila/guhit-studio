// Builds the whole three.js model from a DocState. A full rebuild per
// revision is cheap at this model size. Derived data may be missing while the
// engine is catching up (no rooms, empty footprint, a wall without outline):
// every builder falls back instead of throwing.

import * as THREE from "three";
import type { Asset, AssetCategory, DocState, Footprint, LayerKey, Level, Opening, Project, Stair, Wall } from "../../contract/bindings";
import { planRotationToWorld, planToWorld, type Pt } from "../geom/coords";
import { EMPTY_BOUNDS, type ModelBounds } from "../geom/cameraMath";
import { MeshData, pushPrism } from "../geom/meshData";
import { boundsOf, clipHalfPlane, ensureCCW, offsetPolygon, orientedRect, pointInPolygon, signedArea } from "../geom/polygon";
import { buildRoofInfill, buildRoofMesh, type RoofInput } from "../geom/roofMesh";
import { buildWallMesh } from "../geom/wallMesh";
import { buildAssetForm, lampAnchor, type LampInfo } from "./assets";
import { modelPack } from "./pack";
import type { BuildCache } from "./buildCache";
import { Kit, tagElement } from "./kit";
import type { MaterialLibrary } from "./materials";
import { buildOpening } from "./openings";
import { buildPipeGhost, buildPipes, PipeScene } from "./pipes";
import { defaultAnchor, fixtureSize, lampOptics, type LampSpec, type RoomSpec } from "../light/fixtures";
import { stairOutline, stairsOf } from "../walk/stairs";
import { ceilingHeightMm } from "../../editor2d/mount";
import { DECK_MM, DECK_TOPPING_MM, deckPieces } from "./roofDeck";

/**
 * The layer a placed object is on, by category: lighting and electrical
 * objects share `electrical`, aircon units `aircon`, the rest `assets`. The
 * same rule as `assetLayer` in src/editor2d/model.ts (docs/CONTRACT.md).
 */
export function assetLayerKey(category: AssetCategory): LayerKey {
  if (category === "lighting" || category === "electrical") return "electrical";
  if (category === "aircon") return "aircon";
  return "assets";
}

/** Depth of the plinth: the ground sits this far below the lowest floor. */
export const PLINTH_MM = 150;
export const CUTAWAY_MM = 1200;
const FLOOR_FINISH_MM = 12;

/**
 * `Derived.footprints` holds every closed building on `levelId`, largest
 * first, plus one entry with an empty polygon when there is none. Picks the
 * ones for this level, fixes winding and drops the empty/degenerate entry, so
 * callers (slabs, roof, indoor/outdoor tests) can build one thing per
 * building without special-casing "none" or "several".
 */
export function groupFootprints(footprints: Footprint[] | undefined | null, levelId: string): Pt[][] {
  return (footprints ?? [])
    .filter((f) => f.level_id === levelId)
    .map((f) => ensureCCW(f.polygon))
    .filter((p) => p.length >= 3);
}

/**
 * Which levels a build draws. Everything, or with the cutaway on only the
 * active level and the ones below it.
 */
export function levelFilter(project: Project, opts: { cutaway: boolean; activeLevelId: string | null }): (l: Level) => boolean {
  const levels = project.levels ?? [];
  const lowest = [...levels].sort((a, b) => a.elevation_mm - b.elevation_mm)[0] ?? null;
  const active = (opts.activeLevelId && levels.find((l) => l.id === opts.activeLevelId)) || lowest;
  return (l) => !opts.cutaway || !active || l.elevation_mm <= active.elevation_mm + 1;
}

export interface BuildOptions {
  cutaway: boolean;
  activeLevelId: string | null;
  /**
   * While an AI proposal is live, the elements it would remove: the
   * committed project they still exist in, plus their ids. Drawn as faint
   * red ghosts since `doc` here is the hypothetical post-proposal state and
   * no longer has them.
   */
  ghostsRemoved?: { project: Project; ids: string[] } | null;
  /**
   * Reuses the built form of elements whose inputs did not change. The engine
   * owns one; `buildExportGroup` leaves it out and builds everything fresh.
   */
  cache?: BuildCache;
  /**
   * Uses the CC0 pack's GLB for catalog keys that have one and are loaded.
   * Default on. This function does no I/O: it reads `modelPack`, which the
   * engine fills, so an unloaded model simply falls back to its low-poly form.
   */
  packModels?: boolean;
}

export interface BuiltScene {
  root: THREE.Group;
  /** Roof covering, gable infill and ceiling. Toggled without a rebuild. */
  roofGroup: THREE.Group;
  bounds: ModelBounds;
  /** True when there is nothing to show. */
  empty: boolean;
  /** Ground height in world meters. */
  groundY: number;
  /** Plan bounds of the building on the ground (mm), for the soft contact shadow. Null when there is none. */
  contact: { minX: number; minY: number; maxX: number; maxY: number } | null;
  kit: Kit;
  /** Meshes by element id, for highlighting. */
  byElement: Map<string, THREE.Mesh[]>;
  /** Pipe runs: one solo per pipe (picking, highlights, fades) and one merged batch per system (drawing). */
  pipes: PipeScene;
  /** Lit fixtures (`Asset::light`) on shown levels and visible layers, for the light rig. */
  lamps: LampSpec[];
  /** Rooms on shown levels: the light rig adds their bounce light and, without a fixture, a ghost light. View only. */
  lightRooms: RoomSpec[];
}

class BoundsAcc {
  b: ModelBounds | null = null;
  add(points: Pt[], z0: number, z1: number) {
    const pb = boundsOf(points);
    if (!pb) return;
    if (!this.b) {
      this.b = { ...pb, minZ: z0, maxZ: z1 };
      return;
    }
    const b = this.b;
    b.minX = Math.min(b.minX, pb.minX);
    b.minY = Math.min(b.minY, pb.minY);
    b.maxX = Math.max(b.maxX, pb.maxX);
    b.maxY = Math.max(b.maxY, pb.maxY);
    b.minZ = Math.min(b.minZ, z0);
    b.maxZ = Math.max(b.maxZ, z1);
  }
}

/** Small stable string for a blob of JSON. Not a checksum, just a cache key. */
function hash32(s: string): string {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return (h >>> 0).toString(36);
}

export function buildScene(doc: DocState, lib: MaterialLibrary, opts: BuildOptions): BuiltScene {
  const { project, derived } = doc;
  const kit = new Kit();
  const root = new THREE.Group();
  root.name = "model";
  const roofGroup = new THREE.Group();
  roofGroup.name = "roof";
  const acc = new BoundsAcc();

  lib.begin(project.materials ?? []);
  opts.cache?.begin();
  // Assets are cached across rebuilds; a material edit has to invalidate them.
  const matKey = hash32(JSON.stringify(project.materials ?? []));

  const levels = new Map<string, Level>((project.levels ?? []).map((l) => [l.id, l]));
  const sortedLevels = [...levels.values()].sort((a, b) => a.elevation_mm - b.elevation_mm);
  const lowest = sortedLevels[0] ?? null;
  const groundMm = (lowest?.elevation_mm ?? 0) - PLINTH_MM;
  const activeLevel = (opts.activeLevelId && levels.get(opts.activeLevelId)) || lowest;

  const layer = (key: LayerKey) => project.layers?.find((l) => l.key === key);
  const visible = (key: LayerKey) => layer(key)?.visible !== false;
  const locked = (key: LayerKey) => layer(key)?.locked === true;

  /** Cutaway hides the levels above the active one and cuts the active one. */
  const levelShown = levelFilter(project, opts);
  const levelCut = (l: Level) => (opts.cutaway && activeLevel && l.id === activeLevel.id ? CUTAWAY_MM : null);

  // `Derived.footprints` can hold several detached buildings on one level,
  // largest first, plus one empty-polygon entry when there is none. Every
  // consumer here (slabs, roof, indoor/outdoor test) must handle all of them.
  const footprintsOf = (levelId: string): Pt[][] => groupFootprints(derived.footprints, levelId);

  const walls = new Map<string, Wall>();
  const openingsByWall = new Map<string, Opening[]>();
  for (const e of project.elements) {
    if (e.kind === "wall") walls.set(e.id, e);
    else if (e.kind === "opening") {
      const list = openingsByWall.get(e.wall_id) ?? [];
      list.push(e);
      openingsByWall.set(e.wall_id, list);
    }
  }
  const outlines = new Map((derived.walls ?? []).map((g) => [g.wall_id, g.outline]));

  // ------------------------------------------------------------------ walls
  lib.category = "wall";
  const wallOutlinesByLevel = new Map<string, Pt[]>();
  if (visible("walls")) {
    for (const wall of walls.values()) {
      const level = levels.get(wall.level_id) ?? lowest;
      if (!level || !levelShown(level)) continue;
      const height = wall.height_mm ?? level.height_mm;
      if (!(height > 0)) continue;
      // Walls on the lowest level run down to the ground so nothing floats
      // when there is no slab under them.
      const drop = level === lowest ? PLINTH_MM : 0;
      const cut = levelCut(level);
      const hosted = visible("openings") ? (openingsByWall.get(wall.id) ?? []) : [];
      const md = new MeshData();
      const ok = buildWallMesh(
        {
          start: wall.start,
          end: wall.end,
          thickness: wall.thickness_mm,
          height: height + drop,
          elevation: level.elevation_mm - drop,
          outline: outlines.get(wall.id),
          maxHeight: cut === null ? null : cut + drop,
          openings: hosted.map((o) => ({
            offset: o.offset_mm,
            width: o.width_mm,
            height: o.height_mm,
            sill: o.sill_mm + drop,
          })),
        },
        md,
      );
      if (!ok) continue;
      const mesh = kit.mesh(kit.fromMeshData(md), lib.get(wall.material_id, "mat-chb-painted"), wall.id);
      if (locked("walls")) mesh.userData.locked = true;
      root.add(mesh);
      const outline = outlines.get(wall.id);
      const pts = outline && outline.length >= 3 ? outline : [wall.start, wall.end];
      acc.add(pts, level.elevation_mm - drop, level.elevation_mm + (cut ?? height));
      const list = wallOutlinesByLevel.get(level.id) ?? [];
      list.push(...pts);
      wallOutlinesByLevel.set(level.id, list);
    }
  }

  // --------------------------------------------------------------- openings
  lib.category = "opening";
  if (visible("openings") && visible("walls")) {
    for (const [wallId, list] of openingsByWall) {
      const wall = walls.get(wallId);
      const level = wall ? (levels.get(wall.level_id) ?? lowest) : null;
      if (!wall || !level || !levelShown(level)) continue;
      const cut = levelCut(level);
      kit.cutY = cut === null ? null : (level.elevation_mm + cut) / 1000;
      for (const o of list) {
        const g = buildOpening(kit, lib, o, wall, level.elevation_mm);
        if (!g) continue;
        tagElement(g, o.id, locked("openings"));
        root.add(g);
      }
    }
    kit.cutY = null;
  }

  // ----------------------------------------------------------------- floors
  lib.category = "floor";
  // Stairwells: every flight's rectangle is cut out of the slab and the room
  // floors of the level at its top, so the flight is open above and a walk
  // up it never passes through the floor (walk/stairs.ts finds the flights).
  const wells = new Map<string, Pt[][]>();
  for (const flight of stairsOf(doc)) {
    if (!flight.topLevelId) continue;
    const list = wells.get(flight.topLevelId) ?? [];
    list.push(stairOutline(flight).outline);
    wells.set(flight.topLevelId, list);
  }
  for (const level of sortedLevels) {
    if (!levelShown(level)) continue;
    // A slab per detached building on this level.
    for (const fp of footprintsOf(level.id)) {
      const md = new MeshData();
      const depth = level === lowest ? PLINTH_MM : 200;
      // Slightly inside the outer wall faces, so the slab edge never fights
      // with the wall surfaces that run down to the ground.
      const inner = offsetPolygon(fp, -8);
      for (const piece of subtractWells(inner.length >= 3 ? inner : fp, wells.get(level.id))) {
        pushPrism(md, piece, {
          bottom: () => level.elevation_mm - depth,
          top: () => level.elevation_mm,
        });
      }
      root.add(kit.mesh(kit.fromMeshData(md), lib.get("mat-floor-concrete", undefined, false)));
      acc.add(fp, level.elevation_mm - depth, level.elevation_mm);
    }
  }
  const roomGeo = new Map((derived.rooms ?? []).map((r) => [r.room_id, r]));
  for (const e of project.elements) {
    if (e.kind !== "room") continue;
    const level = levels.get(e.level_id) ?? lowest;
    const poly = ensureCCW(roomGeo.get(e.id)?.polygon ?? []);
    if (!level || !levelShown(level) || poly.length < 3) continue;
    const md = new MeshData();
    for (const piece of subtractWells(poly, wells.get(level.id))) {
      pushPrism(md, piece, {
        bottom: () => level.elevation_mm,
        top: () => level.elevation_mm + FLOOR_FINISH_MM,
        skipBottom: true,
      });
    }
    if (md.triangleCount === 0) continue;
    const mesh = kit.mesh(kit.fromMeshData(md), lib.get(e.floor_material_id, "mat-tile-ceramic"));
    mesh.castShadow = false;
    mesh.userData.soft = true;
    if (visible("rooms")) {
      mesh.userData.elementId = e.id;
      if (locked("rooms")) mesh.userData.locked = true;
    }
    root.add(mesh);
    acc.add(poly, level.elevation_mm, level.elevation_mm + FLOOR_FINISH_MM);
  }

  // Rooms per level, for fixtures (which room a lamp lights) and ghost lights.
  const roomsByLevel = new Map<string, { id: string; polygon: Pt[]; label: Pt; areaM2: number }[]>();
  for (const e of project.elements) {
    if (e.kind !== "room") continue;
    const geo = roomGeo.get(e.id);
    if (!geo || geo.polygon.length < 3) continue;
    const level = levels.get(e.level_id) ?? lowest;
    if (!level) continue;
    const list = roomsByLevel.get(level.id) ?? [];
    list.push({ id: e.id, polygon: geo.polygon, label: geo.label_point, areaM2: Math.abs(geo.area_mm2) / 1e6 });
    roomsByLevel.set(level.id, list);
  }
  const roomAt = (levelId: string, p: Pt): string | null =>
    roomsByLevel.get(levelId)?.find((r) => pointInPolygon(p, r.polygon))?.id ?? null;

  // ---------------------------------------------------------------- columns
  lib.category = "column";
  if (visible("columns")) {
    for (const e of project.elements) {
      if (e.kind !== "column") continue;
      const level = levels.get(e.level_id) ?? lowest;
      if (!level || !levelShown(level)) continue;
      const drop = level === lowest ? PLINTH_MM : 0;
      const h = ((levelCut(level) ?? level.height_mm) + drop) / 1000;
      const w = Math.max(e.width_mm, 10) / 1000;
      const d = Math.max(e.shape === "round" ? e.width_mm : e.depth_mm, 10) / 1000;
      const g = new THREE.Group();
      const [x, y, z] = planToWorld(e.center.x, e.center.y, level.elevation_mm - drop);
      g.position.set(x, y, z);
      g.rotation.y = planRotationToWorld(e.rotation_deg);
      const mat = lib.get(e.material_id, "mat-concrete-fairface", false);
      if (e.shape === "round") kit.cylinder(g, mat, w / 2, h, 0, 0, 0);
      else kit.box(g, mat, w, h, d, 0, 0, 0);
      tagElement(g, e.id, locked("columns"));
      root.add(g);
      acc.add(orientedRect(e.center, e.width_mm, d * 1000, e.rotation_deg), level.elevation_mm, level.elevation_mm + h * 1000);
    }
  }

  // ----------------------------------------------------------------- stairs
  lib.category = "stair";
  if (visible("stairs")) {
    for (const e of project.elements) {
      if (e.kind !== "stair") continue;
      const level = levels.get(e.level_id) ?? lowest;
      if (!level || !levelShown(level)) continue;
      const cut = levelCut(level);
      const mesh = buildStair(kit, lib, e, level, cut === null ? null : cut / 1000);
      if (!mesh) continue;
      tagElement(mesh, e.id, locked("stairs"));
      root.add(mesh);
      const a = (e.rotation_deg * Math.PI) / 180;
      const mid = { x: e.origin.x - Math.sin(a) * (e.run_mm / 2), y: e.origin.y + Math.cos(a) * (e.run_mm / 2) };
      acc.add(orientedRect(mid, e.width_mm, e.run_mm, e.rotation_deg), level.elevation_mm, level.elevation_mm + (cut ?? level.height_mm));
    }
  }

  // ----------------------------------------------------------------- assets
  lib.category = "asset";
  const lamps: LampSpec[] = [];
  {
    for (const e of project.elements) {
      if (e.kind !== "asset") continue;
      const assetLayer = assetLayerKey(e.category);
      if (!visible(assetLayer)) continue;
      const assetsLocked = locked(assetLayer);
      const level = levels.get(e.level_id) ?? lowest;
      if (!level || !levelShown(level)) continue;
      // Outdoor items on the lowest level stand on the ground, not on the plinth height.
      const fps = footprintsOf(level.id);
      const outdoors = level === lowest && !fps.some((fp) => pointInPolygon(e.position, fp));
      const floorMm = outdoors ? groundMm : level.elevation_mm;
      const baseYMm = floorMm + Math.max(e.elevation_mm, 0);
      // Only indoor assets on the cut level are trimmed. Outdoor items (trees,
      // cars, plants) are not part of the building and stay full height.
      const cut = outdoors ? null : levelCut(level);
      const cutY = cut === null ? null : (level.elevation_mm + cut) / 1000;
      // A pack model is finished geometry: it cannot be trimmed at the cutaway
      // height the way the procedural boxes are, so anything that would be cut
      // (a wardrobe, a fridge) keeps its low-poly form while the cut is on.
      const topY = (baseYMm + Math.max(e.height_mm, 0)) / 1000;
      const usePack = opts.packModels !== false && (cutY === null || topY <= cutY) && modelPack.has(e.catalog_key);
      // An asset's form depends on nothing but these values, so an unchanged
      // one is handed straight back by the cache: a wall edit in a scene with
      // three hundred assets rebuilds one wall, not three hundred chairs.
      // A fixture's diffuser takes its lamp's color (MaterialLibrary.lampGlow).
      const lightKey = e.light ? `${Math.round(e.light.kelvin)}` : "";
      // A pendant's cord ends at the ceiling: the level height, or the
      // underside of the slab above (docs/CONTRACT.md, "Devices, fixtures and links").
      const ceilingMm = ceilingHeightMm(level, sortedLevels);
      const cacheKey = `${e.id}|${e.catalog_key}|${e.width_mm}|${e.depth_mm}|${e.height_mm}|${e.elevation_mm}|${baseYMm}|${cutY ?? "-"}|${assetsLocked}|${matKey}|${usePack ? "glb" : "proc"}|${lightKey}|${ceilingMm}`;
      let g = opts.cache?.take(cacheKey) ?? null;
      if (!g) {
        const own = new Kit();
        own.cutY = cutY;
        g = buildAssetForm(own, lib, e, baseYMm / 1000, usePack, { ceilingMm });
        own.cutY = null;
        // A pack model arrives with the pack's shared materials: dress it in
        // this library's copies, which the shell modes may fade.
        if (usePack) {
          g.traverse((o) => {
            const mesh = o as THREE.Mesh;
            if (mesh.isMesh && !Array.isArray(mesh.material) && !mesh.material.userData.libKey) mesh.material = lib.adopt(mesh.material);
          });
        }
        tagElement(g, e.id, assetsLocked);
        if (opts.cache) opts.cache.put(cacheKey, g, [...own.geometries]);
        else kit.adopt(own);
      }
      const [x, y, z] = planToWorld(e.position.x, e.position.y, baseYMm);
      g.position.set(x, y, z);
      g.rotation.y = planRotationToWorld(e.rotation_deg);
      root.add(g);
      if (e.light) {
        // A fixture never shades its own lamp: the light sits inside its form.
        g.traverse((o) => {
          if ((o as THREE.Mesh).isMesh) o.castShadow = false;
        });
        const lamp = addLamp(kit, lib, e, g, level.id, roomAt(level.id, e.position), cutY, assetsLocked, root);
        if (lamp) lamps.push(lamp);
      }
      acc.add(
        orientedRect(e.position, e.width_mm, e.depth_mm, e.rotation_deg),
        floorMm,
        floorMm + Math.max(e.elevation_mm, 0) + e.height_mm,
      );
    }
  }

  // Rooms get the light their lamps bounce, or a soft ghost light when they
  // have no fixture (light/lamps.ts).
  const lightRooms: RoomSpec[] = [];
  for (const level of sortedLevels) {
    if (!levelShown(level)) continue;
    const heightM = ceilingHeightMm(level, sortedLevels) / 1000;
    for (const r of roomsByLevel.get(level.id) ?? []) {
      const floor = planToWorld(r.label.x, r.label.y, level.elevation_mm + FLOOR_FINISH_MM);
      lightRooms.push({ roomId: r.id, levelId: level.id, floor, heightM, areaM2: r.areaM2 });
    }
  }

  // ------------------------------------------------------------------ pipes
  // After the assets, so the category switch below cannot leak into them.
  const pipes = buildPipes({
    doc,
    lib,
    kit,
    cache: opts.cache,
    levelOf: (id) => levels.get(id) ?? lowest,
    levelShown,
    layerVisible: visible,
    layerLocked: locked,
    slabDepth: (l) => (l === lowest ? PLINTH_MM : 200),
    addBounds: (pts, z0, z1) => acc.add(pts, z0, z1),
  });
  for (const solo of pipes.solos.values()) root.add(solo);
  for (const batch of pipes.batches) root.add(batch.mesh);

  // ------------------------------------------------------------------- roof
  lib.category = "roof";
  const top = sortedLevels[sortedLevels.length - 1];
  if (top && project.roof && project.roof.kind !== "none") {
    let fps = footprintsOf(top.id);
    if (fps.length === 0) {
      // No closed footprint yet: cover the walls of the top level if they
      // span a real area.
      const b = boundsOf(wallOutlinesByLevel.get(top.id) ?? []);
      if (b && b.maxX - b.minX > 1000 && b.maxY - b.minY > 1000) {
        fps = [
          [
            { x: b.minX, y: b.minY },
            { x: b.maxX, y: b.minY },
            { x: b.maxX, y: b.maxY },
            { x: b.minX, y: b.maxY },
          ],
        ];
      }
    }
    // One roof per detached building on the top level, same settings each.
    for (const fp of fps) {
      const roof = project.roof;
      const input: RoofInput = {
        kind: roof.kind,
        pitchDeg: roof.pitch_deg,
        overhang: roof.overhang_mm,
        thickness: roof.thickness_mm,
        ridgeAxis: roof.ridge_axis,
        footprint: fp,
        baseHeight: top.elevation_mm + top.height_mm,
        infillThickness: project.settings?.default_wall_thickness_mm ?? 150,
      };
      const md = new MeshData();
      const trim = new MeshData();
      const prof = buildRoofMesh(input, md, trim);
      if (prof && md.triangleCount > 0) {
        roofGroup.add(kit.mesh(kit.fromMeshData(md), lib.get(roof.material_id, "mat-roof-longspan")));
        // Soffit and fascia: plain off-white, the usual painted fibre cement board look.
        if (trim.triangleCount > 0) roofGroup.add(kit.mesh(kit.fromMeshData(trim), lib.plain("#ece9e1", 0.85)));
        const infill = new MeshData();
        buildRoofInfill(input, infill);
        if (infill.triangleCount > 0) {
          const firstWall = [...walls.values()].find((w) => w.level_id === top.id);
          roofGroup.add(kit.mesh(kit.fromMeshData(infill), lib.get(firstWall?.material_id, "mat-chb-painted")));
        }
        if (roof.kind !== "flat") {
          // Flat ceiling at the wall tops, so interiors read as closed rooms.
          const ceiling = new MeshData();
          pushPrism(ceiling, fp, { bottom: () => input.baseHeight, top: () => input.baseHeight + 20 });
          const m = kit.mesh(kit.fromMeshData(ceiling), lib.plain("#f4f2ec", 0.95));
          m.castShadow = false;
          roofGroup.add(m);
        }
        if (!opts.cutaway) acc.add(fp, input.baseHeight, prof.peak);
      }
    }
  }
  // Roof decks: the part of a level that the next level up does not cover,
  // at its ceiling (DECISIONS D26). They hide with the roof.
  for (let i = 0; i + 1 < sortedLevels.length; i++) {
    const level = sortedLevels[i];
    if (!levelShown(level)) continue;
    const pieces = deckPieces(footprintsOf(level.id), footprintsOf(sortedLevels[i + 1].id));
    if (pieces.length === 0) continue;
    const bottom = level.elevation_mm + ceilingHeightMm(level, sortedLevels);
    const top = bottom + DECK_MM + DECK_TOPPING_MM;
    const md = new MeshData();
    for (const piece of pieces) pushPrism(md, piece, { bottom: () => bottom, top: () => top });
    roofGroup.add(kit.mesh(kit.fromMeshData(md), lib.get("mat-floor-concrete", undefined, false)));
    if (!opts.cutaway) for (const piece of pieces) acc.add(piece, bottom, top);
  }
  roofGroup.visible = !opts.cutaway;
  root.add(roofGroup);

  // ---------------------------------------------------------- removed ghosts
  lib.category = "ghost";
  if (opts.ghostsRemoved && opts.ghostsRemoved.ids.length > 0) {
    root.add(buildGhosts(opts.ghostsRemoved.project, opts.ghostsRemoved.ids, lib, kit));
  }
  // Materials asked for outside a build (reference model placeholders) belong to no part of the model.
  lib.category = "";

  opts.cache?.end();
  // Materials of reused groups were never requested during this build: hold
  // on to them so `lib.end` does not dispose a material still on screen.
  root.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (mesh.isMesh && !Array.isArray(mesh.material)) lib.keep(mesh.material);
  });
  lib.end();

  const byElement = new Map<string, THREE.Mesh[]>();
  root.traverse((o) => {
    const id = o.userData.elementId as string | undefined;
    if (!id || !(o as THREE.Mesh).isMesh) return;
    const list = byElement.get(id) ?? [];
    list.push(o as THREE.Mesh);
    byElement.set(id, list);
  });

  const empty = acc.b === null;
  const bounds = acc.b ?? { ...EMPTY_BOUNDS, minZ: groundMm, maxZ: groundMm + 3000 };
  bounds.minZ = Math.min(bounds.minZ, groundMm);
  const contact = lowest ? boundsOf(footprintsOf(lowest.id).flat()) : null;
  return { root, roofGroup, bounds, empty, groundY: groundMm / 1000, contact, kit, byElement, pipes, lamps, lightRooms };
}

/**
 * A plan polygon minus convex holes (stairwells), as pieces to extrude one by
 * one: for each edge of a hole, the part of what is left outside that edge is
 * a piece, and the rest carries on to the next edge. What is inside every
 * edge is the hole, and is dropped. A hole that misses the polygon leaves it
 * whole.
 */
export function subtractWells(poly: Pt[], holes: Pt[][] | undefined): Pt[][] {
  let pieces = [poly];
  for (const hole of holes ?? []) {
    const r = ensureCCW(hole);
    if (r.length < 3) continue;
    const next: Pt[][] = [];
    for (const piece of pieces) {
      let rest = piece;
      for (let i = 0; i < r.length && rest.length >= 3; i++) {
        const a = r[i];
        const b = r[(i + 1) % r.length];
        const len = Math.hypot(b.x - a.x, b.y - a.y);
        if (len < 1e-6) continue;
        // Outward normal of a counter-clockwise edge.
        const nx = (b.y - a.y) / len;
        const ny = -(b.x - a.x) / len;
        const c = nx * a.x + ny * a.y;
        const outside = clipHalfPlane(rest, -nx, -ny, -c);
        if (outside.length >= 3 && Math.abs(signedArea(outside)) > 1) next.push(outside);
        rest = clipHalfPlane(rest, nx, ny, c);
      }
    }
    pieces = next;
  }
  return pieces;
}

const tmpLamp = new THREE.Vector3();

/**
 * The lamp of a lit fixture, for the light rig (light/lamps.ts). The catalog
 * form says where its light comes from and which way it shines
 * (`group.userData.lamp`, scene/assets.ts) and draws its glowing parts
 * (`mesh.userData.lampPart`); light/fixtures.ts picks the kind of light. A
 * form that has no glowing part (a model the catalog does not know) gets a
 * small glowing bulb at the anchor, so a lit object still shows its lamp;
 * above the cutaway height it is left out, the light is not: rooms stay lit
 * when cut.
 */
function addLamp(
  kit: Kit,
  lib: MaterialLibrary,
  e: Asset,
  form: THREE.Object3D,
  levelId: string,
  roomId: string | null,
  cutY: number | null,
  locked: boolean,
  root: THREE.Group,
): LampSpec | null {
  const light = e.light;
  if (!light || !(light.lumens > 0)) return null;
  const size = fixtureSize(e);
  const info = form.userData.lamp as LampInfo | undefined;
  const known = info ?? lampAnchor(e.catalog_key, size.w, size.d, size.h);
  const anchor: [number, number, number] = known ? [...known.anchor] : defaultAnchor(size.h);
  const aim = known?.aim ?? null;
  form.updateMatrix();
  form.updateMatrixWorld(true);
  // The glowing parts the form drew, and how much surface they have.
  let glowAreaM2 = glowArea(form, (mesh) => {
    const mat = mesh.material as THREE.Material | THREE.Material[];
    return mesh.userData.lampPart === true || (!Array.isArray(mat) && mat.userData?.lampGlow === e.id);
  });
  if (glowAreaM2 === 0) {
    const bulb = 0.035;
    const at = tmpLamp.set(...anchor).applyMatrix4(form.matrix);
    if (cutY === null || at.y < cutY) {
      const g = new THREE.Group();
      g.name = "lamp";
      g.position.copy(form.position);
      g.rotation.copy(form.rotation);
      const mesh = kit.ball(g, lib.lampGlow(e.id, light.kelvin), bulb, bulb, bulb, anchor[0], anchor[1], anchor[2]);
      if (mesh) {
        mesh.castShadow = false;
        mesh.receiveShadow = false;
        mesh.userData.lampPart = true;
      }
      tagElement(g, e.id, locked);
      root.add(g);
      glowAreaM2 = 4 * Math.PI * bulb * bulb;
    }
  }
  const optics = lampOptics(e.catalog_key, aim, size, anchor);
  const pos = tmpLamp.set(...anchor).applyMatrix4(form.matrix);
  const position: [number, number, number] = [pos.x, pos.y, pos.z];
  let direction: [number, number, number] | null = null;
  if (optics.kind === "spot") {
    const d = new THREE.Vector3(...(aim ?? [0, -1, 0])).normalize().applyQuaternion(form.quaternion);
    direction = [d.x, d.y, d.z];
  }
  return {
    id: e.id,
    key: e.catalog_key,
    kind: optics.kind,
    position,
    direction,
    coneDeg: optics.coneDeg,
    penumbra: optics.penumbra,
    radius: optics.radius,
    lumens: light.lumens,
    kelvin: light.kelvin,
    on: light.on,
    levelId,
    roomId,
    glowAreaM2: glowAreaM2 > 0 ? glowAreaM2 : null,
  };
}

const triA = new THREE.Vector3();
const triB = new THREE.Vector3();
const triC = new THREE.Vector3();

/** Surface area, m2, of the meshes under `root` that `pick` accepts, in `root`'s own frame. */
export function glowArea(root: THREE.Object3D, pick: (mesh: THREE.Mesh) => boolean): number {
  const toRoot = root.matrixWorld.clone().invert();
  const m = new THREE.Matrix4();
  let area = 0;
  root.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (!mesh.isMesh || !pick(mesh)) return;
    const pos = mesh.geometry.getAttribute("position");
    if (!pos) return;
    m.multiplyMatrices(toRoot, mesh.matrixWorld);
    const index = mesh.geometry.getIndex();
    const count = index ? index.count : pos.count;
    for (let i = 0; i + 2 < count; i += 3) {
      const a = index ? index.getX(i) : i;
      const b = index ? index.getX(i + 1) : i + 1;
      const c = index ? index.getX(i + 2) : i + 2;
      triA.fromBufferAttribute(pos, a).applyMatrix4(m);
      triB.fromBufferAttribute(pos, b).applyMatrix4(m);
      triC.fromBufferAttribute(pos, c).applyMatrix4(m);
      area += triB.sub(triA).cross(triC.sub(triA)).length() / 2;
    }
  });
  return area;
}

/**
 * Concrete stair: sawtooth profile with a sloped soffit, extruded across the
 * width. `maxHeight` (meters above the level floor) is the cutaway height:
 * the profile is clipped to it with the same half-plane cut the plan outlines
 * use, so a cutaway stair stops flush with the cut walls.
 */
function buildStair(kit: Kit, lib: MaterialLibrary, s: Stair, level: Level, maxHeight?: number | null): THREE.Mesh | null {
  const risers = Math.min(Math.max(Math.round(s.riser_count), 1), 200);
  // Contract: going depth = run_mm / riser_count. The last step is flush with the floor above.
  const treads = risers;
  const run = s.run_mm / 1000;
  const width = s.width_mm / 1000;
  const totalRise = level.height_mm / 1000;
  if (!(run > 0.01 && width > 0.01 && totalRise > 0.01)) return null;
  if (maxHeight != null && maxHeight < 0.02) return null;
  const rise = totalRise / risers;
  const going = run / treads;
  const pts: Pt[] = [{ x: 0, y: 0 }];
  for (let i = 0; i < treads; i++) {
    pts.push({ x: i * going, y: (i + 1) * rise });
    pts.push({ x: (i + 1) * going, y: (i + 1) * rise });
  }
  const waist = 0.2;
  const topH = treads * rise;
  const under = Math.max(topH - rise - waist, 0); // soffit height under the top step
  pts.push({ x: run, y: under });
  if (under > 0) {
    const slope = rise / going;
    const y0 = Math.max(run - under / slope, going * 0.5);
    pts.push({ x: y0, y: 0 });
  }
  const poly = maxHeight != null && maxHeight < topH ? clipHalfPlane(pts, 0, 1, maxHeight) : pts;
  if (poly.length < 3) return null;
  const shape = new THREE.Shape();
  shape.moveTo(poly[0].x, poly[0].y);
  for (let i = 1; i < poly.length; i++) shape.lineTo(poly[i].x, poly[i].y);
  shape.closePath();
  const geo = kit.track(new THREE.ExtrudeGeometry(shape, { depth: width, bevelEnabled: false, steps: 1 }));
  // Shape x is the run (plan +y, world -z), shape y is up, the extrusion is the width.
  geo.rotateY(Math.PI / 2);
  geo.translate(-width / 2, 0, 0);
  const mesh = kit.mesh(geo, lib.get("mat-floor-concrete", undefined, false));
  const [x, y, z] = planToWorld(s.origin.x, s.origin.y, level.elevation_mm);
  mesh.position.set(x, y, z);
  mesh.rotation.y = planRotationToWorld(s.rotation_deg);
  return mesh;
}

/**
 * Faint red-tinted forms for elements an AI proposal would remove. Built from
 * the committed project (the elements are already gone from the preview
 * state), so it does not touch `Derived` at all: walls fall back to a plain
 * rectangle and stairs and columns are not clipped by the cutaway. Cheap and
 * approximate on purpose, since this is only shown while a proposal is live.
 */
export function buildGhosts(project: Project, ids: string[], lib: MaterialLibrary, kit: Kit): THREE.Group {
  const group = new THREE.Group();
  group.name = "ghosts";
  if (ids.length === 0) return group;
  const idSet = new Set(ids);
  const levels = new Map<string, Level>((project.levels ?? []).map((l) => [l.id, l]));
  const lowest = [...levels.values()].sort((a, b) => a.elevation_mm - b.elevation_mm)[0] ?? null;
  const walls = new Map<string, Wall>();
  for (const e of project.elements) if (e.kind === "wall") walls.set(e.id, e);
  const ghostMat = lib.ghost("#c23b3b");

  const tint = (obj: THREE.Object3D) => {
    obj.traverse((o) => {
      const mesh = o as THREE.Mesh;
      if (!mesh.isMesh) return;
      mesh.material = ghostMat;
      mesh.castShadow = false;
      mesh.receiveShadow = false;
      mesh.renderOrder = 3;
    });
  };

  for (const e of project.elements) {
    if (!idSet.has(e.id)) continue;
    if (e.kind === "wall") {
      const level = levels.get(e.level_id) ?? lowest;
      if (!level) continue;
      const drop = level === lowest ? PLINTH_MM : 0;
      const md = new MeshData();
      const ok = buildWallMesh(
        {
          start: e.start,
          end: e.end,
          thickness: e.thickness_mm,
          height: (e.height_mm ?? level.height_mm) + drop,
          elevation: level.elevation_mm - drop,
          outline: null,
          openings: [],
          maxHeight: null,
        },
        md,
      );
      if (!ok) continue;
      const mesh = kit.mesh(kit.fromMeshData(md), ghostMat);
      mesh.castShadow = false;
      mesh.receiveShadow = false;
      mesh.renderOrder = 3;
      group.add(mesh);
    } else if (e.kind === "opening") {
      const wall = walls.get(e.wall_id);
      const level = wall ? (levels.get(wall.level_id) ?? lowest) : null;
      if (!wall || !level) continue;
      const g = buildOpening(kit, lib, e, wall, level.elevation_mm);
      if (!g) continue;
      tint(g);
      group.add(g);
    } else if (e.kind === "column") {
      const level = levels.get(e.level_id) ?? lowest;
      if (!level) continue;
      const drop = level === lowest ? PLINTH_MM : 0;
      const h = (level.height_mm + drop) / 1000;
      const w = Math.max(e.width_mm, 10) / 1000;
      const d = Math.max(e.shape === "round" ? e.width_mm : e.depth_mm, 10) / 1000;
      const g = new THREE.Group();
      const [x, y, z] = planToWorld(e.center.x, e.center.y, level.elevation_mm - drop);
      g.position.set(x, y, z);
      g.rotation.y = planRotationToWorld(e.rotation_deg);
      if (e.shape === "round") kit.cylinder(g, ghostMat, w / 2, h, 0, 0, 0);
      else kit.box(g, ghostMat, w, h, d, 0, 0, 0);
      tint(g);
      group.add(g);
    } else if (e.kind === "stair") {
      const level = levels.get(e.level_id) ?? lowest;
      if (!level) continue;
      const mesh = buildStair(kit, lib, e, level, null);
      if (!mesh) continue;
      tint(mesh);
      group.add(mesh);
    } else if (e.kind === "pipe") {
      const level = levels.get(e.level_id) ?? lowest;
      if (!level) continue;
      const mesh = buildPipeGhost(e, level.elevation_mm, ghostMat, kit);
      if (mesh) group.add(mesh);
    } else if (e.kind === "asset") {
      const level = levels.get(e.level_id) ?? lowest;
      if (!level) continue;
      const floorMm = level.elevation_mm + Math.max(e.elevation_mm, 0);
      const g = buildAssetForm(kit, lib, e, floorMm / 1000, false, { ceilingMm: ceilingHeightMm(level, project.levels ?? []) });
      const [x, y, z] = planToWorld(e.position.x, e.position.y, floorMm);
      g.position.set(x, y, z);
      g.rotation.y = planRotationToWorld(e.rotation_deg);
      tint(g);
      group.add(g);
    }
  }
  return group;
}
