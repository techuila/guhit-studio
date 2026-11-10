// The swarm in the window (DECISIONS D56, D57): the contract types it uses,
// and the reveal's interface, which the 2D draft pass, the 3D draft group and
// the agent cursors all read. The reveal itself is reveal.ts (pure).

import type { Point } from "../contract/bindings";

export type {
  SwarmAcceptResult,
  SwarmApplied,
  SwarmConflict,
  SwarmCost,
  SwarmDraftCurrent,
  SwarmDraftReply,
  SwarmDraftState,
  SwarmFeedItem,
  SwarmPart,
  SwarmPartIds,
  SwarmPartState,
  SwarmPlan,
  SwarmRequest,
  SwarmSource,
  SwarmState,
  SwarmStatus,
  SwarmStep,
  SwarmWait,
} from "./contract";

/**
 * How an element comes in:
 * - `line`: drawn from `from` to `to` (a wall, a window across its opening);
 * - `swing`: a door leaf swings open from its hinge;
 * - `fill`: a room's fill fades in;
 * - `settle`: anything else appears in place, as when placed.
 */
export type RevealKind = "line" | "swing" | "fill" | "settle";

/** One element of a step, as the reveal paces it. Plan mm. */
export interface RevealItem {
  id: string;
  /** The part key. */
  part: string;
  kind: RevealKind;
  /** Where the cursor goes first and the drawing starts. */
  from: Point;
  /** Where the drawing ends and the cursor rests. Same as `from` for a fill or a settle. */
  to: Point;
  /** What the draw covers, mm: the draw time follows it. */
  lengthMm: number;
  /** Elements that must be fully shown first: an opening's wall, a room's bounding walls. */
  hosts: string[];
  /** The cursor's status line while it works on this. */
  label: string;
  /** The level it is drawn on; null on every level. */
  levelId: string | null;
  /** A removal: it shows as a red ghost when the cursor reaches it. */
  removed: boolean;
}

/** What a part's cursor does between two moments. */
export type SegmentKind = "glide" | "wait" | "draw" | "rest";

/** One stretch of a part's cursor. Times are `performance.now()` ms. */
export interface RevealSegment {
  part: string;
  kind: SegmentKind;
  /** The element it works on, null when it rests after its last one. */
  id: string | null;
  start: number;
  /** Infinity for the rest after the last element. */
  end: number;
  from: Point;
  to: Point;
  label: string;
  levelId: string | null;
}

/** Where a part's cursor is told to go, for a CSS glide. */
export interface CursorTarget {
  part: string;
  at: Point;
  /** Transition length, ms: 0 jumps. */
  ms: number;
  /** A glide eases in and out; a draw is linear. */
  easing: "glide" | "linear";
  label: string;
  levelId: string | null;
}
