import { describe, expect, it } from "vitest";
import type { Camera, RenderAiRequest, RenderRecord } from "../../contract/bindings";
import { renderWithAi, type FlowDeps } from "./aiFlow";
import { capLine, capShare, driftState, estimateLine, type Draft, type Estimate } from "./aiRender";

const request: RenderAiRequest = {
  source_render_id: "cap-1",
  style_key: null,
  prompt: "",
  quality: "standard",
  keep_geometry: true,
};

const camera: Camera = { id: "", name: "Front", preset: "custom", light: null, position: { x: 0, y: 0, z: 0 }, target: { x: 0, y: 1, z: 0 }, fov_deg: 45 };

function estimate(extra: Partial<Estimate> = {}): Estimate {
  return {
    model: "gemini-3.1-flash-image",
    model_label: "Gemini Flash",
    image_size: "2K",
    price_usd: "0.10",
    why: "the cheaper model for this quality",
    pro: false,
    style_reference_id: null,
    maps: ["lines", "depth"],
    maps_missing: false,
    prompt: "text",
    agents: { day: "2026-11-04", used: 3, cap: 20, default_cap: 20 },
    ...extra,
  };
}

function draft(warn: boolean | null): Draft {
  return {
    pending_id: "p-1",
    image: "data:image/png;base64,AAAA",
    drift: warn === null ? null : { missing: warn ? 0.41 : 0.04, warn, note: warn ? "window W1 moved" : "lines up" },
    overlay: null,
    estimate: estimate(),
    seconds: 1,
    provider: "gemini/fake",
  };
}

const record = { id: "ai-1", camera } as unknown as RenderRecord;

function deps(log: string[], opts: { warn?: boolean | null; missing?: boolean } = {}): FlowDeps {
  let mapsDrawn = false;
  return {
    estimate: async () => {
      log.push("estimate");
      return estimate({ maps_missing: !!opts.missing && !mapsDrawn, maps: opts.missing && !mapsDrawn ? [] : ["lines", "depth"] });
    },
    drawMaps: async () => {
      log.push("maps");
      mapsDrawn = true;
    },
    draft: async () => {
      log.push("draft");
      return draft(opts.warn ?? false);
    },
    keep: async () => {
      log.push("keep");
      return record;
    },
  };
}

describe("renderWithAi", () => {
  it("shows the price before the call it prices", async () => {
    const log: string[] = [];
    await renderWithAi(request, deps(log), {
      onPrice: (e) => log.push(`price ${e.price_usd}`),
      onPhase: (p) => log.push(`phase ${p}`),
    });
    expect(log).toEqual(["phase pricing", "estimate", "price 0.10", "phase rendering", "draft", "keep"]);
    expect(log.indexOf("price 0.10")).toBeLessThan(log.indexOf("draft"));
  });

  it("draws missing maps first, then prices again", async () => {
    const log: string[] = [];
    const prices: Estimate[] = [];
    await renderWithAi(request, deps(log, { missing: true }), { onPrice: (e) => prices.push(e), onPhase: () => {} });
    expect(log).toEqual(["estimate", "maps", "estimate", "draft", "keep"]);
    expect(prices.map((p) => p.maps.length)).toEqual([0, 2]);
  });

  it("holds an image the drift check flags, and keeps a clean one", async () => {
    const held = await renderWithAi(request, deps([], { warn: true }), { onPrice: () => {}, onPhase: () => {} });
    expect(held.kind).toBe("held");
    const log: string[] = [];
    const saved = await renderWithAi(request, deps(log, { warn: false }), { onPrice: () => {}, onPhase: () => {} });
    expect(saved.kind).toBe("saved");
    expect(log).toContain("keep");
    const unchecked = await renderWithAi(request, deps([], { warn: null }), { onPrice: () => {}, onPhase: () => {} });
    expect(unchecked.kind).toBe("saved");
  });

  it("never spends when the estimate fails", async () => {
    const log: string[] = [];
    const d = deps(log);
    d.estimate = async () => {
      throw { code: "invalid", message: "no source", element_ids: [] };
    };
    await expect(renderWithAi(request, d, { onPrice: () => {}, onPhase: () => {} })).rejects.toMatchObject({ code: "invalid" });
    expect(log).not.toContain("draft");
  });
});

describe("price and cap wording", () => {
  it("reads plainly", () => {
    expect(estimateLine(estimate())).toBe("Gemini Flash, 2K, about $0.10");
    expect(estimateLine(null)).toBe("");
    expect(capLine({ day: "d", used: 3, cap: 20, default_cap: 20 })).toBe("17 of 20 left today");
    expect(capLine({ day: "d", used: 25, cap: 20, default_cap: 20 })).toBe("0 of 20 left today");
    expect(capLine({ day: "d", used: 0, cap: 0, default_cap: 20 })).toBe("Agents may not make AI images");
    expect(capShare({ day: "d", used: 5, cap: 20, default_cap: 20 })).toBe(0.25);
    expect(driftState({ drift: { missing: 0.5, warn: true, note: "" } })).toBe("drift");
    expect(driftState({ drift: { missing: 0.02, warn: false, note: "" } })).toBe("ok");
    expect(driftState({})).toBeNull();
  });
});
