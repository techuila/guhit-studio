// Electrical, lighting and aircon objects are `Asset`s; the catalog item says
// what makes them devices (docs/CONTRACT.md, "Devices, fixtures and links").
// Readings the inspector and the schedules share. Pure data, no React.
//
// Guhit coordinates devices. Circuits, loads and ratings are the PEE's work
// (RA 7920) and aircon sizing is the PME's: nothing here checks either.
import type { AirconRole, AirconSpec, CatalogItem, DeviceKind, Element } from "../contract/bindings";

export type AssetEl = Extract<Element, { kind: "asset" }>;

/** The rows of the PH electrical inspection form, in form order. */
export const FORM_ROWS: DeviceKind[] = [
  "lighting_outlet",
  "convenience_receptacle",
  "special_purpose_outlet",
  "switch",
  "panelboard",
  "smoke_detector",
  "buzzer",
  "push_button",
];

export const AIRCON_KINDS: DeviceKind[] = ["aircon_indoor", "aircon_outdoor", "aircon_window"];

/** One and many, as the inspection form and the inspector say them. */
export const DEVICE_LABEL: Record<DeviceKind, [string, string]> = {
  lighting_outlet: ["Lighting outlet", "Lighting outlets"],
  convenience_receptacle: ["Convenience receptacle", "Convenience receptacles"],
  special_purpose_outlet: ["Special purpose outlet (SPO)", "Special purpose outlets (SPO)"],
  switch: ["Switch", "Switches"],
  panelboard: ["Panelboard", "Panelboards"],
  smoke_detector: ["Smoke detector", "Smoke detectors"],
  buzzer: ["Buzzer or chime", "Buzzers and chimes"],
  push_button: ["Push button", "Push buttons"],
  aircon_indoor: ["Aircon indoor unit", "Aircon indoor units"],
  aircon_outdoor: ["Aircon outdoor unit", "Aircon outdoor units"],
  aircon_window: ["Window aircon", "Window aircons"],
};

export const AIRCON_ROLE_LABEL: Record<AirconRole, string> = {
  indoor: "Indoor",
  outdoor: "Outdoor",
  window: "Window",
};

export function catalogItem(catalog: CatalogItem[], key: string): CatalogItem | undefined {
  return catalog.find((c) => c.key === key);
}

/** An object with a device row, a light, an aircon spec or links. */
export function isDevice(asset: AssetEl, item: CatalogItem | undefined): boolean {
  return !!item?.device || !!item?.aircon || asset.light !== null || asset.links.length > 0;
}

/** Devices that control or feed other objects: switches, outlets, push buttons. */
const LINK_SOURCES = new Set<DeviceKind>(["switch", "convenience_receptacle", "special_purpose_outlet", "push_button"]);

export function canLink(asset: AssetEl, item: CatalogItem | undefined): boolean {
  return asset.links.length > 0 || (!!item?.device && LINK_SOURCES.has(item.device));
}

function assetsOf(elements: Element[]): AssetEl[] {
  return elements.filter((e): e is AssetEl => e.kind === "asset");
}

/** What this device controls or feeds, in link order. Ids of deleted objects are skipped. */
export function linkedTo(asset: AssetEl, elements: Element[]): AssetEl[] {
  const byId = new Map(assetsOf(elements).map((a) => [a.id, a]));
  return asset.links.map((id) => byId.get(id)).filter((a): a is AssetEl => a !== undefined);
}

/** The devices that link to this object: the switches of a light, the outlet of an aircon unit. */
export function linkedFrom(assetId: string, elements: Element[]): AssetEl[] {
  return assetsOf(elements).filter((a) => a.id !== assetId && a.links.includes(assetId));
}

/** A switch sharing a light with another switch is a 3-way ("S3" on the plan). */
export function isThreeWay(sw: AssetEl, elements: Element[]): boolean {
  return sw.links.some((light) => linkedFrom(light, elements).some((other) => other.id !== sw.id));
}

/** The device without this link. */
export function withoutLink(asset: AssetEl, id: string): AssetEl {
  return { ...asset, links: asset.links.filter((l) => l !== id) };
}

// ---------------------------------------------------------------- mounting height

/** Wall boxes are set out to their center (switch 1200, outlet 300); everything else to its underside. */
export type MountRef = "center" | "underside";

const CENTER_MOUNTED = new Set<DeviceKind>(["switch", "convenience_receptacle", "special_purpose_outlet", "push_button", "buzzer", "panelboard"]);

export function mountRef(item: CatalogItem | undefined): MountRef {
  return item?.mount === "wall" && item.device && CENTER_MOUNTED.has(item.device) ? "center" : "underside";
}

/**
 * Mounting height in mm. The catalog puts a 115 mm switch box at 1143, so its
 * center reads 1200.5: a reading within 1 mm of a 5 mm step shows the step.
 */
export function mountingHeight(asset: Pick<AssetEl, "elevation_mm" | "height_mm">, ref: MountRef): number {
  const raw = ref === "center" ? asset.elevation_mm + asset.height_mm / 2 : asset.elevation_mm;
  const step = Math.round(raw / 5) * 5;
  return Math.abs(raw - step) <= 1 ? step : Math.round(raw * 10) / 10;
}

/** The `elevation_mm` that puts the device at this mounting height. */
export function elevationFor(heightMm: number, asset: Pick<AssetEl, "height_mm">, ref: MountRef): number {
  return ref === "center" ? heightMm - asset.height_mm / 2 : heightMm;
}

// ---------------------------------------------------------------- light

/** Lumen presets: 400 a small lamp, 700 a downlight, 900 a 9 W LED bulb, 1800 a T8 tube. */
export const LUMEN_PRESETS = [400, 700, 900, 1800];

export type LightColor = "warm" | "neutral" | "daylight";

export const LIGHT_COLORS: Array<{ value: LightColor; label: string; kelvin: number }> = [
  { value: "warm", label: "Warm white", kelvin: 3000 },
  { value: "neutral", label: "Neutral", kelvin: 4000 },
  { value: "daylight", label: "Daylight", kelvin: 6500 },
];

/** The color band a color temperature falls in. 2700 K reads as warm white. */
export function colorOf(kelvin: number): LightColor {
  if (kelvin < 3500) return "warm";
  if (kelvin < 5250) return "neutral";
  return "daylight";
}

export function kelvinOf(color: LightColor): number {
  return LIGHT_COLORS.find((c) => c.value === color)?.kelvin ?? 3000;
}

/** LED bulbs sold in PH give about 100 lm per watt. A hint, not a rating. */
export function approxLedWatts(lumens: number): number {
  return Math.max(1, Math.round(lumens / 100));
}

// ---------------------------------------------------------------- aircon

/** "6.35 mm liquid, 9.52 mm gas", or null for a window unit. */
export function lineSetLabel(spec: AirconSpec): string | null {
  if (spec.liquid_mm <= 0 && spec.gas_mm <= 0) return null;
  return `${fmt(spec.liquid_mm)} mm liquid, ${fmt(spec.gas_mm)} mm gas`;
}

/** "3 to 25 m long, up to 10 m rise". */
export function lineLimitsLabel(spec: AirconSpec): string | null {
  if (spec.max_line_m <= 0) return null;
  return `${fmt(spec.min_line_m)} to ${fmt(spec.max_line_m)} m long, up to ${fmt(spec.max_rise_m)} m rise`;
}

export function hpLabel(hp: number): string {
  return `${fmt(hp)} HP`;
}

function fmt(n: number): string {
  return String(Math.round(n * 100) / 100);
}

// ---------------------------------------------------------------- several selected

/** One circuit tag shared by all, or mixed. */
export function sharedCircuit(assets: AssetEl[]): { value: string; mixed: boolean } {
  const tags = new Set(assets.map((a) => a.circuit));
  return tags.size <= 1 ? { value: assets[0]?.circuit ?? "", mixed: false } : { value: "", mixed: true };
}

/** How many of these fixtures are on. */
export function lightsOn(assets: AssetEl[]): { on: number; total: number } {
  const lit = assets.filter((a) => a.light !== null);
  return { on: lit.filter((a) => a.light?.on).length, total: lit.length };
}
