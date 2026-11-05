// What a walker can stand on and bump into, on every level of one document.
// Pure plan math in millimeters, no three.js, no DOM.
//
// `WalkWorlds` answers the walker (engine/walker.ts, `WalkNav`): the
// collision world of a level (walls, columns, objects, and the strips around
// its stairs), the world on a flight (its sides, and the walls of the lower
// level on the lower half and of the upper level on the upper half), which
// flight a point is on, and where a walker lands when fly mode ends. Worlds
// are built the first time they are asked for and kept until the document or
// the eye height changes, when the engine makes a new `WalkWorlds`.

import type { DocState, Level, Slab } from "../../contract/bindings";
import { groupFootprints, subtractWells } from "../scene/buildScene";
import { docSlabs } from "../scene/slabs";
import { pipeLayer } from "../scene/pipes";
import { buildCollisionWorld, EMPTY_WORLD, levelResolver, polyCollider, railingColliders, type Collider, type CollisionWorld } from "../geom/collision";
import type { Pt } from "../geom/coords";
import { boundsOf, pointInPolygon } from "../geom/polygon";
import { roomsOn } from "../geom/walkStart";
import type { WalkNav } from "../engine/walker";
import type { MinimapScene } from "./minimap";
import {
  approachHeight,
  flightLevelId,
  holdOnFlight,
  onFlight,
  onFootprint,
  rampHeight,
  stairColliders,
  stairLocal,
  stairOutline,
  stairPieces,
  stairPoint,
  stairsOf,
  uAtHeight,
  STAIR_OPEN_RISE_MM,
  type StairInfo,
  type StairOpen,
} from "./stairs";

/** How far beside a flight the floor is looked for, to open its first and last treads at the sides. */
const SIDE_PROBE_MM = 400;

/** A floor this far above the feet still counts as under them when fly mode ends, and a flight snaps as close. */
export const LAND_SNAP_MM = 600;
/** A floor hit this far above a level's floor is still that level (floor finishes, rugs). */
const FLOOR_HIT_SLACK_MM = 100;

export class WalkWorlds implements WalkNav {
  readonly stairs: StairInfo[];
  private readonly levels: Map<string, Level>;
  /** Lowest first. */
  private readonly sorted: Level[];
  private readonly resolveLevel: (id: string) => Level | null;
  private readonly worlds = new Map<string, CollisionWorld>();
  private readonly flights = new Map<string, CollisionWorld>();
  private readonly areas = new Map<string, Pt[][]>();
  private readonly open = new Map<string, StairOpen>();
  /** Slabs on a shown Slabs layer (DECISIONS D50): walkable ones and voids. */
  private readonly slabs: Slab[];

  constructor(
    readonly doc: DocState,
    readonly eyeMm: number,
  ) {
    this.stairs = stairsOf(doc);
    this.levels = new Map((doc.project.levels ?? []).map((l) => [l.id, l]));
    this.sorted = [...this.levels.values()].sort((a, b) => a.elevation_mm - b.elevation_mm);
    this.resolveLevel = levelResolver(doc);
    const layerOn = (key: string) => doc.project.layers?.find((l) => l.key === key)?.visible !== false;
    this.slabs = layerOn("slabs") ? docSlabs(doc) : [];
  }

  /** A level id the document has: unknown ids fall back to the lowest level. */
  levelId(id: string | null): string | null {
    return id !== null ? (this.resolveLevel(id)?.id ?? null) : (this.sorted[0]?.id ?? null);
  }

  lowestLevelId(): string | null {
    return this.sorted[0]?.id ?? null;
  }

  elevation(levelId: string): number {
    return this.resolveLevel(levelId)?.elevation_mm ?? 0;
  }

  levelWorld(levelId: string): CollisionWorld {
    const id = this.levelId(levelId) ?? levelId;
    let world = this.worlds.get(id);
    if (!world) {
      const base = buildCollisionWorld(this.doc, id, { eyeMm: this.eyeMm });
      const strips = this.stairs.flatMap((s) =>
        s.levelId === id ? stairColliders(s, "lower", this.openSides(s)) : s.topLevelId === id ? stairColliders(s, "upper", this.openSides(s)) : [],
      );
      const guards = this.slabColliders(id);
      world = strips.length + guards.length > 0 ? { ...base, colliders: [...base.colliders, ...strips, ...guards] } : base;
      this.worlds.set(id, world);
    }
    return world;
  }

  /**
   * What blocks a walker on a flight: the walls and objects of its lower
   * level (its upper level on the upper half), less the flight's own strips
   * and anything standing on its footprint. The sides are held by
   * `holdOnFlight`.
   */
  flightWorld(s: StairInfo, high: boolean): CollisionWorld {
    const levelId = high && s.topLevelId ? s.topLevelId : s.levelId;
    const key = `${s.id}|${levelId}`;
    let world = this.flights.get(key);
    if (!world) {
      const base = this.levelWorld(levelId);
      // Rails stay: the walker meets exactly the runs the engine gives (a rail
      // the engine draws across the head stops it; a head left open does not).
      world = { ...base, colliders: base.colliders.filter((c) => c.source === "railing" || (c.id !== s.id && !onFootprint(s, c))) };
      this.flights.set(key, world);
    }
    return world;
  }

  holdOnFlight(s: StairInfo, p: Pt, r: number): Pt {
    return holdOnFlight(s, p, r, this.openSides(s));
  }

  /**
   * Which sides of a flight's first and last treads are open: where the floor
   * of the level at that end is beside them, 400 mm out from the side.
   */
  openSides(s: StairInfo): StairOpen {
    let open = this.open.get(s.id);
    if (!open) {
      const hw = s.width / 2 + SIDE_PROBE_MM;
      const lowU = uAtHeight(s, STAIR_OPEN_RISE_MM) / 2;
      const highU = (uAtHeight(s, s.rise - STAIR_OPEN_RISE_MM) + s.run) / 2;
      const floorAt = (levelId: string | null, u: number, v: number) =>
        levelId !== null && (levelId === this.lowestLevelId() || this.inside(levelId, stairPoint(s, u, v)));
      open = {
        low: [floorAt(s.levelId, lowU, -hw), floorAt(s.levelId, lowU, hw)],
        high: [floorAt(s.topLevelId, highU, -hw), floorAt(s.topLevelId, highU, hw)],
      };
      this.open.set(s.id, open);
    }
    return open;
  }

  stairAt(levelId: string, p: Pt): StairInfo | null {
    for (const s of this.stairs) {
      if ((s.levelId === levelId || s.topLevelId === levelId) && onFlight(s, p)) return s;
    }
    return null;
  }

  /**
   * Height above the level floor under `p`: the start of a flight's walking
   * line near its foot, else the top of a floor, balcony or deck slab the
   * walker stands on (`top_offset_mm`: a sunken deck is below the floor).
   */
  approach(levelId: string, p: Pt): number {
    for (const s of this.stairs) {
      if (s.levelId !== levelId) continue;
      const h = approachHeight(s, p);
      if (h !== null) return h;
    }
    for (const s of this.slabs) {
      if (s.slab_kind === "void" || this.levelId(s.level_id) !== levelId) continue;
      if (pointInPolygon(p, s.polygon)) return s.top_offset_mm;
    }
    return 0;
  }

  /**
   * What the slabs of a level add to its collision world: every guard rail
   * standing on it (slab edges, stairwell guards; a flight's own sides are
   * held by `holdOnFlight`), and its voids, less any flight that arrives in
   * one, so a walker never steps into a double height space.
   */
  private slabColliders(levelId: string): Collider[] {
    const out: Collider[] = [];
    const layerOn = (key: string) => this.doc.project.layers?.find((l) => l.key === key)?.visible !== false;
    for (const run of this.doc.derived?.railings ?? []) {
      if (run.kind === "stair_side" || this.levelId(run.level_id) !== levelId) continue;
      if (!layerOn(run.kind === "slab_edge" ? "slabs" : "stairs")) continue;
      out.push(...railingColliders(run.points, run.host_id));
    }
    const flights = this.stairs.filter((s) => s.levelId === levelId || s.topLevelId === levelId).flatMap((s) => stairPieces(s));
    for (const s of this.slabs) {
      if (s.slab_kind !== "void" || this.levelId(s.level_id) !== levelId) continue;
      for (const piece of subtractWells(s.polygon, flights)) {
        const c = polyCollider(piece, "void", s.id);
        if (c) out.push(c);
      }
    }
    return out;
  }

  /** The level a floor at height `z` (mm) belongs to: the highest one at or below it, else the lowest. */
  levelAtHeight(z: number): string | null {
    for (let i = this.sorted.length - 1; i >= 0; i--) {
      if (this.sorted[i].elevation_mm <= z + FLOOR_HIT_SLACK_MM) return this.sorted[i].id;
    }
    return this.sorted[0]?.id ?? null;
  }

  /**
   * Where a walker lands when fly mode ends at plan `p` with its feet at
   * `feetZ`: on a flight when the feet are near its walking line, else on the
   * highest floor at or below the feet that is under `p` (the lowest level is
   * the ground and is everywhere).
   */
  landing(p: Pt, feetZ: number): { levelId: string; stair: StairInfo | null } | null {
    for (const s of this.stairs) {
      if (!onFlight(s, p)) continue;
      const { u } = stairLocal(s, p);
      if (Math.abs(feetZ - (s.bottomZ + rampHeight(s, u))) <= LAND_SNAP_MM) return { levelId: flightLevelId(s, u), stair: s };
    }
    for (let i = this.sorted.length - 1; i >= 0; i--) {
      const l = this.sorted[i];
      if (l.elevation_mm > feetZ + LAND_SNAP_MM) continue;
      if (i === 0 || this.inside(l.id, p)) return { levelId: l.id, stair: null };
    }
    const low = this.sorted[0];
    return low ? { levelId: low.id, stair: null } : null;
  }

  /**
   * True when `p` is over the floor of a level: inside one of its buildings,
   * rooms or walkable slabs, or its walls' extent, and not over one of its voids.
   */
  inside(levelId: string, p: Pt): boolean {
    const onLevel = (s: Slab) => this.levelId(s.level_id) === levelId;
    if (this.slabs.some((s) => s.slab_kind === "void" && onLevel(s) && pointInPolygon(p, s.polygon))) return false;
    let polys = this.areas.get(levelId);
    if (!polys) {
      polys = [
        ...groupFootprints(this.doc.derived?.footprints, levelId),
        ...roomsOn(this.doc, levelId).map((r) => r.polygon),
        ...this.slabs.filter((s) => s.slab_kind !== "void" && onLevel(s)).map((s) => s.polygon),
      ];
      if (polys.length === 0) {
        // The level's own walls, without the stair strips (those ask this very question).
        const b = boundsOf(buildCollisionWorld(this.doc, levelId, { eyeMm: this.eyeMm }).wallPieces.flat());
        if (b) polys.push([{ x: b.minX, y: b.minY }, { x: b.maxX, y: b.minY }, { x: b.maxX, y: b.maxY }, { x: b.minX, y: b.maxY }]);
      }
      this.areas.set(levelId, polys);
    }
    return polys.some((poly) => pointInPolygon(p, poly));
  }

  /** The minimap's picture of a level: its walls, rooms, pipes and the flights that start or end on it. */
  minimapScene(levelId: string | null, version: number): MinimapScene {
    const doc = this.doc;
    const id = this.levelId(levelId);
    const world = id ? this.levelWorld(id) : EMPTY_WORLD;
    const layerOn = (key: string) => doc.project.layers?.find((l) => l.key === key)?.visible !== false;
    return {
      version,
      world,
      rooms: id ? roomsOn(doc, id).map((r) => r.polygon) : [],
      pipes: doc.project.elements.flatMap((e) =>
        e.kind === "pipe" && layerOn(pipeLayer(e.system)) && id !== null && this.levelId(e.level_id) === id
          ? [{ system: e.system, points: e.points, diameterMm: e.diameter_mm }]
          : [],
      ),
      stairs: this.stairs.filter((s) => s.levelId === id || s.topLevelId === id).map((s) => ({ ...stairOutline(s), up: s.levelId === id })),
    };
  }
}
