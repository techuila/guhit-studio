import { describe, expect, it } from "vitest";
import {
  filterPlan,
  halvesVarianceFactor,
  KERNEL,
  kernelWeight,
  luminanceWeight,
  normalWeight,
  PLANE_SHARE,
  planeWeight,
  SIGMA_L,
} from "./denoise";
import { albedoJitter, tent } from "./gbuffer";

describe("a-trous kernel", () => {
  it("is the 5 x 5 B3 spline: symmetric, 9/64 in the middle, summing to 1", () => {
    let sum = 0;
    for (let y = -2; y <= 2; y++) for (let x = -2; x <= 2; x++) sum += kernelWeight(x, y);
    expect(sum).toBeCloseTo(1, 12);
    expect(kernelWeight(0, 0)).toBeCloseTo(9 / 64, 12);
    expect(kernelWeight(2, -1)).toBe(kernelWeight(-2, 1));
    expect(kernelWeight(3, 0)).toBe(0);
    expect(KERNEL[0] + 2 * KERNEL[1] + 2 * KERNEL[2]).toBeCloseTo(1, 12);
  });
});

describe("edge-stopping weights", () => {
  const up: [number, number, number] = [0, 1, 0];

  it("normal: 1 for the same surface, nothing at a corner, little for a gentle bend", () => {
    expect(normalWeight(up, up)).toBe(1);
    expect(normalWeight(up, [1, 0, 0])).toBe(0);
    expect(normalWeight(up, [0, -1, 0])).toBe(0);
    const tilt = (deg: number): [number, number, number] => [Math.sin((deg * Math.PI) / 180), Math.cos((deg * Math.PI) / 180), 0];
    expect(normalWeight(up, tilt(5))).toBeGreaterThan(0.5);
    expect(normalWeight(up, tilt(20))).toBeLessThan(0.05);
    expect(normalWeight(up, tilt(45))).toBeLessThan(1e-6);
  });

  it("plane: 1 on the pixel's plane however far along it, falling off with the distance from it", () => {
    const p: [number, number, number] = [0, -1.5, -3];
    expect(planeWeight(up, p, [2, -1.5, -7], 3)).toBeCloseTo(1, 12);
    // A seat 1 cm above the floor still mixes, a table top 70 cm above does not.
    expect(planeWeight(up, p, [0.1, -1.49, -3], 3)).toBeGreaterThan(0.7);
    expect(planeWeight(up, p, [0.1, -0.8, -3], 3)).toBeLessThan(1e-6);
  });

  it("plane: the tolerance grows with depth", () => {
    const near = planeWeight(up, [0, 0, -2], [0, 0.05, -2], 2);
    const far = planeWeight(up, [0, 0, -20], [0, 0.05, -20], 20);
    expect(far).toBeGreaterThan(near);
    expect(planeWeight(up, [0, 0, -2], [0, 0.05, -2], 2)).toBeCloseTo(Math.exp(-0.05 / (PLANE_SHARE * 2)), 12);
  });

  it("light: 1 for equal light, less the more they differ, wider for a noisier pixel", () => {
    expect(luminanceWeight(1, 1, 0.01)).toBe(1);
    expect(luminanceWeight(1, 1.2, 0.01)).toBeLessThan(luminanceWeight(1, 1.1, 0.01));
    expect(luminanceWeight(1, 1.2, 0.04)).toBeGreaterThan(luminanceWeight(1, 1.2, 0.01));
    // One standard deviation apart weighs exp(-1 / sigma).
    expect(luminanceWeight(1, 1.1, 0.01)).toBeCloseTo(Math.exp(-0.1 / (SIGMA_L * 0.1 + 1e-6)), 9);
    // A converged pixel (no noise) takes only neighbours with its own light.
    expect(luminanceWeight(1, 1.001, 0)).toBeLessThan(1e-6);
  });
});

describe("filter strength", () => {
  it("reaches wide at a Quick render's sample counts, a pass further at 4K", () => {
    expect(filterPlan(40, 1080).passes).toBe(5);
    expect(filterPlan(384, 1080).passes).toBe(5);
    expect(filterPlan(40, 2160).passes).toBe(6);
  });

  it("reaches less far as the samples add up, and never below one pass", () => {
    let last = Infinity;
    for (const n of [1, 16, 64, 256, 512, 1024, 2048, 4096, 1e6]) {
      const { passes } = filterPlan(n, 1080);
      expect(passes).toBeLessThanOrEqual(last);
      expect(passes).toBeGreaterThanOrEqual(1);
      last = passes;
    }
    expect(filterPlan(2400, 1080).passes).toBeLessThan(filterPlan(40, 1080).passes);
  });

  it("turns two half averages into the variance of their mean", () => {
    expect(halvesVarianceFactor(10, 10)).toBeCloseTo(0.25, 12);
    expect(halvesVarianceFactor(0, 10)).toBe(0);
    // Unbiased: the mean over many pixels of (A - B)^2 * k is the variance of the average.
    // mulberry32: a small seeded generator, so the test is the same every run.
    let seed = 7;
    const rand = () => {
      seed = (seed + 0x6d2b79f5) | 0;
      let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
    const [nA, nB] = [9, 8];
    const k = halvesVarianceFactor(nA, nB);
    let est = 0;
    const means: number[] = [];
    const pixels = 20000;
    for (let p = 0; p < pixels; p++) {
      let a = 0;
      let b = 0;
      for (let i = 0; i < nA; i++) a += rand();
      for (let i = 0; i < nB; i++) b += rand();
      means.push((a + b) / (nA + nB));
      a /= nA;
      b /= nB;
      est += (a - b) * (a - b) * k;
    }
    const mu = means.reduce((s, v) => s + v, 0) / pixels;
    const truth = means.reduce((s, v) => s + (v - mu) * (v - mu), 0) / pixels;
    expect(est / pixels / truth).toBeGreaterThan(0.95);
    expect(est / pixels / truth).toBeLessThan(1.05);
  });
});

describe("albedo jitter", () => {
  it("spreads like the tracer's tent filter: within a pixel each way, centred, variance 1/6", () => {
    expect(tent(0)).toBeCloseTo(-1, 12);
    expect(tent(0.5)).toBeCloseTo(0, 12);
    expect(tent(0.999999)).toBeCloseTo(1, 2);
    let sum = 0;
    let sq = 0;
    const n = 4096;
    for (let i = 0; i < n; i++) {
      const [x, y] = albedoJitter(i);
      expect(Math.abs(x)).toBeLessThanOrEqual(1);
      expect(Math.abs(y)).toBeLessThanOrEqual(1);
      sum += x;
      sq += x * x;
    }
    expect(sum / n).toBeCloseTo(0, 2);
    expect(sq / n).toBeCloseTo(1 / 6, 2);
  });
});
