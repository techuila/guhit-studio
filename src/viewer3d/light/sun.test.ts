import { describe, expect, it } from "vitest";
import { MANILA, clockLabel, compassLabel, dayArc, sunPosition, sunTimes, sunVector } from "./sun";

// NOAA Global Monitoring Laboratory sunrise and sunset tables for
// 14.59950 N, 120.98420 E (Manila), 2026, time zone Asia/Manila (UTC+8):
// https://gml.noaa.gov/grad/solcalc/table.php?lat=14.5995&lon=120.9842&year=2026
// (fetched 2026-09-23). Times are rounded to the minute.
const NOAA_MANILA_2026: Array<[month: number, day: number, rise: string, set: string]> = [
  [1, 1, "06:21", "17:38"],
  [3, 20, "06:00", "18:07"],
  [5, 15, "05:28", "18:17"],
  [6, 21, "05:28", "18:28"],
  [9, 23, "05:45", "17:52"],
  [12, 21, "06:16", "17:32"],
];

const minutes = (hhmm: string) => {
  const [h, m] = hhmm.split(":").map(Number);
  return h * 60 + m;
};

describe("sun times", () => {
  it.each(NOAA_MANILA_2026)("Manila %i/%i matches NOAA within 2 minutes", (month, day, rise, set) => {
    const t = sunTimes(MANILA, 2026, month, day);
    expect(t.sunrise).not.toBeNull();
    expect(t.sunset).not.toBeNull();
    // The target is 2 minutes; NOAA's own method lands within its rounding.
    expect(Math.abs((t.sunrise ?? 0) - minutes(rise))).toBeLessThanOrEqual(1);
    expect(Math.abs((t.sunset ?? 0) - minutes(set))).toBeLessThanOrEqual(1);
  });

  it("puts solar noon near 12:05 PM in Manila, which sits west of the UTC+8 meridian", () => {
    const t = sunTimes(MANILA, 2026, 6, 21);
    expect(t.noon).toBeGreaterThan(11 * 60 + 55);
    expect(t.noon).toBeLessThan(12 * 60 + 15);
  });
});

describe("sun position", () => {
  it("puts the June solstice noon sun north of overhead in Manila", () => {
    const noon = sunTimes(MANILA, 2026, 6, 21).noon;
    const p = sunPosition(MANILA, 2026, 6, 21, noon);
    // 90 - (23.44 - 14.60) = 81.2 degrees, towards the north.
    expect(p.altitudeDeg).toBeGreaterThan(80.5);
    expect(p.altitudeDeg).toBeLessThan(82);
    expect(p.azimuthDeg < 10 || p.azimuthDeg > 350).toBe(true);
  });

  it("puts the December solstice noon sun low in the south", () => {
    const noon = sunTimes(MANILA, 2026, 12, 21).noon;
    const p = sunPosition(MANILA, 2026, 12, 21, noon);
    // 90 - (14.60 + 23.44) = 52.0 degrees.
    expect(p.altitudeDeg).toBeGreaterThan(51.3);
    expect(p.altitudeDeg).toBeLessThan(52.7);
    expect(Math.abs(p.azimuthDeg - 180)).toBeLessThan(3);
  });

  it("rises in the east and sets in the west, below the horizon at night", () => {
    expect(compassLabel(sunPosition(MANILA, 2026, 3, 20, 7 * 60).azimuthDeg)).toBe("E");
    expect(compassLabel(sunPosition(MANILA, 2026, 3, 20, 17 * 60 + 30).azimuthDeg)).toBe("W");
    expect(sunPosition(MANILA, 2026, 3, 20, 20 * 60).altitudeDeg).toBeLessThan(-20);
  });

  it("turns the sun with the project's north angle", () => {
    const east = { azimuthDeg: 90, altitudeDeg: 0 };
    const v0 = sunVector(east, 0);
    expect(v0.x).toBeCloseTo(1, 6);
    expect(v0.y).toBeCloseTo(0, 6);
    // True north 90 degrees counter-clockwise from +y points at -x, so east is +y.
    const v90 = sunVector(east, 90);
    expect(v90.x).toBeCloseTo(0, 6);
    expect(v90.y).toBeCloseTo(1, 6);
    const up = sunVector({ azimuthDeg: 0, altitudeDeg: 90 }, 30);
    expect(up.z).toBeCloseTo(1, 6);
  });

  it("traces a day arc from sunrise to sunset", () => {
    const arc = dayArc(MANILA, 2026, 6, 21, 15);
    expect(arc.length).toBeGreaterThan(40);
    expect(arc[0].minutes).toBeGreaterThanOrEqual(5 * 60 + 15);
    expect(arc[arc.length - 1].minutes).toBeLessThanOrEqual(18 * 60 + 30);
    expect(arc.every((p) => p.altitudeDeg >= 0)).toBe(true);
  });

  it("formats clock times", () => {
    expect(clockLabel(0)).toBe("12:00 AM");
    expect(clockLabel(12 * 60)).toBe("12:00 PM");
    expect(clockLabel(15 * 60 + 5)).toBe("3:05 PM");
    expect(clockLabel(20 * 60)).toBe("8:00 PM");
  });
});
