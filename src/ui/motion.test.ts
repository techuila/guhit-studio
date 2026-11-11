// Motion contract (lane L0): the JS twins match the CSS tokens, and reduced
// motion leaves no loop running.
import { describe, expect, it } from "vitest";
import { DUR, LOOP_MS, SPRING_STOPS, TIP_DELAY_MS, ease } from "./motion";

// Read from disk: vitest hands CSS imports back empty. No `@types/node` in
// this project, so Node's fs comes in untyped (as in pipes.test.ts).
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: string): string };
const read = (rel: string) => fs.readFileSync(new URL(rel, import.meta.url), "utf8");
const tokens = read("../styles/tokens.css");
const rootBlock = tokens.slice(tokens.indexOf(":root {"), tokens.indexOf("@media (prefers-reduced-motion"));
const token = (name: string) => new RegExp(`${name}:\\s*([^;]+);`).exec(rootBlock)?.[1].trim();

describe("motion tokens", () => {
  it("DUR, LOOP_MS and TIP_DELAY_MS match tokens.css", () => {
    for (const [key, ms] of Object.entries(DUR)) expect(token(`--dur-${key}`)).toBe(`${ms}ms`);
    expect(token("--dur-loop")).toBe(`${LOOP_MS}ms`);
    expect(token("--tip-delay")).toBe(`${TIP_DELAY_MS}ms`);
  });

  it("keeps --dur-loop and --tip-delay out of the reduced-motion overrides", () => {
    const reduce = tokens.slice(tokens.indexOf("@media (prefers-reduced-motion"));
    const block = reduce.slice(0, reduce.indexOf("}\n}") + 3);
    expect(block).not.toContain("--dur-loop");
    expect(block).not.toContain("--tip-delay");
  });

  it("the JS spring follows the stops of --ease-spring exactly", () => {
    const css = token("--ease-spring") ?? "";
    const inner = /^linear\((.*)\)$/.exec(css)?.[1] ?? "";
    const parts = inner.split(",").map((p) => p.trim().split(/\s+/));
    const stops = parts.map(([y, x], i) => [x ? parseFloat(x) / 100 : i === 0 ? 0 : 1, parseFloat(y)]);
    expect(stops).toEqual(SPRING_STOPS.map(([x, y]) => [x, y]));
    for (const [x, y] of SPRING_STOPS) expect(ease.spring(x)).toBeCloseTo(y, 6);
  });

  it("the JS spring overshoots like the CSS one (6%), not more", () => {
    let peak = 0;
    for (let i = 0; i <= 1000; i++) peak = Math.max(peak, ease.spring(i / 1000));
    expect(peak).toBeCloseTo(1.06, 6);
    expect(ease.spring(0)).toBe(0);
    expect(ease.spring(1)).toBe(1);
    expect(ease.spring(-1)).toBe(0);
    expect(ease.spring(2)).toBe(1);
  });
});

describe("reduced motion", () => {
  const global = read("./global.css");
  const controls = read("./controls.module.css");

  it("runs every animation once, with no delay, so nothing loops or staggers", () => {
    const block = global.slice(global.lastIndexOf("@media (prefers-reduced-motion: reduce)"));
    expect(block).toContain("animation-duration: 1ms !important");
    expect(block).toContain("animation-iteration-count: 1 !important");
    expect(block).toContain("animation-delay: 0ms !important");
  });

  it("stops the spinner instead of strobing it", () => {
    const block = controls.slice(controls.lastIndexOf("@media (prefers-reduced-motion: reduce)"));
    expect(block).toMatch(/\.spinner\s*\{[^}]*animation:\s*none/);
  });

  it("times the spinner with --dur-loop", () => {
    expect(controls).toMatch(/\.spinner\s*\{\s*animation:\s*global\(ui-spin\)\s*var\(--dur-loop\)/);
  });
});
