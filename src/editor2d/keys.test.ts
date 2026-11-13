import { describe, expect, it } from "vitest";
import { isTypingTarget, planKeyBlocked, selectionKey } from "./keys";

const el = (tagName: string, o: { editable?: boolean; inside?: string } = {}) => ({
  tagName,
  isContentEditable: !!o.editable,
  closest: (sel: string) => (o.inside && sel.includes(o.inside) ? {} : null),
});
const gate = { defaultPrevented: false, typing: false, modal: false, following: false };

describe("which keys the plan leaves alone", () => {
  it("counts every form control and editable text as typing", () => {
    expect(isTypingTarget(el("INPUT") as unknown as EventTarget)).toBe(true);
    expect(isTypingTarget(el("SELECT") as unknown as EventTarget)).toBe(true);
    expect(isTypingTarget(el("TEXTAREA") as unknown as EventTarget)).toBe(true);
    expect(isTypingTarget(el("DIV", { editable: true }) as unknown as EventTarget)).toBe(true);
    // A custom slider, such as the render compare handle.
    expect(isTypingTarget(el("DIV", { inside: 'role="slider"' }) as unknown as EventTarget)).toBe(true);
    expect(isTypingTarget(el("BODY") as unknown as EventTarget)).toBe(false);
    expect(isTypingTarget(null)).toBe(false);
  });

  it("leaves keys to a field, an open dialog or another handler", () => {
    expect(planKeyBlocked("PageUp", gate)).toBe(false);
    expect(planKeyBlocked("PageUp", { ...gate, typing: true })).toBe(true);
    expect(planKeyBlocked("PageDown", { ...gate, modal: true })).toBe(true);
    expect(planKeyBlocked("Escape", { ...gate, modal: true })).toBe(true);
    expect(planKeyBlocked("Escape", { ...gate, defaultPrevented: true })).toBe(true);
  });

  it("lets Escape stop following first (DECISIONS D37)", () => {
    expect(planKeyBlocked("Escape", { ...gate, following: true })).toBe(true);
    expect(planKeyBlocked("PageUp", { ...gate, following: true })).toBe(false);
  });
});

describe("keys on a selected run", () => {
  const over = { pointerInside: true, picked: false };

  it("page keys move it 100 mm, 10 with Shift, only with the pointer over the plan", () => {
    expect(selectionKey("PageUp", { shift: false, alt: false }, over)).toEqual({ kind: "step", dz: 100 });
    expect(selectionKey("PageDown", { shift: true, alt: false }, over)).toEqual({ kind: "step", dz: -10 });
    expect(selectionKey("PageUp", { shift: false, alt: false }, { ...over, pointerInside: false })).toBeNull();
    expect(selectionKey("PageUp", { shift: false, alt: true }, over)).toBeNull();
  });

  it("h types a height over the plan, and is the Pan tool elsewhere", () => {
    expect(selectionKey("h", { shift: false, alt: false }, over)).toEqual({ kind: "height" });
    expect(selectionKey("h", { shift: false, alt: false }, { ...over, pointerInside: false })).toBeNull();
  });

  it("Escape steps out of a pick, and leaves the selection to the shell", () => {
    expect(selectionKey("Escape", { shift: false, alt: false }, { ...over, picked: true })).toEqual({ kind: "unpick" });
    expect(selectionKey("Escape", { shift: false, alt: false }, over)).toBeNull();
  });
});
