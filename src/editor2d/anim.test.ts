import { afterEach, describe, expect, it, vi } from "vitest";
import { Anim, breathe, mix, mixP, sampleTrack, trackDone } from "./anim";
import { ease } from "../ui/motion";

const linear = (x: number): number => x;

describe("track sampling", () => {
  const t = { from: 0, to: 100, start: 1000, duration: 200, easing: linear, drop: false };

  it("holds the start value before it begins and the end value after", () => {
    expect(sampleTrack(t, 900)).toBe(0);
    expect(sampleTrack(t, 1000)).toBe(0);
    expect(sampleTrack(t, 1100)).toBe(50);
    expect(sampleTrack(t, 1200)).toBe(100);
    expect(sampleTrack(t, 9000)).toBe(100);
  });

  it("is done only once the duration has passed", () => {
    expect(trackDone(t, 1199)).toBe(false);
    expect(trackDone(t, 1200)).toBe(true);
  });

  it("a zero duration is already at the target", () => {
    const z = { ...t, duration: 0 };
    expect(sampleTrack(z, 1000)).toBe(100);
    expect(trackDone(z, 1000)).toBe(true);
  });
});

describe("Anim", () => {
  it("falls back to the rest value for a key it does not know", () => {
    const a = new Anim();
    expect(a.value("nothing", 0.5)).toBe(0.5);
    expect(a.running()).toBe(0);
  });

  it("runs a fade and reports it until it lands", () => {
    const a = new Anim();
    a.tick(0);
    a.to("hov:w1", 1, 120, { from: 0 });
    expect(a.value("hov:w1")).toBe(0);
    expect(a.tick(60)).toBe(1);
    expect(a.value("hov:w1")).toBeCloseTo(0.5);
    expect(a.tick(120)).toBe(0);
    expect(a.value("hov:w1")).toBe(1);
  });

  it("retargets mid flight from the value it is at, not from the start", () => {
    const a = new Anim();
    a.tick(0);
    a.to("x", 1, 100, { from: 0 });
    a.tick(50);
    expect(a.value("x")).toBeCloseTo(0.5);
    // Reverse half way: it must leave from 0.5, never jump back to 1 or 0.
    a.to("x", 0, 100, { drop: true });
    expect(a.value("x")).toBeCloseTo(0.5);
    a.tick(75);
    expect(a.value("x")).toBeCloseTo(0.375);
    a.tick(150);
    expect(a.value("x")).toBe(0);
  });

  it("does not restart a track that is already heading to the same target", () => {
    const a = new Anim();
    a.tick(0);
    a.to("x", 1, 100, { from: 0 });
    a.tick(50);
    a.to("x", 1, 100, { from: 0 });
    expect(a.value("x")).toBeCloseTo(0.5);
    a.tick(100);
    expect(a.value("x")).toBe(1);
  });

  it("keeps a resting track when asked to move somewhere it already is, so a later flip animates", () => {
    const a = new Anim();
    a.tick(0);
    // The door ghost appears already swinging right.
    a.to("ghost.swing", 1, 120, { from: 1 });
    expect(a.value("ghost.swing", 1)).toBe(1);
    expect(a.running()).toBe(0);
    // F flips it: this must sweep through the wall plane, not jump.
    a.to("ghost.swing", -1, 120);
    a.tick(60);
    expect(a.value("ghost.swing")).toBeCloseTo(0);
    a.tick(120);
    expect(a.value("ghost.swing")).toBe(-1);
  });

  it("forgets dropped tracks once they finish so the key rests again", () => {
    const a = new Anim();
    a.tick(0);
    a.to("rm:w1", 0, 100, { from: 1, drop: true });
    a.tick(50);
    expect(a.has("rm:w1")).toBe(true);
    a.tick(100);
    expect(a.has("rm:w1")).toBe(false);
    expect(a.value("rm:w1", 0)).toBe(0);
  });

  it("keeps tracks that were not marked droppable", () => {
    const a = new Anim();
    a.tick(0);
    a.to("sel:w1", 1, 100, { from: 0 });
    a.tick(200);
    expect(a.has("sel:w1")).toBe(true);
    expect(a.value("sel:w1")).toBe(1);
  });

  it("staggers with a delay", () => {
    const a = new Anim();
    a.tick(0);
    a.to("grip:0", 1, 100, { from: 0 });
    a.to("grip:1", 1, 100, { from: 0, delayMs: 30 });
    a.tick(30);
    expect(a.value("grip:0")).toBeCloseTo(0.3);
    expect(a.value("grip:1")).toBe(0);
    a.tick(100);
    expect(a.value("grip:0")).toBe(1);
    expect(a.value("grip:1")).toBeCloseTo(0.7);
    expect(a.running()).toBe(1);
    a.tick(130);
    expect(a.running()).toBe(0);
  });

  it("reports nothing running once everything has landed, so the loop can stop", () => {
    const a = new Anim();
    a.tick(0);
    a.to("a", 1, 80, { from: 0 });
    a.to("b", 0, 180, { from: 1, drop: true });
    expect(a.tick(10)).toBe(2);
    expect(a.tick(100)).toBe(1);
    expect(a.tick(180)).toBe(0);
  });

  it("goes straight to the end state with a zero duration (reduced motion)", () => {
    const a = new Anim();
    a.tick(0);
    a.to("hov:w1", 1, 0, { from: 0 });
    a.to("rm:w1", 0, 0, { from: 1, drop: true });
    expect(a.value("hov:w1")).toBe(1);
    expect(a.value("rm:w1", 0)).toBe(0);
    expect(a.running()).toBe(0);
    a.tick(1);
    expect(a.value("hov:w1")).toBe(1);
    expect(a.has("rm:w1")).toBe(false);
  });

  it("clears by key, by prefix and wholesale", () => {
    const a = new Anim();
    a.tick(0);
    a.to("hov:a", 1, 100, { from: 0 });
    a.to("hov:b", 1, 100, { from: 0 });
    a.to("sel:a", 1, 100, { from: 0 });
    a.clear("hov:a");
    expect(a.size()).toBe(2);
    a.clearPrefix("hov:");
    expect(a.size()).toBe(1);
    a.clearAll();
    expect(a.size()).toBe(0);
  });

  it("visits keys by prefix with their sampled values", () => {
    const a = new Anim();
    a.tick(0);
    a.to("fl:w1", 0, 100, { from: 1, drop: true });
    a.to("fl:w2", 0, 100, { from: 1, drop: true });
    a.to("sel:w3", 1, 100, { from: 0 });
    a.tick(50);
    const seen: Record<string, number> = {};
    a.each("fl:", (id, v) => {
      seen[id] = v;
    });
    expect(Object.keys(seen).sort()).toEqual(["w1", "w2"]);
    expect(seen.w1).toBeCloseTo(0.5);
  });

  it("never moves the clock backwards", () => {
    const a = new Anim();
    a.tick(500);
    a.tick(100);
    expect(a.now).toBe(500);
  });

  it("uses the easing it was given", () => {
    const a = new Anim();
    a.tick(0);
    a.to("x", 1, 100, { from: 0, easing: ease.out });
    a.tick(50);
    // ease-out is well past half way at the mid point.
    expect(a.value("x")).toBeGreaterThan(0.7);
  });
});

describe("breathe", () => {
  it("stays inside its range and returns to the middle each period", () => {
    for (let t = 0; t < 3200; t += 37) {
      const v = breathe(t, 1600, 0.55, 0.85);
      expect(v).toBeGreaterThanOrEqual(0.55 - 1e-9);
      expect(v).toBeLessThanOrEqual(0.85 + 1e-9);
    }
    expect(breathe(0, 1600, 0.55, 0.85)).toBeCloseTo(0.7);
    expect(breathe(400, 1600, 0.55, 0.85)).toBeCloseTo(0.85);
    expect(breathe(1200, 1600, 0.55, 0.85)).toBeCloseTo(0.55);
  });

  it("is a steady tint when motion is off", () => {
    expect(breathe(0, 1600, 0.55, 0.85, false)).toBe(0.85);
    expect(breathe(900, 1600, 0.55, 0.85, false)).toBe(0.85);
  });
});

describe("mix", () => {
  it("interpolates numbers and points", () => {
    expect(mix(10, 20, 0.25)).toBe(12.5);
    expect(mixP({ x: 0, y: 10 }, { x: 100, y: 0 }, 0.5)).toEqual({ x: 50, y: 5 });
  });
});

describe("reduced motion", () => {
  afterEach(() => {
    vi.resetModules();
    vi.unstubAllGlobals();
  });

  it("dur() is 0 and every animation lands on its end state at once", async () => {
    const matchMedia = (q: string) => ({ matches: q.includes("reduce"), media: q, addEventListener() {}, removeEventListener() {} });
    vi.stubGlobal("window", { matchMedia });
    vi.stubGlobal("matchMedia", matchMedia);
    vi.resetModules();
    const { dur, motionOK } = await import("../ui/motion");
    expect(motionOK()).toBe(false);
    expect(dur("base")).toBe(0);

    const a = new Anim();
    a.tick(0);
    a.to("add:w1", 0, dur("base"), { from: 1, drop: true });
    a.to("sel:w1", 1, dur("hover"), { from: 0 });
    expect(a.value("add:w1", 0)).toBe(0);
    expect(a.value("sel:w1", 1)).toBe(1);
    expect(a.running()).toBe(0);
  });
});
