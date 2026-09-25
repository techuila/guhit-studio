import * as THREE from "three";
import { describe, expect, it } from "vitest";
import type { Asset, DocState } from "../../contract/bindings";
import { lampAnchor } from "../scene/assets";
import { buildScene, PLINTH_MM } from "../scene/buildScene";
import { MaterialLibrary } from "../scene/materials";
import { lampOptics, type LampSpec, type RoomSpec } from "./fixtures";
import { BOUNCE_SHARE, LampRig, planRooms, shadowSplit } from "./lamps";
import { lampCandela, LUX } from "./model";

describe("lampCandela", () => {
  it("spreads a point lamp over the whole sphere", () => {
    expect(lampCandela(1000, "point")).toBeCloseTo(1000 / (4 * Math.PI), 6);
  });

  it("gives a hard edged spot its cone's solid angle", () => {
    const solid = 2 * Math.PI * (1 - Math.cos(Math.PI / 6));
    expect(lampCandela(1000, "spot", 60, 0)).toBeCloseTo(1000 / solid, 6);
  });

  it("makes a fully fading 180 degree spot a flat diffuser: flux over pi", () => {
    expect(lampCandela(900, "spot", 180, 1)).toBeCloseTo(900 / Math.PI, 6);
  });

  it("puts more candela on the axis as the penumbra narrows the full-strength cone", () => {
    expect(lampCandela(700, "spot", 100, 0.55)).toBeGreaterThan(lampCandela(700, "spot", 100, 0));
  });
});

describe("shadowSplit", () => {
  it("keeps four lamp shadows, mostly spots, at least one point when there is one", () => {
    expect(shadowSplit(0, 0)).toEqual({ points: 0, spots: 0 });
    expect(shadowSplit(5, 0)).toEqual({ points: 4, spots: 0 });
    expect(shadowSplit(0, 6)).toEqual({ points: 0, spots: 4 });
    expect(shadowSplit(3, 10)).toEqual({ points: 1, spots: 3 });
    expect(shadowSplit(3, 2)).toEqual({ points: 2, spots: 2 });
    expect(shadowSplit(1, 1)).toEqual({ points: 1, spots: 1 });
  });
});

describe("lampOptics", () => {
  it("shines ceiling fixtures down and lets lamps and wall lights glow all around", () => {
    const size = { w: 0.3, d: 0.3, h: 0.06 };
    expect(lampOptics("light-ceiling", [0, -1, 0], size, [0, -0.03, 0]).kind).toBe("spot");
    expect(lampOptics("light-wall", null, size, [0, 0.1, 0]).kind).toBe("point");
    expect(lampOptics("light-outdoor", null, size, [0, 0.1, 0]).kind).toBe("point");
  });

  it("opens a pendant's cone to the rim of its shade", () => {
    const size = { w: 0.35, d: 0.35, h: 0.4 };
    const a = lampAnchor("light-pendant", size.w, size.d, size.h);
    expect(a).not.toBeNull();
    const o = lampOptics("light-pendant", a!.aim, size, a!.anchor);
    const half = (Math.atan(0.175 / a!.anchor[1]) * 180) / Math.PI;
    expect(o.kind).toBe("spot");
    expect(o.coneDeg).toBeCloseTo(half * 2 + 8, 6);
  });
});

const rect = (x0: number, y0: number, x1: number, y1: number) => [
  { x: x0, y: y0 },
  { x: x1, y: y0 },
  { x: x1, y: y1 },
  { x: x0, y: y1 },
];

function fixture(id: string, key: string, x: number, y: number, size: [number, number, number], elevation: number, rotation = 0): Asset {
  return {
    id,
    level_id: "L0",
    catalog_key: key,
    name: key,
    category: "lighting",
    position: { x, y },
    rotation_deg: rotation,
    width_mm: size[0],
    depth_mm: size[1],
    height_mm: size[2],
    elevation_mm: elevation,
    light: { lumens: 900, kelvin: 2700, on: true },
    links: [],
    circuit: "",
  };
}

/** Two storeys, fixtures on the ground floor: its ceiling is the slab underside, 2800. */
function house(): DocState {
  const fp = rect(0, 0, 8000, 8000);
  const assets = [
    fixture("pendant", "light-pendant", 2000, 2000, [350, 350, 400], 2000),
    fixture("ceiling", "light-ceiling", 5000, 5000, [300, 300, 60], 2740),
    fixture("lantern", "light-outdoor", 4000, 8050, [150, 150, 250], 2100, 180),
    fixture("odd", "glow-cube", 6000, 2000, [200, 200, 200], 0),
  ];
  return {
    revision: 1,
    project: {
      id: "p",
      name: "Fixtures",
      levels: [
        { id: "L0", name: "Ground", elevation_mm: 0, height_mm: 3000 },
        { id: "L1", name: "First", elevation_mm: 3000, height_mm: 3000 },
      ],
      layers: [],
      materials: [],
      roof: { kind: "none" },
      settings: null,
      elements: assets.map((a) => ({ kind: "asset", ...a })),
    },
    derived: {
      walls: [],
      rooms: [],
      footprints: [
        { level_id: "L0", polygon: fp, area_mm2: 64e6 },
        { level_id: "L1", polygon: fp, area_mm2: 64e6 },
      ],
    },
  } as unknown as DocState;
}

describe("fixture lamps in the built scene", () => {
  const built = buildScene(house(), new MaterialLibrary(), { cutaway: false, activeLevelId: null });
  built.root.updateMatrixWorld(true);
  const spec = (id: string) => {
    const s = built.lamps.find((l) => l.id === id);
    expect(s).toBeDefined();
    return s!;
  };

  it("puts each light at the catalog form's anchor", () => {
    const ceiling = spec("ceiling");
    // 2740 underside, anchor 30 mm under it.
    expect(ceiling.position[0]).toBeCloseTo(5, 5);
    expect(ceiling.position[1]).toBeCloseTo(2.71, 5);
    expect(ceiling.position[2]).toBeCloseTo(-5, 5);
    expect(ceiling.kind).toBe("spot");
    expect(ceiling.direction![1]).toBeCloseTo(-1, 6);
    const pendant = spec("pendant");
    expect(pendant.position[1]).toBeCloseTo(2 + 0.4 * 0.55 * 0.4, 5);
  });

  it("turns a wall fixture's anchor with the fixture", () => {
    const lantern = spec("lantern");
    const a = lampAnchor("light-outdoor", 0.15, 0.15, 0.25)!;
    // Outside the footprint it stands on the ground, the plinth height below
    // the floor. Turned half way round, the anchor's depth offset points the
    // other way in plan.
    expect(lantern.kind).toBe("point");
    expect(lantern.position[0]).toBeCloseTo(4, 5);
    expect(lantern.position[1]).toBeCloseTo((2100 - PLINTH_MM) / 1000 + a.anchor[1], 5);
    expect(Math.abs(lantern.position[2] - -8.05)).toBeCloseTo(Math.abs(a.anchor[2]), 5);
  });

  it("adds no glow of its own when the form has glowing parts, and measures theirs", () => {
    const own: string[] = [];
    built.root.traverse((o) => {
      if (o.name === "lamp") own.push(o.userData.elementId as string);
    });
    expect(own).toEqual(["odd"]);
    for (const id of ["pendant", "ceiling", "lantern"]) expect(spec(id).glowAreaM2).toBeGreaterThan(0.005);
  });

  it("gives an object the catalog has no form for a small glowing bulb", () => {
    const odd = spec("odd");
    expect(odd.kind).toBe("point");
    expect(odd.glowAreaM2).toBeCloseTo(4 * Math.PI * 0.035 * 0.035, 6);
  });

  it("hangs the pendant's cord from the slab above, not the level height", () => {
    const box = new THREE.Box3();
    for (const mesh of built.byElement.get("pendant") ?? []) box.expandByObject(mesh);
    expect(box.max.y).toBeCloseTo(2.8, 3);
  });
});

const room = (roomId: string, areaM2: number): RoomSpec => ({ roomId, levelId: "L0", floor: [0, 0, 0], heightM: 2.8, areaM2 });
const lamp = (id: string, roomId: string | null, lumens = 900): LampSpec => ({
  id,
  key: "light-ceiling",
  kind: "spot",
  position: [0, 2.7, 0],
  direction: [0, -1, 0],
  coneDeg: 180,
  penumbra: 1,
  radius: 0.1,
  lumens,
  kelvin: 3000,
  on: true,
  levelId: "L0",
  roomId,
  glowAreaM2: 0.1,
});

describe("room lights", () => {
  it("gives lit rooms a bounce light and dark rooms a ghost and a bounce, within a third of the budget", () => {
    const rooms = [room("a", 20), room("b", 12), room("c", 8), room("d", 15), room("e", 5)];
    const specs = [lamp("1", "a"), lamp("2", "b"), lamp("3", "c")];
    const all = planRooms(specs, rooms, 24);
    expect(all.bounce.map((r) => r.roomId)).toEqual(["a", "b", "c", "d", "e"]);
    expect(all.ghosts.map((r) => r.roomId)).toEqual(["d", "e"]);
    const tight = planRooms(specs, rooms, 9);
    expect(tight.bounce.map((r) => r.roomId)).toEqual(["a", "b", "c"]);
    expect(tight.ghosts).toEqual([]);
  });

  it("bounces a share of the lit lumens in the room, and follows the switches", () => {
    const rig = new LampRig();
    rig.build([lamp("1", "a"), lamp("2", "a", 600), lamp("3", null)], [room("a", 20)], new Map());
    expect(rig.roles()).toEqual({ fixture: 3, ghost: 0, bounce: 1 });
    const state = { lampsLit: true, ghostLit: true, scale: 1, overrides: new Map<string, boolean>() };
    rig.apply(state);
    expect(rig.roomIntensity("a", "bounce")).toBeCloseTo(((1500 * BOUNCE_SHARE) / Math.PI) * LUX, 9);
    rig.apply({ ...state, overrides: new Map([["2", false]]) });
    expect(rig.roomIntensity("a", "bounce")).toBeCloseTo(((900 * BOUNCE_SHARE) / Math.PI) * LUX, 9);
    rig.apply({ ...state, lampsLit: false, ghostLit: false });
    expect(rig.roomIntensity("a", "bounce")).toBe(0);
    rig.dispose();
  });

  it("leaves the bounce lights out of the path tracer, which bounces light itself", () => {
    const rig = new LampRig();
    rig.build([lamp("1", "a")], [room("a", 20), room("b", 10)], new Map());
    rig.apply({ lampsLit: true, ghostLit: true, scale: 1, overrides: new Map() });
    expect(rig.roles()).toEqual({ fixture: 1, ghost: 1, bounce: 2 });
    expect(rig.roomIntensity("b", "bounce")).toBeGreaterThan(0);
    const names = rig.cloneLights().map((l) => l.name);
    expect(names.some((n) => n.startsWith("bounce:"))).toBe(false);
    expect(names.filter((n) => n.startsWith("lamp:") || n.startsWith("ghost:"))).toHaveLength(2);
    rig.dispose();
  });
});
