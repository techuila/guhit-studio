import { describe, expect, it } from "vitest";
import type { DocState, Element } from "../../contract/bindings";
import { defaultSetCommand, droppedPosition, dropIndex, nextRevisionMark, parseSheetSvg, projectViews, shiftFor, today, viewLabel, viewTitle } from "./sheetModel";

const SVG = `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="420mm" height="297mm" viewBox="0 0 420 297">
<g id="drawing-area" data-x="15" data-y="15" data-w="314" data-h="267" data-page="420 297"/>
<g class="viewport" id="vp-v1" data-id="v1" data-x="40" data-y="20" data-w="120" data-h="90" data-frame="35 20 130 103" data-scale="100" data-placed="false">
</g>
<g class="viewport" id="vp-a&amp;b" data-id="a&amp;b" data-x="200" data-y="20" data-w="80" data-h="40" data-frame="200 20 80 53" data-scale="" data-placed="true">
</g>
</svg>`;

describe("sheet svg", () => {
  it("reads the drawing area and every viewport box", () => {
    const l = parseSheetSvg(SVG)!;
    expect(l.page).toEqual({ w: 420, h: 297 });
    expect(l.area).toEqual({ x: 15, y: 15, w: 314, h: 267 });
    expect(l.viewports).toHaveLength(2);
    expect(l.viewports[0]).toMatchObject({ id: "v1", x: 40, y: 20, w: 120, h: 90, scale: 100, placed: false, frame: { x: 35, y: 20, w: 130, h: 103 } });
    expect(l.viewports[1]).toMatchObject({ id: "a&b", scale: null, placed: true });
  });

  it("is null for an svg without the layout data", () => {
    expect(parseSheetSvg("<svg></svg>")).toBeNull();
  });

  it("turns a drag into the content center from the bottom left", () => {
    const box = { x: 40, y: 20, w: 120, h: 90 };
    // Not moved: the center of the box, y measured up from the bottom.
    expect(droppedPosition(box, 0, 0, 297)).toEqual({ x: 100, y: 232 });
    // Dragged 10 mm right and 30 mm down the page.
    expect(droppedPosition(box, 10, 30, 297)).toEqual({ x: 110, y: 202 });
  });

  it("drops a dragged row on the nearest index and shifts the rows between", () => {
    expect(dropIndex(0, 70, 36, 5)).toBe(2);
    expect(dropIndex(3, -200, 36, 5)).toBe(0);
    expect(dropIndex(1, 900, 36, 5)).toBe(4);
    expect([0, 1, 2, 3].map((i) => shiftFor(i, 0, 2))).toEqual([0, -1, -1, 0]);
    expect([0, 1, 2, 3].map((i) => shiftFor(i, 3, 1))).toEqual([0, 1, 1, 0]);
  });
});

const doc = (elements: Element[]): Pick<DocState, "project"> =>
  ({
    project: {
      levels: [
        { id: "l1", name: "Ground Floor", elevation_mm: 0, height_mm: 3000 },
        { id: "l2", name: "Second Floor", elevation_mm: 3000, height_mm: 3000 },
      ],
      elements,
    },
  }) as unknown as Pick<DocState, "project">;

const mark = (id: string, kind: "elevation" | "section", label: string) =>
  ({ kind: "view_mark", id, level_id: "l1", mark_kind: kind, label, start: { x: 0, y: 0 }, end: { x: 1, y: 0 }, flip: false, depth_mm: null }) as unknown as Element;
const wall = (level: string) => ({ kind: "wall", id: `w-${level}`, level_id: level }) as unknown as Element;

describe("views and the default set", () => {
  const d = doc([wall("l1"), wall("l2"), mark("e1", "elevation", "North"), mark("e2", "elevation", "South"), mark("e3", "elevation", "East"), mark("s1", "section", "A"), { kind: "roof", id: "r" } as unknown as Element]);

  it("titles views as the sheets print them", () => {
    expect(viewTitle(d, { type: "plan", level_id: "l1", sheet: "plan" })).toBe("GROUND FLOOR PLAN");
    expect(viewTitle(d, { type: "plan", level_id: "l1", sheet: "reflected_ceiling" })).toBe("GROUND FLOOR REFLECTED CEILING PLAN");
    expect(viewTitle(d, { type: "elevation", mark_id: "e1" })).toBe("NORTH ELEVATION");
    expect(viewTitle(d, { type: "section", mark_id: "s1" })).toBe("SECTION A-A");
    expect(viewTitle(d, { type: "schedule", schedule: "doors_windows" })).toBe("SCHEDULE OF DOORS AND WINDOWS");
  });

  it("lists every drawing the project has", () => {
    const groups = projectViews(d).map((v) => v.group);
    expect(groups.filter((g) => g === "Plans")).toHaveLength(2);
    expect(groups.filter((g) => g === "Elevations")).toHaveLength(3);
    expect(groups.filter((g) => g === "Schedules")).toHaveLength(4);
  });

  it("names views in lists in sentence case, keeping a section's marks (paper titles stay uppercase)", () => {
    const labels = projectViews(d).map((v) => v.label);
    expect(labels).toContain("North elevation");
    expect(labels).toContain("Section A-A");
    expect(labels.some((l) => l === l.toUpperCase() && /[A-Z]{3}/.test(l))).toBe(false);
    expect(viewLabel(d, { type: "section", mark_id: "s1" })).toBe("Section A-A");
    expect(viewTitle(d, { type: "section", mark_id: "s1" })).toBe("SECTION A-A");
  });

  it("makes plans per level, elevations two to a sheet, the roof and the schedules", () => {
    const cmd = defaultSetCommand(d, "a1");
    expect(cmd.type).toBe("batch");
    if (cmd.type !== "batch") return;
    const sheets = cmd.commands.map((c) => (c.type === "upsert_sheet" ? c.sheet : null));
    expect(sheets.map((s) => s?.title)).toEqual(["Ground floor plan", "Second floor plan", "Elevations", "Elevations", "Sections", "Roof plan", "Schedules"]);
    expect(sheets.map((s) => s?.viewports.length)).toEqual([1, 1, 2, 1, 1, 1, 4]);
    expect(sheets.every((s) => s?.paper === "a1" && s.id === "" && s.number === "")).toBe(true);
  });

  it("counts revision marks and dates", () => {
    expect(nextRevisionMark([])).toBe("1");
    expect(nextRevisionMark(["1", "2", "4"])).toBe("3");
    expect(today(new Date(2026, 10, 4))).toBe("2026-11-04");
  });
});
