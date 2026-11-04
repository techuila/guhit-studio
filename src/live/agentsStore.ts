// Agents in the window (DECISIONS D42, D47): what the UI shows of the agents
// working on the project, with or without a live session. Filled by
// agentsWiring.ts from the `agents` app event, `agents_list` and the live
// store (other computers' agents in a session). Pure parts in agents.ts.
//
// `views` changes about once per agent call (seconds apart), so components
// may select from it freely; still select one agent or one field where you can.

import { create } from "zustand";
import type { AgentSummary } from "../contract/bindings";
import type { AgentView, UndoLine } from "./agents";

export type AgentsTab = "agents" | "changes";

/** What an agent just changed, tinted on the plan and in 3D in its color while it fades. */
export interface Trail {
  ids: string[];
  /** 1 when fresh, down to 0 as it fades. */
  strength: number;
  token: number;
}

export interface AgentsState {
  /** This computer's agents, newest call first, as the app sends them. */
  local: AgentSummary[];
  /** The app answers `agents_list`. False against an older engine: the UI shows no agents. */
  available: boolean;
  /** Every agent to show: this computer's, then other computers' in a live session. */
  views: AgentView[];
  /**
   * Every agent seen while the window runs, by participant id, kept after
   * it goes, so History and the review still name and color its steps.
   */
  known: Record<string, { name: string; color: number }>;
  /** What each agent just changed. */
  trails: Record<string, Trail>;
  /** The Agents panel and its tab, null when closed. */
  panel: AgentsTab | null;
  /**
   * "Show its steps": History opens filtered to this agent. The token makes
   * the same request twice open it twice.
   */
  historyRequest: { filter: string; token: number } | null;
  /**
   * The review's "Undo to here", asked before it acts: every author whose
   * steps it takes back. `revision` is the one the steps were counted at.
   */
  undoAsk: { steps: number; revision: number | null; lines: UndoLine[] } | null;

  setLocal: (local: AgentSummary[]) => void;
  setAvailable: (on: boolean) => void;
  setViews: (views: AgentView[]) => void;
  setTrail: (id: string, trail: Trail | null) => void;
  openPanel: (tab?: AgentsTab) => void;
  closePanel: () => void;
  requestHistory: (filter: string) => void;
  askUndo: (ask: AgentsState["undoAsk"]) => void;
}

let seq = 1;

export const useAgents = create<AgentsState>((set, get) => ({
  local: [],
  available: false,
  views: [],
  known: {},
  trails: {},
  panel: null,
  historyRequest: null,
  undoAsk: null,

  setLocal: (local) => set({ local }),
  setAvailable: (available) => {
    if (get().available !== available) set({ available });
  },
  setViews: (views) =>
    set((s) => {
      let known = s.known;
      for (const a of views) {
        const k = known[a.id];
        if (k && k.name === a.name && k.color === a.color) continue;
        if (known === s.known) known = { ...s.known };
        known[a.id] = { name: a.name, color: a.color };
      }
      return { views, known };
    }),
  setTrail: (id, trail) =>
    set((s) => {
      if (!trail) {
        if (!(id in s.trails)) return s;
        const trails = { ...s.trails };
        delete trails[id];
        return { trails };
      }
      return { trails: { ...s.trails, [id]: trail } };
    }),
  openPanel: (tab) => set((s) => ({ panel: tab ?? s.panel ?? "agents" })),
  closePanel: () => set({ panel: null }),
  requestHistory: (filter) => set({ historyRequest: { filter, token: seq++ } }),
  askUndo: (undoAsk) => set({ undoAsk }),
}));

/** The ids of every agent known in this window, for telling agent steps apart. */
export function knownAgentIds(): Set<string> {
  return new Set(Object.keys(useAgents.getState().known));
}
