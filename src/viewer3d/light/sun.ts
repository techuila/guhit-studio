// Where the sun is for a site, a date and a local time. NOAA's solar position
// and sunrise equations (the NOAA Global Monitoring Laboratory solar
// calculator, after Meeus, "Astronomical Algorithms"), written out here so
// there is no dependency (docs/CONTRACT.md, "Sun and light"). Pure: no
// three.js, no DOM.
//
// Azimuth is degrees clockwise from true north, altitude degrees above the
// horizon, both geometric (no refraction). Sunrise and sunset use the usual
// -0.833 degree altitude, which covers refraction and the sun's radius, and
// match NOAA's published tables to the minute.

import type { Site } from "../../contract/bindings";

const RAD = Math.PI / 180;
const DAY_MS = 86_400_000;
/** Altitude of the sun's center at sunrise and sunset. */
export const HORIZON_DEG = -0.833;

/** Manila, the site when a project has none (mirror of `defaults::default_site`). */
export const MANILA: Site = { city: "manila", latitude_deg: 14.5995, longitude_deg: 120.9842, utc_offset_min: 480 };

export interface SunPosition {
  /** Degrees clockwise from true north, 0 to 360. */
  azimuthDeg: number;
  /** Degrees above the horizon, negative below it. */
  altitudeDeg: number;
}

/** Julian centuries since J2000 at an instant. */
const centuries = (ms: number) => (ms / DAY_MS + 2440587.5 - 2451545) / 36525;

interface SolarTerms {
  /** Declination, radians. */
  declination: number;
  /** Equation of time, minutes. */
  equationOfTime: number;
}

/** Declination and equation of time (NOAA, from Meeus). */
function solarTerms(t: number): SolarTerms {
  const l0 = ((280.46646 + t * (36000.76983 + t * 0.0003032)) % 360) * RAD;
  const m = (357.52911 + t * (35999.05029 - 0.0001537 * t)) * RAD;
  const e = 0.016708634 - t * (0.000042037 + 0.0000001267 * t);
  const center =
    Math.sin(m) * (1.914602 - t * (0.004817 + 0.000014 * t)) + Math.sin(2 * m) * (0.019993 - 0.000101 * t) + Math.sin(3 * m) * 0.000289;
  const omega = (125.04 - 1934.136 * t) * RAD;
  const apparent = (l0 / RAD + center - 0.00569 - 0.00478 * Math.sin(omega)) * RAD;
  const meanObliquity = 23 + (26 + (21.448 - t * (46.815 + t * (0.00059 - t * 0.001813))) / 60) / 60;
  const obliquity = (meanObliquity + 0.00256 * Math.cos(omega)) * RAD;
  const declination = Math.asin(Math.sin(obliquity) * Math.sin(apparent));
  const y = Math.tan(obliquity / 2) ** 2;
  const eot =
    y * Math.sin(2 * l0) -
    2 * e * Math.sin(m) +
    4 * e * y * Math.sin(m) * Math.cos(2 * l0) -
    0.5 * y * y * Math.sin(4 * l0) -
    1.25 * e * e * Math.sin(2 * m);
  return { declination, equationOfTime: (4 * eot) / RAD };
}

/** The instant (UTC ms) of a local date and time at a site. */
export function siteInstant(site: Site, year: number, month: number, day: number, minutes: number): number {
  return Date.UTC(year, month - 1, day, 0, 0) + (minutes - site.utc_offset_min) * 60_000;
}

/** Sun position at an instant (UTC ms) seen from a latitude and longitude. */
export function sunPositionAt(ms: number, latitudeDeg: number, longitudeDeg: number): SunPosition {
  const { declination, equationOfTime } = solarTerms(centuries(ms));
  const phi = latitudeDeg * RAD;
  // True solar time in minutes after solar midnight, then the hour angle.
  const utcMin = (((ms % DAY_MS) + DAY_MS) % DAY_MS) / 60_000;
  const solarMin = (((utcMin + equationOfTime + 4 * longitudeDeg) % 1440) + 1440) % 1440;
  const hourAngle = (solarMin / 4 - 180) * RAD;
  const cosZenith = Math.min(
    1,
    Math.max(-1, Math.sin(phi) * Math.sin(declination) + Math.cos(phi) * Math.cos(declination) * Math.cos(hourAngle)),
  );
  const zenith = Math.acos(cosZenith);
  // Azimuth from north, clockwise: atan2 of the east and north components.
  const east = -Math.cos(declination) * Math.sin(hourAngle);
  const north = Math.cos(phi) * Math.sin(declination) - Math.sin(phi) * Math.cos(declination) * Math.cos(hourAngle);
  const azimuth = Math.atan2(east, north) / RAD;
  return { azimuthDeg: ((azimuth % 360) + 360) % 360, altitudeDeg: 90 - zenith / RAD };
}

/** Sun position for a local date and time at a site. */
export function sunPosition(site: Site, year: number, month: number, day: number, minutes: number): SunPosition {
  return sunPositionAt(siteInstant(site, year, month, day, minutes), site.latitude_deg, site.longitude_deg);
}

export interface SunTimes {
  /** Minutes after local midnight, or null when the sun does not rise or set that day. */
  sunrise: number | null;
  sunset: number | null;
  /** Solar noon, minutes after local midnight. */
  noon: number;
}

/**
 * Sunrise, solar noon and sunset (NOAA's method: solve with the sun of
 * midnight UTC, then once more with the sun at that first answer), in local
 * minutes. `altitudeDeg` picks the event: -0.833 is sunrise and sunset, -6
 * civil twilight.
 */
export function sunTimes(site: Site, year: number, month: number, day: number, altitudeDeg = HORIZON_DEG): SunTimes {
  const midnightUtc = Date.UTC(year, month - 1, day);
  const lat = site.latitude_deg * RAD;
  const lng = site.longitude_deg;
  const local = (utcMin: number) => ((((utcMin + site.utc_offset_min) % 1440) + 1440) % 1440);
  // Minutes after midnight UTC of `day` at which the sun crosses the altitude.
  const cross = (rising: boolean): number | null => {
    let at = 720;
    for (let i = 0; i < 2; i++) {
      const { declination, equationOfTime } = solarTerms(centuries(midnightUtc + at * 60_000));
      const cosH = (Math.cos((90 - altitudeDeg) * RAD) - Math.sin(lat) * Math.sin(declination)) / (Math.cos(lat) * Math.cos(declination));
      if (cosH < -1 || cosH > 1) return null;
      const h = Math.acos(cosH) / RAD;
      at = 720 - 4 * (lng + (rising ? h : -h)) - equationOfTime;
    }
    return at;
  };
  const noonTerms = solarTerms(centuries(midnightUtc + (720 - 4 * lng) * 60_000));
  const noonUtc = 720 - 4 * lng - noonTerms.equationOfTime;
  const rise = cross(true);
  const set = cross(false);
  return { sunrise: rise === null ? null : local(rise), sunset: set === null ? null : local(set), noon: local(noonUtc) };
}

/**
 * Unit vector towards the sun in plan space (x east, y north, z up), with
 * true north `northAngleDeg` counter-clockwise from +y (ProjectSettings).
 */
export function sunVector(sun: SunPosition, northAngleDeg: number): { x: number; y: number; z: number } {
  // Plan angle of the azimuth: true north sits at 90 + north degrees, and
  // compass azimuth turns clockwise from it.
  const a = (90 + northAngleDeg - sun.azimuthDeg) * RAD;
  const alt = sun.altitudeDeg * RAD;
  return { x: Math.cos(a) * Math.cos(alt), y: Math.sin(a) * Math.cos(alt), z: Math.sin(alt) };
}

export interface ArcPoint extends SunPosition {
  minutes: number;
}

/** The sun's track over one day, above `minAltitudeDeg`, every `stepMin` minutes. */
export function dayArc(site: Site, year: number, month: number, day: number, stepMin = 10, minAltitudeDeg = 0): ArcPoint[] {
  const out: ArcPoint[] = [];
  for (let m = 0; m <= 1440; m += stepMin) {
    const p = sunPosition(site, year, month, day, m);
    if (p.altitudeDeg >= minAltitudeDeg) out.push({ minutes: m, ...p });
  }
  return out;
}

/** "6:05 AM", "12:00 PM". Wraps past midnight. */
export function clockLabel(minutes: number): string {
  const m = ((Math.round(minutes) % 1440) + 1440) % 1440;
  const h24 = Math.floor(m / 60);
  const h12 = h24 % 12 === 0 ? 12 : h24 % 12;
  return `${h12}:${String(m % 60).padStart(2, "0")} ${h24 < 12 ? "AM" : "PM"}`;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** "Jun 21". */
export function dateLabel(month: number, day: number): string {
  return `${MONTHS[Math.min(Math.max(Math.round(month), 1), 12) - 1]} ${Math.round(day)}`;
}

const COMPASS = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];

/** "SE" for 135 degrees. */
export function compassLabel(azimuthDeg: number): string {
  return COMPASS[Math.round((((azimuthDeg % 360) + 360) % 360) / 22.5) % 16];
}

/** Days in a month of a year, for date inputs. */
export function daysInMonth(year: number, month: number): number {
  return new Date(Date.UTC(year, month, 0)).getUTCDate();
}
