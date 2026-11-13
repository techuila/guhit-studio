// Shared controls (lane L0): roving focus for radio groups and menus, and a
// destructive confirm that starts on Cancel.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Segmented, rovingNext, rovingNextEnabled, rovingTabIndex } from "./controls";
import { ConfirmDialog, Menu, modalToastArea } from "./Dialog";

describe("rovingNext", () => {
  it("moves with the arrows and wraps at both ends", () => {
    expect(rovingNext("ArrowRight", 0, 3)).toBe(1);
    expect(rovingNext("ArrowDown", 2, 3)).toBe(0);
    expect(rovingNext("ArrowLeft", 0, 3)).toBe(2);
    expect(rovingNext("ArrowUp", 1, 3)).toBe(0);
  });

  it("goes to the ends with Home and End", () => {
    expect(rovingNext("Home", 2, 4)).toBe(0);
    expect(rovingNext("End", 0, 4)).toBe(3);
  });

  it("only answers the arrows of its orientation", () => {
    expect(rovingNext("ArrowRight", 0, 3, "vertical")).toBeNull();
    expect(rovingNext("ArrowDown", 0, 3, "vertical")).toBe(1);
    expect(rovingNext("ArrowDown", 0, 3, "horizontal")).toBeNull();
    expect(rovingNext("ArrowRight", 0, 3, "horizontal")).toBe(1);
  });

  it("ignores other keys and empty groups", () => {
    expect(rovingNext("a", 0, 3)).toBeNull();
    expect(rovingNext("Enter", 0, 3)).toBeNull();
    expect(rovingNext("ArrowRight", 0, 0)).toBeNull();
  });

  it("starts from an end when nothing is active yet", () => {
    expect(rovingNext("ArrowRight", -1, 3)).toBe(0);
    expect(rovingNext("ArrowLeft", -1, 3)).toBe(2);
  });

  it("gives the tab stop to the active item, or the first when none is", () => {
    expect([0, 1, 2].map((i) => rovingTabIndex(i, 1))).toEqual([-1, 0, -1]);
    expect([0, 1, 2].map((i) => rovingTabIndex(i, -1))).toEqual([0, -1, -1]);
  });
});

describe("rovingNextEnabled (a group with a disabled item)", () => {
  // Four options, the second one disabled: indices stay those of the full list.
  const enabled = [true, false, true, true];

  it("skips the disabled item and answers with full-list indices", () => {
    expect(rovingNextEnabled("ArrowRight", 0, enabled)).toBe(2);
    expect(rovingNextEnabled("ArrowLeft", 2, enabled)).toBe(0);
    expect(rovingNextEnabled("ArrowRight", 3, enabled)).toBe(0);
    expect(rovingNextEnabled("ArrowLeft", 0, enabled)).toBe(3);
  });

  it("goes to the enabled ends with Home and End", () => {
    expect(rovingNextEnabled("Home", 3, [false, true, true, false])).toBe(1);
    expect(rovingNextEnabled("End", 1, [false, true, true, false])).toBe(2);
  });

  it("moves from a disabled item to the nearest enabled one that way", () => {
    expect(rovingNextEnabled("ArrowRight", 1, enabled)).toBe(2);
    expect(rovingNextEnabled("ArrowLeft", 1, enabled)).toBe(0);
  });

  it("does nothing when every item is disabled, or for other keys", () => {
    expect(rovingNextEnabled("ArrowRight", 0, [false, false])).toBeNull();
    expect(rovingNextEnabled("a", 0, enabled)).toBeNull();
  });

  it("keeps the tab stop off disabled items", () => {
    const off = (i: number) => !enabled[i];
    expect([0, 1, 2, 3].map((i) => rovingTabIndex(i, 2, off))).toEqual([-1, -1, 0, -1]);
    // The active item is disabled: the first enabled one takes the stop.
    expect([0, 1, 2, 3].map((i) => rovingTabIndex(i, 1, off))).toEqual([0, -1, -1, -1]);
    expect([0, 1, 2, 3].map((i) => rovingTabIndex(i, -1, (i) => i < 2))).toEqual([-1, -1, 0, -1]);
  });
});

describe("toasts over a modal", () => {
  const view = { width: 1600, height: 1000 };

  it("sit inside the modal, above its footer", () => {
    const panel = { offsetLeft: 32, offsetTop: 50, offsetWidth: 1536, offsetHeight: 900 };
    expect(modalToastArea(panel, 840, view)).toEqual({ left: 32, right: 32, bottom: 1000 - 890 + 12 });
  });

  it("sit above the bottom edge of a modal with no footer", () => {
    const panel = { offsetLeft: 580, offsetTop: 300, offsetWidth: 440, offsetHeight: 400 };
    expect(modalToastArea(panel, null, view)).toEqual({ left: 580, right: 580, bottom: 1000 - 700 + 12 });
  });
});

describe("Segmented", () => {
  it("is one tab stop: only the checked option is reachable with Tab", () => {
    const html = renderToStaticMarkup(
      <Segmented
        label="View"
        value="split"
        onChange={() => {}}
        options={[
          { value: "plan", label: "Plan" },
          { value: "split", label: "Split" },
          { value: "3d", label: "3D" },
        ]}
      />,
    );
    const radios = html.match(/<button[^>]*role="radio"[^>]*>/g) ?? [];
    expect(radios).toHaveLength(3);
    expect(radios.map((r) => /tabindex="(-?\d)"/.exec(r)?.[1])).toEqual(["-1", "0", "-1"]);
    expect(radios[1]).toContain('aria-checked="true"');
    expect(radios.every((r) => r.includes("data-roving"))).toBe(true);
  });
});

describe("ConfirmDialog", () => {
  const autofocused = (html: string) => /<button[^>]*data-autofocus[^>]*>(?:<span>)?([^<]*)/.exec(html)?.[1];

  it("starts on Cancel when the action is destructive", () => {
    const html = renderToStaticMarkup(
      <ConfirmDialog title="End the session?" message="Everyone leaves." confirmLabel="End and go back" danger onConfirm={() => {}} onCancel={() => {}} />,
    );
    expect(autofocused(html)).toBe("Cancel");
    expect(html.match(/data-autofocus/g)).toHaveLength(1);
  });

  it("starts on the confirm button otherwise", () => {
    const html = renderToStaticMarkup(
      <ConfirmDialog title="Open the file?" message="The project opens." confirmLabel="Open the file" onConfirm={() => {}} onCancel={() => {}} />,
    );
    expect(autofocused(html)).toBe("Open the file");
    expect(html.match(/data-autofocus/g)).toHaveLength(1);
  });
});

describe("Menu", () => {
  it("gives the highlighted item the only tab stop, so focus can rest on it", () => {
    const html = renderToStaticMarkup(
      <Menu
        onClose={() => {}}
        items={[
          { key: "rename", label: "Rename", onSelect: () => {} },
          { key: "trash", label: "Move to trash", danger: true, onSelect: () => {} },
        ]}
      />,
    );
    const items = html.match(/<button[^>]*role="menuitem"[^>]*>/g) ?? [];
    expect(items.map((r) => /tabindex="(-?\d)"/.exec(r)?.[1])).toEqual(["0", "-1"]);
  });
});
