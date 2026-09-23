import { describe, expect, it } from "vitest";
import { MaterialLibrary } from "../scene/materials";
import { applyLook, mixLook, SHELL_LOOKS, ShellView } from "./shell";

function material(category: MaterialLibrary["category"], id = "mat-chb-painted") {
  const lib = new MaterialLibrary();
  lib.category = category;
  return { lib, mat: lib.get(id, undefined, false) };
}

describe("applyLook", () => {
  it("fades an opaque wall to a faint, see-through material without depth writes", () => {
    const { lib, mat } = material("wall");
    const version = mat.version;
    applyLook(mat, SHELL_LOOKS.xray);
    expect(mat.transparent).toBe(true);
    expect(mat.depthWrite).toBe(false);
    expect(mat.opacity).toBeCloseTo(SHELL_LOOKS.xray.cat.wall, 6);
    expect(mat.visible).toBe(true);
    // The shader only changes on the flip to see-through.
    expect(mat.version).toBe(version + 1);
    applyLook(mat, mixLook(SHELL_LOOKS.xray, SHELL_LOOKS.hidden, 0.5));
    expect(mat.version).toBe(version + 1);
    lib.dispose();
  });

  it("hides the wall in hidden mode and keeps the floor as a ghost", () => {
    const { lib, mat } = material("wall");
    // Disappearing is what the sun sees: reported once, not on every frame.
    expect(applyLook(mat, SHELL_LOOKS.hidden)).toBe(true);
    expect(applyLook(mat, SHELL_LOOKS.hidden)).toBe(false);
    expect(mat.visible).toBe(false);
    const floor = material("floor", "mat-floor-concrete");
    applyLook(floor.mat, SHELL_LOOKS.hidden);
    expect(floor.mat.visible).toBe(true);
    expect(floor.mat.opacity).toBeCloseTo(SHELL_LOOKS.hidden.cat.floor, 6);
    lib.dispose();
    floor.lib.dispose();
  });

  it("puts the solid look back exactly", () => {
    const { lib, mat } = material("wall");
    applyLook(mat, SHELL_LOOKS.xray);
    applyLook(mat, SHELL_LOOKS.solid);
    expect(mat.transparent).toBe(false);
    expect(mat.depthWrite).toBe(true);
    expect(mat.opacity).toBe(1);
    expect(mat.visible).toBe(true);
    lib.dispose();
  });

  it("keeps glass as glass and scales its own opacity", () => {
    const { lib, mat } = material("opening", "mat-glass-clear");
    applyLook(mat, SHELL_LOOKS.solid);
    expect(mat.transparent).toBe(true);
    expect(mat.opacity).toBeCloseTo(0.35, 6);
    applyLook(mat, SHELL_LOOKS.xray);
    expect(mat.opacity).toBeCloseTo(0.35 * SHELL_LOOKS.xray.cat.opening, 6);
    lib.dispose();
  });

  it("never touches pipes", () => {
    const lib = new MaterialLibrary();
    lib.category = "pipe";
    const mat = lib.pipe("#2b7bd0");
    applyLook(mat, SHELL_LOOKS.hidden);
    expect(mat.visible).toBe(true);
    expect(mat.transparent).toBe(false);
    expect(mat.opacity).toBe(1);
    lib.dispose();
  });
});

describe("ShellView", () => {
  it("retargets from wherever the look is mid-change", () => {
    const view = new ShellView();
    view.begin("xray");
    view.sample(0.5);
    const mid = view.look.cat.wall;
    expect(mid).toBeGreaterThan(SHELL_LOOKS.xray.cat.wall);
    expect(mid).toBeLessThan(1);
    view.begin("solid");
    view.sample(0);
    expect(view.look.cat.wall).toBeCloseTo(mid, 6);
    view.sample(1);
    expect(view.look).toEqual(SHELL_LOOKS.solid);
    view.dispose();
  });
});
