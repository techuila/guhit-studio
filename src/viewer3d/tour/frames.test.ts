import { describe, expect, it } from "vitest";
import { doorsAt, frameIndexAt, fxLayers, heldFrames, poseAt, smoothstep, type TourFrame, type TourFrames } from "./frames";

const at = (x: number) => ({ x, y: 0, z: 1600 });

function frames(list: Array<Partial<TourFrame> & { shot: number; x: number }>): TourFrames {
  return {
    tour_id: "t",
    name: "T",
    fps: 10,
    seconds: list.length / 10,
    frame_count: list.length,
    shots: [],
    frames: list.map((f, i) => ({ t: i / 10, shot: f.shot, position: at(f.x), target: at(f.x + 3000), fov_deg: 60, fx: f.fx })),
  };
}

describe("tour frames", () => {
  it("blends between two frames of one shot, never across a cut", () => {
    const data = frames([
      { shot: 0, x: 0 },
      { shot: 0, x: 100 },
      { shot: 1, x: 5000 },
    ]);
    expect(poseAt(data, 0.05).position.x).toBeCloseTo(50);
    // Between the last frame of shot 0 and the first of shot 1: a jump stays a jump.
    expect(poseAt(data, 0.15).position.x).toBe(100);
    expect(poseAt(data, 99).position.x).toBe(5000);
    expect(frameIndexAt(data, 0.1)).toBe(1);
    expect(frameIndexAt(data, -1)).toBe(0);
    expect(frameIndexAt(data, 99)).toBe(2);
  });

  it("lays transitions over the picture the same way everywhere", () => {
    expect(fxLayers(undefined)).toEqual({ held: 0, color: null, colorAlpha: 0, plan: 0, blur: 0 });
    // A dissolve fades the held frame out.
    expect(fxLayers({ kind: "dissolve", mix: 0.25, from_frame: 3 }).held).toBeCloseTo(0.75);
    // From black at the very start.
    expect(fxLayers({ kind: "dissolve", mix: 0.25 })).toMatchObject({ held: 0, color: "black", colorAlpha: 0.75 });
    // A dip: the held frame under a rising color, then the color fading off this frame.
    expect(fxLayers({ kind: "dip_white", mix: 0.25, from_frame: 3 })).toMatchObject({ held: 1, color: "white", colorAlpha: 0.5 });
    expect(fxLayers({ kind: "dip_black", mix: 0.75, from_frame: 3 })).toMatchObject({ held: 0, color: "black", colorAlpha: 0.5 });
    expect(fxLayers({ kind: "whip_pan", mix: 0.5, blur: 1 }).blur).toBe(1);
    expect(fxLayers({ kind: "plan_lift", mix: 0 }).plan).toBe(1);
    expect(fxLayers({ kind: "plan_lift", mix: 0.6 }).plan).toBe(0);
  });

  it("swings the doors a tour opens, eased, and holds them open (GA P1-9)", () => {
    const data = { doors: [{ id: "d1", from: 2, to: 3, open: 1 }] };
    // Before the swing the leaf rests as drawn: no entry.
    expect(doorsAt(data, 1).has("d1")).toBe(false);
    // Halfway through, half way from as drawn (null) to open.
    expect(doorsAt(data, 2.5).get("d1")).toEqual({ from: null, to: 1, k: 0.5 });
    expect(doorsAt(data, 2.25).get("d1")!.k).toBeCloseTo(smoothstep(0.25));
    // Open from then on.
    expect(doorsAt(data, 9).get("d1")).toEqual({ from: 1, to: 1, k: 1 });
    // A second move starts from where the first left it.
    const twice = { doors: [{ id: "d1", from: 5, to: 6, open: 0 }, { id: "d1", from: 2, to: 3, open: 1 }] };
    expect(doorsAt(twice, 5.5).get("d1")).toEqual({ from: 1, to: 0, k: 0.5 });
    expect(doorsAt({}, 4).size).toBe(0);
  });

  it("knows which frames a later transition holds", () => {
    const data = frames([
      { shot: 0, x: 0 },
      { shot: 0, x: 1 },
      { shot: 1, x: 9, fx: { kind: "dissolve", mix: 0, from_frame: 1 } },
      { shot: 1, x: 9, fx: { kind: "dissolve", mix: 0.5, from_frame: 1 } },
    ]);
    expect([...heldFrames(data)]).toEqual([1]);
  });
});
