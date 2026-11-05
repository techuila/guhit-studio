import { describe, expect, it } from "vitest";
import { cleanSeconds, clock, draggedSeconds, withShot, type TourEl } from "./tourActions";

describe("tour timeline", () => {
  it("a drag tracks the pointer 1:1 at the scale it started with", () => {
    // 20 px a second: 60 px right is 3 s longer, 30 px left 1.5 s shorter.
    expect(draggedSeconds(4, 60, 20)).toBe(7);
    expect(draggedSeconds(4, -30, 20)).toBe(2.5);
    // Never under 0.1 s nor over 600 s.
    expect(draggedSeconds(1, -400, 20)).toBe(0.1);
    expect(draggedSeconds(590, 1000, 20)).toBe(600);
  });

  it("a drop keeps tenths of a second", () => {
    expect(cleanSeconds(2.34)).toBe(2.3);
    expect(cleanSeconds(0.01)).toBe(0.1);
    expect(cleanSeconds(Number.NaN)).toBe(0.1);
    expect(cleanSeconds(900)).toBe(600);
  });

  it("reads times like a video editor", () => {
    expect(clock(65)).toBe("1:05");
    expect(clock(8.46, true)).toBe("0:08.5");
  });

  it("edits one shot and keeps the rest", () => {
    const shot = (id: string, seconds: number) => ({
      id,
      name: id,
      kind: "hold" as const,
      keys: [],
      seconds,
      ease: "linear" as const,
      light: null,
      light_end: null,
      transition: { kind: "cut" as const, seconds: 0 },
    });
    const t: TourEl = { kind: "tour", id: "t", name: "T", fps: 30, shots: [shot("a", 1), shot("b", 2)] };
    const next = withShot(t, 1, { seconds: 5 });
    expect(next.shots.map((s) => s.seconds)).toEqual([1, 5]);
    expect(t.shots[1].seconds).toBe(2);
  });
});
