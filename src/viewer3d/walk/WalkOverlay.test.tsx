// The walk overlay's key hint is a list of keys and what they do (V30): no
// middle-dot strings, key caps through SHIFT, and styled tooltips (U36).
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SHIFT } from "../../shell/actions";
import { FLY_KEYS, WALK_KEYS, WalkOverlay } from "./WalkOverlay";

const render = (nav: "walk" | "fly") =>
  renderToStaticMarkup(<WalkOverlay open nav={nav} engineRef={{ current: null }} locked={false} lockable={false} />);

describe("the walk key hint", () => {
  it("lists one key and its action per item, with no separators", () => {
    for (const nav of ["walk", "fly"] as const) {
      const html = render(nav);
      const hint = html.slice(html.indexOf('data-testid="walk-hint"'), html.indexOf("</ul>"));
      expect(hint).not.toContain("·");
      expect(hint.match(/<li/g)).toHaveLength((nav === "fly" ? FLY_KEYS : WALK_KEYS).length);
      expect(hint).toContain("<kbd>Esc</kbd><kbd>Esc</kbd> orbit");
    }
  });

  it("names Shift the way the shortcut list does", () => {
    const shift = SHIFT.replace(/\+$/, "");
    expect(WALK_KEYS.find(([, what]) => what === "run")?.[0]).toEqual([shift]);
    expect(render("walk")).toContain(`<kbd>${shift}</kbd> run`);
  });

  it("uses the app's tooltips on the overlay's buttons and minimap", () => {
    const html = render("walk");
    expect(html).not.toMatch(/ title=/);
    expect(html).toContain('data-tip="Click a spot to go there"');
    expect(html).toContain('aria-label="Walk settings"');
  });
});
