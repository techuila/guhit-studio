import { describe, expect, it } from "vitest";
import type { ToolDef } from "../shell/actions";
import { LANE_TOOLS, laneTool, withLaneTools } from "./laneTools";

const def = (tool: string, group: ToolDef["group"]): ToolDef =>
  ({ tool, label: tool, phrase: tool, icon: "select", key: null, group, keywords: "" }) as ToolDef;

describe("lane tools (Wave 2 hooks)", () => {
  it("are all off until their lane ships them, and answer for their own tool only", () => {
    expect(LANE_TOOLS.map((t) => t.tool)).toEqual(["grid", "dimension_string", "view_mark", "lot", "roof"]);
    expect(LANE_TOOLS.every((t) => t.rail.tool === t.tool)).toBe(true);
    expect(LANE_TOOLS.filter((t) => t.enabled).map((t) => t.tool)).toEqual(expect.arrayContaining(["grid", "dimension_string", "view_mark", "lot"]));
    expect(laneTool("roof")?.tool).toBe("roof");
    expect(laneTool("wall")).toBeNull();
    expect(laneTool("stair")).toBeNull();
  });

  it("do nothing as stubs", () => {
    for (const t of LANE_TOOLS.filter((x) => !x.enabled)) {
      expect(t.hint(null as never)).toBeNull();
      expect(t.key(null as never, { key: "Enter" } as KeyboardEvent)).toBe(false);
    }
  });

  it("sit after the last tool of their group on the rail", () => {
    const base = [def("select", "pick"), def("wall", "draw"), def("slab", "draw"), def("door", "place"), def("dimension", "annotate"), def("pan", "view")];
    const out = withLaneTools(base, [def("roof", "draw"), def("grid", "annotate")]);
    expect(out.map((t) => t.tool)).toEqual(["select", "wall", "slab", "roof", "door", "dimension", "grid", "pan"]);
    expect(withLaneTools(base, []).map((t) => t.tool)).toEqual(base.map((t) => t.tool));
  });
});
