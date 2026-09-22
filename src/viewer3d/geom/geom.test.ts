import { describe, expect, it } from "vitest";
import { cameraToPose, planToWorld, vec3ToWorld, worldToVec3 } from "./coords";
import {
  clipToXRange,
  ensureCCW,
  longAxis,
  offsetPolygon,
  pointInPolygon,
  signedArea,
  triangulate,
} from "./polygon";
import { MeshData, pushPrism } from "./meshData";
import { buildWallMesh, clampOpenings, type WallInput } from "./wallMesh";
import { buildRoofInfill, buildRoofMesh, roofProfile, type RoofInput } from "./roofMesh";
import {
  axonometric,
  exteriorCorner,
  eyeLevel,
  fitDistance,
  roomInterior,
  sunDirection,
  topView,
  type ModelBounds,
} from "./cameraMath";

const rect = (x0: number, y0: number, x1: number, y1: number) => [
  { x: x0, y: y0 },
  { x: x1, y: y0 },
  { x: x1, y: y1 },
  { x: x0, y: y1 },
];

describe("coords", () => {
  it("maps plan to three: x stays, north goes to -z, height to y, in meters", () => {
    expect(planToWorld(1000, 2000, 3000)).toEqual([1, 3, -2]);
    expect(vec3ToWorld({ x: -5000, y: -7000, z: 4500 })).toEqual([-5, 4.5, 7]);
  });
  it("round trips", () => {
    const v = { x: 1234, y: -5678, z: 910 };
    const w = vec3ToWorld(v);
    const back = worldToVec3(w[0], w[1], w[2]);
    expect(back.x).toBeCloseTo(v.x, 6);
    expect(back.y).toBeCloseTo(v.y, 6);
    expect(back.z).toBeCloseTo(v.z, 6);
  });
  it("converts a contract camera", () => {
    const pose = cameraToPose({
      position: { x: 0, y: -1000, z: 1600 },
      target: { x: 0, y: 0, z: 1600 },
      fov_deg: 50,
    });
    expect(pose.position).toEqual([0, 1.6, 1]);
    expect(pose.target).toEqual([0, 1.6, 0]);
    expect(pose.fovDeg).toBe(50);
  });
});

describe("polygon", () => {
  it("fixes winding and drops repeated points", () => {
    const cw = [...rect(0, 0, 10, 10)].reverse();
    cw.push({ ...cw[0] });
    const p = ensureCCW(cw);
    expect(p).toHaveLength(4);
    expect(signedArea(p)).toBeCloseTo(100);
  });
  it("clips to an x range", () => {
    const p = clipToXRange(rect(0, -5, 100, 5), 20, 50);
    expect(signedArea(p)).toBeCloseTo(300);
  });
  it("clips a mitred end without losing the miter", () => {
    // 45 degree miter at the start
    const outline = [
      { x: -5, y: -5 },
      { x: 100, y: -5 },
      { x: 100, y: 5 },
      { x: 5, y: 5 },
    ];
    const p = clipToXRange(outline, -Infinity, 50);
    expect(signedArea(p)).toBeCloseTo(55 * 10 - 50);
  });
  it("offsets a rectangle outward with square corners", () => {
    const p = offsetPolygon(rect(0, 0, 10, 20), 2);
    expect(signedArea(p)).toBeCloseTo(14 * 24);
  });
  it("triangulates an L shape to the same area", () => {
    const L = [
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 10, y: 4 },
      { x: 4, y: 4 },
      { x: 4, y: 10 },
      { x: 0, y: 10 },
    ];
    let area = 0;
    for (const [a, b, c] of triangulate(L)) area += Math.abs(signedArea([L[a], L[b], L[c]]));
    expect(area).toBeCloseTo(signedArea(L));
  });
  it("finds the long axis of a rotated room", () => {
    const a = Math.PI / 6;
    const rot = rect(0, 0, 6000, 3000).map((p) => ({
      x: p.x * Math.cos(a) - p.y * Math.sin(a),
      y: p.x * Math.sin(a) + p.y * Math.cos(a),
    }));
    const ax = longAxis(rot);
    expect(ax.length).toBeCloseTo(6000, 3);
    expect(ax.width).toBeCloseTo(3000, 3);
    expect(Math.abs(ax.dir.x)).toBeCloseTo(Math.cos(a), 6);
  });
  it("tests points", () => {
    expect(pointInPolygon({ x: 5, y: 5 }, rect(0, 0, 10, 10))).toBe(true);
    expect(pointInPolygon({ x: 15, y: 5 }, rect(0, 0, 10, 10))).toBe(false);
  });
});

describe("prism", () => {
  it("is closed and faces outward", () => {
    const md = new MeshData();
    pushPrism(md, rect(0, 0, 2000, 3000), { bottom: () => 0, top: () => 1000 });
    expect(md.signedVolume()).toBeCloseTo(6, 6);
    expect(md.triangleCount).toBe(12);
  });
  it("accepts clockwise input", () => {
    const md = new MeshData();
    pushPrism(md, rect(0, 0, 2000, 3000).reverse(), { bottom: () => 0, top: () => 1000 });
    expect(md.signedVolume()).toBeCloseTo(6, 6);
  });
});

describe("wall mesh", () => {
  const base: WallInput = {
    start: { x: 0, y: 0 },
    end: { x: 8000, y: 0 },
    thickness: 150,
    height: 3000,
    elevation: 0,
    outline: rect(0, -75, 8000, 75),
    openings: [],
  };

  it("builds a closed solid wall with the exact volume", () => {
    const md = new MeshData();
    expect(buildWallMesh(base, md)).toBe(true);
    expect(md.signedVolume()).toBeCloseTo(8 * 0.15 * 3, 6);
    const b = md.bounds()!;
    expect(b.min).toEqual([0, 0, -0.075]);
    expect(b.max).toEqual([8, 3, 0.075]);
  });

  it("cuts a door and a window: volume drops by exactly the holes", () => {
    const md = new MeshData();
    buildWallMesh(
      {
        ...base,
        openings: [
          { offset: 1500, width: 900, height: 2100, sill: 0 },
          { offset: 3500, width: 1500, height: 1200, sill: 900 },
        ],
      },
      md,
    );
    const holes = 0.9 * 2.1 * 0.15 + 1.5 * 1.2 * 0.15;
    expect(md.signedVolume()).toBeCloseTo(8 * 0.15 * 3 - holes, 6);
  });

  it("has reveal faces: jambs, sill top and lintel underside inside the hole", () => {
    const md = new MeshData();
    buildWallMesh({ ...base, openings: [{ offset: 3500, width: 1500, height: 1200, sill: 900 }] }, md);
    let jambArea = 0;
    let sillTop = 0;
    let lintelUnder = 0;
    const p = md.positions;
    const n = md.normals;
    for (let i = 0; i < p.length; i += 9) {
      const ax = p[i + 3] - p[i];
      const ay = p[i + 4] - p[i + 1];
      const az = p[i + 5] - p[i + 2];
      const bx = p[i + 6] - p[i];
      const by = p[i + 7] - p[i + 1];
      const bz = p[i + 8] - p[i + 2];
      const area = Math.hypot(ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx) / 2;
      const cy = (p[i + 1] + p[i + 4] + p[i + 7]) / 3;
      const cx = (p[i] + p[i + 3] + p[i + 6]) / 3;
      if (Math.abs(n[i]) > 0.99 && cx > 0.1 && cx < 7.9) jambArea += area;
      if (n[i + 1] > 0.99 && Math.abs(cy - 0.9) < 1e-9) sillTop += area;
      if (n[i + 1] < -0.99 && Math.abs(cy - 2.1) < 1e-9) lintelUnder += area;
    }
    expect(jambArea).toBeCloseTo(2 * 1.2 * 0.15, 6);
    expect(sillTop).toBeCloseTo(1.5 * 0.15, 6);
    expect(lintelUnder).toBeCloseTo(1.5 * 0.15, 6);
  });

  it("keeps mitred ends and works for any direction and elevation", () => {
    const md = new MeshData();
    // vertical wall going north, mitred at both ends, on an upper level
    buildWallMesh(
      {
        start: { x: 8000, y: 0 },
        end: { x: 8000, y: 6000 },
        thickness: 150,
        height: 2700,
        elevation: 3000,
        outline: [
          { x: 8075, y: -75 },
          { x: 8075, y: 6075 },
          { x: 7925, y: 5925 },
          { x: 7925, y: 75 },
        ],
        openings: [{ offset: 3000, width: 1200, height: 1200, sill: 900 }],
      },
      md,
    );
    const area = ((6150 + 5850) / 2) * 150; // trapezoid, mm2
    expect(md.signedVolume()).toBeCloseTo((area * 2700) / 1e9 - 1.2 * 1.2 * 0.15, 6);
    const b = md.bounds()!;
    expect(b.min[1]).toBeCloseTo(3);
    expect(b.max[1]).toBeCloseTo(5.7);
    expect(b.min[2]).toBeCloseTo(-6.075);
    expect(b.max[0]).toBeCloseTo(8.075);
  });

  it("falls back to a rectangle when the outline is missing", () => {
    const md = new MeshData();
    buildWallMesh({ ...base, outline: null }, md);
    expect(md.signedVolume()).toBeCloseTo(8 * 0.15 * 3, 6);
  });

  it("handles overlapping and out of range openings without breaking the solid", () => {
    const md = new MeshData();
    buildWallMesh(
      {
        ...base,
        openings: [
          { offset: 1000, width: 1000, height: 2100, sill: 0 },
          { offset: 1500, width: 1000, height: 1000, sill: 500 },
          { offset: 7900, width: 1000, height: 5000, sill: 0 },
          { offset: 20000, width: 900, height: 2100, sill: 0 },
        ],
      },
      md,
    );
    // union of the first two: 500..1500 x 0..2100 plus 1500..2000 x 500..1500
    const holes = (1.0 * 2.1 + 0.5 * 1.0) * 0.15 + (8 - 0.001 - 7.4) * 3 * 0.15;
    expect(md.signedVolume()).toBeCloseTo(8 * 0.15 * 3 - holes, 5);
  });

  it("cutaway clamps the wall and its openings", () => {
    const md = new MeshData();
    buildWallMesh(
      { ...base, maxHeight: 1200, openings: [{ offset: 3500, width: 1500, height: 1200, sill: 900 }] },
      md,
    );
    expect(md.bounds()!.max[1]).toBeCloseTo(1.2);
    expect(md.signedVolume()).toBeCloseTo(8 * 0.15 * 1.2 - 1.5 * 0.3 * 0.15, 6);
  });

  it("clamps openings to the wall", () => {
    expect(clampOpenings([{ offset: 100, width: 900, height: 2100, sill: 0 }], 0, 8000, 3000)).toEqual([
      { a: 1, b: 550, bottom: 0, top: 2100 },
    ]);
    expect(clampOpenings([{ offset: NaN, width: 900, height: 2100, sill: 0 }], 0, 8000, 3000)).toEqual([]);
  });
});

describe("roof", () => {
  const fp = rect(-75, -75, 8075, 6075);
  const gable: RoofInput = {
    kind: "gable",
    pitchDeg: 20,
    overhang: 600,
    thickness: 150,
    ridgeAxis: "x",
    footprint: fp,
    baseHeight: 3000,
  };
  const tan20 = Math.tan((20 * Math.PI) / 180);

  it("gable underside touches the wall top at the eaves and peaks on the ridge", () => {
    const prof = roofProfile(gable)!;
    expect(prof.under({ x: 0, y: -75 })).toBeCloseTo(3000);
    expect(prof.under({ x: 0, y: 6075 })).toBeCloseTo(3000);
    expect(prof.under({ x: 0, y: 3000 })).toBeCloseTo(3000 + 3075 * tan20);
    expect(prof.ridge).toBeCloseTo(3000);
    // the overhang dips below the wall top
    expect(prof.under({ x: 0, y: -675 })).toBeCloseTo(3000 - 600 * tan20);
  });

  it("gable ridge follows ridge_axis y", () => {
    const prof = roofProfile({ ...gable, ridgeAxis: "y" })!;
    expect(prof.ridge).toBeCloseTo(4000);
    expect(prof.under({ x: 4000, y: 0 })).toBeCloseTo(3000 + 4075 * tan20);
  });

  it("gable covering is closed, outward, and spans the overhang", () => {
    const md = new MeshData();
    buildRoofMesh(gable, md);
    const vertical = 150 / Math.cos((20 * Math.PI) / 180);
    expect(md.signedVolume()).toBeCloseTo((9350 * 7350 * vertical) / 1e9, 5);
    const b = md.bounds()!;
    expect(b.min[0]).toBeCloseTo(-0.675);
    expect(b.max[0]).toBeCloseTo(8.675);
    expect(b.min[2]).toBeCloseTo(-6.675);
    expect(b.max[1]).toBeCloseTo((3000 + 3075 * tan20 + vertical) / 1000);
  });

  it("gable infill closes both end walls up to the ridge and nothing on the eave sides", () => {
    const md = new MeshData();
    buildRoofInfill(gable, md);
    const b = md.bounds()!;
    expect(b.min[1]).toBeCloseTo(3);
    expect(b.max[1]).toBeCloseTo((3000 + 3075 * tan20) / 1000);
    // outer + inner face on two gable ends, each a triangle of base 6150
    let area = 0;
    const p = md.positions;
    for (let i = 0; i < p.length; i += 9) {
      const ax = p[i + 3] - p[i];
      const ay = p[i + 4] - p[i + 1];
      const az = p[i + 5] - p[i + 2];
      const bx = p[i + 6] - p[i];
      const by = p[i + 7] - p[i + 1];
      const bz = p[i + 8] - p[i + 2];
      area += Math.hypot(ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx) / 2;
      // every face is vertical and looks along x
      expect(Math.abs(md.normals[i])).toBeCloseTo(1);
    }
    const tri = (6.15 * 3.075 * tan20) / 2;
    expect(area).toBeCloseTo(4 * tri, 5);
  });

  it("shed rises along the axis and flat is a slab", () => {
    const shed = roofProfile({ ...gable, kind: "shed" })!;
    expect(shed.under({ x: -75, y: 0 })).toBeCloseTo(3000);
    expect(shed.under({ x: 8075, y: 0 })).toBeCloseTo(3000 + 8150 * tan20);
    const md = new MeshData();
    buildRoofMesh({ ...gable, kind: "flat" }, md);
    expect(md.signedVolume()).toBeCloseTo((9350 * 7350 * 150) / 1e9, 6);
    expect(md.bounds()!.min[1]).toBeCloseTo(3);
    expect(md.bounds()!.max[1]).toBeCloseTo(3.15);
  });

  it("returns nothing for kind none or an empty footprint", () => {
    expect(buildRoofMesh({ ...gable, kind: "none" }, new MeshData())).toBeNull();
    expect(buildRoofMesh({ ...gable, footprint: [] }, new MeshData())).toBeNull();
  });
});

describe("camera presets", () => {
  const b: ModelBounds = { minX: -75, minY: -75, maxX: 8075, maxY: 6075, minZ: 0, maxZ: 4300 };
  const dist = (p: { x: number; y: number; z: number }, q: { x: number; y: number; z: number }) =>
    Math.hypot(p.x - q.x, p.y - q.y, p.z - q.z);

  it("fit distance grows for narrow aspect", () => {
    expect(fitDistance(5000, 45, 0.5)).toBeGreaterThan(fitDistance(5000, 45, 1.6));
  });
  it("eye level stands at 1600 mm south of the house", () => {
    const p = eyeLevel(b, 1.6);
    expect(p.position.z).toBe(1600);
    expect(p.position.y).toBeLessThan(b.minY);
  });
  it("corner view is south-west and above", () => {
    const p = exteriorCorner(b, 1.6);
    expect(p.position.x).toBeLessThan(b.minX);
    expect(p.position.y).toBeLessThan(b.minY);
    expect(p.position.z).toBeGreaterThan(b.maxZ);
    expect(dist(p.position, p.target)).toBeGreaterThan(5400);
  });
  it("top looks straight down with north up", () => {
    const p = topView(b, 1.6);
    expect(p.position.x).toBeCloseTo(p.target.x);
    expect(p.position.y).toBeLessThan(p.target.y);
    expect(p.target.y - p.position.y).toBeLessThan(50);
    expect(p.position.z).toBeGreaterThan(b.maxZ);
  });
  it("axonometric uses a long lens from the isometric direction", () => {
    const p = axonometric(b, 1.6);
    expect(p.fov_deg).toBeLessThan(20);
    const d = dist(p.position, p.target);
    expect((p.position.z - p.target.z) / d).toBeCloseTo(Math.sin((35.264 * Math.PI) / 180), 3);
  });
  it("room interior stands inside the room and looks along the long axis", () => {
    const room = rect(5050, 75, 7925, 5925);
    const p = roomInterior(room, { x: 6487, y: 3000 }, 0, "Bedroom");
    expect(pointInPolygon(p.position, room)).toBe(true);
    expect(p.position.z).toBe(1600);
    expect(Math.abs(p.target.y - p.position.y)).toBeGreaterThan(1000);
    expect(p.target.x).toBeCloseTo(p.position.x);
  });
  it("sun comes from the south-east and turns with north", () => {
    const s = sunDirection(0);
    expect(s.x).toBeGreaterThan(0);
    expect(s.y).toBeLessThan(0);
    expect(s.z).toBeGreaterThan(0.5);
    expect(Math.hypot(s.x, s.y, s.z)).toBeCloseTo(1);
    const r = sunDirection(90); // true north now points to -x (west)
    expect(r.x).toBeCloseTo(-s.y);
    expect(r.y).toBeCloseTo(s.x);
  });
});
