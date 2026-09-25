import { describe, expect, it } from "vitest";
import { METER_KEY, renderExposureShift } from "./model";

describe("renderExposureShift", () => {
  it("leaves an image within a stop of the key alone", () => {
    expect(renderExposureShift(METER_KEY, 1)).toBe(0);
    expect(renderExposureShift(METER_KEY - 0.9, 1)).toBe(0);
    expect(renderExposureShift(METER_KEY + 0.9, 0)).toBe(0);
  });

  it("brightens a room lit through its windows by the gap past one stop, up to six by day", () => {
    expect(renderExposureShift(METER_KEY - 5, 1)).toBeCloseTo(4, 9);
    expect(renderExposureShift(METER_KEY - 9, 1)).toBe(6);
  });

  it("keeps night dark: half a stop brighter at most", () => {
    expect(renderExposureShift(METER_KEY - 5, 0.2)).toBe(0.5);
  });

  it("darkens a much too bright image by at most a stop", () => {
    expect(renderExposureShift(METER_KEY + 1.5, 1)).toBeCloseTo(-0.5, 9);
    expect(renderExposureShift(METER_KEY + 4, 1)).toBe(-1);
  });
});
