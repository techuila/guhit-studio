import { describe, expect, it } from "vitest";
import type { Camera, RenderRecord, RenderAiSettings } from "../../contract/bindings";
import {
  buildPrompt,
  clampPercent,
  costForQuality,
  flattenGroups,
  formatResolution,
  groupRenders,
  percentFromKey,
  percentFromPointer,
  providerLabel,
} from "./renderStore";

const camera: Camera = {
  id: "c1",
  name: "Front",
  preset: "exterior_corner",
  light: null,
  position: { x: 0, y: 0, z: 0 },
  target: { x: 0, y: 0, z: 0 },
  fov_deg: 50,
};

function rec(id: string, at: string, extra: Partial<RenderRecord> = {}): RenderRecord {
  return {
    id,
    created_at: at,
    source: "model_view",
    revision: 3,
    camera,
    style_key: null,
    prompt: "",
    image_path: `/p/${id}.png`,
    source_render_id: null,
    provider: null,
    info: null,
    ...extra,
  };
}

function ai(id: string, at: string, sourceId: string | null): RenderRecord {
  return rec(id, at, {
    source: "ai_visualization",
    source_render_id: sourceId,
    provider: "gemini/gemini-3.1-flash-image",
  });
}

describe("divider maths", () => {
  it("clamps to 0..100", () => {
    expect(clampPercent(-4)).toBe(0);
    expect(clampPercent(140)).toBe(100);
    expect(clampPercent(33.3)).toBeCloseTo(33.3);
    expect(clampPercent(Number.NaN)).toBe(50);
  });

  it("tracks the pointer 1:1", () => {
    // A 800 px wide box starting at x = 100: the divider is exactly where the
    // pointer is, and a 200 px drag moves it by 200 px worth of percent.
    expect(percentFromPointer(100, 100, 800)).toBe(0);
    expect(percentFromPointer(500, 100, 800)).toBe(50);
    const start = percentFromPointer(300, 100, 800);
    const after = percentFromPointer(500, 100, 800);
    expect(((after - start) / 100) * 800).toBeCloseTo(200);
  });

  it("moves 2 percent per arrow key and snaps with Home and End", () => {
    expect(percentFromKey("ArrowRight", 50)).toBe(52);
    expect(percentFromKey("ArrowLeft", 50)).toBe(48);
    expect(percentFromKey("PageUp", 50)).toBe(60);
    expect(percentFromKey("Home", 50)).toBe(0);
    expect(percentFromKey("End", 50)).toBe(100);
    expect(percentFromKey("ArrowLeft", 1)).toBe(0);
    expect(percentFromKey("a", 50)).toBeNull();
  });
});

describe("grouping", () => {
  it("puts a visualization with the capture it came from, newest group first", () => {
    const view1 = rec("v1", "2026-09-20T10:00:00Z");
    const view2 = rec("v2", "2026-09-21T10:00:00Z");
    const viz1 = ai("a1", "2026-09-22T10:00:00Z", "v1");
    const viz2 = ai("a2", "2026-09-22T11:00:00Z", "v1");
    const groups = groupRenders([view1, view2, viz1, viz2]);

    expect(groups).toHaveLength(2);
    expect(groups[0].source?.id).toBe("v1");
    expect(groups[0].visualizations.map((r) => r.id)).toEqual(["a2", "a1"]);
    expect(groups[1].source?.id).toBe("v2");
    expect(flattenGroups(groups).map((r) => r.id)).toEqual(["v1", "a2", "a1", "v2"]);
  });

  it("keeps a visualization whose source is gone", () => {
    const groups = groupRenders([ai("a1", "2026-09-22T10:00:00Z", "missing")]);
    expect(groups).toHaveLength(1);
    expect(groups[0].source).toBeNull();
    expect(groups[0].visualizations[0].id).toBe("a1");
  });
});

describe("prompt and labels", () => {
  it("puts the building type first", () => {
    expect(buildPrompt("two-storey-house", "capiz windows, late afternoon")).toBe(
      "building type: two storey house. capiz windows, late afternoon",
    );
    expect(buildPrompt("office", "  ")).toBe("building type: office");
  });

  it("reads the cost per quality from the provider hint", () => {
    const settings: RenderAiSettings = {
      provider: "gemini",
      has_api_key: true,
      model: "gemini-3.1-flash-image",
      cost_hint: "$0.045 to $0.151 per image",
    };
    expect(costForQuality(settings, "draft")).toBe("about $0.045 per image");
    expect(costForQuality(settings, "standard")).toBe("$0.045 to $0.151 per image");
    expect(costForQuality(settings, "high")).toBe("about $0.151 per image");
    expect(costForQuality({ ...settings, cost_hint: "about $0.04 an image" }, "high")).toBe("about $0.04 an image");
    expect(costForQuality(null, "high")).toBe("");

    // The wording the backend actually sends, one amount per level.
    const perLevel: RenderAiSettings = {
      ...settings,
      cost_hint: "About $0.05 per draft, $0.10 per standard, $0.24 per high image (Google list price)",
    };
    expect(costForQuality(perLevel, "draft")).toBe("about $0.05 per image");
    expect(costForQuality(perLevel, "standard")).toBe("about $0.10 per image");
    expect(costForQuality(perLevel, "high")).toBe("about $0.24 per image");
  });

  it("formats the resolution badge and the model name", () => {
    expect(formatResolution(1301, 800)).toBe("1301 x 800 px");
    expect(formatResolution(0, 800)).toBe("");
    expect(providerLabel("gemini/gemini-3.1-flash-image")).toBe("Gemini 3.1 flash image");
    expect(providerLabel(null)).toBe("");
  });
});
