// Other people and agents in the 3D view (DECISIONS D37, D38, D47): whose
// pointer shows, and which elements are tinted in whose color. Reads the
// live store's shapes and the agents store's, never writes them. Pure:
// tested in peers.test.ts.

import type { LiveStatus } from "../../contract/bindings";
import type { AgentView } from "../../live/agents";
import { useAgents, type Trail } from "../../live/agentsStore";
import { peerIndex, type Peers } from "../../live/format";

/** Elements tinted in one color, as the engine takes them (`Highlights`). */
export interface Tint {
  ids: string[];
  /** A CSS color the engine can read: hex or rgb(). */
  color: string;
}

/** People whose pointer is on the model in 3D, in the host's order. Every level shows. Agents: `agentPointerIds`. */
export function pointerIds(status: LiveStatus, peers: Peers): string[] {
  if (status.mode === "off") return [];
  const out: string[] = [];
  for (const p of status.participants) {
    if (p.id === status.self_id || p.agent) continue;
    if (peers[p.id]?.presence.pointer_3d) out.push(p.id);
  }
  return out;
}

/** The agents' part of the tints: what each just changed (its trail), then what it works on. */
export interface AgentTints {
  views: readonly AgentView[];
  trails: Readonly<Record<string, Trail>>;
}

function currentAgents(): AgentTints {
  const st = useAgents.getState();
  return { views: st.views, trails: st.trails };
}

/**
 * Each other person's selection in their resolved color, in the host's
 * order, then each agent's trail and focus (with or without a session):
 * where two selected the same element, the first one's color shows.
 */
export function peerTints(status: LiveStatus, peers: Peers, colorOf: (index: number) => string, agents: AgentTints = currentAgents()): Tint[] {
  const out: Tint[] = [];
  if (status.mode !== "off") {
    for (const p of status.participants) {
      if (p.id === status.self_id || p.agent) continue;
      const ids = peers[p.id]?.presence.selection ?? [];
      if (ids.length > 0) out.push({ ids, color: colorOf(peerIndex(p.color)) });
    }
  }
  for (const a of agents.views) {
    const trail = agents.trails[a.id];
    if (trail && trail.ids.length > 0 && trail.strength > 0) out.push({ ids: trail.ids, color: colorOf(peerIndex(a.color)) });
  }
  for (const a of agents.views) {
    const ids = a.presence.selection;
    if (ids.length > 0 && a.activity !== "stopped") out.push({ ids, color: colorOf(peerIndex(a.color)) });
  }
  return out;
}

/** The History row under the pointer (`useApp.historyTint`): a participant color, or the accent for this computer's own steps. */
export function historyTintOf(
  tint: { ids: string[]; color: number | null } | null,
  colorOf: (index: number) => string,
  accent: string,
): Tint | null {
  if (!tint || tint.ids.length === 0) return null;
  return { ids: tint.ids, color: tint.color === null ? accent : colorOf(peerIndex(tint.color)) };
}

/** Same ids in the same colors: nothing to retint. */
export function sameTints(a: readonly Tint[], b: readonly Tint[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (!sameTint(a[i], b[i])) return false;
  return true;
}

export function sameTint(a: Tint | null, b: Tint | null): boolean {
  if (a === b) return true;
  if (!a || !b || a.color !== b.color || a.ids.length !== b.ids.length) return false;
  for (let i = 0; i < a.ids.length; i++) if (a.ids[i] !== b.ids[i]) return false;
  return true;
}
