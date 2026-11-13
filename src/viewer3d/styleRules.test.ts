// House style for the 3D view, Visuals, sheets, views and tours (DECISIONS
// D58, docs/MOTION.md): the AI-slop patterns removed in the UI cleanup stay out.
import { describe, expect, it } from "vitest";

// Vitest stubs CSS imports, so the files are read from disk. The test runs in
// Node; `getBuiltinModule` keeps Node types out of the app's tsconfig.
interface Fs {
  readFileSync(path: string, encoding: "utf8"): string;
  readdirSync(path: string, options: { recursive: true }): string[];
}
const fs = (globalThis as unknown as { process: { getBuiltinModule(id: "node:fs"): Fs } }).process.getBuiltinModule("node:fs");

const here = new URL(".", import.meta.url).pathname; // src/viewer3d/
const dirs = [here, new URL("../shell/sheets/", import.meta.url).pathname, new URL("../shell/views/", import.meta.url).pathname, new URL("../shell/tours/", import.meta.url).pathname];
const sheets: Record<string, string> = Object.fromEntries(
  dirs.flatMap((dir) =>
    fs
      .readdirSync(dir, { recursive: true })
      .filter((p) => p.endsWith(".css") && !p.startsWith("live/"))
      .map((p) => [`${dir.replace(/.*\/src\//, "src/")}${p}`, fs.readFileSync(dir + p, "utf8")]),
  ),
);

/** Each declaration with the file and line it is on, comments blanked out. */
function declarations(): Array<{ where: string; text: string }> {
  return Object.entries(sheets).flatMap(([file, css]) => {
    const lines = css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " ")).split("\n");
    return lines.map((text, i) => ({ where: `${file}:${i + 1}`, text: text.trim() })).filter((d) => d.text !== "");
  });
}

const offenders = (test: (text: string) => boolean) => declarations().filter((d) => test(d.text)).map((d) => `${d.where}  ${d.text}`);

describe("3D, Visuals, sheets, views and tours styles", () => {
  it("covers every module", () => {
    expect(Object.keys(sheets).length).toBeGreaterThanOrEqual(10);
    expect(Object.keys(sheets).some((f) => f.includes("/live/"))).toBe(false);
  });

  it("has no frosted glass (V24)", () => {
    expect(offenders((t) => t.startsWith("backdrop-filter"))).toEqual([]);
  });

  it("has no coloured left border (the user's hard rule)", () => {
    expect(offenders((t) => /^border-left(-color)?:/.test(t) && !/var\(--line(-strong)?\)/.test(t))).toEqual([]);
  });

  it("has no tracked all-caps labels (V18): paper titles are drawn, not styled", () => {
    expect(offenders((t) => /text-transform:\s*uppercase/.test(t))).toEqual([]);
  });

  it("keeps mono for typed values only (V29)", () => {
    expect(offenders((t) => /font(-family)?:.*--font-mono/.test(t)).filter((o) => !o.includes("pipeHandle.module.css"))).toEqual([]);
  });

  it("has no type under the 11 px floor (V10)", () => {
    expect(offenders((t) => /^font(-size)?:[^;]*\b([0-9]|10)(\.\d+)?px/.test(t))).toEqual([]);
  });

  it("does not stagger lists (M8) or bounce what appears (M9)", () => {
    expect(offenders((t) => t.startsWith("animation-delay"))).toEqual([]);
    expect(offenders((t) => t.startsWith("animation:") && t.includes("--ease-spring"))).toEqual([]);
  });

  it("loops only for indeterminate progress, timed by --dur-loop (M12)", () => {
    expect(offenders((t) => t.startsWith("animation:") && t.includes("infinite") && !t.includes("--dur-loop"))).toEqual([]);
  });
});
