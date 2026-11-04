import * as THREE from "three";
import { describe, expect, it } from "vitest";
import type { DocState, RailingRun } from "../../contract/bindings";
import { MeshData } from "../geom/meshData";
import { walkFov, WALK_VFOV_MAX, WALK_VFOV_MIN } from "../engine/walker";
import { tameHdri } from "../engine/environment";
import { traceGlass } from "../render/renderScene";
import { signedArea } from "../geom/polygon";
import { BALCONY, slabDoc, STAIR, twoLevelDoc, VOID } from "../walk/testModel";
import { buildScene, exteriorSide } from "./buildScene";
import { MaterialLibrary } from "./materials";
import { Highlight } from "../engine/highlights";
import { MeshFade } from "../engine/meshFade";
import { convexPieces, pushFascia, pushRailing, pushSkirting, RAIL, skirtingRuns, SKIRTING_HEIGHT_MM, type RailingMeshes } from "./trim";

const rect = (x0: number, y0: number, x1: number, y1: number) => [
  { x: x0, y: y0 },
  { x: x1, y: y0 },
  { x: x1, y: y1 },
  { x: x0, y: y1 },
];

/** Plan heights (mm) of a MeshData: world y is up, in meters. */
function heights(md: MeshData): { min: number; max: number } {
  const b = md.bounds();
  return b ? { min: b.min[1] * 1000, max: b.max[1] * 1000 } : { min: NaN, max: NaN };
}

const meshes = (): RailingMeshes => ({ steel: new MeshData(), glass: new MeshData(), solid: new MeshData() });

describe("skirting", () => {
  const door = { a: { x: 1000, y: 0 }, b: { x: 1900, y: 0 }, halfThick: 75 };

  it("runs along every wall and opens at the doors", () => {
    const runs = skirtingRuns(rect(75, 75, 3925, 2925), [door]);
    // The south edge splits round the door; the other three are whole.
    expect(runs).toHaveLength(5);
    const south = runs.filter(([a, b]) => Math.abs(a.y - 75) < 1 && Math.abs(b.y - 75) < 1);
    expect(south).toHaveLength(2);
    expect(south.map(([a, b]) => [Math.round(Math.min(a.x, b.x)), Math.round(Math.max(a.x, b.x))])).toEqual([
      [75, 1000],
      [1900, 3925],
    ]);
  });

  it("ignores a door in a wall it does not follow", () => {
    const far = { a: { x: 1000, y: 3000 + 400 }, b: { x: 1900, y: 3400 }, halfThick: 75 };
    expect(skirtingRuns(rect(75, 75, 3925, 2925), [far])).toHaveLength(4);
  });

  it("stands on the floor, 100 mm tall, inside the room", () => {
    const md = new MeshData();
    pushSkirting(md, rect(0, 0, 3000, 3000), [], 12);
    const h = heights(md);
    expect(h.min).toBeCloseTo(12, 3);
    expect(h.max).toBeCloseTo(12 + SKIRTING_HEIGHT_MM, 3);
    const b = md.bounds()!;
    // Plan x stays inside the room: the strip is on the room side of the wall face.
    expect(b.min[0]).toBeGreaterThanOrEqual(-1e-6);
    expect(b.max[0]).toBeLessThanOrEqual(3 + 1e-6);
  });
});

describe("railings", () => {
  const straight: Pick<RailingRun, "points" | "height_mm" | "style"> = {
    points: [
      { x: 0, y: 0, z: 0 },
      { x: 3000, y: 0, z: 0 },
    ],
    height_mm: 1000,
    style: "bars",
  };

  it("draws bars as posts, a top and a bottom rail and upright bars", () => {
    const out = meshes();
    pushRailing(out, straight, 3000);
    const h = heights(out.steel);
    expect(h.min).toBeCloseTo(3000, 3);
    expect(h.max).toBeCloseTo(4000, 3);
    // 3 posts, 2 rails and about 25 bars, 12 triangles each.
    expect(out.steel.triangleCount).toBeGreaterThan(25 * 12);
    expect(out.glass.triangleCount).toBe(0);
  });

  it("draws glass as a panel under a handrail", () => {
    const out = meshes();
    pushRailing(out, { ...straight, style: "glass" }, 0);
    const g = heights(out.glass);
    expect(g.min).toBeCloseTo(RAIL.glassFoot, 3);
    expect(g.max).toBeCloseTo(1000 - RAIL.topD, 3);
    expect(heights(out.steel).max).toBeCloseTo(1000, 3);
  });

  it("draws solid as a parapet with a cap", () => {
    const out = meshes();
    pushRailing(out, { ...straight, style: "solid" }, 0);
    expect(heights(out.solid).max).toBeCloseTo(1000, 3);
    expect(out.steel.triangleCount).toBe(0);
  });

  it("follows a flight's slope", () => {
    const out = meshes();
    pushRailing(out, { points: [{ x: 0, y: 0, z: 0 }, { x: 0, y: 3000, z: 1500 }], height_mm: 900, style: "bars" }, 0);
    expect(heights(out.steel).max).toBeCloseTo(2400, 3);
  });

  it("is cut at the cutaway height", () => {
    const out = meshes();
    pushRailing(out, { points: [{ x: 0, y: 0, z: 0 }, { x: 0, y: 3000, z: 1500 }], height_mm: 900, style: "bars" }, 0, 1200);
    expect(heights(out.steel).max).toBeLessThanOrEqual(1200 + 1e-6);
  });
});

describe("voids and fascia", () => {
  it("splits a void into convex pieces", () => {
    expect(convexPieces(rect(0, 0, 2000, 2000))).toHaveLength(1);
    const ell = [
      { x: 0, y: 0 },
      { x: 2000, y: 0 },
      { x: 2000, y: 1000 },
      { x: 1000, y: 1000 },
      { x: 1000, y: 2000 },
      { x: 0, y: 2000 },
    ];
    const parts = convexPieces(ell);
    expect(parts).toHaveLength(4);
    const area = parts.reduce((sum, p) => sum + Math.abs(signedArea(p)), 0);
    expect(area).toBeCloseTo(3_000_000, 0);
  });

  it("hangs a fascia board below the roof edge and splits a gable's rakes at the ridge", () => {
    const md = new MeshData();
    // A gable over x 0..8000, ridge along x at y = 3000.
    const under = (p: { x: number; y: number }) => 2800 + (3000 - Math.abs(p.y - 3000)) * 0.5;
    pushFascia(md, rect(0, 0, 8000, 6000), under, 100, { across: (p) => p.y, at: 3000 });
    const h = heights(md);
    expect(h.min).toBeCloseTo(2800 - 180, 3);
    expect(h.max).toBeCloseTo(2800 + 1500 + 100, 3);
    // Six pieces (two eaves, four half rakes), a closed box each.
    expect(md.triangleCount).toBe(6 * 12);
  });
});

/** The first thing a ray straight down from high above hits, world meters. */
function firstHitY(root: THREE.Object3D, xMm: number, yMm: number): number | null {
  root.updateMatrixWorld(true);
  const ray = new THREE.Raycaster(new THREE.Vector3(xMm / 1000, 50, -yMm / 1000), new THREE.Vector3(0, -1, 0));
  const hits = ray.intersectObject(root, true).filter((h) => (h.object as THREE.Mesh).isMesh && h.object.visible);
  return hits[0]?.point.y ?? null;
}

function build(doc: DocState) {
  doc.project.roof = { ...doc.project.roof, kind: "none" };
  return buildScene(doc, new MaterialLibrary(), { cutaway: false, activeLevelId: null, packModels: false });
}

describe("slabs in 3D", () => {
  it("cuts a void out of the upper floor, so the room below is double height", () => {
    const withVoid = build(slabDoc());
    expect(firstHitY(withVoid.root, 3000, 3000)).toBeLessThan(0.1);
    const without = slabDoc();
    without.project.elements = without.project.elements.filter((e) => e.id !== VOID);
    expect(firstHitY(build(without).root, 3000, 3000)).toBeCloseTo(3, 2);
  });

  it("draws a balcony at its level, with its rail picked as the balcony", () => {
    const built = build(slabDoc());
    const parts = built.byElement.get(BALCONY) ?? [];
    // Body, finish, rail steel and rail glass.
    expect(parts.length).toBeGreaterThanOrEqual(4);
    expect(firstHitY(built.root, 2500, -800)).toBeCloseTo(3, 2);
    const box = new THREE.Box3();
    for (const m of parts) box.expandByObject(m);
    expect(box.max.y).toBeCloseTo(4, 2);
    expect(built.byElement.get(VOID)?.length ?? 0).toBeGreaterThan(0);
  });

  it("draws a sunk deck below the floor", () => {
    const built = build(slabDoc());
    expect(firstHitY(built.root, 9000, 2000)).toBeCloseTo(-0.15, 3);
  });

  it("leaves slabs out and the floor whole when the Slabs layer is hidden", () => {
    const d = slabDoc();
    // Projects from schema 5 on already list the Slabs layer (DECISIONS D50).
    const slabs = d.project.layers.find((l) => l.key === "slabs");
    if (slabs) slabs.visible = false;
    else d.project.layers.push({ key: "slabs", visible: false, locked: false });
    const built = build(d);
    expect(built.byElement.get(BALCONY)).toBeUndefined();
    expect(firstHitY(built.root, 3000, 3000)).toBeCloseTo(3, 2);
  });

  it("draws a flight of n risers with n - 1 treads, the last one a riser short of the floor above", () => {
    const built = build(twoLevelDoc());
    const [stair] = built.byElement.get(STAIR) ?? [];
    expect(stair).toBeDefined();
    stair.geometry.computeBoundingBox();
    // 16 risers of 187.5 mm: the 15th tread at 2812.5, the floor above at 3000.
    expect(stair.geometry.boundingBox!.max.y).toBeCloseTo(2.8125, 4);
  });

  it("knows which face of an outer wall is outdoors", () => {
    const fp = [rect(-75, -75, 8075, 6075)];
    const south = { kind: "wall", id: "w", level_id: "l", start: { x: 0, y: 0 }, end: { x: 8000, y: 0 }, thickness_mm: 150, height_mm: null, material_id: null } as const;
    // Walking east along the south wall, the right side (+z) is south: outdoors.
    expect(exteriorSide({ ...south }, true, fp)).toBe(1);
    expect(exteriorSide({ ...south, start: { x: 8000, y: 0 }, end: { x: 0, y: 0 } }, true, fp)).toBe(-1);
    expect(exteriorSide({ ...south }, false, fp)).toBe(0);
  });
});

/** Two levels: a 12 m ground floor and an 8 m upper floor, so the ground floor has a roof deck east of x 8000 (the reviewer's case). */
function deckDoc(extra: DocState["project"]["elements"], groundWidth = 12000): DocState {
  const d = twoLevelDoc();
  d.project.elements = d.project.elements.filter((e) => e.kind !== "stair");
  d.project.roof = { ...d.project.roof, kind: "none" };
  d.derived.footprints = [
    { level_id: d.project.levels[0].id, polygon: rect(0, 0, groundWidth, 8000), area_mm2: groundWidth * 8000 },
    { level_id: "test-level-2", polygon: rect(0, 0, 8000, 8000), area_mm2: 64e6 },
  ];
  d.derived.rooms = [];
  d.project.elements.push(...extra);
  return d;
}

const voidAt = (id: string, poly: { x: number; y: number }[]) =>
  ({ kind: "slab", id, level_id: "test-level-2", slab_kind: "void", polygon: poly, thickness_mm: 0, top_offset_mm: 0, material_id: null, railing: null }) as const;

describe("voids cut every floor under them", () => {
  const lib = () => new MaterialLibrary();
  const opts = { cutaway: false, activeLevelId: null, packModels: false };

  it("cut the roof deck of the level below", () => {
    const open = buildScene(deckDoc([voidAt("v", rect(9000, 2000, 11000, 4000))]), lib(), opts);
    const closed = buildScene(deckDoc([]), lib(), opts);
    expect(firstHitY(closed.root, 10000, 3000)).toBeGreaterThan(2.9);
    expect(firstHitY(open.root, 10000, 3000)).toBeLessThan(0.1);
  });

  it("cut a floor slab on their level", () => {
    const slab = { kind: "slab", id: "f", level_id: "test-level-2", slab_kind: "floor", polygon: rect(8000, 0, 12000, 8000), thickness_mm: 200, top_offset_mm: 0, material_id: null, railing: null } as const;
    const closed = buildScene(deckDoc([slab], 8000), lib(), opts);
    const open = buildScene(deckDoc([slab, voidAt("v", rect(9000, 2000, 11000, 4000))], 8000), lib(), opts);
    expect(firstHitY(closed.root, 10000, 3000)).toBeCloseTo(3, 2);
    // Open down to the lawn: nothing of the model under the void.
    expect(firstHitY(open.root, 10000, 3000)).toBeNull();
    // The rest of the slab is still there.
    expect(firstHitY(open.root, 8500, 6000)).toBeCloseTo(3, 2);
  });

  it("and a stairwell cuts a slab it rises through", () => {
    const d = twoLevelDoc();
    d.project.roof = { ...d.project.roof, kind: "none" };
    d.project.elements.push({ kind: "slab", id: "f", level_id: "test-level-2", slab_kind: "floor", polygon: rect(100, 1000, 1200, 5200), thickness_mm: 200, top_offset_mm: 0, material_id: null, railing: null });
    const built = buildScene(d, lib(), opts);
    // Over the middle of the flight the first thing below is the stair, well under the upper floor.
    expect(firstHitY(built.root, 600, 3000)).toBeLessThan(2.5);
  });
});

describe("lens, sky and glass", () => {
  it("walks with an 80 degree wide lens, kept within a vertical range", () => {
    const hfov = (v: number, a: number) => (2 * Math.atan(Math.tan((v * Math.PI) / 360) * a) * 180) / Math.PI;
    expect(hfov(walkFov(16 / 9), 16 / 9)).toBeCloseTo(80, 3);
    expect(walkFov(16 / 9)).toBeCloseTo(50.5, 1);
    expect(walkFov(4)).toBe(WALK_VFOV_MIN);
    expect(walkFov(0.6)).toBe(WALK_VFOV_MAX);
  });

  it("takes the sun out of a photo sky and measures what is left", () => {
    const w = 64;
    const h = 32;
    const data = new Float32Array(w * h * 4).fill(1);
    // A small, very bright sun high in the sky.
    for (const [x, y] of [
      [20, 4],
      [21, 4],
      [20, 5],
      [21, 5],
    ]) {
      const i = (y * w + x) * 4;
      data[i] = data[i + 1] = data[i + 2] = 50000;
    }
    const mean = tameHdri({ data, width: w, height: h });
    let peak = 0;
    for (let i = 0; i < data.length; i += 4) peak = Math.max(peak, data[i]);
    expect(peak).toBeLessThan(10);
    expect(mean).toBeGreaterThan(0.9);
    expect(mean).toBeLessThan(1.5);
  });

  it("keeps glass and occlusion on a highlighted copy", () => {
    const lib = new MaterialLibrary();
    for (const base of [lib.get("mat-glass-clear", undefined, false), lib.get("mat-chb-painted")]) {
      const mesh = new THREE.Mesh(new THREE.BoxGeometry(), base);
      new Highlight("x", "hover", [mesh]);
      const copy = mesh.material as THREE.MeshStandardMaterial;
      expect(copy).not.toBe(base);
      expect(copy.customProgramCacheKey()).toBe(base.customProgramCacheKey());
      expect(copy.onBeforeCompile).toBe(base.onBeforeCompile);
      expect(copy.premultipliedAlpha).toBe(base.premultipliedAlpha);
    }
    const wall = lib.get("mat-chb-painted");
    const faded = new THREE.Mesh(new THREE.BoxGeometry(), wall);
    new MeshFade([faded]).setOpacity(0.5);
    expect(faded.material).not.toBe(wall);
    expect((faded.material as THREE.Material).customProgramCacheKey()).toBe(wall.customProgramCacheKey());
  });

  it("gives the path tracer real transmissive glass", () => {
    const lib = new MaterialLibrary();
    const pane = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 0.008), lib.get("mat-glass-clear", undefined, false));
    const wall = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), lib.get("mat-chb-painted"));
    const root = new THREE.Group().add(pane, wall);
    const owned: { dispose(): void }[] = [];
    expect(traceGlass(root, owned)).toBe(1);
    const glass = pane.material as THREE.MeshPhysicalMaterial;
    expect(glass.isMeshPhysicalMaterial).toBe(true);
    expect(glass.transmission).toBe(1);
    expect(glass.transparent).toBe(false);
    expect(wall.material).not.toBeInstanceOf(THREE.MeshPhysicalMaterial);
  });
});
