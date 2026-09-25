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

import type { DocState, Level } from "../../contract/bindings";
import { groupFootprints } from "../scene/buildScene";
import { pipeLayer } from "../scene/pipes";
import { buildCollisionWorld, EMPTY_WORLD, levelResolver, type CollisionWorld } from "../geom/collision";
import type { Pt } from "../geom/coords";
import { boundsOf, pointInPolygon } from "../geom/polygon";
import { roomsOn } from "../geom/walkStart";
import type { WalkNav } from "../engine/walker";
import type { MinimapScene } from "./minimap";
import { approachHeight, flightLevelId, onFlight, rampHeight, stairColliders, stairLocal, stairOutline, stairsOf, type StairInfo } from "./stairs";

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

  constructor(
    readonly doc: DocState,
    readonly eyeMm: number,
  ) {
    this.stairs = stairsOf(doc);
    this.levels = new Map((doc.project.levels ?? []).map((l) => [l.id, l]));
    this.sorted = [...this.levels.values()].sort((a, b) => a.elevation_mm - b.elevation_mm);
    this.resolveLevel = levelResolver(doc);
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
        s.levelId === id ? stairColliders(s, "lower") : s.topLevelId === id ? stairColliders(s, "upper") : [],
      );
      world = strips.length > 0 ? { ...base, colliders: [...base.colliders, ...strips] } : base;
      this.worlds.set(id, world);
    }
    return world;
  }

  flightWorld(s: StairInfo, high: boolean): CollisionWorld {
    const levelId = high && s.topLevelId ? s.topLevelId : s.levelId;
    const key = `${s.id}|${levelId}`;
    let world = this.flights.get(key);
    if (!world) {
      const base = this.levelWorld(levelId);
      world = { ...base, colliders: [...base.colliders.filter((c) => c.id !== s.id), ...stairColliders(s, "flight")] };
      this.flights.set(key, world);
    }
    return world;
  }

  stairAt(levelId: string, p: Pt): StairInfo | null {
    for (const s of this.stairs) {
      if ((s.levelId === levelId || s.topLevelId === levelId) && onFlight(s, p)) return s;
    }
    return null;
  }

  approach(levelId: string, p: Pt): number {
    for (const s of this.stairs) {
      if (s.levelId !== levelId) continue;
      const h = approachHeight(s, p);
      if (h !== null) return h;
    }
    return 0;
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

  /** True when `p` is over the floor of a level: inside one of its buildings or rooms, or its walls' extent. */
  inside(levelId: string, p: Pt): boolean {
    let polys = this.areas.get(levelId);
    if (!polys) {
      polys = [...groupFootprints(this.doc.derived?.footprints, levelId), ...roomsOn(this.doc, levelId).map((r) => r.polygon)];
      if (polys.length === 0) {
        const b = boundsOf(this.levelWorld(levelId).wallPieces.flat());
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
