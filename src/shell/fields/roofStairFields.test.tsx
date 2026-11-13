// Roof and stair fields (lane L2) render what the engine worked out, and a
// shape change keeps each tread's depth.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { DocState, Element, RoofGeometry, StairGeometry } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";
import probe from "../../editor2d/roofsStairs.fixture.json";
import { StairToolOptions } from "../../editor2d/StairToolOptions";
import { setStairShape } from "../../editor2d/stairTool";
import { RoofFields, RoofSection, shedRise } from "./RoofFields";
import { StairFields, treadCount, withShape } from "./StairFields";

type Of<K extends Element["kind"]> = Extract<Element, { kind: K }>;

const [L1, L2] = probe.levels;
const elements = probe.elements as unknown as Element[];

function doc(): DocState {
  const d = structuredClone(fixture as unknown as DocState);
  d.project.levels = [
    { id: L1, name: "Ground Floor", elevation_mm: 0, height_mm: 3000 },
    { id: L2, name: "Second Floor", elevation_mm: 3000, height_mm: 3000 },
  ];
  d.project.elements = structuredClone(elements);
  d.derived = { ...d.derived, stairs: probe.stairs as unknown as StairGeometry[], roofs: probe.roofs as unknown as RoofGeometry[] };
  return d;
}

describe("stair fields", () => {
  it("shows the L's shape, turn, first flight and the engine's tread and riser", () => {
    const d = doc();
    const html = renderToStaticMarkup(<StairFields el={d.project.elements[0] as Of<"stair">} doc={d} />);
    expect(html).toContain("Turns");
    expect(html).toContain("First flight");
    expect(html).toContain("17 risers make 15 treads");
    expect(html).toContain("250 mm");
  });

  it("keeps each tread's depth when the shape changes, and gives a new U its gap", () => {
    const straight = { ...(elements[0] as Of<"stair">), shape: "straight" as const, run_mm: 16 * 250 };
    expect(treadCount(straight)).toBe(16);
    const u = withShape(straight, "u");
    expect(u).toMatchObject({ shape: "u", run_mm: 15 * 250, well_mm: 100, riser_count: 17 });
    const low = withShape({ ...straight, riser_count: 3, run_mm: 500 }, "l");
    expect(low).toMatchObject({ riser_count: 4, run_mm: 500 });
  });
});

describe("roof fields", () => {
  it("shows a carport's areas and its posts button", () => {
    const d = doc();
    const html = renderToStaticMarkup(<RoofFields el={d.project.elements[3] as Of<"roof">} doc={d} />);
    expect(html).toContain("20.16 m²");
    expect(html).toContain("Add posts under open eaves");
    expect(html).toContain("Hip");
  });

  it("leaves the posts out for the roof over the top floor, and offers hip in the project section", () => {
    const d = doc();
    expect(renderToStaticMarkup(<RoofFields el={d.project.elements[2] as Of<"roof">} doc={d} />)).not.toContain("Add posts");
    expect(renderToStaticMarkup(<RoofSection doc={d} />)).toContain("Hip");
  });
});

describe("stair tool flyout", () => {
  it("lists the shapes and, for an L or U, the turns", () => {
    setStairShape("l");
    const html = renderToStaticMarkup(<StairToolOptions />);
    expect(html).toContain("Straight");
    expect(html).toContain("quarter turn");
    expect(html).toContain("Right");
    setStairShape("straight");
  });
});

describe("a shed's high side", () => {
  it("reads its axis and flip as the side it rises toward", () => {
    expect(shedRise({ ridge_axis: "y" })).toBe("north");
    expect(shedRise({ ridge_axis: "y", flip: true })).toBe("south");
    expect(shedRise({ ridge_axis: "x", flip: false })).toBe("east");
    expect(shedRise({ ridge_axis: "x", flip: true })).toBe("west");
  });
});
