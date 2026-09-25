import * as THREE from "three";
import { describe, expect, it } from "vitest";
import { floatGeometry, needsFloat } from "./renderScene";

/** A triangle stored the way the pack's meshopt models are: normalized Int16 positions and Int8 normals, interleaved. */
function quantized(): THREE.BufferGeometry {
  const g = new THREE.BufferGeometry();
  const pos = new THREE.InterleavedBuffer(new Int16Array([32767, 0, 0, 0, 0, 0, 32767, 0, 0, 16384, 0, 0]), 4);
  g.setAttribute("position", new THREE.InterleavedBufferAttribute(pos, 3, 0, true));
  const nor = new THREE.InterleavedBuffer(new Int8Array([0, 127, 0, 0, 0, 127, 0, 0, 0, 127, 0, 0]), 4);
  g.setAttribute("normal", new THREE.InterleavedBufferAttribute(nor, 3, 0, true));
  g.setIndex([0, 1, 2]);
  return g;
}

describe("float copies for the path tracer", () => {
  it("spots quantized, normalized or interleaved geometry and leaves plain floats alone", () => {
    expect(needsFloat(quantized())).toBe(true);
    expect(needsFloat(new THREE.BoxGeometry(1, 1, 1))).toBe(false);
  });

  it("scales quantized values back to what the raster view draws", () => {
    const src = quantized();
    const out = floatGeometry(src);
    expect(needsFloat(out)).toBe(false);
    const p = out.getAttribute("position");
    expect(p.array).toBeInstanceOf(Float32Array);
    expect(p.getX(0)).toBeCloseTo(1, 4);
    expect(p.getZ(1)).toBeCloseTo(1, 4);
    expect(p.getY(2)).toBeCloseTo(0.5, 3);
    expect(out.getAttribute("normal").getY(0)).toBeCloseTo(1, 4);
    expect(Array.from(out.getIndex()!.array)).toEqual([0, 1, 2]);
    // The source, shared with the live view, is untouched.
    expect(src.getAttribute("position").array).toBeInstanceOf(Int16Array);
  });
});
