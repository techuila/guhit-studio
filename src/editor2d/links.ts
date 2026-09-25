// Device links (`Asset::links`): which objects can link, what a click of the
// link tool does, 3-way switches and the arcs drawn between a device and its
// loads. Pure, tested in links.test.ts. Contract: docs/CONTRACT.md,
// "Devices, fixtures and links". Guhit draws links; it never checks circuits.

import type { Asset, CatalogItem, Command, DeviceKind, Element } from "../contract/bindings";
import type { P } from "./geom";
import { add, cross, dist, left, lerp, mul, sub, unit } from "./geom";

export type AssetEl = Extract<Element, { kind: "asset" }>;

/**
 * What an object is to the link tool. A switch controls lights, an aircon or
 * special purpose outlet feeds aircon units (indoor and window).
 */
export type LinkRole = "switch" | "outlet" | "light" | "unit";

/**
 * The device kind of an object: from its catalog item, or from its catalog
 * key when the catalog is not loaded (the keys are stable slugs).
 */
export function deviceKindOf(a: Pick<Asset, "catalog_key">, catalog: ReadonlyMap<string, Pick<CatalogItem, "device">>): DeviceKind | null {
  const item = catalog.get(a.catalog_key);
  if (item) return item.device;
  const k = a.catalog_key;
  if (k.startsWith("switch-")) return "switch";
  if (k === "outlet-spo" || k === "outlet-aircon") return "special_purpose_outlet";
  if (k.startsWith("outlet-")) return "convenience_receptacle";
  if (k.startsWith("aircon-indoor-")) return "aircon_indoor";
  if (k.startsWith("aircon-outdoor-")) return "aircon_outdoor";
  if (k === "aircon-window") return "aircon_window";
  if (k === "light-floor-lamp" || k === "light-table-lamp") return null;
  if (k.startsWith("light-")) return "lighting_outlet";
  return null;
}

export function linkRole(a: Pick<Asset, "category">, device: DeviceKind | null): LinkRole | null {
  if (device === "switch") return "switch";
  if (device === "special_purpose_outlet") return "outlet";
  if (device === "aircon_indoor" || device === "aircon_window") return "unit";
  if (device === "lighting_outlet" || a.category === "lighting") return "light";
  return null;
}

/** True for the role that holds the `links` list. */
export function isController(r: LinkRole | null): r is "switch" | "outlet" {
  return r === "switch" || r === "outlet";
}

/** True when a `controller` can control or feed a `load`. */
export function controls(controller: LinkRole | null, load: LinkRole | null): boolean {
  return (controller === "switch" && load === "light") || (controller === "outlet" && load === "unit");
}

/** One UpdateElement on the controller: adds the load to its links, or removes it. */
export function toggleLinkCommand(controller: AssetEl, loadId: string): Command {
  const links = controller.links.includes(loadId) ? controller.links.filter((id) => id !== loadId) : [...controller.links, loadId];
  return { type: "update_element", element: { ...controller, links } };
}

export interface Linkable {
  el: AssetEl;
  role: LinkRole;
}

/** Every object among `elements` that links, by id. */
export function linkables(elements: readonly Element[], catalog: ReadonlyMap<string, Pick<CatalogItem, "device">>): Map<string, Linkable> {
  const out = new Map<string, Linkable>();
  for (const el of elements) {
    if (el.kind !== "asset") continue;
    const role = linkRole(el, deviceKindOf(el, catalog));
    if (role) out.set(el.id, { el, role });
  }
  return out;
}

/** What one click of the link tool does. */
export type LinkClick =
  /** The clicked device becomes the one being linked. */
  | { kind: "pick"; id: string }
  /** The link between the two toggles: one command on the controller. */
  | { kind: "toggle"; controllerId: string; loadId: string; adding: boolean; command: Command }
  /** Nothing linkable, or the device itself: drop it and start over. */
  | { kind: "clear" }
  | { kind: "none" };

/**
 * One click of the link tool. With a switch (or outlet) picked, a click on a
 * light (or unit) toggles their link; it works the other way round too: pick
 * a light, then click its switches. Clicking another device picks it instead.
 */
export function resolveLinkClick(sourceId: string | null, hitId: string | null, lookup: (id: string) => Linkable | null): LinkClick {
  const hit = hitId ? lookup(hitId) : null;
  const source = sourceId ? lookup(sourceId) : null;
  if (!hit) return source ? { kind: "clear" } : { kind: "none" };
  if (!source) return { kind: "pick", id: hit.el.id };
  if (hit.el.id === source.el.id) return { kind: "clear" };
  let controller: AssetEl | null = null;
  let loadId: string | null = null;
  if (controls(source.role, hit.role)) {
    controller = source.el;
    loadId = hit.el.id;
  } else if (controls(hit.role, source.role)) {
    controller = hit.el;
    loadId = source.el.id;
  }
  if (!controller || !loadId) return { kind: "pick", id: hit.el.id };
  return {
    kind: "toggle",
    controllerId: controller.id,
    loadId,
    adding: !controller.links.includes(loadId),
    command: toggleLinkCommand(controller, loadId),
  };
}

/**
 * Switches that share a light with another switch: two switches on one
 * light make a 3-way (drawn "S3"). Links to missing lights do not count.
 */
export function threeWaySwitches(items: readonly Linkable[]): Set<string> {
  const lights = new Set(items.filter((i) => i.role === "light").map((i) => i.el.id));
  const count = new Map<string, number>();
  const switches = items.filter((i) => i.role === "switch");
  for (const s of switches) {
    for (const id of new Set(s.el.links)) if (lights.has(id)) count.set(id, (count.get(id) ?? 0) + 1);
  }
  const out = new Set<string>();
  for (const s of switches) if (s.el.links.some((id) => (count.get(id) ?? 0) >= 2)) out.add(s.el.id);
  return out;
}

/** A link as drawn: from the controller to one load. `key` is stable per pair. */
export interface LinkPair {
  key: string;
  controllerId: string;
  loadId: string;
}

export const linkKey = (controllerId: string, loadId: string): string => `${controllerId}>${loadId}`;

/**
 * The links to draw for these ids: a controller shows its loads, a load shows
 * the controllers that link it. Only pairs where both ends exist.
 */
export function linksOf(ids: Iterable<string>, items: ReadonlyMap<string, Linkable>): LinkPair[] {
  const out: LinkPair[] = [];
  const seen = new Set<string>();
  const push = (c: string, l: string): void => {
    const key = linkKey(c, l);
    if (seen.has(key) || !items.has(c) || !items.has(l)) return;
    seen.add(key);
    out.push({ key, controllerId: c, loadId: l });
  };
  for (const id of ids) {
    const it = items.get(id);
    if (!it) continue;
    if (isController(it.role)) {
      for (const l of it.el.links) if (controls(it.role, items.get(l)?.role ?? null)) push(id, l);
    } else {
      for (const c of items.values()) {
        if (isController(c.role) && controls(c.role, it.role) && c.el.links.includes(id)) push(c.el.id, id);
      }
    }
  }
  return out;
}

/** Every link on the plan, for the faint overview while the link tool is on. */
export function allLinks(items: ReadonlyMap<string, Linkable>): LinkPair[] {
  const controllers = [...items.values()].filter((i) => isController(i.role)).map((i) => i.el.id);
  return linksOf(controllers, items);
}

// ---------------------------------------------------------------- arcs

/** How far a link bows, as a share of its length. */
export const BOW_RATIO = 0.18;

export interface ArcGeometry {
  /** Quadratic curve control point. */
  control: P;
  /** The middle of the curve, where the flip handle sits. */
  apex: P;
}

/**
 * A link from `from` to `to` bowing to one side: `bow` +1 bows to the left
 * of the direction of travel, -1 to the right, values between animate a flip
 * through the straight line.
 */
export function linkArc(from: P, to: P, bow: number): ArcGeometry {
  const m = lerp(from, to, 0.5);
  const n = left(unit(sub(to, from)));
  const h = dist(from, to) * BOW_RATIO * bow;
  return { apex: add(m, mul(n, h)), control: add(m, mul(n, 2 * h)) };
}

/**
 * The side a link bows to until the user flips it: away from the other loads
 * of the same device, so a fan of links spreads out instead of crossing.
 * Left when there is nothing to avoid.
 */
export function defaultBow(from: P, to: P, others: readonly P[]): 1 | -1 {
  if (others.length === 0) return 1;
  let x = 0;
  let y = 0;
  for (const p of others) {
    x += p.x;
    y += p.y;
  }
  const c = { x: x / others.length, y: y / others.length };
  const s = cross(sub(to, from), sub(c, from));
  if (Math.abs(s) < 1e-6 * Math.max(1, dist(from, to) ** 2)) return 1;
  return s > 0 ? -1 : 1;
}

/**
 * The ends of a link between two symbol anchors: each end stops at the
 * radius its symbol keeps clear, so the line never runs through an "S".
 * Null when the symbols touch.
 */
export function trimLink(a: P & { r: number }, b: P & { r: number }): { from: P; to: P } | null {
  const L = dist(a, b);
  if (L <= a.r + b.r + 1) return null;
  const u = unit(sub(b, a));
  return { from: add(a, mul(u, a.r)), to: sub(b, mul(u, b.r)) };
}

/** A point on the quadratic curve at `t`, 0 at `from` and 1 at `to`. */
export function arcPoint(from: P, control: P, to: P, t: number): P {
  const u = 1 - t;
  return {
    x: u * u * from.x + 2 * u * t * control.x + t * t * to.x,
    y: u * u * from.y + 2 * u * t * control.y + t * t * to.y,
  };
}
