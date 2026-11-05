// The plan tools that live in the lanes' own files (Wave 2, DECISIONS D54):
// what the plan controller lends such a tool and what it asks of one. The
// list of them is laneTools.ts. controller.ts dispatches to them one line
// per event, and the shell's tool rail and palette list the enabled ones
// (shell/actions.ts). Frozen with the hubs after the contract step: a lane
// changes its own tool module, never this file.

import type { Command } from "../contract/bindings";
import type { ToolDef } from "../shell/actions";
import type { Tool } from "../state/store";
import type { P } from "./geom";
import type { DocIndex } from "./model";
import type { RenderContext } from "./render";

/** What the plan controller lends a lane's tool. */
export interface PlanToolHost {
  /** The plan's index of the level on screen; null before the first draw. */
  index(): DocIndex | null;
  /** The level on screen. */
  levelId(): string | null;
  /**
   * Snaps a plan point as the drawing tools do (endpoints, faces, the grid,
   * angle locks from `from`, ortho and Shift) and shows the snap marker.
   */
  snap(world: P, from: P | null): P;
  /** Sends one command, one undo step. False when it was refused; the shell already said why. */
  run(command: Command): Promise<boolean>;
  /** Asks for a redraw on the plan's next frame. */
  invalidate(): void;
  /** Plan mm per CSS pixel at the current zoom. */
  pxMm(): number;
  /** True while the pointer is over the plan. A tool key without a draft acts only then, so Tab still moves focus elsewhere. */
  pointerInside(): boolean;
}

/** A plan tool in a lane's own file. The controller keeps no state for it. */
export interface PlanTool {
  tool: Tool;
  /** Off until the lane ships the tool: the rail and the palette leave it out. */
  enabled: boolean;
  /** Its rail button and palette entry. */
  rail: ToolDef;
  /** The status bar hint while it is active; null for none. */
  hint(host: PlanToolHost): string | null;
  /** The pointer moved over the plan. `world` is not snapped yet. */
  hover(host: PlanToolHost, world: P): void;
  /** A primary press. `presses` is 2 on the second press of a double click. */
  down(host: PlanToolHost, world: P, presses: number): void;
  /** A right click: finish what can be finished, else drop the draft. */
  finish(host: PlanToolHost): void;
  /** A key while it is active (Enter, Escape, Backspace, typed lengths). True when it used the key. */
  key(host: PlanToolHost, e: KeyboardEvent): boolean;
  /** Its ghost over the plan, drawn after the model. */
  draw(rc: RenderContext, host: PlanToolHost): void;
  /** Another tool was picked: drop any draft. */
  reset(): void;
}

/** A tool that does nothing yet: what each lane module starts from. */
export function stubTool(tool: Tool, rail: Omit<ToolDef, "tool">): PlanTool {
  return {
    tool,
    enabled: false,
    rail: { ...rail, tool },
    hint: () => null,
    hover: () => {},
    down: () => {},
    finish: () => {},
    key: () => false,
    draw: () => {},
    reset: () => {},
  };
}
