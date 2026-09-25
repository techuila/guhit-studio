import { describe, expect, it } from "vitest";
import * as THREE from "three";
import type { Asset, AssetLight } from "../../contract/bindings";
import catalogSource from "../../../crates/guhit-model/src/defaults.rs?raw";
import { buildAssetForm, hasAssetForm, KNOWN_ASSET_KEYS, lampAnchor, lampMaterial, type LampInfo } from "./assets";
import { Kit } from "./kit";
import { MaterialLibrary } from "./materials";

interface Item {
  key: string;
  w: number;
  d: number;
  h: number;
  elevation: number;
  mount: "floor" | "wall" | "ceiling" | "opening";
  light: AssetLight | null;
}

/**
 * The catalog, read from the Rust source: key, size, elevation, mount and
 * light, so a new key without a form fails here.
 */
const CATALOG: Item[] = catalogSource
  .split("\n")
  .map((line) => {
    const m = line.match(/item\("([a-z0-9-]+)",\s*"[^"]*",\s*A::\w+,\s*([-\d.]+),\s*([-\d.]+),\s*([-\d.]+),\s*([-\d.]+)\)/);
    if (!m) return null;
    const mount = line.match(/M::(Wall|Ceiling|Opening|Floor)/)?.[1].toLowerCase() as Item["mount"] | undefined;
    const lit = line.match(/,\s*([\d.]+),\s*([\d.]+)\),?\s*$/);
    const light = line.trimStart().startsWith("lit(") && lit ? { lumens: Number(lit[1]), kelvin: Number(lit[2]), on: true } : null;
    return { key: m[1], w: Number(m[2]), d: Number(m[3]), h: Number(m[4]), elevation: Number(m[5]), mount: mount ?? "floor", light };
  })
  .filter((x): x is Item => x !== null);

const NEW_KEYS = CATALOG.slice(CATALOG.findIndex((c) => c.key === "floor-drain")).map((c) => c.key);

function asset(item: Item, patch: Partial<Asset> = {}): Asset {
  return {
    id: `a-${item.key}`,
    level_id: "L1",
    catalog_key: item.key,
    name: item.key,
    category: "furniture",
    position: { x: 0, y: 0 },
    rotation_deg: 0,
    width_mm: item.w,
    depth_mm: item.d,
    height_mm: item.h,
    elevation_mm: item.elevation,
    light: item.light,
    links: [],
    circuit: "",
    ...patch,
  };
}

function build(item: Item, patch: Partial<Asset> = {}, cutY: number | null = null) {
  const kit = new Kit();
  kit.cutY = cutY;
  const lib = new MaterialLibrary();
  const g = buildAssetForm(kit, lib, asset(item, patch), Math.max(item.elevation, 0) / 1000);
  g.updateMatrixWorld(true);
  return { g, kit, lib };
}

function meshes(g: THREE.Object3D): THREE.Mesh[] {
  const out: THREE.Mesh[] = [];
  g.traverse((o) => {
    if ((o as THREE.Mesh).isMesh) out.push(o as THREE.Mesh);
  });
  return out;
}

function triangles(g: THREE.Object3D): number {
  let n = 0;
  for (const m of meshes(g)) {
    const geo = m.geometry;
    n += (geo.index ? geo.index.count : geo.getAttribute("position").count) / 3;
  }
  return n;
}

const item = (key: string) => {
  const it = CATALOG.find((c) => c.key === key);
  if (!it) throw new Error(`not in the catalog: ${key}`);
  return it;
};

describe("catalog forms", () => {
  it("reads the catalog, mounts and lights from defaults.rs", () => {
    expect(CATALOG.length).toBeGreaterThanOrEqual(57);
    expect(NEW_KEYS.length).toBe(33);
    expect(item("switch-1").mount).toBe("wall");
    expect(item("light-ceiling").mount).toBe("ceiling");
    expect(item("aircon-window").mount).toBe("opening");
    expect(item("light-tube").light).toEqual({ lumens: 1800, kelvin: 6500, on: true });
    expect(item("switch-1").light).toBeNull();
  });

  it("has a form for every catalog key", () => {
    for (const { key } of CATALOG) expect(hasAssetForm(key), key).toBe(true);
    for (const key of NEW_KEYS) expect(KNOWN_ASSET_KEYS, key).toContain(key);
  });

  it("builds every new key into more than the fallback box", () => {
    for (const key of NEW_KEYS) {
      const { g } = build(item(key));
      expect(meshes(g).length, key).toBeGreaterThan(0);
      expect(triangles(g), key).toBeGreaterThan(12);
    }
  });

  it("keeps triangle counts low: plates under 200, units under 3000", () => {
    const plates = ["switch-1", "switch-2", "switch-3", "outlet-duplex", "outlet-counter", "outlet-spo", "outlet-aircon", "outlet-outdoor", "doorbell-button"];
    for (const key of plates) expect(triangles(build(item(key)).g), key).toBeLessThan(200);
    for (const key of NEW_KEYS) expect(triangles(build(item(key)).g), key).toBeLessThan(3000);
  });

  it("stays inside its footprint and never reaches behind the wall", () => {
    for (const key of NEW_KEYS) {
      const it = item(key);
      const box = new THREE.Box3().setFromObject(build(it).g);
      const w = it.w / 1000;
      const d = it.d / 1000;
      if (it.mount === "wall") {
        // The back sits on the wall face and nothing goes behind it.
        expect(box.min.z, key).toBeGreaterThanOrEqual(-d / 2 - 1e-6);
        expect(box.min.z, key).toBeLessThan(-d / 2 + 0.002);
      }
      expect(box.min.x, key).toBeGreaterThan(-w / 2 - 0.03);
      expect(box.max.x, key).toBeLessThan(w / 2 + 0.03);
    }
  });

  it("hangs ceiling items from the ceiling", () => {
    for (const key of NEW_KEYS.filter((k) => item(k).mount === "ceiling" && k !== "light-pendant")) {
      const it = item(key);
      const box = new THREE.Box3().setFromObject(build(it).g);
      expect(box.max.y, key).toBeCloseTo(it.h / 1000, 3);
      expect(box.min.y, key).toBeGreaterThanOrEqual(-1e-6);
    }
    // The pendant's cord and canopy reach the 3000 mm ceiling from 2000 mm.
    const pendant = new THREE.Box3().setFromObject(build(item("light-pendant")).g);
    expect(pendant.max.y).toBeCloseTo(1.0, 3);
  });

  it("follows a given ceiling height for the pendant cord", () => {
    const it = item("light-pendant");
    const g = buildAssetForm(new Kit(), new MaterialLibrary(), asset(it), 2, false, { ceilingMm: 2700 });
    expect(new THREE.Box3().setFromObject(g).max.y).toBeCloseTo(0.7, 3);
  });

  it("puts the septic tank below the ground, faint, with solid covers at grade", () => {
    const it = item("septic-tank");
    const { g } = build(it);
    const box = new THREE.Box3().setFromObject(g);
    expect(box.min.y).toBeCloseTo(it.elevation / 1000, 3);
    expect(box.max.y).toBeLessThan(0.02);
    const faint = meshes(g).filter((m) => (m.material as THREE.Material).transparent);
    const solid = meshes(g).filter((m) => !(m.material as THREE.Material).transparent);
    expect(faint.length).toBeGreaterThan(0);
    expect(solid.length).toBeGreaterThan(0);
    for (const m of faint) expect((m.material as THREE.Material).opacity).toBeLessThan(0.5);
    for (const m of solid) expect(new THREE.Box3().setFromObject(m).min.y).toBeGreaterThan(-0.1);
  });

  it("cuts devices above the cutaway like the furniture", () => {
    const it = item("light-ceiling");
    const { g } = build(it, {}, 1.2);
    expect(meshes(g)).toHaveLength(0);
    const sw = item("switch-1");
    const low = build(sw, {}, 1.2);
    // Tilted rockers overshoot the cut by a fraction of a millimeter, like any rotated part.
    expect(new THREE.Box3().setFromObject(low.g).max.y).toBeLessThanOrEqual(1.2 - sw.elevation / 1000 + 0.001);
  });

  it("builds any size the perf script throws at it", () => {
    for (const key of NEW_KEYS) {
      const { g } = build(item(key), { width_mm: 1000, depth_mm: 800, height_mm: 900, elevation_mm: 0, light: undefined as unknown as null });
      for (const m of meshes(g)) {
        const pos = m.geometry.getAttribute("position");
        for (let i = 0; i < pos.count; i++) expect(Number.isFinite(pos.getX(i)) && Number.isFinite(pos.getY(i)), key).toBe(true);
      }
    }
  });
});

describe("light fixtures", () => {
  const LIGHTS = NEW_KEYS.filter((k) => item(k).light !== null);

  it("are the eight light keys", () => {
    expect(LIGHTS.sort()).toEqual(
      ["light-ceiling", "light-downlight", "light-floor-lamp", "light-outdoor", "light-pendant", "light-table-lamp", "light-tube", "light-wall"].sort(),
    );
  });

  /** One glow material per fixture form, on the lampPart meshes only, off, casting no shadow. */
  function glowOf(g: THREE.Object3D, key: string): THREE.MeshStandardMaterial {
    const parts = meshes(g).filter((m) => m.userData.lampPart === true);
    expect(parts.length, key).toBeGreaterThan(0);
    const mats = new Set(parts.map((p) => p.material));
    expect(mats.size, key).toBe(1);
    const mat = [...mats][0] as THREE.MeshStandardMaterial;
    expect(mat.emissiveIntensity, key).toBe(0);
    for (const p of parts) expect(p.castShadow, key).toBe(false);
    for (const m of meshes(g).filter((m) => m.userData.lampPart !== true)) expect(m.material, key).not.toBe(mat);
    return mat;
  }

  it("tag their glowing part with the fixture's glow material", () => {
    const lib = new MaterialLibrary();
    const perFixture = typeof (lib as unknown as { lampGlow?: unknown }).lampGlow === "function";
    for (const key of LIGHTS) {
      const it = item(key);
      const mat = glowOf(buildAssetForm(new Kit(), lib, asset(it), 0), key);
      // The light rig's per-fixture material when the library has one, else the per-kelvin one.
      if (perFixture) expect(mat.userData.lampGlow, key).toBe(`a-${key}`);
      else expect(mat, key).toBe(lampMaterial(lib, it.light!.kelvin));
    }
  });

  it("fall back to one shared material per color temperature", () => {
    const lib = new MaterialLibrary();
    // A library without the per-fixture glow.
    (lib as unknown as { lampGlow?: unknown }).lampGlow = undefined;
    for (const key of LIGHTS) {
      const it = item(key);
      const mat = glowOf(buildAssetForm(new Kit(), lib, asset(it), 0), key);
      expect(mat, key).toBe(lampMaterial(lib, it.light!.kelvin));
      expect(mat.userData.lampKelvin).toBe(it.light!.kelvin);
    }
  });

  it("share one material per color temperature, a different one per temperature", () => {
    const lib = new MaterialLibrary();
    const warm = lampMaterial(lib, 2700);
    expect(lampMaterial(lib, 2700)).toBe(warm);
    expect(lampMaterial(lib, 3000)).not.toBe(warm);
    expect(lampMaterial(lib, 6500).emissive.b).toBeGreaterThan(warm.emissive.b);
    expect(warm.emissive.r).toBeGreaterThan(warm.emissive.b);
  });

  it("carry a light anchor inside or just under the fixture", () => {
    for (const key of LIGHTS) {
      const it = item(key);
      const { g } = build(it);
      const lamp = g.userData.lamp as LampInfo;
      expect(lamp, key).toBeDefined();
      expect(lamp.kelvin).toBe(it.light!.kelvin);
      expect(lamp.lumens).toBe(it.light!.lumens);
      expect(lamp.on).toBe(true);
      const w = it.w / 1000;
      const d = it.d / 1000;
      const h = it.h / 1000;
      expect(lampAnchor(key, w, d, h)?.anchor).toEqual(lamp.anchor);
      const [x, y, z] = lamp.anchor;
      expect(Math.abs(x), key).toBeLessThanOrEqual(w / 2);
      expect(Math.abs(z), key).toBeLessThanOrEqual(d / 2);
      expect(y, key).toBeGreaterThanOrEqual(-0.05);
      expect(y, key).toBeLessThanOrEqual(h);
      if (it.mount === "ceiling") expect(lamp.aim, key).toEqual([0, -1, 0]);
    }
  });

  it("puts the anchor of bulb fixtures at the bulb", () => {
    for (const key of ["light-pendant", "light-floor-lamp", "light-table-lamp"]) {
      const it = item(key);
      const { g } = build(it);
      const lamp = g.userData.lamp as LampInfo;
      const anchor = new THREE.Vector3(...lamp.anchor);
      const inGlow = meshes(g)
        .filter((m) => m.userData.lampPart === true)
        .some((m) => new THREE.Box3().setFromObject(m).containsPoint(anchor));
      expect(inGlow, key).toBe(true);
    }
  });

  it("leave a fixture with no light untagged", () => {
    const { g } = build(item("light-ceiling"), { light: null });
    expect(g.userData.lamp).toBeUndefined();
    expect(meshes(g).some((m) => m.userData.lampPart)).toBe(false);
  });

  it("tag a fixture that is switched off, with on false", () => {
    const it = item("light-wall");
    const { g } = build(it, { light: { ...it.light!, on: false } });
    expect((g.userData.lamp as LampInfo).on).toBe(false);
  });
});

describe("kit rounds", () => {
  it("shares one unit geometry per shape and keeps the triangle count down", () => {
    const kit = new Kit();
    const mat = new THREE.MeshStandardMaterial();
    const parent = new THREE.Group();
    const a = kit.round(parent, mat, 0.1, 0.2, 0, 0, 0, { segments: 12 })!;
    const b = kit.round(parent, mat, 0.3, 0.1, 1, 0, 0, { segments: 12 })!;
    expect(a.geometry).toBe(b.geometry);
    expect(a.geometry.index!.count / 3).toBe(48);
    const shade = kit.round(parent, mat, 0.2, 0.2, 0, 0, 0, { segments: 16, top: 0.5, open: true })!;
    // Open shade: both faces of the wall, no caps.
    expect(shade.geometry.getAttribute("position").count / 3).toBe(64);
    kit.dispose();
    expect(kit.geometries).toHaveLength(0);
  });

  it("cuts a round at the cutaway", () => {
    const kit = new Kit();
    kit.cutY = 1;
    const parent = new THREE.Group();
    const mat = new THREE.MeshStandardMaterial();
    expect(kit.round(parent, mat, 0.1, 1, 0, 1.5, 0, { clip: { baseY: 0 } })).toBeNull();
    const m = kit.round(parent, mat, 0.1, 1, 0, 0.5, 0, { clip: { baseY: 0 } })!;
    expect(m.scale.y).toBeCloseTo(0.5, 6);
  });
});
