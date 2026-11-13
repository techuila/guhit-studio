import { describe, expect, it } from "vitest";
import { pt } from "../geom";
import { azimuthDeg, bearingText, compassOf, elevationName, letterAt, lookDir, nextGridLabel, nextLetter, nextNumber, numbered } from "./labels";

describe("grid and section labels", () => {
  it("runs letters without I and O, then doubles them", () => {
    expect([0, 1, 7, 8, 12, 13, 23].map(letterAt)).toEqual(["A", "B", "H", "J", "N", "P", "Z"]);
    expect(letterAt(24)).toBe("AA");
    expect(letterAt(25)).toBe("AB");
  });

  it("takes the first free letter or number", () => {
    expect(nextLetter([])).toBe("A");
    expect(nextLetter(["A", " b ", "D"])).toBe("C");
    expect(nextNumber(["1", "2", "4"])).toBe("3");
    expect(nextNumber(["A"])).toBe("1");
  });

  it("numbers vertical lines and letters the others", () => {
    const used = ["1", "A"];
    expect(nextGridLabel(used, pt(0, 0), pt(100, 9000))).toBe("2");
    expect(nextGridLabel(used, pt(0, 0), pt(9000, 100))).toBe("B");
    // A 45 degree line is not more vertical than horizontal.
    expect(nextGridLabel(used, pt(0, 0), pt(5000, 5000))).toBe("B");
  });

  it("numbers a taken name", () => {
    expect(numbered("North", [])).toBe("North");
    expect(numbered("North", ["north", "North 2"])).toBe("North 3");
  });
});

describe("compass and bearings", () => {
  it("measures azimuths from true north", () => {
    expect(azimuthDeg(pt(0, 1), 0)).toBeCloseTo(0);
    expect(azimuthDeg(pt(1, 0), 0)).toBeCloseTo(90);
    expect(azimuthDeg(pt(0, -1), 0)).toBeCloseTo(180);
    // North turned 90 degrees counter-clockwise points along -x.
    expect(azimuthDeg(pt(-1, 0), 90)).toBeCloseTo(0);
    expect(compassOf(pt(-1, 0), 90)).toBe("North");
  });

  it("looks left of start to end, right when flipped", () => {
    expect(lookDir(pt(0, 0), pt(1000, 0), false)).toEqual(pt(-0, 1));
    expect(lookDir(pt(0, 0), pt(1000, 0), true)).toEqual(pt(0, -1));
  });

  it("names an elevation by the side it looks toward", () => {
    // Standing north of the house looking south: the north side.
    expect(elevationName(pt(0, -1), 0)).toBe("North");
    expect(elevationName(pt(-1, 0), 0)).toBe("East");
    expect(elevationName(pt(0, 1), 0)).toBe("South");
  });

  it("writes quadrant bearings to the minute", () => {
    expect(bearingText(pt(0, 0), pt(100, 100), 0)).toBe("N 45°00' E");
    expect(bearingText(pt(0, 0), pt(100, -100), 0)).toBe("S 45°00' E");
    expect(bearingText(pt(0, 0), pt(-100, -100), 0)).toBe("S 45°00' W");
    expect(bearingText(pt(0, 0), pt(-100, 100), 0)).toBe("N 45°00' W");
    const a = (44.5 * Math.PI) / 180;
    expect(bearingText(pt(0, 0), pt(Math.sin(a), -Math.cos(a)), 0)).toBe("S 44°30' E");
  });
});
