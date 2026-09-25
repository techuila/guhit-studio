// Where the house stands, for the sun: bundled Philippine city presets and
// helpers over `ProjectSettings::site`. There is no geocoding service
// (docs/CONTRACT.md, "Sun and light"). Pure data, no React.
import type { ProjectSettings, Site } from "../contract/bindings";

export interface SitePreset {
  /** Stored in `Site::city`. */
  key: string;
  label: string;
  latitude: number;
  longitude: number;
}

/** Philippine time, UTC+8 all year, no daylight saving. */
export const PH_UTC_OFFSET_MIN = 480;

/** City centers, rounded to about 100 m. Manila first: it is the default site. */
export const SITE_PRESETS: SitePreset[] = [
  { key: "manila", label: "Manila", latitude: 14.5995, longitude: 120.9842 },
  { key: "quezon_city", label: "Quezon City", latitude: 14.676, longitude: 121.0437 },
  { key: "baguio", label: "Baguio", latitude: 16.4023, longitude: 120.596 },
  { key: "laoag", label: "Laoag", latitude: 18.1978, longitude: 120.5936 },
  { key: "iloilo", label: "Iloilo", latitude: 10.7202, longitude: 122.5621 },
  { key: "cebu", label: "Cebu", latitude: 10.3157, longitude: 123.8854 },
  { key: "puerto_princesa", label: "Puerto Princesa", latitude: 9.7392, longitude: 118.7353 },
  { key: "davao", label: "Davao", latitude: 7.1907, longitude: 125.4553 },
  { key: "zamboanga", label: "Zamboanga", latitude: 6.9214, longitude: 122.079 },
];

export const CUSTOM_SITE = "custom";

/** Mirror of `defaults::default_site`: Manila. */
export const DEFAULT_SITE: Site = {
  city: "manila",
  latitude_deg: 14.5995,
  longitude_deg: 120.9842,
  utc_offset_min: PH_UTC_OFFSET_MIN,
};

/** The site in use: the project's, or Manila when it has none. */
export function siteOf(settings: Pick<ProjectSettings, "site"> | null | undefined): Site {
  return settings?.site ?? DEFAULT_SITE;
}

export function presetByKey(key: string): SitePreset | undefined {
  return SITE_PRESETS.find((p) => p.key === key);
}

/** A site at a preset city, on Philippine time. */
export function siteFromPreset(key: string): Site | null {
  const p = presetByKey(key);
  return p ? { city: p.key, latitude_deg: p.latitude, longitude_deg: p.longitude, utc_offset_min: PH_UTC_OFFSET_MIN } : null;
}

/** The preset at exactly these coordinates (within about 10 m), if any. */
export function presetAt(latitude: number, longitude: number): SitePreset | undefined {
  return SITE_PRESETS.find((p) => Math.abs(p.latitude - latitude) < 1e-4 && Math.abs(p.longitude - longitude) < 1e-4);
}

/**
 * The site after a hand edit of latitude, longitude or UTC offset. It stays a
 * preset city only while its coordinates still match that city.
 */
export function editSite(site: Site, patch: Partial<Omit<Site, "city">>): Site {
  const next = { ...site, ...patch };
  const at = presetAt(next.latitude_deg, next.longitude_deg);
  return { ...next, city: at && next.utc_offset_min === PH_UTC_OFFSET_MIN ? at.key : CUSTOM_SITE };
}

/** "Manila", or "14.60 N, 120.98 E" for a custom site. */
export function siteLabel(site: Site): string {
  const p = presetByKey(site.city);
  if (p) return p.label;
  const lat = `${Math.abs(site.latitude_deg).toFixed(2)} ${site.latitude_deg >= 0 ? "N" : "S"}`;
  const lng = `${Math.abs(site.longitude_deg).toFixed(2)} ${site.longitude_deg >= 0 ? "E" : "W"}`;
  return `${lat}, ${lng}`;
}

/** "UTC+8", "UTC+5:30", "UTC-3". */
export function utcLabel(offsetMin: number): string {
  const sign = offsetMin < 0 ? "-" : "+";
  const abs = Math.abs(Math.round(offsetMin));
  const h = Math.floor(abs / 60);
  const m = abs % 60;
  return `UTC${sign}${h}${m ? `:${String(m).padStart(2, "0")}` : ""}`;
}
