// The MOD shortcuts and the palette rows that show them (docs/SHORTCUTS.md):
// MOD+, opens Settings as in every Mac app, the copilot moved to MOD+J, and
// Shift+MOD+S still opens Export.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { CommandPalette } from "./CommandPalette";
import { MOD, nextVersionLabel, paletteActions } from "./actions";
import { modShortcut } from "./shortcuts";

describe("MOD shortcuts", () => {
  it("MOD+, opens Settings and MOD+J asks the copilot", () => {
    expect(modShortcut(",", "Comma", false, false)).toBe("settings");
    expect(modShortcut("j", "KeyJ", false, false)).toBe("copilot");
  });

  it("keeps the other bindings", () => {
    expect(modShortcut("s", "KeyS", false, false)).toBe("version");
    expect(modShortcut("s", "KeyS", true, false)).toBe("export");
    expect(modShortcut("e", "KeyE", false, false)).toBe("export");
    expect(modShortcut("z", "KeyZ", false, false)).toBe("undo");
    expect(modShortcut("z", "KeyZ", true, false)).toBe("redo");
    expect(modShortcut("y", "KeyY", false, false)).toBe("redo");
    expect(modShortcut("0", "Digit0", false, false)).toBe("fit");
    expect(modShortcut("=", "Equal", false, false)).toBe("zoom-in");
    expect(modShortcut("-", "Minus", false, false)).toBe("zoom-out");
  });

  it("matches MOD+Alt+R by the physical key (Option+R is a symbol on macOS)", () => {
    expect(modShortcut("®", "KeyR", false, true)).toBe("render");
    expect(modShortcut("r", "KeyR", false, false)).toBeNull();
  });

  it("leaves unbound keys to the browser", () => {
    expect(modShortcut("q", "KeyQ", false, false)).toBeNull();
    expect(modShortcut("k", "KeyK", false, false)).toBeNull();
  });
});

describe("palette rows", () => {
  const byId = (id: string) => paletteActions().find((a) => a.id === id);

  it("Settings shows MOD+, and Ask the copilot shows MOD+J", () => {
    expect(byId("settings")?.shortcut).toBe(`${MOD},`);
    expect(byId("dock-copilot")?.shortcut).toBe(`${MOD}J`);
  });

  it("calls the project file a .guhit file, not a bundle", () => {
    expect(byId("import-bundle")?.title).toBe("Open .guhit file");
    expect(paletteActions().some((a) => /\bbundle\b/i.test(a.title))).toBe(false);
  });

  it("the input is a combobox that points at the highlighted command", () => {
    const html = renderToStaticMarkup(<CommandPalette onClose={() => undefined} />);
    expect(html).toContain('role="combobox"');
    expect(html).toContain('aria-controls="palette-list"');
    expect(html).toContain('aria-activedescendant="palette-cmd-0"');
    expect(html).toContain('id="palette-list"');
    expect(html).toContain('id="palette-cmd-0"');
  });
});

describe("version names", () => {
  it("numbers versions the same in the panel and on MOD+S", () => {
    expect(nextVersionLabel(0)).toBe("Version 1");
    expect(nextVersionLabel(2)).toBe("Version 3");
  });
});
