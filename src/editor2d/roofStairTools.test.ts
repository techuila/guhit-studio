// The roof tool's clicks and closing, and the stair tool's shapes, keys,
// hint and engine ghost (lane L2).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApplyResult, Command, DocState, Element, StairGeometry } from "../contract/bindings";
import { ipc } from "../contract/ipc";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import { useApp } from "../state/store";
import type { P } from "./geom";
import { polygonArea, pt } from "./geom";
import { buildIndex } from "./model";
import type { PlanToolHost } from "./planTool";
import { closedOutline, roofDraft, roofFrom, roofTool, roofToolShape, setRoofToolShape } from "./roofTool";
import probe from "./roofsStairs.fixture.json";
import { clearStairGhosts, newStair, onStairGhost, setStairShape, setStairTurn, stairGhost, stairToolHint, stairToolKey, useStairTool } from "./stairTool";

const base = fixture as unknown as DocState;
const level = base.project.levels[0].id;

function fakeHost() {
  const sent: Command[] = [];
  const index = buildIndex(base, level);
  const host: PlanToolHost = {
    index: () => index,
    levelId: () => level,
    snap: (w) => w,
    run: async (c) => {
      sent.push(c);
      return true;
    },
    invalidate: () => {},
    pxMm: () => 10,
    pointerInside: () => true,
  };
  return { host, sent };
}

const key = (k: string) => ({ key: k }) as KeyboardEvent;
const click = (h: PlanToolHost, p: P, presses = 1) => {
  roofTool.hover(h, p);
  roofTool.down(h, p, presses);
};
const flush = () => new Promise((r) => setTimeout(r, 0));
const roofOf = (c: Command | undefined): Extract<Element, { kind: "roof" }> => {
  if (!c || c.type !== "add_element" || c.element.kind !== "roof") throw new Error(`not a roof: ${c?.type}`);
  return c.element;
};

describe("roof tool", () => {
  beforeEach(() => {
    roofTool.reset();
    setRoofToolShape("shed");
  });
  afterEach(() => roofTool.reset());

  it("is on, and closes two corners as the rectangle on that diagonal (Enter)", async () => {
    const { host, sent } = fakeHost();
    expect(roofTool.enabled).toBe(true);
    click(host, pt(9000, 5000));
    click(host, pt(12000, 0));
    expect(roofTool.key(host, key("Enter"))).toBe(true);
    await flush();
    const roof = roofOf(sent[0]);
    expect(roof).toMatchObject({
      id: "",
      level_id: level,
      shape: "shed",
      pitch_deg: 15,
      ridge_axis: "x",
      overhang_mm: 300,
      thickness_mm: 100,
      eave_mm: null,
      material_id: null,
      fascia: { size_mm: 150, material_id: null },
      gutter: { size_mm: 125, material_id: null },
    });
    if (roof.outline.type !== "polygon") throw new Error("not drawn");
    expect(roof.outline.points).toHaveLength(4);
    expect(polygonArea(roof.outline.points)).toBeCloseTo(3000 * 5000);
    expect(roofDraft()).toHaveLength(0);
  });

  it("closes on a click on the first corner, counter-clockwise, in the shape picked with a key", async () => {
    const { host, sent } = fakeHost();
    expect(roofTool.key(host, key("h"))).toBe(true);
    expect(roofToolShape()).toBe("hip");
    // Clockwise clicks: the roof still goes counter-clockwise.
    for (const p of [pt(0, 0), pt(0, 4000), pt(6000, 4000), pt(6000, 0)]) click(host, p);
    click(host, pt(30, 20));
    await flush();
    const roof = roofOf(sent[0]);
    expect(roof.shape).toBe("hip");
    expect(roof.pitch_deg).toBe(25);
    if (roof.outline.type !== "polygon") throw new Error("not drawn");
    expect(polygonArea(roof.outline.points)).toBeGreaterThan(0);
  });

  it("closes on a double click, and on a right click with two corners", async () => {
    const { host, sent } = fakeHost();
    click(host, pt(0, 0));
    click(host, pt(3000, 3000));
    roofTool.down(host, pt(3000, 3000), 2);
    await flush();
    expect(sent).toHaveLength(1);
    click(host, pt(0, 0));
    click(host, pt(4000, 2000));
    roofTool.finish(host);
    await flush();
    expect(sent).toHaveLength(2);
  });

  it("steps back with Backspace, drops with Escape, and lets Escape through when idle", () => {
    const { host, sent } = fakeHost();
    click(host, pt(0, 0));
    click(host, pt(3000, 0));
    click(host, pt(3000, 3000));
    expect(roofTool.key(host, key("Backspace"))).toBe(true);
    expect(roofDraft()).toHaveLength(2);
    expect(roofTool.key(host, key("Escape"))).toBe(true);
    expect(roofDraft()).toHaveLength(0);
    expect(roofTool.key(host, key("Escape"))).toBe(false);
    // Idle, other letters are the shell's.
    expect(roofTool.key(host, key("w"))).toBe(false);
    expect(sent).toHaveLength(0);
  });

  it("refuses an outline with no area and keeps the draft", async () => {
    const { host, sent } = fakeHost();
    click(host, pt(0, 0));
    click(host, pt(5000, 0));
    roofTool.key(host, key("Enter"));
    await flush();
    expect(sent).toHaveLength(0);
    expect(roofDraft()).toHaveLength(2);
  });

  it("sets each shape from H, G, S and F", () => {
    const { host } = fakeHost();
    for (const [k, shape] of [
      ["g", "gable"],
      ["f", "flat"],
      ["S", "shed"],
      ["h", "hip"],
    ] as const) {
      roofTool.key(host, key(k));
      expect(roofToolShape()).toBe(shape);
    }
    expect(roofTool.hint(host)).toContain("hip roof");
  });

  it("builds the rectangle on a diagonal either way round", () => {
    expect(polygonArea(closedOutline([pt(5, 5), pt(0, 0)]))).toBeCloseTo(25);
    expect(roofFrom([pt(0, 0), pt(10, 10)], "lv", "flat")).toMatchObject({ shape: "flat", pitch_deg: 25, level_id: "lv" });
  });
});

describe("stair tool", () => {
  beforeEach(() => {
    setStairShape("straight");
    setStairTurn("left");
    clearStairGhosts();
  });
  afterEach(() => vi.restoreAllMocks());

  it("places a straight flight of n - 1 treads of 250 mm, leaving the new fields out", () => {
    const s = newStair("lv", 3000, pt(0, 0), 90);
    expect(s).toMatchObject({ kind: "stair", id: "", riser_count: 17, run_mm: 16 * 250, width_mm: 1000, rotation_deg: 90, railing: { height_mm: 900, style: "bars" } });
    expect("shape" in s).toBe(false);
  });

  it("places an L of n - 2 treads, half its risers in the first flight and a square landing", () => {
    setStairShape("l");
    setStairTurn("right");
    const s = newStair("lv", 3000, pt(0, 0), 0);
    expect(s).toMatchObject({ shape: "l", turn: "right", riser_count: 17, run_mm: 15 * 250, first_flight_risers: null, landing_mm: null });
    expect(s.well_mm).toBeUndefined();
  });

  it("places a U with a 100 mm gap, and at least 4 risers on a low level", () => {
    setStairShape("u");
    const s = newStair("lv", 400, pt(0, 0), 0);
    expect(s).toMatchObject({ shape: "u", turn: "left", well_mm: 100, riser_count: 4, run_mm: 2 * 250 });
  });

  it("cycles the shape with L and flips the turn with T for an L or U only", () => {
    expect(stairToolKey("t")).toBe(false);
    expect(stairToolKey("L")).toBe(true);
    expect(useStairTool.getState().shape).toBe("l");
    expect(stairToolKey("t")).toBe(true);
    expect(useStairTool.getState().turn).toBe("right");
    expect(stairToolHint()).toBe("Click to place an L stair turning right. R rotates, L changes the shape, T the turn");
    stairToolKey("l");
    expect(useStairTool.getState().shape).toBe("u");
    stairToolKey("l");
    expect(useStairTool.getState().shape).toBe("straight");
    expect(stairToolHint()).toBe("Click to place a straight stair. R rotates, L changes the shape");
    expect(stairToolKey("x")).toBe(false);
  });

  it("asks the engine once per shape for the ghost and moves the answer with the pointer", async () => {
    const doc = { ...base, revision: 7 } as DocState;
    useApp.setState({ doc });
    const answer = probe.stairs[0] as unknown as StairGeometry;
    const asked = vi.spyOn(ipc, "docPreview").mockImplementation(async () => {
      return { state: { ...doc, derived: { ...doc.derived, stairs: [answer] } }, diff: { added: [answer.stair_id], modified: [], removed: [], summary: "" } } as ApplyResult;
    });
    let redraws = 0;
    const off = onStairGhost(() => redraws++);
    setStairShape("l");
    setStairTurn("right");
    // The fixture's L stood with its first riser's middle at (1500, 800).
    const at = (x: number, y: number) => newStair(level, 3000, pt(x, y), 0);
    expect(stairGhost(at(1500, 800))).toBeNull();
    expect(stairGhost(at(1600, 800))).toBeNull();
    await flush();
    expect(asked).toHaveBeenCalledTimes(1);
    expect(redraws).toBe(1);
    const g = stairGhost(at(2500, 1800))!;
    expect(g.flights[1].outline[0].x).toBeCloseTo(answer.flights[1].outline[0].x + 1000);
    expect(g.flights[1].outline[0].y).toBeCloseTo(answer.flights[1].outline[0].y + 1000);
    expect(asked).toHaveBeenCalledTimes(1);
    // Another shape is another question.
    setStairTurn("left");
    stairGhost(at(0, 0));
    await flush();
    expect(asked).toHaveBeenCalledTimes(2);
    off();
    useApp.setState({ doc: null });
  });

  it("keeps a refused shape as no ghost, without asking again", async () => {
    useApp.setState({ doc: { ...base, revision: 3 } as DocState });
    const asked = vi.spyOn(ipc, "docPreview").mockRejectedValue({ code: "invalid", message: "too few risers" });
    const el = newStair(level, 3000, pt(0, 0), 0);
    expect(stairGhost(el)).toBeNull();
    await flush();
    expect(stairGhost(el)).toBeNull();
    await flush();
    expect(asked).toHaveBeenCalledTimes(1);
    useApp.setState({ doc: null });
  });
});
