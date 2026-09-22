import { describe, expect, it, vi } from "vitest";
import { ease } from "../../ui/motion";
import { Animator } from "./animator";

const full = () => new Animator(() => true);
const reduced = () => new Animator(() => false);

describe("Animator", () => {
  it("starts at the from value and lands on the target", () => {
    const a = full();
    a.set("k", 0);
    a.to("k", 1, 0, { duration: 100, easing: (x) => x });
    expect(a.value("k")).toBe(0);
    expect(a.active()).toBe(1);
    a.sample(50);
    expect(a.value("k")).toBeCloseTo(0.5, 5);
    expect(a.animating()).toBe(true);
    a.sample(100);
    expect(a.value("k")).toBe(1);
    expect(a.active()).toBe(0);
    expect(a.animating()).toBe(false);
  });

  it("reports whether anything moved, so the engine can stop rendering", () => {
    const a = full();
    a.set("k", 0);
    a.to("k", 1, 0, { duration: 100, easing: (x) => x });
    expect(a.sample(20)).toBe(true);
    expect(a.sample(200)).toBe(true); // the landing frame
    expect(a.sample(300)).toBe(false); // nothing left to do
  });

  it("retargets mid-flight from the current value instead of queueing", () => {
    const a = full();
    a.set("k", 0);
    a.to("k", 1, 0, { duration: 100, easing: (x) => x });
    a.sample(40);
    const mid = a.value("k");
    expect(mid).toBeCloseTo(0.4, 5);
    // A second call at 40 ms starts from 0.4, it does not restart at 0.
    a.to("k", 0, 40, { duration: 100, easing: (x) => x });
    expect(a.active()).toBe(1);
    expect(a.value("k")).toBeCloseTo(0.4, 5);
    a.sample(90);
    expect(a.value("k")).toBeCloseTo(0.2, 5);
    a.sample(140);
    expect(a.value("k")).toBe(0);
    expect(a.active()).toBe(0);
  });

  it("holds a key through its stagger delay", () => {
    const a = full();
    a.set("k", 0);
    a.to("k", 1, 0, { duration: 100, delay: 60, easing: (x) => x });
    a.sample(30);
    expect(a.value("k")).toBe(0);
    expect(a.active()).toBe(1);
    a.sample(110);
    expect(a.value("k")).toBeCloseTo(0.5, 5);
    a.sample(160);
    expect(a.value("k")).toBe(1);
  });

  it("fires onDone once, when the value lands", () => {
    const a = full();
    const done = vi.fn();
    a.to("k", 1, 0, { duration: 100, onDone: done });
    a.sample(50);
    expect(done).not.toHaveBeenCalled();
    a.sample(100);
    expect(done).toHaveBeenCalledTimes(1);
    a.sample(200);
    expect(done).toHaveBeenCalledTimes(1);
  });

  it("loops without ever landing, and stops when the key is removed", () => {
    const a = full();
    a.set("k", 0.55);
    a.to("k", 0.85, 0, { duration: 800, easing: (x) => x, loop: true });
    a.sample(400);
    expect(a.value("k")).toBeCloseTo(0.7, 5);
    a.sample(800);
    expect(a.value("k")).toBeCloseTo(0.85, 5);
    a.sample(1200); // swinging back
    expect(a.value("k")).toBeCloseTo(0.7, 5);
    a.sample(1600);
    expect(a.value("k")).toBeCloseTo(0.55, 5);
    expect(a.active()).toBe(1); // still the one looping track
    a.remove("k");
    expect(a.active()).toBe(0);
    expect(a.animating()).toBe(false);
  });

  it("finishes every track at once, for a capture", () => {
    const a = full();
    const done = vi.fn();
    a.to("a", 1, 0, { duration: 100, onDone: done });
    a.to("b", 5, 0, { duration: 900 });
    a.to("loop", 0.85, 0, { duration: 800, loop: true });
    a.sample(20);
    expect(a.active()).toBe(3);
    a.finishAll();
    expect(a.value("a")).toBe(1);
    expect(a.value("b")).toBe(5);
    expect(a.value("loop")).toBe(0.85);
    expect(a.active()).toBe(0);
    expect(done).toHaveBeenCalledTimes(1);
  });

  it("removes a key without firing its onDone", () => {
    const a = full();
    const done = vi.fn();
    a.to("k", 1, 0, { duration: 100, onDone: done });
    a.remove("k");
    expect(a.active()).toBe(0);
    expect(a.has("k")).toBe(false);
    expect(done).not.toHaveBeenCalled();
    expect(a.value("k", 42)).toBe(42);
  });

  it("uses the shared easing curves from src/ui/motion", () => {
    const a = full();
    a.set("k", 0);
    a.to("k", 1, 0, { duration: 100, easing: ease.out });
    a.sample(50);
    // --ease-out is front loaded: it is well past halfway at half time.
    expect(a.value("k")).toBeGreaterThan(0.8);
    expect(a.value("k")).toBeLessThan(1);
  });

  describe("reduced motion", () => {
    it("jumps straight to the end state and never animates", () => {
      const a = reduced();
      const done = vi.fn();
      a.set("k", 0);
      a.to("k", 1, 0, { duration: 240, onDone: done });
      expect(a.value("k")).toBe(1);
      expect(a.active()).toBe(0);
      expect(a.animating()).toBe(false);
      expect(done).toHaveBeenCalledTimes(1);
      expect(a.sample(120)).toBe(false);
    });

    it("holds a looping track steady instead of pulsing", () => {
      const a = reduced();
      a.set("k", 0.55);
      a.to("k", 0.85, 0, { duration: 800, loop: true });
      expect(a.value("k")).toBe(0.85);
      expect(a.active()).toBe(0);
      a.sample(400);
      expect(a.value("k")).toBe(0.85);
      a.sample(1600);
      expect(a.value("k")).toBe(0.85);
    });

    it("ignores a stagger delay", () => {
      const a = reduced();
      a.set("k", 0);
      a.to("k", 1, 0, { duration: 180, delay: 240 });
      expect(a.value("k")).toBe(1);
      expect(a.active()).toBe(0);
    });
  });

  it("a zero duration jumps even with motion on", () => {
    const a = full();
    a.set("k", 0);
    a.to("k", 1, 0, { duration: 0 });
    expect(a.value("k")).toBe(1);
    expect(a.active()).toBe(0);
  });

  it("keeps independent keys apart and clears them all", () => {
    const a = full();
    a.set("hl:one", 0);
    a.set("hl:two", 0);
    a.to("hl:one", 1, 0, { duration: 100, easing: (x) => x });
    a.to("hl:two", 1, 0, { duration: 200, easing: (x) => x });
    a.sample(100);
    expect(a.value("hl:one")).toBe(1);
    expect(a.value("hl:two")).toBeCloseTo(0.5, 5);
    expect(a.active()).toBe(1);
    expect(a.keys().sort()).toEqual(["hl:one", "hl:two"]);
    a.clear();
    expect(a.active()).toBe(0);
    expect(a.keys()).toEqual([]);
  });
});

describe("hold", () => {
  it("parks a track without ending it, and finish still lands it", () => {
    const a = new Animator(() => true);
    const done = vi.fn();
    a.set("k", 0);
    a.to("k", 1, 0, { duration: 100, easing: (x) => x, onDone: done });
    a.hold("k", 0.5);
    expect(a.value("k")).toBe(0.5);
    expect(a.active()).toBe(0); // parked: the frame loop can stop
    a.sample(500);
    expect(a.value("k")).toBe(0.5);
    expect(done).not.toHaveBeenCalled();
    a.finish("k");
    expect(a.value("k")).toBe(1);
    expect(done).toHaveBeenCalledTimes(1);
  });
});
