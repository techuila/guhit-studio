// Every plan tool that lives in a lane's own file (planTool.ts says what
// one is). The controller dispatches to `laneTool(tool)`; the rail and the
// palette list the enabled ones. Frozen with the hubs after the contract
// step: a lane turns its tool on in its own module (`enabled`).

import type { ToolDef } from "../shell/actions";
import type { Tool } from "../state/store";
import { dimStringTool } from "./permit/dimStringTool";
import { gridTool } from "./permit/gridTool";
import { lotTool } from "./permit/lotTool";
import { viewMarkTool } from "./permit/viewMarkTool";
import type { PlanTool } from "./planTool";
import { roofTool } from "./roofTool";

/** Lane L1's permit tools, then lane L2's roof tool. */
export const LANE_TOOLS: readonly PlanTool[] = [gridTool, dimStringTool, viewMarkTool, lotTool, roofTool];

/** The lane tool for `tool`, or null for the controller's own tools. */
export function laneTool(tool: Tool): PlanTool | null {
  return LANE_TOOLS.find((t) => t.tool === tool) ?? null;
}

/** `extra` tools each placed after the last tool of their group (at the end when the group is new). */
export function withLaneTools(base: readonly ToolDef[], extra: readonly ToolDef[]): ToolDef[] {
  const out = [...base];
  for (const t of extra) {
    let at = out.length;
    for (let i = out.length - 1; i >= 0; i--) {
      if (out[i].group === t.group) {
        at = i + 1;
        break;
      }
    }
    out.splice(at, 0, t);
  }
  return out;
}
