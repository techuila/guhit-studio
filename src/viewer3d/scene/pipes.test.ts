import { describe, expect, it } from "vitest";
import * as THREE from "three";
import type { DocState, Element, Pipe, PipeNetwork } from "../../contract/bindings";
import { PIPE_COLOR_HEX, PIPE_COLOR_VAR, PIPE_SYSTEM_ORDER } from "../../contract/pipes";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";
import { tracePipesOnce } from "../render/renderScene";
import { BuildCache } from "./buildCache";
import { buildScene, CUTAWAY_MM } from "./buildScene";
import { buildExportGroup } from "./exportScene";
import { MaterialLibrary } from "./materials";
import { buildPipeParts, cutRun, PIPE_PICK_LAYER, pipeTopMm, pipeWorldPoints } from "./pipes";

const base = fixture as unknown as DocState;
const LEVEL = base.project.levels[0].id;

const cold: Pipe = {
  id: "pipe-cold",
  level_id: LEVEL,
  system: "cold_water",
  material: "ppr",
  diameter_mm: 20,
  name: "Kitchen supply",
  points: [
    { x: 500, y: 3000, z: 300 },
    { x: 4500, y: 3000, z: 300 },
    { x: 4500, y: 3000, z: 1100 },
  ],
};
const branch: Pipe = {
  id: "pipe-branch",
  level_id: LEVEL,
  system: "cold_water",
  material: "ppr",
  diameter_mm: 20,
  name: "",
  points: [
    { x: 2000, y: 3000, z: 300 },
    { x: 2000, y: 4000, z: 300 },
  ],
};
const drain: Pipe = {
  id: "pipe-drain",
  level_id: LEVEL,
  system: "drainage",
  material: "upvc",
  diameter_mm: 100,
  name: "Building drain",
  points: [
    { x: 1000, y: 5000, z: 200 },
    { x: 1000, y: 5000, z: -300 },
    { x: 7000, y: 5000, z: -400 },
  ],
};

const network: PipeNetwork = {
  fittings: [
    { kind: "elbow", pipe_id: cold.id, branch_pipe_id: null, level_id: LEVEL, position: { x: 4500, y: 3000, z: 300 }, diameter_mm: 20, angle_deg: 90 },
    { kind: "tee", pipe_id: cold.id, branch_pipe_id: branch.id, level_id: LEVEL, position: { x: 2000, y: 3000, z: 300 }, diameter_mm: 20, angle_deg: 90 },
  ],
  penetrations: [
    { kind: "slab", pipe_id: drain.id, host_id: null, level_id: LEVEL, position: { x: 1000, y: 5000, z: -75 }, direction: { x: 0, y: 0, z: -1 }, diameter_mm: 100 },
  ],
  takeoff: [],
  total_length_m: 0,
  elbow_count: 1,
  tee_count: 1,
  sleeve_count: 1,
};

function withPipes(patch?: (d: DocState) => void): DocState {
  const d = structuredClone(base);
  d.project.elements.push(...([cold, branch, drain].map((p) => ({ kind: "pipe", ...p })) as Element[]));
  d.derived.pipes = structuredClone(network);
  patch?.(d);
  return d;
}

function tris(g: { triangleCount: number }): number {
  return g.triangleCount;
}

describe("pipe colors", () => {
  it("mirror the four pipe tokens in tokens.css", async () => {
    // Read from disk: vitest hands CSS imports back empty. No `@types/node`
    // in this project, so Node's fs comes in untyped.
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: string): string };
    const tokens = fs.readFileSync(new URL("../../styles/tokens.css", import.meta.url), "utf8");
    for (const system of PIPE_SYSTEM_ORDER) {
      const m = new RegExp(`${PIPE_COLOR_VAR[system]}:\\s*(#[0-9a-fA-F]{6})`).exec(tokens);
      expect(m, PIPE_COLOR_VAR[system]).not.toBeNull();
      expect(PIPE_COLOR_HEX[system].toLowerCase()).toBe(m![1].toLowerCase());
    }
  });
});

describe("buildPipeParts", () => {
  const none = () => undefined;

  it("draws a straight run as one capped tube", () => {
    const straight = { ...cold, points: cold.points.slice(0, 2) };
    const { body, sleeves } = buildPipeParts(straight, 0, [], [], () => 0, none);
    // 12 sides: 24 triangles of tube, 12 per end cap.
    expect(tris(body)).toBe(24 + 12 + 12);
    expect(tris(sleeves)).toBe(0);
  });

  it("puts a round joint at a bend", () => {
    const straight = buildPipeParts({ ...cold, points: cold.points.slice(0, 2) }, 0, [], [], () => 0, none);
    const bent = buildPipeParts(cold, 0, [], [], () => 0, none);
    expect(tris(bent.body) - tris(straight.body)).toBe(24 + 120);
  });

  it("sits at the level elevation plus the height above the floor", () => {
    const straight = { ...cold, points: cold.points.slice(0, 2) };
    const { body } = buildPipeParts(straight, 3000, [], [], () => 0, none);
    const ys = body.positions.filter((_, i) => i % 3 === 1);
    expect(Math.min(...ys)).toBeCloseTo(3.3 - 0.01, 6);
    expect(Math.max(...ys)).toBeCloseTo(3.3 + 0.01, 6);
  });

  it("adds a fitting at a tee and a sleeve at a penetration", () => {
    const plain = buildPipeParts(cold, 0, [], [], () => 0, none);
    const withTee = buildPipeParts(cold, 0, [network.fittings[1]], [], () => 0, (id) => (id === branch.id ? branch : undefined));
    // Collar (tube and two rings) plus the stub toward the branch and its ring.
    expect(tris(withTee.body) - tris(plain.body)).toBe(24 * 3 + 24 + 24);
    const sleeved = buildPipeParts(drain, 0, [], network.penetrations, () => 210, none);
    // 16 sides for a 100 mm pipe: a tube and two end rings.
    expect(tris(sleeved.sleeves)).toBe(32 + 32 + 32);
    const ys = sleeved.sleeves.positions.filter((_, i) => i % 3 === 1);
    // The sleeve is 210 mm long, centered on the crossing.
    expect(Math.max(...ys) - Math.min(...ys)).toBeCloseTo(0.21, 4);
  });
});

describe("pipes in buildScene", () => {
  it("builds one solo per pipe and one batch per system plus one for sleeves", () => {
    const lib = new MaterialLibrary();
    const built = buildScene(withPipes(), lib, { cutaway: false, activeLevelId: null });
    try {
      expect([...built.pipes.solos.keys()].sort()).toEqual([branch.id, cold.id, drain.id].sort());
      expect(built.pipes.batches).toHaveLength(3);
      for (const id of [cold.id, branch.id, drain.id]) expect(built.byElement.get(id)?.length).toBeGreaterThan(0);
      for (const solo of built.pipes.solos.values()) {
        expect(solo.parent).toBe(built.root);
        solo.traverse((o) => {
          if ((o as THREE.Mesh).isMesh) expect(o.layers.mask).toBe(1 << PIPE_PICK_LAYER);
        });
      }
      for (const b of built.pipes.batches) {
        expect(b.mesh.userData.batch).toBe(true);
        expect(b.mesh.userData.elementId).toBeUndefined();
        expect(b.mesh.layers.mask).toBe(1);
      }
    } finally {
      built.kit.dispose();
      lib.dispose();
    }
  });

  it("leaves out pipes on a hidden layer", () => {
    const lib = new MaterialLibrary();
    const d = withPipes((x) => {
      x.project.layers = [...x.project.layers.filter((l) => l.key !== "cold_water"), { key: "cold_water", visible: false, locked: false }];
    });
    const built = buildScene(d, lib, { cutaway: false, activeLevelId: null });
    try {
      expect([...built.pipes.solos.keys()]).toEqual([drain.id]);
      expect(built.pipes.batches).toHaveLength(2);
    } finally {
      built.kit.dispose();
      lib.dispose();
    }
  });

  it("uses the pipe token colors and never the shell categories", () => {
    const lib = new MaterialLibrary();
    const built = buildScene(withPipes(), lib, { cutaway: false, activeLevelId: null });
    try {
      const mesh = built.byElement.get(drain.id)!.find((m) => m.userData.part === "body")!;
      const mat = mesh.material as THREE.MeshStandardMaterial;
      expect(`#${mat.color.getHexString(THREE.SRGBColorSpace)}`).toBe(PIPE_COLOR_HEX.drainage);
      expect(mat.userData.shellCat).toBe("pipe");
    } finally {
      built.kit.dispose();
      lib.dispose();
    }
  });

  it("hands unchanged pipes back from the build cache", () => {
    const lib = new MaterialLibrary();
    const cache = new BuildCache();
    const d = withPipes();
    const a = buildScene(d, lib, { cutaway: false, activeLevelId: null, cache });
    const soloA = a.pipes.solos.get(drain.id);
    for (const o of [...a.root.children]) o.removeFromParent();
    const d2 = structuredClone(d);
    const moved = d2.project.elements.find((e) => e.id === cold.id);
    if (moved?.kind === "pipe") moved.points[0].x += 100;
    const b = buildScene(d2, lib, { cutaway: false, activeLevelId: null, cache });
    try {
      expect(b.pipes.solos.get(drain.id)).toBe(soloA);
      expect(b.pipes.solos.get(cold.id)).not.toBe(a.pipes.solos.get(cold.id));
    } finally {
      a.kit.dispose();
      b.kit.dispose();
      cache.dispose();
      lib.dispose();
    }
  });

  it("promotes a pipe out of its batch and back", () => {
    const lib = new MaterialLibrary();
    const built = buildScene(withPipes(), lib, { cutaway: false, activeLevelId: null });
    try {
      const batch = built.pipes.batches.find((b) => b.has(cold.id))!;
      const index = batch.mesh.geometry.getIndex()!;
      const before = Array.from(index.array as Uint32Array);
      built.pipes.promote([cold.id]);
      const during = Array.from(index.array as Uint32Array);
      const changed = during.filter((v, i) => v !== before[i]).length;
      expect(changed).toBeGreaterThan(0);
      // The cold run draws itself now.
      built.pipes.meshesOf(cold.id).forEach((m) => expect(m.layers.mask).toBe(1));
      // Every triangle of the hidden range collapsed to one vertex.
      for (let i = 0; i < during.length; i += 3) {
        if (during[i] !== before[i] || during[i + 1] !== before[i + 1] || during[i + 2] !== before[i + 2]) {
          expect(during[i]).toBe(during[i + 1]);
          expect(during[i + 1]).toBe(during[i + 2]);
        }
      }
      built.pipes.promote([]);
      expect(Array.from(index.array as Uint32Array)).toEqual(before);
      built.pipes.meshesOf(cold.id).forEach((m) => expect(m.layers.mask).toBe(1 << PIPE_PICK_LAYER));
    } finally {
      built.kit.dispose();
      lib.dispose();
    }
  });
});

describe("pipes in the cutaway", () => {
  const none = () => undefined;
  const ys = (b: { positions: number[] }) => b.positions.filter((_, i) => i % 3 === 1);

  it("cuts a run at the cut height and drops the parts above it", () => {
    const pts = [
      { x: 0, y: 0, z: 300 },
      { x: 1000, y: 0, z: 300 },
      { x: 1000, y: 0, z: 2400 },
      { x: 3000, y: 0, z: 2400 },
      { x: 3000, y: 0, z: 600 },
      { x: 4000, y: 0, z: 600 },
    ];
    const pieces = cutRun(pts, 1200);
    expect(pieces).toEqual([
      [pts[0], pts[1], { x: 1000, y: 0, z: 1200 }],
      [{ x: 3000, y: 0, z: 1200 }, pts[4], pts[5]],
    ]);
    // Wholly above or wholly below.
    expect(cutRun([{ x: 0, y: 0, z: 2450 }, { x: 3000, y: 0, z: 2450 }], 1200)).toEqual([]);
    expect(cutRun(pts.slice(0, 2), 1200)).toEqual([pts.slice(0, 2)]);
    // A point exactly at the cut stays; a run that only touches the cut leaves nothing.
    expect(cutRun([{ x: 0, y: 0, z: 1200 }, { x: 0, y: 0, z: 2000 }], 1200)).toEqual([]);
  });

  it("caps a riser at the cut, flush with the cut walls", () => {
    const riser: Pipe = { ...cold, id: "riser", points: [{ x: 0, y: 0, z: 300 }, { x: 0, y: 0, z: 2600 }] };
    const whole = buildPipeParts(riser, 0, [], [], () => 0, none);
    const cut = buildPipeParts(riser, 0, [], [], () => 0, none, 1200);
    // Same parts: one tube and two caps, the top cap now at the cut.
    expect(tris(cut.body)).toBe(tris(whole.body));
    expect(Math.max(...ys(cut.body))).toBeCloseTo(1.2, 6);
    expect(Math.max(...ys(whole.body))).toBeCloseTo(2.6, 6);
    // Counted from the level floor.
    expect(Math.max(...ys(buildPipeParts(riser, 3000, [], [], () => 0, none, 1200).body))).toBeCloseTo(4.2, 6);
  });

  it("leaves out fittings and sleeves above the cut", () => {
    const high = { ...network.fittings[0], position: { x: 4500, y: 3000, z: 1100 } };
    const low = network.fittings[1];
    const tee = buildPipeParts(cold, 0, [low], [], () => 0, (id) => (id === branch.id ? branch : undefined), 700);
    const plain = buildPipeParts(cold, 0, [], [], () => 0, none, 700);
    expect(tris(tee.body)).toBeGreaterThan(tris(plain.body));
    const elbow = buildPipeParts(cold, 0, [high], [], () => 0, none, 700);
    expect(tris(elbow.body)).toBe(tris(plain.body));
    const wallSleeve = { ...network.penetrations[0], kind: "wall" as const, position: { x: 1000, y: 5000, z: 1500 } };
    expect(tris(buildPipeParts(drain, 0, [], [wallSleeve], () => 210, none, 1200).sleeves)).toBe(0);
    expect(tris(buildPipeParts(drain, 0, [], network.penetrations, () => 210, none, 1200).sleeves)).toBeGreaterThan(0);
  });

  const tall: Pipe = {
    id: "pipe-vent",
    level_id: LEVEL,
    system: "vent",
    material: "upvc",
    diameter_mm: 50,
    name: "Vent stack",
    points: [
      { x: 1000, y: 5000, z: 200 },
      { x: 1000, y: 5000, z: 3450 },
    ],
  };
  const hot: Pipe = {
    id: "pipe-hot",
    level_id: LEVEL,
    system: "hot_water",
    material: "ppr",
    diameter_mm: 20,
    name: "Hot water",
    points: [
      { x: 0, y: 2000, z: 2450 },
      { x: 3000, y: 2000, z: 2450 },
    ],
  };
  const topOf = (built: ReturnType<typeof buildScene>, id: string): number => {
    let top = -Infinity;
    built.pipes.solos.get(id)?.traverse((o) => {
      const mesh = o as THREE.Mesh;
      if (!mesh.isMesh) return;
      mesh.geometry.computeBoundingBox();
      top = Math.max(top, mesh.geometry.boundingBox!.max.y);
    });
    return top;
  };
  const withTall = (patch?: (d: DocState) => void) =>
    withPipes((d) => {
      d.project.elements.push({ kind: "pipe", ...tall } as Element, { kind: "pipe", ...hot } as Element);
      patch?.(d);
    });

  it("cuts runs on the active level with the walls, and keeps them whole without the cutaway", () => {
    const lib = new MaterialLibrary();
    const open = buildScene(withTall(), lib, { cutaway: false, activeLevelId: LEVEL });
    const cut = buildScene(withTall(), lib, { cutaway: true, activeLevelId: LEVEL });
    try {
      expect(topOf(open, tall.id)).toBeCloseTo(3.45, 4);
      expect(topOf(cut, tall.id)).toBeCloseTo(CUTAWAY_MM / 1000, 4);
      // A run wholly above the cut keeps an empty solo, so the engine still counts it as shown.
      expect(cut.pipes.solos.has(hot.id)).toBe(true);
      expect(cut.byElement.get(hot.id)).toBeUndefined();
      // Batches draw the cut geometry too.
      const vents = cut.pipes.batches.find((b) => b.has(tall.id))!;
      vents.mesh.geometry.computeBoundingBox();
      expect(vents.mesh.geometry.boundingBox!.max.y).toBeLessThanOrEqual(CUTAWAY_MM / 1000 + 1e-6);
    } finally {
      open.kit.dispose();
      cut.kit.dispose();
      lib.dispose();
    }
  });

  it("keeps runs whole for X-ray and hidden, and on the levels below the active one", () => {
    const lib = new MaterialLibrary();
    const whole = buildScene(withTall(), lib, { cutaway: true, activeLevelId: LEVEL, pipesWhole: true });
    const upper = withTall((d) => {
      d.project.levels.push({ id: "level-2", name: "Second Floor", elevation_mm: 3000, height_mm: 2800 });
    });
    const fromAbove = buildScene(upper, lib, { cutaway: true, activeLevelId: "level-2" });
    try {
      expect(topOf(whole, tall.id)).toBeCloseTo(3.45, 4);
      expect(topOf(fromAbove, tall.id)).toBeCloseTo(3.45, 4);
    } finally {
      whole.kit.dispose();
      fromAbove.kit.dispose();
      lib.dispose();
    }
  });

  it("cuts a run from a lower level at the active level's cut height", () => {
    const lib = new MaterialLibrary();
    const two = withTall((d) => {
      d.project.levels.push({ id: "level-2", name: "Second Floor", elevation_mm: 2800, height_mm: 2800 });
      const stack = d.project.elements.find((e) => e.id === tall.id);
      // New points: the test runs share theirs.
      if (stack?.kind === "pipe") stack.points = [stack.points[0], { ...stack.points[1], z: 6500 }];
    });
    const built = buildScene(two, lib, { cutaway: true, activeLevelId: "level-2" });
    try {
      // The upper level's cut: 2800 + 1200 mm, in world meters.
      expect(topOf(built, tall.id)).toBeCloseTo(4.0, 4);
      // A ground floor run under that height is not touched.
      expect(topOf(built, cold.id)).toBeCloseTo(1.1, 4);
    } finally {
      built.kit.dispose();
      lib.dispose();
    }
  });

  it("keeps runs wholly under the cut as they are, cache included", () => {
    const lib = new MaterialLibrary();
    const cache = new BuildCache();
    const d = withTall();
    const a = buildScene(d, lib, { cutaway: false, activeLevelId: LEVEL, cache });
    const coldA = a.pipes.solos.get(cold.id);
    for (const o of [...a.root.children]) o.removeFromParent();
    const b = buildScene(d, lib, { cutaway: true, activeLevelId: LEVEL, cache });
    try {
      expect(b.pipes.solos.get(cold.id)).toBe(coldA);
    } finally {
      a.kit.dispose();
      b.kit.dispose();
      cache.dispose();
      lib.dispose();
    }
  });

  it("finds a run's points in world meters, for a focus with nothing drawn", () => {
    const pts = pipeWorldPoints(withTall(), tall.id);
    expect(pts.map((p) => [p.x, p.y, p.z])).toEqual([
      [1, 0.2, -5],
      [1, 3.45, -5],
    ]);
    expect(pipeWorldPoints(base, "nope")).toEqual([]);
  });

  it("caches cut and whole runs apart", () => {
    const lib = new MaterialLibrary();
    const cache = new BuildCache();
    const d = withTall();
    const a = buildScene(d, lib, { cutaway: false, activeLevelId: LEVEL, cache });
    const soloA = a.pipes.solos.get(tall.id);
    for (const o of [...a.root.children]) o.removeFromParent();
    const b = buildScene(d, lib, { cutaway: true, activeLevelId: LEVEL, cache });
    try {
      expect(b.pipes.solos.get(tall.id)).not.toBe(soloA);
      expect(topOf(b, tall.id)).toBeCloseTo(CUTAWAY_MM / 1000, 4);
    } finally {
      a.kit.dispose();
      b.kit.dispose();
      cache.dispose();
      lib.dispose();
    }
  });

  it("knows the highest pipe point, for the cutaway sweep", () => {
    expect(pipeTopMm(withTall())).toBe(3450);
    expect(pipeTopMm(base)).toBeNull();
  });
});

describe("pipes in the path traced render", () => {
  it("go into the render once: solos hidden, batches drawn", () => {
    const lib = new MaterialLibrary();
    const built = buildScene(withPipes(), lib, { cutaway: false, activeLevelId: null });
    try {
      tracePipesOnce(built);
      const seen = new Map<string, number>();
      let batches = 0;
      // What the tracer gathers: every visible mesh, layers ignored.
      built.root.traverseVisible((o) => {
        const mesh = o as THREE.Mesh;
        if (!mesh.isMesh) return;
        if (mesh.userData.batch) batches++;
        if (mesh.userData.pipe) seen.set(mesh.userData.elementId, (seen.get(mesh.userData.elementId) ?? 0) + 1);
      });
      expect(seen.size).toBe(0);
      expect(batches).toBe(built.pipes.batches.length);
      for (const id of [cold.id, branch.id, drain.id]) expect(built.pipes.batches.some((b) => b.has(id))).toBe(true);
    } finally {
      built.kit.dispose();
      lib.dispose();
    }
  });
});

describe("pipes in the export", () => {
  it("exports every run once, named pipe-<id> under Pipes, without the batches", () => {
    const { group, dispose } = buildExportGroup(withPipes());
    try {
      expect(group.getObjectByName("Pipes")).toBeTruthy();
      for (const p of [cold, branch, drain]) expect(group.getObjectByName(`pipe-${p.id}`)).toBeTruthy();
      let batches = 0;
      group.traverse((o) => {
        if (o.userData.batch) batches++;
      });
      expect(batches).toBe(0);
    } finally {
      dispose();
    }
  });
});
