// Agent cursors of swarm parts (DECISIONS D47, D56). While a part's cursor
// has reveal work, its agent's diamond follows the reveal instead of the
// agent's own presence: at each segment boundary it is given the segment's
// end point and a CSS transition as long as the segment (a glide eases in and
// out, a draw is linear), the same CSS glide agent cursors use. The CSS keeps
// which property moves, so a pan, zoom or orbit still moves it at once.

import type { DocState, Point, Vec3 } from "../contract/bindings";
import { onRevealTick, reveal, useSwarm } from "./swarmStore";
import type { CursorTarget, RevealSegment, SwarmStatus } from "./types";

/** How high over its level's floor a part's 3D marker sits, mm: about where hands work. */
export const WORK_HEIGHT_MM = 1200;

/** The part an agent draws for in the open swarm, or null. */
export function partOfAgent(status: SwarmStatus | null, agentId: string): string | null {
  return status?.parts.find((p) => p.agent_id === agentId)?.key ?? null;
}

/** Agents of parts whose cursor is on `levelId` (or on every level) at `now`. */
export function swarmCursorAgents(status: SwarmStatus | null, levelId: string | null, now: number): string[] {
  if (!status) return [];
  const out: string[] = [];
  for (const p of status.parts) {
    if (!p.agent_id) continue;
    const seg = reveal.segment(p.key, now);
    if (seg && (levelId === null || seg.levelId === null || seg.levelId === levelId)) out.push(p.agent_id);
  }
  return out;
}

/** Every agent bound to a part of the open swarm. */
export function swarmAgents(status: SwarmStatus | null): Set<string> {
  return new Set((status?.parts ?? []).flatMap((p) => (p.agent_id ? [p.agent_id] : [])));
}

/**
 * The cursors to show on a level: agents with a presence point there, except
 * swarm parts, which show where their reveal cursor is.
 */
export function cursorIdsWithSwarm(presence: readonly string[], status: SwarmStatus | null, levelId: string | null, now: number): string[] {
  const bound = swarmAgents(status);
  const out = presence.filter((id) => !bound.has(id));
  for (const id of swarmCursorAgents(status, levelId, now)) if (!out.includes(id)) out.push(id);
  return out;
}

/** The CSS for a target: the segment's length and easing. 0 ms jumps. */
export function transitionOf(t: CursorTarget): { duration: string; easing: string } {
  return { duration: `${Math.round(t.ms)}ms`, easing: t.easing === "glide" ? "var(--ease-in-out)" : "linear" };
}

/** A plan point of a part's cursor as a model point for the 3D view. */
export function pointIn3d(at: Point, levelId: string | null, doc: DocState | null): Vec3 {
  const levels = doc?.project.levels ?? [];
  const level = levels.find((l) => l.id === levelId) ?? [...levels].sort((a, b) => a.elevation_mm - b.elevation_mm)[0];
  return { x: Math.round(at.x), y: Math.round(at.y), z: Math.round((level?.elevation_mm ?? 0) + WORK_HEIGHT_MM) };
}

function sameSegment(a: RevealSegment | null, b: RevealSegment | null): boolean {
  if (a === null || b === null) return a === b;
  return a.part === b.part && a.kind === b.kind && a.start === b.start && a.end === b.end && a.id === b.id;
}

/**
 * Follows the reveal cursor of `agentId`'s part: `apply` gets each new
 * target (now, and at every boundary), and null when the agent has no part
 * or its part's cursor has nothing to show, so its own presence takes over.
 * Returns the unsubscribe.
 */
export function followSwarmCursor(agentId: string, apply: (target: CursorTarget | null) => void): () => void {
  let last: RevealSegment | null | undefined;
  const run = (now: number) => {
    const part = partOfAgent(useSwarm.getState().status, agentId);
    const seg = part ? reveal.segment(part, now) : null;
    if (last !== undefined && sameSegment(last, seg)) return;
    last = seg;
    apply(part && seg ? reveal.target(part, now) : null);
  };
  run(performance.now());
  const offTick = onRevealTick(run);
  const offStore = useSwarm.subscribe((s, p) => {
    if (s.status !== p.status) run(performance.now());
  });
  return () => {
    offTick();
    offStore();
  };
}
