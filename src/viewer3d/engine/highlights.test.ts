import * as THREE from "three";
import { describe, expect, it } from "vitest";
import { Highlight, SELECT_COLOR } from "./highlights";

function mesh(): THREE.Mesh {
  return new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshStandardMaterial({ color: 0xffffff }));
}

const red = new THREE.Color("#c92a2a");
const hull = (m: THREE.Mesh) => (m.children[0] as THREE.Mesh).material as THREE.ShaderMaterial;

describe("Highlight tint", () => {
  it("tints in the participant's color, not the selection's", () => {
    const m = mesh();
    const hl = new Highlight("w1", "tint", [m], red);
    hl.apply(1);
    const c = (m.material as THREE.MeshStandardMaterial).color;
    // Halfway from white to the peer red: green and blue drop well below red.
    expect(c.r).toBeGreaterThan(c.g + 0.2);
    expect(c.r).toBeGreaterThan(c.b + 0.2);
    hl.release();
    expect((m.material as THREE.MeshStandardMaterial).color.getHex()).toBe(0xffffff);
  });

  it("starts from the plain look and keeps the base material untouched", () => {
    const m = mesh();
    const base = m.material as THREE.MeshStandardMaterial;
    const hl = new Highlight("w1", "tint", [m], red);
    hl.apply(0);
    expect((m.material as THREE.MeshStandardMaterial).color.getHex()).toBe(0xffffff);
    hl.apply(1);
    expect(base.color.getHex()).toBe(0xffffff);
    hl.release();
    expect(m.material).toBe(base);
  });

  it("recolors its outline when it changes hands", () => {
    const m = mesh();
    const hl = new Highlight("w1", "tint", [m], red);
    hl.addHullOutline(m, new THREE.Vector2(800, 600));
    expect(hull(m).uniforms.color.value.equals(red)).toBe(true);
    hl.retarget("selected");
    expect(hull(m).uniforms.color.value.equals(SELECT_COLOR)).toBe(true);
    hl.retarget("tint", new THREE.Color("#1864ab"));
    expect(hull(m).uniforms.color.value.getHexString()).toBe(new THREE.Color("#1864ab").getHexString());
    hl.release();
    expect(m.children.length).toBe(0);
  });

  it("a plain selection still uses the selection color", () => {
    const m = mesh();
    const hl = new Highlight("w1", "selected", [m]);
    hl.apply(1);
    const c = (m.material as THREE.MeshStandardMaterial).color;
    // Teal: green and blue above red.
    expect(c.g).toBeGreaterThan(c.r);
    expect(c.b).toBeGreaterThan(c.r);
    hl.release();
  });
});
