import { describe, expect, it } from "vitest";
import {
  ACTIVE_REST,
  ACTIVE_SLICE_MS,
  COL_STEP,
  IDLE_AFTER_MS,
  IDLE_SLICE_MAX_MS,
  IDLE_SLICE_MS,
  learnedCost,
  nextBand,
  nextSlice,
  paceMode,
  previewEvery,
  PROBE_ROWS,
  restAfter,
  ROW_STEP,
  RowCosts,
  samplingDone,
  sleepBeforePoll,
  sliceTarget,
} from "./pacing";

/** Row costs for a 1080 row image: every row `ms` at full width. */
function flat(ms: number, rows = 1080): RowCosts {
  const c = new RowCosts(rows);
  c.record(0, rows, ms * rows);
  return c;
}

describe("pace mode", () => {
  it("is active right after input and idle once the user has been away long enough", () => {
    expect(paceMode(1000, 900, true, true)).toBe("active");
    expect(paceMode(900 + IDLE_AFTER_MS - 1, 900, true, true)).toBe("active");
    expect(paceMode(900 + IDLE_AFTER_MS, 900, true, true)).toBe("idle");
  });

  it("is idle while the window is hidden or out of focus, input or not", () => {
    expect(paceMode(1000, 1000, false, true)).toBe("idle");
    expect(paceMode(1000, 1000, true, false)).toBe("idle");
  });

  it("aims at short slices while active and 60 to 100 ms ones while idle", () => {
    expect(sliceTarget("active")).toBe(ACTIVE_SLICE_MS);
    expect(ACTIVE_SLICE_MS).toBeLessThanOrEqual(16);
    expect(sliceTarget("idle")).toBe(IDLE_SLICE_MS);
    expect(IDLE_SLICE_MS).toBeGreaterThanOrEqual(60);
    expect(IDLE_SLICE_MS).toBeLessThanOrEqual(100);
  });

  it("leaves the GPU alone after each slice only while the user works", () => {
    expect(restAfter("active", 10)).toBe(10 * ACTIVE_REST);
    expect(restAfter("idle", 10)).toBe(0);
    expect(restAfter("active", 10, 0.5)).toBe(5);
  });

  it("updates the preview less often while the user works", () => {
    expect(previewEvery("active")).toBeGreaterThan(previewEvery("idle"));
  });
});

describe("when sampling stops", () => {
  const at = (samples: number, elapsedMs: number, saving = false) =>
    samplingDone({ saving, samples, targetSamples: 384, elapsedMs, budgetMs: 60_000 });

  it("runs until the time budget or the sample target", () => {
    expect(at(10, 30_000)).toBe(false);
    expect(at(10, 60_000)).toBe(true);
    expect(at(384, 20_000)).toBe(true);
  });

  it("does not let the time budget end a render before its first whole sample", () => {
    expect(at(0, 60_000)).toBe(false);
    expect(at(0, 180_000)).toBe(false);
    expect(at(1, 60_001)).toBe(true);
  });

  it("stops on Stop and save, samples or not", () => {
    expect(at(0, 1_000, true)).toBe(true);
    expect(at(5, 1_000, true)).toBe(true);
  });
});

describe("row costs", () => {
  it("predicts from what was measured, and counts unmeasured rows as the dearest one", () => {
    const c = new RowCosts(100);
    expect(Number.isNaN(c.dearest)).toBe(true);
    expect(Number.isNaN(c.sample)).toBe(true);
    c.record(0, 10, 10); // 1 ms a row
    c.record(10, 10, 30); // 3 ms a row
    expect(c.dearest).toBeCloseTo(3, 9);
    expect(c.predict(0, 10)).toBeCloseTo(10, 9);
    expect(c.predict(15, 10)).toBeCloseTo(5 * 3 + 5 * 3, 9);
    expect(c.known(0, 20)).toBe(true);
    expect(c.known(0, 21)).toBe(false);
    expect(c.sample).toBeCloseTo(10 + 30 + 80 * 3, 9);
  });

  it("scales a slice across part of the width up to the whole row", () => {
    const c = new RowCosts(10);
    c.record(0, 10, 5, 0.25);
    expect(c.predict(0, 10)).toBeCloseTo(20, 9);
  });

  it("leans on the newest reading of a row", () => {
    const c = new RowCosts(1);
    c.record(0, 1, 10);
    c.record(0, 1, 20);
    expect(c.ms[0]).toBeCloseTo(10 * 0.3 + 20 * 0.7, 9);
  });
});

describe("bands and slices", () => {
  it("probes a first, unmeasured sample with a small band", () => {
    const c = new RowCosts(1080);
    expect(nextBand(c, 0, "idle")).toBe(PROBE_ROWS);
    expect(nextSlice(c, { row: 0, bandRows: 0, col: 0 }, 1920, "active")).toEqual({ rows: PROBE_ROWS, cols: 1920, predictedMs: Number.NaN });
  });

  it("cuts bands to the slice time, in whole steps of rows", () => {
    const c = flat(1); // 1 ms a row
    const active = nextBand(c, 0, "active");
    expect(active % ROW_STEP).toBe(0);
    expect(c.predict(0, active)).toBeLessThanOrEqual(ACTIVE_SLICE_MS);
    const idle = nextBand(c, 0, "idle");
    expect(idle).toBeGreaterThan(active);
    expect(c.predict(0, idle)).toBeLessThanOrEqual(IDLE_SLICE_MS);
    expect(c.predict(0, idle + ROW_STEP)).toBeGreaterThan(IDLE_SLICE_MS);
  });

  it("traces a whole cheap sample in one slice while idle: a single tile", () => {
    const c = flat(IDLE_SLICE_MAX_MS / 1080 / 2);
    expect(nextSlice(c, { row: 0, bandRows: 0, col: 0 }, 1920, "idle")).toMatchObject({ rows: 1080, cols: 1920 });
    // Not while the user works.
    expect(nextSlice(c, { row: 0, bandRows: 0, col: 0 }, 1920, "active").rows).toBeLessThan(1080);
  });

  it("never leaves a sliver of rows for the next band", () => {
    const c = flat(1);
    const rows = nextBand(c, 1080 - ROW_STEP - 3, "idle");
    expect(rows).toBe(ROW_STEP + 3);
  });

  it("cuts a band across when even one step of rows is too long: a slow GPU still gets short slices", () => {
    const c = flat(4); // a step of 8 rows takes 32 ms
    const s = nextSlice(c, { row: 0, bandRows: 0, col: 0 }, 1920, "active");
    expect(s.rows).toBe(ROW_STEP);
    expect(s.cols % COL_STEP).toBe(0);
    expect(s.cols).toBeLessThan(1920);
    expect(s.predictedMs).toBeLessThanOrEqual(ACTIVE_SLICE_MS);
  });

  it("drops to short slices at once when input comes in the middle of a long band", () => {
    const c = flat(1);
    const band = nextBand(c, 0, "idle");
    // The user comes back halfway across that band: the rest of it goes in short slices.
    const s = nextSlice(c, { row: 0, bandRows: band, col: 960 }, 1920, "active");
    expect(s.rows).toBe(band);
    expect(s.cols).toBeLessThan(960);
    expect(s.predictedMs).toBeLessThanOrEqual(ACTIVE_SLICE_MS);
  });

  it("finishes the rest of a band in one slice when the user leaves", () => {
    const c = flat(1);
    const s = nextSlice(c, { row: 0, bandRows: 16, col: 640 }, 1920, "idle");
    expect(s).toMatchObject({ rows: 16, cols: 1280 });
  });
});

describe("waiting on the GPU", () => {
  it("naps through part of a long slice, never on a hidden page or a short slice", () => {
    expect(sleepBeforePoll(80, false)).toBeGreaterThan(20);
    expect(sleepBeforePoll(80, false)).toBeLessThan(80 / 2);
    expect(sleepBeforePoll(80, true)).toBe(0);
    expect(sleepBeforePoll(6, false)).toBe(0);
    expect(sleepBeforePoll(Number.NaN, false)).toBe(0);
  });

  it("learns a slice's time only when the wait saw it end, and half the nap otherwise", () => {
    expect(learnedCost(12, 0, true)).toBe(12);
    expect(learnedCost(42, 38, false)).toBe(42);
    // Done before the nap ended: it took at most the nap.
    expect(learnedCost(40, 38, true)).toBe(19);
  });
});
