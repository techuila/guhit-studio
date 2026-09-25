// How the light leaves a fixture. The fixture's form, its glowing parts and
// the point its light comes from belong to the catalog (scene/assets.ts:
// `group.userData.lamp`, `lampAnchor`), so there is one source of truth for
// where a lamp sits. This file only decides what kind of light that is:
// a spot for fixtures that shine down out of a ceiling or a shade (the
// catalog gives them an aim), a point for the ones that glow all around,
// and the cone and emitter size (docs/CONTRACT.md, "Devices, fixtures and
// links").
//
// Local frame, as in scene/assets.ts: meters, x across the width, y up from
// the underside, z across the depth with the back (the wall side) at -z.

import type { Asset } from "../../contract/bindings";

export type LampKind = "point" | "spot";
type V3 = [number, number, number];

/** The kind of light a fixture gives and its shape. */
export interface LampOptics {
  kind: LampKind;
  /** Spots: full cone, degrees. */
  coneDeg: number;
  /** Spots: share of the cone that fades out at its edge, 0 to 1. */
  penumbra: number;
  /** Emitter size in meters: soft shadows in the path tracer. */
  radius: number;
}

/** A lit fixture in the built scene: what the light rig needs, world meters. */
export interface LampSpec {
  /** The asset's element id. */
  id: string;
  key: string;
  kind: LampKind;
  position: V3;
  /** Unit vector the spot shines along, world. Null for point lamps. */
  direction: V3 | null;
  coneDeg: number;
  penumbra: number;
  radius: number;
  lumens: number;
  kelvin: number;
  /** `Asset::light.on` in the model. */
  on: boolean;
  levelId: string;
  /** The room the fixture hangs in, or null outdoors. */
  roomId: string | null;
  /** Area of the fixture's glowing parts, m2: sets how bright they look. Null when unknown. */
  glowAreaM2: number | null;
}

/**
 * A room on a shown level, for the light the view adds to it (light/lamps.ts):
 * the light its lamps bounce off the floor and walls, and a soft ghost light
 * when it has no fixture. View only, never in the model.
 */
export interface RoomSpec {
  roomId: string;
  levelId: string;
  /** The room's label point on its floor, world meters. */
  floor: V3;
  /** Floor to ceiling, meters. */
  heightM: number;
  areaM2: number;
}

const m = (mm: number, fallback: number) => (Number.isFinite(mm) && mm > 1 ? mm / 1000 : fallback);
const DEG = 180 / Math.PI;

/** A fixture's size in meters, as the catalog builds its form. */
export function fixtureSize(asset: Pick<Asset, "width_mm" | "depth_mm" | "height_mm">): { w: number; d: number; h: number } {
  return { w: m(asset.width_mm, 0.5), d: m(asset.depth_mm, 0.5), h: m(asset.height_mm, 0.5) };
}

/** Where the light of an object the catalog has no anchor for comes from: its middle, a little high. */
export function defaultAnchor(h: number): V3 {
  return [0, h * 0.6, 0];
}

/**
 * The light a fixture gives, from its catalog key, the aim the catalog gives
 * it (null: all around) and its size. A fixture that shines down gets a spot
 * so its ceiling is not lit from a few centimeters away:
 *
 * - Flush ceiling light: a diffuser facing down spreads its light about as a
 *   cosine. The widest spot three.js has (180 degrees) with its whole cone
 *   fading comes within a few percent of that, and sends nothing up.
 * - Tube: the bare tube also lights the walls to its sides, a flatter fade.
 * - Downlight: a recessed wide flood, 100 degrees.
 * - Pendant: the open bottom of the shade, as seen from the bulb, with a
 *   short fade at the shade's rim.
 *
 * Wall lights, the outdoor lantern and floor and table lamps glow all
 * around: point lights.
 */
export function lampOptics(key: string, aim: V3 | null, size: { w: number; d: number; h: number }, anchor: V3): LampOptics {
  const small = Math.min(size.w, size.d);
  if (!aim) {
    const radius = key === "light-floor-lamp" ? 0.08 : key === "light-wall" ? 0.05 : 0.04;
    return { kind: "point", coneDeg: 0, penumbra: 0, radius };
  }
  switch (key) {
    case "light-ceiling":
      return { kind: "spot", coneDeg: 180, penumbra: 1, radius: small * 0.35 };
    case "light-tube":
      return { kind: "spot", coneDeg: 180, penumbra: 0.7, radius: Math.min(Math.max(size.w, size.d) * 0.2, 0.25) };
    case "light-downlight":
      return { kind: "spot", coneDeg: 100, penumbra: 0.55, radius: small * 0.3 };
    case "light-pendant": {
      // From the bulb to the rim of the shade's opening.
      const drop = Math.max(anchor[1], 0.02);
      const half = Math.min(Math.max(Math.atan(small / 2 / drop) * DEG, 35), 80);
      return { kind: "spot", coneDeg: half * 2 + 8, penumbra: 0.3, radius: 0.04 };
    }
    default:
      return { kind: "spot", coneDeg: 150, penumbra: 0.6, radius: small * 0.3 };
  }
}

/** Soft light for a room with no fixture: about 40 lm per square meter, 200 to 800 lm. */
export function ghostLumens(areaM2: number): number {
  return Math.min(Math.max(40 * areaM2, 200), 800);
}
