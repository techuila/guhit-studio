import * as THREE from "three";
import { describe, expect, it } from "vitest";
import { onView, ProjectionWatch, samePoint, screenPoint } from "./project";

/** A camera at plan (0, -10 m), 1.6 m up, looking north at the same height. */
function camera(aspect = 2): THREE.PerspectiveCamera {
  const c = new THREE.PerspectiveCamera(60, aspect, 0.1, 1000);
  c.position.set(0, 1.6, 10);
  c.lookAt(0, 1.6, 0);
  c.updateMatrixWorld();
  c.updateProjectionMatrix();
  return c;
}

describe("screenPoint", () => {
  it("puts the point looked at in the middle of the canvas", () => {
    const s = screenPoint(camera(), { x: 0, y: 0, z: 1600 }, 800, 400);
    expect(s.inFront).toBe(true);
    expect(s.x).toBeCloseTo(400, 6);
    expect(s.y).toBeCloseTo(200, 6);
  });

  it("puts east to the right and higher up nearer the top", () => {
    const s = screenPoint(camera(), { x: 1000, y: 0, z: 2600 }, 800, 400);
    expect(s.x).toBeGreaterThan(400);
    expect(s.y).toBeLessThan(200);
  });

  it("matches three's own projection", () => {
    const c = camera();
    const s = screenPoint(c, { x: 2500, y: 3000, z: 400 }, 800, 400);
    const ndc = new THREE.Vector3(2.5, 0.4, -3).project(c);
    expect(s.x).toBeCloseTo(((ndc.x + 1) / 2) * 800, 6);
    expect(s.y).toBeCloseTo(((1 - ndc.y) / 2) * 400, 6);
  });

  it("flags a point behind the eyes", () => {
    // Plan y -20 m is south of the camera, which looks north.
    const s = screenPoint(camera(), { x: 0, y: -20000, z: 1600 }, 800, 400);
    expect(s.inFront).toBe(false);
    expect(onView(s, 800, 400, 1000)).toBe(false);
  });
});

describe("onView", () => {
  it("keeps points on the canvas and a margin around it", () => {
    expect(onView({ x: 10, y: 10, inFront: true }, 100, 100)).toBe(true);
    expect(onView({ x: -5, y: 10, inFront: true }, 100, 100)).toBe(false);
    expect(onView({ x: -5, y: 10, inFront: true }, 100, 100, 8)).toBe(true);
    expect(onView({ x: 110, y: 50, inFront: true }, 100, 100, 8)).toBe(false);
  });
});

describe("ProjectionWatch", () => {
  it("fires once for a change and stays quiet at rest", () => {
    const c = camera();
    const w = new ProjectionWatch();
    expect(w.changed(c, 800, 400)).toBe(true);
    expect(w.changed(c, 800, 400)).toBe(false);
    c.position.x += 0.001;
    c.updateMatrixWorld();
    expect(w.changed(c, 800, 400)).toBe(true);
    expect(w.changed(c, 800, 400)).toBe(false);
  });

  it("sees a resize and a new field of view", () => {
    const c = camera();
    const w = new ProjectionWatch();
    w.changed(c, 800, 400);
    expect(w.changed(c, 801, 400)).toBe(true);
    c.fov = 50;
    c.updateProjectionMatrix();
    expect(w.changed(c, 801, 400)).toBe(true);
  });
});

describe("samePoint", () => {
  it("tells a moved pointer from a presence update that left it where it was", () => {
    expect(samePoint({ x: 1, y: 2, z: 3 }, { x: 1, y: 2, z: 3 })).toBe(true);
    expect(samePoint({ x: 1, y: 2, z: 3 }, { x: 1, y: 2, z: 4 })).toBe(false);
    expect(samePoint(null, { x: 1, y: 2, z: 3 })).toBe(false);
    expect(samePoint(null, null)).toBe(true);
  });
});
