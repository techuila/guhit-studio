import { describe, expect, it } from "vitest";
import {
  CUSTOM_SITE,
  DEFAULT_SITE,
  SITE_PRESETS,
  editSite,
  presetAt,
  siteFromPreset,
  siteLabel,
  siteOf,
  utcLabel,
} from "./site";

describe("site presets", () => {
  it("lists the PH cities, Manila first", () => {
    expect(SITE_PRESETS.map((p) => p.label)).toEqual([
      "Manila",
      "Quezon City",
      "Baguio",
      "Laoag",
      "Iloilo",
      "Cebu",
      "Puerto Princesa",
      "Davao",
      "Zamboanga",
    ]);
  });

  it("keeps every city inside the Philippines, on UTC+8", () => {
    for (const p of SITE_PRESETS) {
      expect(p.latitude).toBeGreaterThan(4.5);
      expect(p.latitude).toBeLessThan(21.5);
      expect(p.longitude).toBeGreaterThan(116);
      expect(p.longitude).toBeLessThan(127);
      expect(siteFromPreset(p.key)?.utc_offset_min).toBe(480);
    }
  });

  it("uses unique keys", () => {
    expect(new Set(SITE_PRESETS.map((p) => p.key)).size).toBe(SITE_PRESETS.length);
    expect(SITE_PRESETS.some((p) => p.key === CUSTOM_SITE)).toBe(false);
  });

  it("defaults to Manila, as defaults::default_site does", () => {
    expect(siteOf({ site: null })).toEqual(DEFAULT_SITE);
    expect(DEFAULT_SITE).toEqual({ city: "manila", latitude_deg: 14.5995, longitude_deg: 120.9842, utc_offset_min: 480 });
    expect(siteFromPreset("manila")).toEqual(DEFAULT_SITE);
    expect(siteFromPreset("nowhere")).toBeNull();
  });

  it("builds a preset site", () => {
    expect(siteFromPreset("davao")).toEqual({ city: "davao", latitude_deg: 7.1907, longitude_deg: 125.4553, utc_offset_min: 480 });
  });
});

describe("editing a site", () => {
  it("turns custom when the coordinates leave the city", () => {
    const cebu = siteFromPreset("cebu")!;
    expect(editSite(cebu, { latitude_deg: 10.4 })).toEqual({ ...cebu, latitude_deg: 10.4, city: CUSTOM_SITE });
  });

  it("turns custom when the time zone changes", () => {
    expect(editSite(DEFAULT_SITE, { utc_offset_min: 540 }).city).toBe(CUSTOM_SITE);
  });

  it("finds the city again when the coordinates match it", () => {
    const custom = { city: CUSTOM_SITE, latitude_deg: 0, longitude_deg: 0, utc_offset_min: 480 };
    expect(editSite(custom, { latitude_deg: 16.4023, longitude_deg: 120.596 }).city).toBe("baguio");
    expect(presetAt(9.7392, 118.7353)?.key).toBe("puerto_princesa");
    expect(presetAt(9.8, 118.7353)).toBeUndefined();
  });
});

describe("site labels", () => {
  it("names a city or writes the coordinates", () => {
    expect(siteLabel(DEFAULT_SITE)).toBe("Manila");
    expect(siteLabel({ city: CUSTOM_SITE, latitude_deg: 13.4125, longitude_deg: 122.5621, utc_offset_min: 480 })).toBe("13.41 N, 122.56 E");
    expect(siteLabel({ city: CUSTOM_SITE, latitude_deg: -6.2, longitude_deg: -35.1, utc_offset_min: -180 })).toBe("6.20 S, 35.10 W");
  });

  it("writes the UTC offset", () => {
    expect(utcLabel(480)).toBe("UTC+8");
    expect(utcLabel(330)).toBe("UTC+5:30");
    expect(utcLabel(-180)).toBe("UTC-3");
    expect(utcLabel(0)).toBe("UTC+0");
  });
});
