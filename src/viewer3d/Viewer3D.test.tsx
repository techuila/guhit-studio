// The 3D toolbar: styled tooltips that also show on keyboard focus, names that
// survive the labels hiding in a narrow pane, key names through SHIFT, and one
// tab stop per radio group with the arrow keys (U22, U32, U36).
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SHIFT } from "../shell/actions";
import { Viewer3D } from "./Viewer3D";

const html = renderToStaticMarkup(<Viewer3D />);
const toolbar = html.slice(html.indexOf('role="toolbar"'));
const group = (testId: string) => {
  const at = html.indexOf(`data-testid="${testId}-group"`);
  return html.slice(html.lastIndexOf("<div", at), html.indexOf("</div>", at));
};
const tag = (testId: string) => {
  const at = html.indexOf(`data-testid="${testId}"`);
  return html.slice(html.lastIndexOf("<button", at), html.indexOf(">", at) + 1);
};

describe("the 3D toolbar", () => {
  it("uses the app's tooltips, never native titles", () => {
    expect(toolbar).not.toMatch(/<button[^>]* title=/);
    expect(tag("preset-exterior_corner")).toContain('data-tip="Exterior corner view"');
    expect(tag("toggle-roof")).toContain('data-tip="Show or hide the roof"');
    expect(tag("sun-control")).toContain("data-tip=");
  });

  it("names every button even when its label hides (container under 640 px)", () => {
    expect(tag("preset-top")).toContain('aria-label="Top"');
    expect(tag("preset-fit")).toContain('aria-label="Fit"');
    expect(tag("nav-walk")).toContain('aria-label="Walk"');
    expect(tag("shell-xray")).toContain('aria-label="X-ray"');
    expect(tag("save-view")).toContain('aria-label="Save view"');
    expect(tag("render-button")).toContain('aria-label="Render"');
    expect(tag("sun-control")).toMatch(/aria-label="Sun and light, \d/);
  });

  it("spells keys through SHIFT", () => {
    expect(tag("nav-walk")).toContain(`(${SHIFT}W)`);
    expect(html).toContain(`${SHIFT}W to walk`);
    expect(html).not.toContain("Shift+W");
  });

  it("gives each radio group one tab stop: the checked option", () => {
    for (const id of ["nav", "shell"]) {
      const g = group(id);
      expect(g).toContain('role="radiogroup"');
      expect(g.match(/tabindex="0"/g)).toHaveLength(1);
      expect(g.match(/tabindex="-1"/g)).toHaveLength(2);
      expect(g).toMatch(/aria-checked="true" aria-label="[^"]+" tabindex="0"/);
    }
  });
});
