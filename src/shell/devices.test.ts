import { describe, expect, it } from "vitest";
import type { AirconSpec, CatalogItem, Element } from "../contract/bindings";
import {
  approxLedWatts,
  canLink,
  colorOf,
  elevationFor,
  isDevice,
  isThreeWay,
  kelvinOf,
  lightsOn,
  lineLimitsLabel,
  lineSetLabel,
  linkedFrom,
  linkedTo,
  mountRef,
  mountingHeight,
  sharedCircuit,
  withoutLink,
  type AssetEl,
} from "./devices";

const item = (patch: Partial<CatalogItem>): CatalogItem => ({
  key: "k",
  name: "n",
  category: "electrical",
  width_mm: 70,
  depth_mm: 40,
  height_mm: 115,
  elevation_mm: 1143,
  mount: "wall",
  device: null,
  light: null,
  aircon: null,
  ...patch,
});

const asset = (id: string, patch: Partial<AssetEl> = {}): AssetEl => ({
  kind: "asset",
  id,
  level_id: "g",
  catalog_key: "switch-1",
  name: id,
  category: "electrical",
  position: { x: 0, y: 0 },
  rotation_deg: 0,
  width_mm: 70,
  depth_mm: 40,
  height_mm: 115,
  elevation_mm: 1143,
  light: null,
  links: [],
  circuit: "",
  ...patch,
});

describe("links", () => {
  const s1 = asset("s1", { links: ["l1", "l2", "gone"] });
  const s2 = asset("s2", { links: ["l2"] });
  const l1 = asset("l1", { catalog_key: "light-ceiling" });
  const l2 = asset("l2", { catalog_key: "light-ceiling" });
  const elements: Element[] = [s1, s2, l1, l2];

  it("lists what a device controls, skipping deleted ids", () => {
    expect(linkedTo(s1, elements).map((a) => a.id)).toEqual(["l1", "l2"]);
  });

  it("lists the devices that control a light", () => {
    expect(linkedFrom("l2", elements).map((a) => a.id)).toEqual(["s1", "s2"]);
    expect(linkedFrom("l1", elements).map((a) => a.id)).toEqual(["s1"]);
  });

  it("calls two switches on one light a 3-way", () => {
    expect(isThreeWay(s1, elements)).toBe(true);
    expect(isThreeWay(s2, elements)).toBe(true);
    expect(isThreeWay(asset("s3", { links: ["l1"] }), [asset("s3", { links: ["l1"] }), l1])).toBe(false);
  });

  it("removes one link", () => {
    expect(withoutLink(s1, "l2").links).toEqual(["l1", "gone"]);
    expect(s1.links).toEqual(["l1", "l2", "gone"]);
  });

  it("knows which devices link", () => {
    expect(canLink(asset("s"), item({ device: "switch" }))).toBe(true);
    expect(canLink(asset("o"), item({ device: "special_purpose_outlet" }))).toBe(true);
    expect(canLink(asset("l"), item({ device: "lighting_outlet" }))).toBe(false);
    expect(canLink(asset("x", { links: ["a"] }), undefined)).toBe(true);
  });

  it("knows devices from furniture", () => {
    expect(isDevice(asset("s"), item({ device: "switch" }))).toBe(true);
    expect(isDevice(asset("lamp", { light: { lumens: 400, kelvin: 2700, on: true } }), item({ category: "lighting" }))).toBe(true);
    expect(isDevice(asset("sofa", { category: "furniture" }), item({ category: "furniture", mount: "floor" }))).toBe(false);
  });
});

describe("mounting height", () => {
  it("reads wall boxes to their center and shows the catalog's 1200 as 1200", () => {
    const sw = item({ device: "switch" });
    expect(mountRef(sw)).toBe("center");
    expect(mountingHeight(asset("s"), "center")).toBe(1200);
    expect(mountingHeight(asset("o", { elevation_mm: 243 }), "center")).toBe(300);
    expect(mountingHeight(asset("o", { elevation_mm: 1000 }), "center")).toBe(1057.5);
  });

  it("reads lights and aircon units to their underside", () => {
    expect(mountRef(item({ device: "lighting_outlet", mount: "ceiling" }))).toBe("underside");
    expect(mountRef(item({ device: "aircon_indoor" }))).toBe("underside");
    expect(mountRef(undefined)).toBe("underside");
    expect(mountingHeight(asset("u", { elevation_mm: 2300 }), "underside")).toBe(2300);
  });

  it("writes the elevation back", () => {
    expect(elevationFor(1200, asset("s"), "center")).toBe(1142.5);
    expect(elevationFor(2300, asset("u"), "underside")).toBe(2300);
  });
});

describe("light", () => {
  it("bands color temperatures", () => {
    expect(colorOf(2700)).toBe("warm");
    expect(colorOf(3000)).toBe("warm");
    expect(colorOf(4000)).toBe("neutral");
    expect(colorOf(6500)).toBe("daylight");
    expect(kelvinOf("neutral")).toBe(4000);
    expect(kelvinOf("daylight")).toBe(6500);
  });

  it("hints the LED watts", () => {
    expect(approxLedWatts(900)).toBe(9);
    expect(approxLedWatts(1800)).toBe(18);
    expect(approxLedWatts(40)).toBe(1);
  });
});

describe("aircon", () => {
  const split: AirconSpec = { role: "indoor", hp: 1.5, liquid_mm: 6.35, gas_mm: 9.52, min_line_m: 3, max_line_m: 25, max_rise_m: 10, included_line_m: 3 };
  it("writes the line set and its limits", () => {
    expect(lineSetLabel(split)).toBe("6.35 mm liquid, 9.52 mm gas");
    expect(lineLimitsLabel(split)).toBe("3 to 25 m long, up to 10 m rise");
  });
  it("has no line set for a window unit", () => {
    const win: AirconSpec = { ...split, role: "window", liquid_mm: 0, gas_mm: 0, min_line_m: 0, max_line_m: 0, max_rise_m: 0, included_line_m: 0 };
    expect(lineSetLabel(win)).toBeNull();
    expect(lineLimitsLabel(win)).toBeNull();
  });
});

describe("several selected", () => {
  it("shares a circuit tag or reports it mixed", () => {
    expect(sharedCircuit([asset("a", { circuit: "L1" }), asset("b", { circuit: "L1" })])).toEqual({ value: "L1", mixed: false });
    expect(sharedCircuit([asset("a", { circuit: "L1" }), asset("b", { circuit: "C2" })])).toEqual({ value: "", mixed: true });
  });

  it("counts the fixtures that are on", () => {
    const on = { lumens: 900, kelvin: 3000, on: true };
    expect(lightsOn([asset("a", { light: on }), asset("b", { light: { ...on, on: false } }), asset("s")])).toEqual({ on: 1, total: 2 });
  });
});
