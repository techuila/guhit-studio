import { describe, expect, it } from "vitest";
import {
  emptyTyped,
  formatArea,
  formatLength,
  isTypedKey,
  parsePolar,
  parseSize,
  typedKey,
  typedValues,
  type TypedState,
} from "./typed";

function type(mode: "polar" | "rect", keys: string[]): TypedState | null {
  let t: TypedState | null = emptyTyped(mode);
  for (const k of keys) {
    if (!t) t = emptyTyped(mode);
    t = typedKey(t, k);
  }
  return t;
}

describe("typed input", () => {
  it("collects a length", () => {
    const t = type("polar", ["3", "0", "0", "0"])!;
    expect(typedValues(t, "mm")).toEqual({ a: 3000, b: null });
  });

  it("switches to the angle on <", () => {
    const t = type("polar", [..."3000<45"])!;
    expect(t.active).toBe(1);
    expect(typedValues(t, "mm")).toEqual({ a: 3000, b: 45 });
  });

  it("switches to depth on a comma", () => {
    const t = type("rect", [..."4000,3000"])!;
    expect(typedValues(t, "mm")).toEqual({ a: 4000, b: 3000 });
  });

  it("tab toggles the field and backspace edits then closes", () => {
    let t = type("polar", ["1", "2", "Tab", "9", "0"])!;
    expect(t.fields).toEqual(["12", "90"]);
    t = typedKey(t, "Backspace")!;
    t = typedKey(t, "Backspace")!;
    expect(t.fields).toEqual(["12", ""]);
    t = typedKey(t, "Backspace")!;
    expect(t.active).toBe(0);
    t = typedKey(t, "Backspace")!;
    expect(t.fields).toEqual(["1", ""]);
    expect(typedKey(t, "Backspace")).toBeNull();
  });

  it("uses the display unit", () => {
    const t = type("polar", [..."3.5"])!;
    expect(typedValues(t, "m").a).toBeCloseTo(3500);
    expect(isTypedKey("7")).toBe(true);
    expect(isTypedKey("a")).toBe(false);
  });

  it("parses text forms", () => {
    expect(parsePolar("3000<45", "mm")).toEqual({ length: 3000, angle: 45 });
    expect(parsePolar("3000", "mm")).toEqual({ length: 3000, angle: null });
    expect(parsePolar("abc", "mm")).toBeNull();
    expect(parseSize("4000,3000", "mm")).toEqual({ width: 4000, depth: 3000 });
    expect(parseSize("4 x 3", "m")).toEqual({ width: 4000, depth: 3000 });
    expect(parseSize("4000", "mm")).toBeNull();
  });

  it("formats", () => {
    expect(formatLength(2850.4, "mm")).toBe("2850");
    expect(formatLength(3000, "m")).toBe("3.00");
    expect(formatLength(2855, "m")).toBe("2.855");
    expect(formatArea(28518750)).toBe("28.52 m²");
  });
});
