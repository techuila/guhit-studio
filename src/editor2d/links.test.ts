import { describe, expect, it } from "vitest";
import type { CatalogItem } from "../contract/bindings";
import { pt } from "./geom";
import type { AssetEl, Linkable } from "./links";
import {
  allLinks,
  arcPoint,
  controls,
  defaultBow,
  deviceKindOf,
  linkArc,
  linkRole,
  linksOf,
  resolveLinkClick,
  threeWaySwitches,
  toggleLinkCommand,
} from "./links";

function asset(id: string, key: string, category: AssetEl["category"], x: number, y: number, links: string[] = []): AssetEl {
  return {
    kind: "asset",
    id,
    level_id: "l",
    catalog_key: key,
    name: key,
    category,
    position: pt(x, y),
    rotation_deg: 0,
    width_mm: 100,
    depth_mm: 100,
    height_mm: 100,
    elevation_mm: 0,
    light: null,
    links,
    circuit: "",
  };
}

const NO_CATALOG = new Map<string, Pick<CatalogItem, "device">>();
const item = (a: AssetEl): Linkable => {
  const role = linkRole(a, deviceKindOf(a, NO_CATALOG));
  if (!role) throw new Error(`${a.catalog_key} does not link`);
  return { el: a, role };
};

function world(...els: AssetEl[]): { map: Map<string, Linkable>; lookup: (id: string) => Linkable | null } {
  const map = new Map<string, Linkable>();
  for (const e of els) {
    const role = linkRole(e, deviceKindOf(e, NO_CATALOG));
    if (role) map.set(e.id, { el: e, role });
  }
  return { map, lookup: (id) => map.get(id) ?? null };
}

describe("link roles", () => {
  it("reads the device kind from the catalog, else from the key", () => {
    const catalog = new Map<string, Pick<CatalogItem, "device">>([["switch-2", { device: "switch" }], ["tv-console", { device: null }]]);
    expect(deviceKindOf({ catalog_key: "switch-2" }, catalog)).toBe("switch");
    expect(deviceKindOf({ catalog_key: "tv-console" }, catalog)).toBeNull();
    expect(deviceKindOf({ catalog_key: "outlet-aircon" }, NO_CATALOG)).toBe("special_purpose_outlet");
    expect(deviceKindOf({ catalog_key: "aircon-indoor-2hp" }, NO_CATALOG)).toBe("aircon_indoor");
    expect(deviceKindOf({ catalog_key: "light-downlight" }, NO_CATALOG)).toBe("lighting_outlet");
  });

  it("pairs switches with lights and aircon outlets with units", () => {
    expect(item(asset("s", "switch-1", "electrical", 0, 0)).role).toBe("switch");
    expect(item(asset("o", "outlet-aircon", "electrical", 0, 0)).role).toBe("outlet");
    expect(item(asset("u", "aircon-window", "aircon", 0, 0)).role).toBe("unit");
    expect(item(asset("l", "light-floor-lamp", "lighting", 0, 0)).role).toBe("light");
    expect(linkRole({ category: "aircon" }, "aircon_outdoor")).toBeNull();
    expect(linkRole({ category: "electrical" }, "convenience_receptacle")).toBeNull();
    expect(controls("switch", "light")).toBe(true);
    expect(controls("outlet", "unit")).toBe(true);
    expect(controls("switch", "unit")).toBe(false);
    expect(controls("light", "switch")).toBe(false);
  });
});

describe("link tool clicks", () => {
  const s1 = asset("s1", "switch-1", "electrical", 0, 0, ["l1"]);
  const l1 = asset("l1", "light-ceiling", "lighting", 2000, 2000);
  const l2 = asset("l2", "light-ceiling", "lighting", 3000, 2000);
  const aco = asset("o1", "outlet-aircon", "electrical", 0, 2000);
  const acu = asset("u1", "aircon-indoor-1hp", "aircon", 500, 2000);
  const { lookup } = world(s1, l1, l2, aco, acu);

  it("picks a device, then toggles each load clicked: one UpdateElement per change", () => {
    expect(resolveLinkClick(null, "s1", lookup)).toEqual({ kind: "pick", id: "s1" });
    const add = resolveLinkClick("s1", "l2", lookup);
    expect(add.kind).toBe("toggle");
    if (add.kind !== "toggle") throw new Error();
    expect(add.adding).toBe(true);
    expect(add.command).toEqual({ type: "update_element", element: { ...s1, links: ["l1", "l2"] } });
    const remove = resolveLinkClick("s1", "l1", lookup);
    if (remove.kind !== "toggle") throw new Error();
    expect(remove.adding).toBe(false);
    expect(remove.command).toEqual({ type: "update_element", element: { ...s1, links: [] } });
  });

  it("works from the load too: the command still goes to the switch", () => {
    const r = resolveLinkClick("l2", "s1", lookup);
    if (r.kind !== "toggle") throw new Error(r.kind);
    expect(r.controllerId).toBe("s1");
    expect(r.loadId).toBe("l2");
    expect(r.command.type).toBe("update_element");
  });

  it("links an aircon outlet to its unit, never a switch to a unit", () => {
    const r = resolveLinkClick("o1", "u1", lookup);
    if (r.kind !== "toggle") throw new Error(r.kind);
    expect(r.command).toEqual({ type: "update_element", element: { ...aco, links: ["u1"] } });
    // A switch then a unit: the unit becomes the device being linked.
    expect(resolveLinkClick("s1", "u1", lookup)).toEqual({ kind: "pick", id: "u1" });
  });

  it("clicking nothing, or the device again, starts over", () => {
    expect(resolveLinkClick("s1", null, lookup)).toEqual({ kind: "clear" });
    expect(resolveLinkClick("s1", "s1", lookup)).toEqual({ kind: "clear" });
    expect(resolveLinkClick(null, null, lookup)).toEqual({ kind: "none" });
    expect(resolveLinkClick(null, "wall-1", lookup)).toEqual({ kind: "none" });
  });

  it("toggles on the list without duplicates", () => {
    const c = toggleLinkCommand({ ...s1, links: ["l1", "l2"] }, "l1");
    expect(c).toEqual({ type: "update_element", element: { ...s1, links: ["l2"] } });
  });
});

describe("3-way switches", () => {
  it("marks two switches sharing a light", () => {
    const a = asset("a", "switch-1", "electrical", 0, 0, ["l1", "l2"]);
    const b = asset("b", "switch-2", "electrical", 0, 0, ["l2"]);
    const c = asset("c", "switch-1", "electrical", 0, 0, ["l3"]);
    const lights = ["l1", "l2", "l3"].map((id) => asset(id, "light-ceiling", "lighting", 0, 0));
    const items = [a, b, c, ...lights].map(item);
    expect([...threeWaySwitches(items)].sort()).toEqual(["a", "b"]);
  });

  it("ignores links to missing lights and a switch listing a light twice", () => {
    const a = asset("a", "switch-1", "electrical", 0, 0, ["gone", "l1", "l1"]);
    const b = asset("b", "switch-1", "electrical", 0, 0, ["gone"]);
    const items = [a, b, asset("l1", "light-ceiling", "lighting", 0, 0)].map(item);
    expect(threeWaySwitches(items).size).toBe(0);
  });
});

describe("links to draw", () => {
  const s1 = asset("s1", "switch-1", "electrical", 0, 0, ["l1", "l2", "gone"]);
  const s2 = asset("s2", "switch-1", "electrical", 0, 0, ["l2"]);
  const l1 = asset("l1", "light-ceiling", "lighting", 0, 0);
  const l2 = asset("l2", "light-ceiling", "lighting", 0, 0);
  const { map } = world(s1, s2, l1, l2);

  it("a switch shows its lights, a light shows its switches", () => {
    expect(linksOf(["s1"], map).map((p) => p.key)).toEqual(["s1>l1", "s1>l2"]);
    expect(linksOf(["l2"], map).map((p) => p.key)).toEqual(["s1>l2", "s2>l2"]);
    expect(allLinks(map)).toHaveLength(3);
  });
});

describe("link arcs", () => {
  it("bows to one side and flips through the straight line", () => {
    const a = pt(0, 0);
    const b = pt(1000, 0);
    const left = linkArc(a, b, 1);
    expect(left.apex.y).toBeCloseTo(180);
    expect(arcPoint(a, left.control, b, 0.5).y).toBeCloseTo(left.apex.y);
    expect(linkArc(a, b, -1).apex.y).toBeCloseTo(-180);
    expect(linkArc(a, b, 0).apex.y).toBeCloseTo(0);
  });

  it("bows away from the other loads of the device", () => {
    const s = pt(0, 0);
    // The other light is to the left of s -> l1: bow right.
    expect(defaultBow(s, pt(1000, 0), [pt(1000, 1000)])).toBe(-1);
    expect(defaultBow(s, pt(1000, 1000), [pt(1000, 0)])).toBe(1);
    expect(defaultBow(s, pt(1000, 0), [])).toBe(1);
  });
});
