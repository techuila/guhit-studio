// The rail flyout rule (orchestrator ruling R16, D44) and the remembered style
// the status bar shows.
import { describe, expect, it } from "vitest";
import type { CatalogItem } from "../contract/bindings";
import { PIPE_MATERIAL_LABEL } from "../contract/pipes";
import type { ToolOptions } from "../state/store";
import { STAYS_OPEN, railClick, rememberedStyle } from "./railFlyout";

const wall = { tool: "wall" as const, flyout: "wall" as const };
const door = { tool: "door" as const, flyout: "door" as const };
const asset = { tool: "asset" as const, flyout: "asset" as const };
const select = { tool: "select" as const };

describe("railClick", () => {
  it("the first click activates the tool and opens nothing", () => {
    expect(railClick(wall, { tool: "select", open: null, assetKey: null })).toEqual({ activate: true, flyout: null });
    expect(railClick(door, { tool: "wall", open: null, assetKey: null })).toEqual({ activate: true, flyout: null });
  });

  it("a click on the active tool opens its options, and a second closes them", () => {
    expect(railClick(door, { tool: "door", open: null, assetKey: null })).toEqual({ activate: false, flyout: "door" });
    expect(railClick(door, { tool: "door", open: "door", assetKey: null })).toEqual({ activate: false, flyout: null });
  });

  it("the corner mark opens the options at once, activating the tool", () => {
    expect(railClick(door, { tool: "select", open: null, assetKey: null }, true)).toEqual({ activate: true, flyout: "door" });
  });

  it("a tool's first click closes another tool's open flyout", () => {
    expect(railClick(door, { tool: "wall", open: "wall", assetKey: null })).toEqual({ activate: true, flyout: null });
  });

  it("the object tool opens its library until an object is picked", () => {
    expect(railClick(asset, { tool: "select", open: null, assetKey: null })).toEqual({ activate: false, flyout: "asset" });
    expect(railClick(asset, { tool: "select", open: "asset", assetKey: null })).toEqual({ activate: false, flyout: null });
    expect(railClick(asset, { tool: "select", open: null, assetKey: "sofa" })).toEqual({ activate: true, flyout: null });
  });

  it("a tool without options just activates", () => {
    expect(railClick(select, { tool: "wall", open: "wall", assetKey: null })).toEqual({ activate: true, flyout: null });
  });

  it("only the wall flyout stays open after a pick", () => {
    expect(STAYS_OPEN.has("wall")).toBe(true);
    expect(STAYS_OPEN.has("door")).toBe(false);
  });
});

describe("rememberedStyle", () => {
  const options: ToolOptions = {
    wallThicknessMm: 150,
    wallRole: "building",
    openingStyle: "swing_single",
    assetKey: "sofa",
    pipeSystem: "cold_water",
    pipeMaterial: null,
    pipeDiameterMm: null,
    pipeElevationMm: null,
    slabKind: "balcony",
    slabRailing: true,
  } as ToolOptions;
  const settings = { default_wall_thickness_mm: 100, display_unit: "mm" as const };

  it("names the wall's thickness and type", () => {
    expect(rememberedStyle("wall", options, settings)).toBe("150 mm, building wall");
    expect(rememberedStyle("wall", { ...options, wallThicknessMm: null, wallRole: "site" }, settings)).toBe("100 mm, fence or site wall");
  });

  it("names the door, window and slab style", () => {
    expect(rememberedStyle("door", options, settings)).toBe("Single swing");
    expect(rememberedStyle("window", { ...options, openingStyle: "jalousie" }, settings)).toBe("Jalousie");
    expect(rememberedStyle("slab", options, settings)).toBe("Balcony, railing on");
  });

  it("names the stair's shape and turn, and the pipe's system and size", () => {
    expect(rememberedStyle("stair", options, settings)).toBe("Straight");
    expect(rememberedStyle("stair", options, settings, [], { shape: "l", turn: "right" })).toBe("L, turns right");
    expect(rememberedStyle("pipe", options, settings)).toBe("Cold water pipe, PPR 20 mm");
    expect(rememberedStyle("pipe", { ...options, pipeSystem: "drainage", pipeDiameterMm: 75 }, settings)).toBe(`Drainage pipe, ${PIPE_MATERIAL_LABEL.upvc} 75 mm`);
  });

  it("names the picked object, and nothing for tools without options", () => {
    const catalog = [{ key: "sofa", name: "Sofa, 3 seat" }] as CatalogItem[];
    expect(rememberedStyle("asset", options, settings, catalog)).toBe("Sofa, 3 seat");
    expect(rememberedStyle("select", options, settings)).toBeNull();
  });
});
