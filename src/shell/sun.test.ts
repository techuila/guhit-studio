import { describe, expect, it } from "vitest";
import { DEFAULT_SITE, siteFromPreset } from "./site";
import { formatClock, presetAtTime, stepMinutes, stepPreset, sunPresets, sunsetMinutes, toggleLamps } from "./sun";

const hm = (h: number, m: number) => h * 60 + m;

describe("sunset", () => {
  // PAGASA and timeanddate.com list these local sunsets for Manila.
  it.each([
    [6, 21, hm(18, 28)],
    [12, 21, hm(17, 32)],
    [9, 23, hm(17, 53)],
    [3, 20, hm(18, 7)],
  ])("sets in Manila on %i/%i within two minutes of the tables", (month, day, expected) => {
    const m = sunsetMinutes(DEFAULT_SITE, 2026, month, day)!;
    expect(Math.abs(m - expected)).toBeLessThanOrEqual(2);
  });

  it("sets earlier in Davao than in Manila in June, east and nearer the equator", () => {
    const davao = sunsetMinutes(siteFromPreset("davao")!, 2026, 6, 21)!;
    const manila = sunsetMinutes(DEFAULT_SITE, 2026, 6, 21)!;
    expect(davao).toBeLessThan(manila - 20);
  });

  it("follows the site's UTC offset", () => {
    const utc = sunsetMinutes({ ...DEFAULT_SITE, utc_offset_min: 0 }, 2026, 6, 21)!;
    const ph = sunsetMinutes(DEFAULT_SITE, 2026, 6, 21)!;
    expect(ph - utc).toBeCloseTo(480, 0);
  });

  it("returns null where the sun does not set", () => {
    expect(sunsetMinutes({ ...DEFAULT_SITE, latitude_deg: 80 }, 2026, 6, 21)).toBeNull();
  });
});

describe("sun presets", () => {
  const presets = sunPresets(DEFAULT_SITE, 2026, 9, 23);

  it("has Morning 8:00, Noon, Afternoon 3 PM, Dusk and Night 8 PM, in time order", () => {
    expect(presets.map((p) => p.id)).toEqual(["morning", "noon", "afternoon", "dusk", "night"]);
    expect(presets.map((p) => formatClock(p.minutes))).toEqual(["8:00 AM", "12:00 PM", "3:00 PM", formatClock(presets[3].minutes), "8:00 PM"]);
  });

  it("puts Dusk 20 minutes after sunset, with the lamps on", () => {
    const dusk = presets.find((p) => p.id === "dusk")!;
    expect(dusk.minutes).toBe(Math.round(sunsetMinutes(DEFAULT_SITE, 2026, 9, 23)! + 20));
    expect(dusk.lamps).toBe("on");
    expect(presets.filter((p) => p.id !== "dusk").every((p) => p.lamps === "auto")).toBe(true);
  });

  it("steps to the next and previous preset, wrapping at the ends", () => {
    expect(stepPreset(presets, hm(10, 7), 1).id).toBe("noon");
    expect(stepPreset(presets, hm(10, 7), -1).id).toBe("morning");
    expect(stepPreset(presets, hm(12, 0), 1).id).toBe("afternoon");
    expect(stepPreset(presets, hm(12, 0), -1).id).toBe("morning");
    expect(stepPreset(presets, hm(20, 0), 1).id).toBe("morning");
    expect(stepPreset(presets, hm(8, 0), -1).id).toBe("night");
    expect(stepPreset(presets, hm(5, 0), -1).id).toBe("night");
    expect(stepPreset(presets, hm(22, 0), 1).id).toBe("morning");
    expect(stepPreset(presets, hm(16, 0), 1).id).toBe("dusk");
  });

  it("knows the preset a time sits on", () => {
    expect(presetAtTime(presets, hm(15, 0))?.id).toBe("afternoon");
    expect(presetAtTime(presets, hm(15, 15))).toBeUndefined();
  });
});

describe("U and I", () => {
  it("steps 15 minutes on the quarter-hour grid", () => {
    expect(stepMinutes(hm(10, 0), 1)).toBe(hm(10, 15));
    expect(stepMinutes(hm(10, 0), -1)).toBe(hm(9, 45));
    expect(stepMinutes(hm(10, 7), 1)).toBe(hm(10, 15));
    expect(stepMinutes(hm(10, 7), -1)).toBe(hm(10, 0));
  });

  it("stays inside the day", () => {
    expect(stepMinutes(0, -1)).toBe(0);
    expect(stepMinutes(hm(23, 45), 1)).toBe(1439);
    expect(stepMinutes(1439, -1)).toBe(hm(23, 45));
  });

  it("scrubs when held: repeated steps add up", () => {
    let m = hm(6, 0);
    for (let i = 0; i < 8; i++) m = stepMinutes(m, 1);
    expect(m).toBe(hm(8, 0));
  });
});

describe("Shift+N and the clock", () => {
  it("switches the lamps between on and auto", () => {
    expect(toggleLamps("auto")).toBe("on");
    expect(toggleLamps("on")).toBe("auto");
    expect(toggleLamps("off")).toBe("on");
  });

  it("writes a 12 hour clock", () => {
    expect(formatClock(0)).toBe("12:00 AM");
    expect(formatClock(hm(12, 0))).toBe("12:00 PM");
    expect(formatClock(hm(18, 8))).toBe("6:08 PM");
    expect(formatClock(hm(9, 5))).toBe("9:05 AM");
  });
});
