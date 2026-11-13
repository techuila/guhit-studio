import { describe, expect, it } from "vitest";
import type { LiveCamera } from "../../contract/bindings";
import { cameraToPose } from "../geom/coords";
import { followFov, liveCameraOf, panCamera, sameLiveCamera, usableLiveCamera } from "./camera";

const cam = (over: Partial<LiveCamera> = {}): LiveCamera => ({
  position: { x: 1000, y: -2000, z: 1600 },
  target: { x: 4000, y: 3000, z: 1200 },
  fov_deg: 45,
  nav: "orbit",
  ...over,
});

describe("liveCameraOf", () => {
  it("turns three.js meters into plan mm with z up, rounded to 1 mm", () => {
    // World: x east, y up, z south.
    const c = liveCameraOf({ x: 1.0004, y: 1.6, z: 2.0 }, { x: 4, y: 1.2, z: -3.0006 }, 44.999, "walk");
    expect(c).toEqual({ position: { x: 1000, y: -2000, z: 1600 }, target: { x: 4000, y: 3001, z: 1200 }, fov_deg: 45, nav: "walk" });
  });

  it("round trips through the pose the follower flies to", () => {
    const c = cam();
    const pose = cameraToPose(c);
    const back = liveCameraOf(
      { x: pose.position[0], y: pose.position[1], z: pose.position[2] },
      { x: pose.target[0], y: pose.target[1], z: pose.target[2] },
      pose.fovDeg,
      c.nav,
    );
    expect(back).toEqual(c);
  });

  it("never writes a negative zero", () => {
    const c = liveCameraOf({ x: -0.0001, y: 0, z: 0.0001 }, { x: 1, y: 0, z: 0 }, 45, "orbit");
    expect(Object.is(c.position.x, -0)).toBe(false);
    expect(Object.is(c.position.y, -0)).toBe(false);
  });
});

describe("sameLiveCamera", () => {
  it("treats a resting view as unchanged", () => {
    expect(sameLiveCamera(cam(), cam())).toBe(true);
  });

  it("sees a 1 mm move, a fov change and a nav change", () => {
    expect(sameLiveCamera(cam(), cam({ position: { x: 1001, y: -2000, z: 1600 } }))).toBe(false);
    expect(sameLiveCamera(cam(), cam({ target: { x: 4000, y: 3000, z: 1199 } }))).toBe(false);
    expect(sameLiveCamera(cam(), cam({ fov_deg: 46 }))).toBe(false);
    expect(sameLiveCamera(cam(), cam({ nav: "fly" }))).toBe(false);
  });

  it("ignores sub-epsilon jitter", () => {
    expect(sameLiveCamera(cam(), cam({ position: { x: 1000.3, y: -2000, z: 1600 } }))).toBe(true);
  });

  it("has nothing to compare against at first", () => {
    expect(sameLiveCamera(null, cam())).toBe(false);
  });
});

describe("usableLiveCamera", () => {
  it("refuses non-finite values and an eye on its own target", () => {
    expect(usableLiveCamera(cam())).toBe(true);
    expect(usableLiveCamera(cam({ fov_deg: Number.NaN }))).toBe(false);
    expect(usableLiveCamera(cam({ position: { x: Number.POSITIVE_INFINITY, y: 0, z: 0 } }))).toBe(false);
    expect(usableLiveCamera(cam({ target: { x: 1000, y: -2000, z: 1600 } }))).toBe(false);
    expect(usableLiveCamera(null)).toBe(false);
  });

  it("clamps the field of view to the contract's range", () => {
    expect(followFov(2)).toBe(5);
    expect(followFov(200)).toBe(110);
    expect(followFov(Number.NaN)).toBe(45);
    expect(followFov(60)).toBe(60);
  });
});

describe("panCamera", () => {
  it("follows an agent's point with this camera's direction and distance (DECISIONS D47)", () => {
    const from = cam({ nav: "walk" });
    const next = panCamera(from, { x: 6000, y: 1000, z: 900.4 });
    expect(next.target).toEqual({ x: 6000, y: 1000, z: 900 });
    expect(next.position).toEqual({ x: 3000, y: -4000, z: 1300 });
    expect(next.fov_deg).toBe(45);
    expect(next.nav).toBe("orbit");
    expect(usableLiveCamera(next)).toBe(true);
  });
});
