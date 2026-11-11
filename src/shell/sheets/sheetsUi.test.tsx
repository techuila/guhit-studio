// The Sheets panel by keyboard: one tab stop in the sheet list with the arrow
// keys (U22), and a viewport row that the keyboard can pick (U46).
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { DocState, Sheet } from "../../contract/bindings";
import { SheetList } from "./SheetList";
import { SheetTab } from "./SheetTab";

const sheet = (id: string, number: string, viewports: Sheet["viewports"] = []): Sheet => ({
  id,
  number,
  title: `Sheet ${number}`,
  paper: "a3",
  orientation: "landscape",
  viewports,
  revision_ids: [],
});

const noop = () => {};

describe("the sheet list", () => {
  it("is one tab stop: the selected sheet", () => {
    const html = renderToStaticMarkup(<SheetList sheets={[sheet("a", "A-1"), sheet("b", "A-2"), sheet("c", "A-3")]} selected="b" onSelect={noop} onMove={noop} />);
    expect(html.match(/tabindex="0"/g)).toHaveLength(1);
    expect(html.match(/tabindex="-1"/g)).toHaveLength(2);
    expect(html).toMatch(/aria-selected="true" tabindex="0"/);
  });
});

describe("a sheet's viewport rows", () => {
  const doc = {
    project: { levels: [{ id: "l1", name: "Ground Floor", elevation_mm: 0, height_mm: 3000 }], elements: [], settings: {} },
  } as unknown as DocState;
  const vp = (id: string): Sheet["viewports"][number] => ({ id, view: { type: "roof_plan" }, scale_denominator: null, position: null, title: null });

  it("gives each row a button that says whether its view is picked", () => {
    const html = renderToStaticMarkup(<SheetTab doc={doc} sheet={sheet("a", "A-1", [vp("v1"), vp("v2")])} selectedViewport="v2" onSelectViewport={noop} onDelete={noop} />);
    const rows = html.match(/<button type="button" class="[^"]*" aria-pressed="(true|false)"[^>]*data-testid="viewport-row"/g) ?? [];
    expect(rows).toHaveLength(2);
    expect(rows[0]).toContain('aria-pressed="false"');
    expect(rows[1]).toContain('aria-pressed="true"');
    // The section heading and the view names are sentence case: uppercase
    // stays on the paper (R11, G22).
    expect(html).toContain("Views on this sheet");
    expect(html).toContain(">Roof plan</button>");
    expect(html).not.toContain("ROOF PLAN");
    // The kind pill is left out where it would repeat the title (G31).
    expect(html).not.toContain(">Roof plan</span>");
  });

  it("keeps the kind pill when the title says something else", () => {
    const named = { ...vp("v1"), title: "Roof framing" };
    const html = renderToStaticMarkup(<SheetTab doc={doc} sheet={sheet("a", "A-1", [named])} selectedViewport={null} onSelectViewport={noop} onDelete={noop} />);
    expect(html).toContain(">Roof framing</button>");
    expect(html).toContain(">Roof plan</span>");
  });
});
