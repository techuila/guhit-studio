import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Command, DocState, Element } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";
import { useApp } from "../../state/store";
import type { P } from "../geom";
import { pt } from "../geom";
import { buildIndex } from "../model";
import type { PlanTool, PlanToolHost } from "../planTool";
import { dimStringTool } from "./dimStringTool";
import { gridTool } from "./gridTool";
import { lotTool, lotFrom, pasteSurvey } from "./lotTool";
import { flipFor, markLabel, viewMarkTool } from "./viewMarkTool";

const base = fixture as unknown as DocState;
const level = base.project.levels[0].id;

function fakeHost(doc: DocState = base, over = { plan: true }) {
  const sent: Command[] = [];
  const index = buildIndex(doc, level);
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
    pointerInside: () => over.plan,
  };
  return { host, sent };
}

const key = (k: string) => ({ key: k }) as KeyboardEvent;
const click = (t: PlanTool, h: PlanToolHost, p: P, presses = 1) => {
  t.hover(h, p);
  t.down(h, p, presses);
};
const added = (c: Command | undefined): Element => {
  if (!c || (c.type !== "add_element" && c.type !== "update_element")) throw new Error(`not an element command: ${c?.type}`);
  return c.element;
};
const flush = () => new Promise((r) => setTimeout(r, 0));

const TOOLS = [gridTool, dimStringTool, viewMarkTool, lotTool];
beforeEach(() => TOOLS.forEach((t) => t.reset()));
afterEach(() => TOOLS.forEach((t) => t.reset()));

describe("grid tool", () => {
  it("numbers an up and down line and letters an across line", async () => {
    const { host, sent } = fakeHost();
    expect(gridTool.enabled).toBe(true);
    click(gridTool, host, pt(0, 0));
    click(gridTool, host, pt(200, 9000));
    click(gridTool, host, pt(-1000, 7000));
    click(gridTool, host, pt(9000, 7000));
    await flush();
    expect(sent.map((c) => added(c))).toMatchObject([
      { kind: "grid", id: "", label: "1", start: pt(0, 0), end: pt(200, 9000) },
      { kind: "grid", id: "", label: "A" },
    ]);
  });

  it("skips taken labels", async () => {
    const d = structuredClone(base);
    d.project.elements.push({ kind: "grid", id: "g", label: "A", start: pt(0, 0), end: pt(5000, 0) } as Element);
    const { host, sent } = fakeHost(d);
    click(gridTool, host, pt(0, 3000));
    click(gridTool, host, pt(6000, 3000));
    await flush();
    expect(added(sent[0])).toMatchObject({ label: "B" });
  });

  it("places a typed length along the pointer", async () => {
    const { host, sent } = fakeHost();
    click(gridTool, host, pt(0, 0));
    gridTool.hover(host, pt(100, 0));
    for (const k of ["4", "5", "0", "0"]) expect(gridTool.key(host, key(k))).toBe(true);
    expect(gridTool.key(host, key("Enter"))).toBe(true);
    await flush();
    expect(added(sent[0])).toMatchObject({ start: pt(0, 0), end: pt(4500, 0), label: "A" });
  });

  it("refuses a line under 100 mm and drops the draft on Escape", async () => {
    const { host, sent } = fakeHost();
    click(gridTool, host, pt(0, 0));
    click(gridTool, host, pt(50, 0));
    await flush();
    expect(sent).toHaveLength(0);
    expect(gridTool.hint(host)).toMatch(/other end/);
    expect(gridTool.key(host, key("Escape"))).toBe(true);
    expect(gridTool.hint(host)).toMatch(/two ends/);
    // Nothing left to drop: Escape goes back to the controller (Select).
    expect(gridTool.key(host, key("Escape"))).toBe(false);
  });

  it("keeps tool letters while drawing, so they do not switch tools", () => {
    const { host } = fakeHost();
    expect(gridTool.key(host, key("w"))).toBe(false);
    click(gridTool, host, pt(0, 0));
    expect(gridTool.key(host, key("w"))).toBe(true);
  });
});

describe("dimension string tool", () => {
  it("clicks the points, places the line, then sets the offset with the pointer", async () => {
    const { host, sent } = fakeHost();
    click(dimStringTool, host, pt(0, 0));
    click(dimStringTool, host, pt(2500, 0));
    click(dimStringTool, host, pt(8000, 0));
    expect(dimStringTool.key(host, key("Enter"))).toBe(true);
    expect(dimStringTool.hint(host)).toMatch(/offset/);
    click(dimStringTool, host, pt(4000, -900));
    await flush();
    expect(added(sent[0])).toEqual({
      kind: "dimension",
      id: "",
      level_id: level,
      a: pt(0, 0),
      b: pt(8000, 0),
      offset_mm: -900,
      text_override: null,
      through: [pt(2500, 0)],
    });
  });

  it("takes a typed offset and a right click to place the line", async () => {
    const { host, sent } = fakeHost();
    click(dimStringTool, host, pt(0, 0));
    click(dimStringTool, host, pt(1000, 0));
    click(dimStringTool, host, pt(3000, 0));
    dimStringTool.finish(host);
    for (const k of ["-", "6", "0", "0"]) dimStringTool.key(host, key(k));
    dimStringTool.key(host, key("Enter"));
    await flush();
    expect(added(sent[0])).toMatchObject({ offset_mm: -600, through: [pt(1000, 0)] });
  });

  it("places on a double click and refuses with one point", async () => {
    const { host, sent } = fakeHost();
    click(dimStringTool, host, pt(0, 0));
    dimStringTool.key(host, key("Enter"));
    expect(dimStringTool.hint(host)).toMatch(/next point/);
    click(dimStringTool, host, pt(4000, 0));
    click(dimStringTool, host, pt(4000, 0), 2);
    expect(dimStringTool.hint(host)).toMatch(/offset/);
    expect(dimStringTool.key(host, key("Backspace"))).toBe(true);
    expect(dimStringTool.hint(host)).toMatch(/Enter/);
    await flush();
    expect(sent).toHaveLength(0);
  });
});

describe("section and elevation tool", () => {
  it("clicks the line, then the side it looks at", async () => {
    const { host, sent } = fakeHost();
    click(viewMarkTool, host, pt(0, -2000));
    click(viewMarkTool, host, pt(0, 8000));
    // Right of a line going up (+y) is +x: it looks right.
    click(viewMarkTool, host, pt(3000, 3000));
    await flush();
    expect(added(sent[0])).toMatchObject({ kind: "view_mark", mark_kind: "section", label: "A", flip: true, level_id: level, depth_mm: null });
  });

  it("switches to an elevation on Tab and names it by the side it sees", async () => {
    const { host, sent } = fakeHost();
    expect(viewMarkTool.key(host, key("Tab"))).toBe(true);
    // North of the house, looking south (right of a line going west to east).
    click(viewMarkTool, host, pt(-1000, 9000));
    click(viewMarkTool, host, pt(9000, 9000));
    click(viewMarkTool, host, pt(4000, 3000));
    await flush();
    expect(added(sent[0])).toMatchObject({ mark_kind: "elevation", label: "North", flip: true });
    viewMarkTool.key(host, key("Tab"));
  });

  it("leaves Tab to move focus while it has no draft and the pointer is off the plan (GR3 P1-4)", () => {
    const over = { plan: false };
    const { host, sent } = fakeHost(base, over);
    // Focus on a button, the pointer elsewhere: Tab is not the tool's.
    expect(viewMarkTool.key(host, key("Tab"))).toBe(false);
    expect(viewMarkTool.key(host, key("Tab"))).toBe(false);
    // Over the plan, Tab switches the kind as before.
    over.plan = true;
    expect(viewMarkTool.key(host, key("Tab"))).toBe(true);
    expect(viewMarkTool.key(host, key("Tab"))).toBe(true);
    // With a draft, Tab is the tool's wherever the pointer went.
    click(viewMarkTool, host, pt(0, -2000));
    over.plan = false;
    expect(viewMarkTool.key(host, key("Tab"))).toBe(true);
    expect(viewMarkTool.key(host, key("Escape"))).toBe(true);
    expect(viewMarkTool.key(host, key("Tab"))).toBe(false);
    expect(sent).toHaveLength(0);
    // Back to a section for the tests after.
    over.plan = true;
    viewMarkTool.key(host, key("Tab"));
  });

  it("labels the next free letter and numbers a taken elevation", () => {
    const marks = [
      { mark_kind: "section" as const, label: "A" },
      { mark_kind: "elevation" as const, label: "North" },
    ];
    expect(markLabel(marks, "section", pt(0, 0), pt(1000, 0), false, 0)).toBe("B");
    expect(markLabel(marks, "elevation", pt(0, 0), pt(1000, 0), true, 0)).toBe("North 2");
    expect(flipFor(pt(0, 0), pt(1000, 0), pt(500, -10))).toBe(true);
    expect(flipFor(pt(0, 0), pt(1000, 0), pt(500, 10))).toBe(false);
  });
});

describe("lot tool", () => {
  const corners = [pt(-2000, -2000), pt(-2000, 10000), pt(12000, 10000), pt(12000, -2000)];

  it("closes on the first corner and sends the corners counter-clockwise", async () => {
    const { host, sent } = fakeHost();
    for (const c of corners) click(lotTool, host, c);
    click(lotTool, host, pt(-1990, -1995));
    await flush();
    const lot = added(sent[0]);
    expect(sent[0].type).toBe("add_element");
    expect(lot).toMatchObject({ kind: "lot", id: "", grade_mm: 0 });
    if (lot.kind !== "lot") throw new Error("lot");
    expect(lot.corners).toEqual([...corners].reverse());
    expect(lot.sides).toHaveLength(4);
    expect(lot.sides.every((s) => s.role === "side" && s.setback_mm === 0)).toBe(true);
  });

  it("replaces the corners of the project's lot, keeping its id", async () => {
    const d = structuredClone(base);
    const sides = [0, 1, 2].map(() => ({ role: "road", setback_mm: 2000, road: "Mabini St", road_width_mm: 6000 }));
    d.project.elements.push({ kind: "lot", id: "lot1", corners: [pt(0, 0), pt(9000, 0), pt(0, 9000)], sides, grade_mm: 150 } as Element);
    const { host, sent } = fakeHost(d);
    expect(lotTool.hint(host)).toMatch(/replaces/);
    for (const c of corners.slice(0, 3)) click(lotTool, host, c);
    expect(lotTool.key(host, key("Enter"))).toBe(true);
    await flush();
    expect(sent[0].type).toBe("update_element");
    // Same corner count: the sides stay.
    expect(added(sent[0])).toMatchObject({ kind: "lot", id: "lot1", grade_mm: 150, sides });
  });

  it("refuses to close with two corners", async () => {
    const { host, sent } = fakeHost();
    click(lotTool, host, pt(0, 0));
    click(lotTool, host, pt(5000, 0));
    lotTool.key(host, key("Enter"));
    await flush();
    expect(sent).toHaveLength(0);
    expect(lotFrom([pt(0, 0), pt(10, 0), pt(10, 10)], null).corners).toEqual([pt(0, 0), pt(10, 0), pt(10, 10)]);
  });

  it("sends a pasted technical description from the last click", async () => {
    const sent: Command[] = [];
    const toasts: string[] = [];
    const prev = useApp.getState();
    useApp.setState({
      dispatch: async (c: Command) => {
        sent.push(c);
        return { state: base, diff: { summary: "Set the lot: 180.00 m², closes within 2 mm" } } as never;
      },
      toast: (_k: string, m: string) => void toasts.push(m),
    });
    try {
      const { host } = fakeHost();
      click(lotTool, host, pt(1000, 2000));
      lotTool.key(host, key("Escape"));
      const ok = await pasteSurvey(host, "thence N 45-30 E 15.00 m. to point 2; thence S 44-30 E 12.00 m. to point 3;");
      expect(ok).toBe(true);
      expect(sent[0]).toEqual({
        type: "set_lot_from_survey",
        start: pt(1000, 2000),
        legs: [
          { bearing: "N 45-30 E", distance_mm: 15000 },
          { bearing: "S 44-30 E", distance_mm: 12000 },
        ],
      });
      expect(toasts[0]).toMatch(/180/);
      expect(await pasteSurvey(host, "nothing here")).toBe(false);
      expect(sent).toHaveLength(1);
    } finally {
      useApp.setState({ dispatch: prev.dispatch, toast: prev.toast });
    }
  });
});
