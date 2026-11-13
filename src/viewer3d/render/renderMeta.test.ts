// How a capture is named in the Source picker (G21, C26): the view and a
// short time, never "rev N".
import { describe, expect, it } from "vitest";
import { captureLabel } from "./renderMeta";

describe("capture names in the Source picker", () => {
  const now = new Date(2026, 10, 11, 9, 0);
  const at = (d: Date) => d.toISOString();

  it("names a capture from today by its view and the time alone", () => {
    const label = captureLabel("Exterior corner", at(new Date(2026, 10, 11, 5, 21)), now);
    expect(label.startsWith("Exterior corner, ")).toBe(true);
    expect(label).toMatch(/5:21/);
    expect(label).not.toMatch(/Nov|rev/);
  });

  it("adds the date for another day, and names an unnamed view", () => {
    const label = captureLabel("", at(new Date(2026, 10, 9, 17, 5)), now);
    expect(label.startsWith("View, ")).toBe(true);
    expect(label).toMatch(/9/);
    expect(label).toMatch(/5:05/);
  });

  it("falls back to the view alone for a bad time", () => {
    expect(captureLabel("Top", "not a time", now)).toBe("Top");
  });
});
