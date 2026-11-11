// The rail takes Esc for an open flyout only where the flyout is the thing
// in front: the flyout, the rail, what opened it, or the plan with nothing
// typed in. The copilot's composer, a popover or a field keep their Esc.
import { describe, expect, it } from "vitest";
import { flyoutOwnsEscape } from "./ToolRail";

/** A stand-in element: its tag, and the ancestors `closest` can find. */
function el(tagName: string, within: string[] = [], editable = false) {
  return { tagName, isContentEditable: editable, closest: (sel: string) => (within.includes(sel) ? {} : null) };
}
const holds = (...kids: object[]) => ({ contains: (n: unknown) => kids.includes(n as object) });

describe("flyoutOwnsEscape", () => {
  const railButton = el("BUTTON");
  const flyoutSearch = el("INPUT");
  const rail = holds(railButton, flyoutSearch);
  const statusButton = el("BUTTON");
  const opener = holds(statusButton);

  it("takes Esc when nothing has focus", () => {
    expect(flyoutOwnsEscape(null, rail, null)).toBe(true);
    expect(flyoutOwnsEscape(el("BODY") as unknown as EventTarget, rail, null)).toBe(true);
  });

  it("takes Esc from the flyout, the rail and what opened it", () => {
    expect(flyoutOwnsEscape(flyoutSearch as unknown as EventTarget, rail, null)).toBe(true);
    expect(flyoutOwnsEscape(railButton as unknown as EventTarget, rail, null)).toBe(true);
    expect(flyoutOwnsEscape(statusButton as unknown as EventTarget, rail, opener)).toBe(true);
  });

  it("takes Esc on the plan, but not from a field typed into there", () => {
    expect(flyoutOwnsEscape(el("CANVAS", ["[data-plan-area]"]) as unknown as EventTarget, rail, null)).toBe(true);
    expect(flyoutOwnsEscape(el("INPUT", ["[data-plan-area]"]) as unknown as EventTarget, rail, null)).toBe(false);
  });

  it("leaves Esc to the copilot's composer, its popovers and other fields", () => {
    expect(flyoutOwnsEscape(el("TEXTAREA") as unknown as EventTarget, rail, null)).toBe(false);
    expect(flyoutOwnsEscape(el("BUTTON") as unknown as EventTarget, rail, null)).toBe(false);
    expect(flyoutOwnsEscape(el("DIV", [], true) as unknown as EventTarget, rail, null)).toBe(false);
  });
});
