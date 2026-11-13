// Built-in fields (lane L5) show what each built-in has, with the defaults
// for its width until it carries its own options.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { CatalogItem, DocState, Element } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";
import { BuiltInFields } from "./BuiltInFields";

type Asset = Extract<Element, { kind: "asset" }>;

const doc = fixture as unknown as DocState;

function run(key: string, parametric: CatalogItem["parametric"], built_in: Asset["built_in"] = null, width_mm = 2400): { el: Asset; item: CatalogItem } {
  const el = {
    kind: "asset", id: "a1", level_id: doc.project.levels[0].id, catalog_key: key, name: key, category: "kitchen", position: { x: 0, y: 0 },
    rotation_deg: 0, width_mm, depth_mm: 600, height_mm: 900, elevation_mm: 0, light: null, links: [], circuit: "", built_in,
  } as unknown as Asset;
  const item = { key, name: key, category: "kitchen", width_mm, depth_mm: 600, height_mm: 900, elevation_mm: 0, parametric } as unknown as CatalogItem;
  return { el, item };
}

describe("built-in fields", () => {
  it("shows a kitchen run's wall cabinets, sink, cooktop and doors, at the defaults for its width", () => {
    const { el, item } = run("kitchen-run", "kitchen_run");
    const html = renderToStaticMarkup(<BuiltInFields el={el} item={item} doc={doc} />);
    for (const label of ["Wall cabinets", "Sink", "Cooktop", "Doors"]) expect(html).toContain(label);
    // Sink 700 and cooktop 1800 from the left end of a 2400 run.
    expect(html).toContain('value="700');
    expect(html).toContain('value="1800');
    expect(html).toContain("Up to 9 across 2400 mm");
  });

  it("shows a run with no cooktop as None", () => {
    const { el, item } = run("kitchen-run", "kitchen_run", { upper: false, sink_at_mm: 1200, cooktop_at_mm: null, doors: 4 });
    const html = renderToStaticMarkup(<BuiltInFields el={el} item={item} doc={doc} />);
    expect(html).toContain("None");
    expect(html).toContain('value="1200');
  });

  it("gives a vanity a mirror cabinet and no cooktop, a wardrobe only doors, wall shelves nothing", () => {
    const vanity = run("vanity", "vanity", null, 900);
    const v = renderToStaticMarkup(<BuiltInFields el={vanity.el} item={vanity.item} doc={doc} />);
    expect(v).toContain("Mirror cabinet");
    expect(v).not.toContain("Cooktop");
    const wardrobe = run("wardrobe-builtin", "wardrobe", null, 1800);
    const w = renderToStaticMarkup(<BuiltInFields el={wardrobe.el} item={wardrobe.item} doc={doc} />);
    expect(w).toContain("Doors");
    expect(w).not.toContain("Sink");
    const shelf = run("shelf-wall", "shelf", null, 1200);
    expect(renderToStaticMarkup(<BuiltInFields el={shelf.el} item={shelf.item} doc={doc} />)).toBe("");
  });
});
