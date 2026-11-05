// Built-ins (lane L5): kitchen runs with their wall cabinets, built-in
// wardrobes, vanities, counters and wall shelves, sized in code to whatever
// width, depth and height the object has (DECISIONS D51, "next, in Wave 2").
// Their options are `Asset::built_in`; an asset without them gets the
// defaults below, which `guhit_core::placement::built_in_defaults` mirrors
// rule for rule, so the plan symbol, the 3D form and the engine agree.
//
// The 3D forms live in assets.ts with every other catalog form, so a
// built-in is picked, highlighted, cached and cut away like any object.
// `buildBuiltIns` is the scene hook for parts that are not one object; there
// are none yet.

import type { BuiltInKind, BuiltInParams } from "../../contract/bindings";
import type { SceneHost } from "./buildScene";

/** Catalog keys drawn as built-ins (`CatalogItem::parametric` in catalog.rs). */
export const BUILT_IN_KIND: Readonly<Record<string, BuiltInKind>> = {
  "kitchen-run": "kitchen_run",
  "wardrobe-builtin": "wardrobe",
  vanity: "vanity",
  counter: "counter",
  "shelf-wall": "shelf",
};

/** True for a catalog key that is a built-in. */
export function isBuiltInKey(key: string): boolean {
  return Object.prototype.hasOwnProperty.call(BUILT_IN_KIND, key);
}

const round50 = (v: number) => Math.round(v / 50) * 50;
const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), Math.max(lo, hi));

/**
 * The options a built-in has when its asset carries none: a kitchen run's
 * sink 30 percent along from the left end (seen from the front) and its
 * cooktop at three quarters, at least 900 mm on, a vanity's basin in the
 * middle under a mirror cabinet, about one door per 450 mm (500 for a
 * wardrobe). Same rules as `guhit_core::placement::built_in_defaults`.
 */
export function builtInDefaults(kind: BuiltInKind, widthMm: number): BuiltInParams {
  const w = Number.isFinite(widthMm) && widthMm > 0 ? widthMm : 600;
  const doors = (per: number, min: number) => Math.max(min, Math.round(w / per));
  switch (kind) {
    case "kitchen_run": {
      const sink = w >= 900 ? round50(clamp(w * 0.3, 450, w - 450)) : round50(w / 2);
      let cooktop: number | null = null;
      if (w >= 1800) {
        const at = round50(clamp(Math.max(w * 0.75, sink + 900), 350, w - 350));
        if (Math.abs(at - sink) >= 600) cooktop = at;
      }
      return { upper: true, sink_at_mm: sink, cooktop_at_mm: cooktop, doors: doors(450, 1) };
    }
    case "vanity":
      return { upper: true, sink_at_mm: round50(w / 2), cooktop_at_mm: null, doors: doors(450, 1) };
    case "counter":
      return { upper: false, sink_at_mm: null, cooktop_at_mm: null, doors: doors(450, 1) };
    case "wardrobe":
      return { upper: false, sink_at_mm: null, cooktop_at_mm: null, doors: doors(500, 2) };
    case "shelf":
      return { upper: false, sink_at_mm: null, cooktop_at_mm: null, doors: 0 };
  }
}

/**
 * The options to draw a built-in with: its own, or the defaults for its
 * width. Null for a key that is not a built-in.
 */
export function builtInParams(key: string, widthMm: number, given?: BuiltInParams | null): BuiltInParams | null {
  const kind = BUILT_IN_KIND[key];
  if (!kind) return null;
  if (given) return given;
  return builtInDefaults(kind, widthMm);
}

/** Scene hook after the placed objects. Built-ins are drawn as objects (assets.ts); nothing else yet. */
export function buildBuiltIns(_host: SceneHost): void {}
