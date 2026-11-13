// Connects the agents store to the backend (DECISIONS D47). `startAgents`
// runs once from LiveRoot's effect, next to `startLive`, and returns its
// cleanup:
//   - the `agents` app event and `agents_list` (at start, and when a live
//     session starts or ends) fill this computer's agents,
//   - other computers' agents come from the live store (participants with
//     `agent` set and their presence),
//   - when an agent makes a step, what it changed becomes its trail, tinted
//     on the plan and in 3D in its color and fading (`TRAIL_*`),
//   - a slow tick takes quiet agents off the bar after 10 minutes.

import type { AgentSummary, HistoryStep } from "../contract/bindings";
import { ipc, onAppEvent } from "../contract/ipc";
import { useApp } from "../state/store";
import { agentViews, inFirstSeenOrder, newAgentSteps, sameViews, stepKey } from "./agents";
import { knownAgentIds, useAgents } from "./agentsStore";
import { sessionKey } from "./wiring";
import { useLive } from "./liveStore";

/** A trail holds full strength this long, then fades in two steps. */
export const TRAIL_HOLD_MS = 2400;
export const TRAIL_STEP_MS = 520;
const TRAIL_FADE = [0.55, 0.25];

/** How often the bar re-checks who has gone quiet. */
const TICK_MS = 30_000;

/** Several agent steps in a burst read the history once. */
const HISTORY_DEBOUNCE_MS = 120;

export function startAgents(): () => void {
  let stopped = false;
  const stops: Array<() => void> = [];
  const timers = new Set<number>();
  const later = (fn: () => void, ms: number): number => {
    const id = window.setTimeout(() => {
      timers.delete(id);
      fn();
    }, ms);
    timers.add(id);
    return id;
  };

  // ------------------------------------------------ the list

  /** First-seen order: nothing trades places when an agent calls. */
  const order = new Map<string, number>();
  const recompute = () => {
    const live = useLive.getState();
    const st = useAgents.getState();
    const next = inFirstSeenOrder(agentViews(st.local, live.status, live.peers, Date.now()), order);
    if (sameViews(next, st.views)) return;
    st.setViews(next);
  };

  /** Steps each agent had at the last list, to notice new ones. */
  let steps = new Map<string, number>();
  const setLocal = (list: AgentSummary[]) => {
    const st = useAgents.getState();
    st.setAvailable(true);
    st.setLocal(list);
    recompute();
    let grew = false;
    const next = new Map<string, number>();
    for (const a of list) {
      next.set(a.participant.id, a.steps);
      const before = steps.get(a.participant.id);
      if (before !== undefined && a.steps > before) grew = true;
    }
    steps = next;
    if (grew) readHistorySoon();
  };

  let loadSeq = 0;
  const load = async () => {
    const seq = ++loadSeq;
    try {
      const list = await ipc.agentsList();
      if (stopped || seq !== loadSeq) return;
      setLocal(list);
    } catch {
      // An engine before agents: nothing to show. The `agents` event turns them on.
      if (!stopped && seq === loadSeq && useAgents.getState().local.length === 0) useAgents.getState().setAvailable(false);
    }
  };

  stops.push(
    onAppEvent((event) => {
      if (event.type === "agents") setLocal(event.agents);
    }),
  );

  // Other computers' agents live in the session's participants and presence.
  stops.push(
    useLive.subscribe((s, p) => {
      if (sessionKey(s.status) !== sessionKey(p.status)) void load();
      if (s.status === p.status && s.peers === p.peers) return;
      // Presence moves many times a second: only a session with agents in it, or one that had some, recomputes.
      const hasRemote = s.status.participants.some((x) => x.agent) || useAgents.getState().views.some((a) => !a.local);
      if (hasRemote) recompute();
    }),
  );

  const tick = window.setInterval(() => {
    if (useAgents.getState().views.length > 0) useAgents.getState().setViews([...useAgents.getState().views]);
  }, TICK_MS);
  stops.push(() => window.clearInterval(tick));

  // ------------------------------------------------ trails

  /** Keys of the steps already shown, so only new ones trail. Null until the first read of a project. */
  let seen: Set<string> | null = null;
  let project: string | null = useApp.getState().doc?.project.id ?? null;
  let pending = 0;

  const readHistorySoon = () => {
    if (pending) return;
    pending = later(() => {
      pending = 0;
      void readHistory();
    }, HISTORY_DEBOUNCE_MS);
  };

  const readHistory = async () => {
    if (!useApp.getState().doc) return;
    const at = project;
    let undo: HistoryStep[];
    try {
      undo = (await ipc.historyList()).undo;
    } catch {
      return;
    }
    if (stopped || at !== project) return;
    const keys = new Set(undo.map(stepKey));
    if (seen === null) {
      // The first read of a project: what is there already is not news.
      seen = keys;
      return;
    }
    const fresh = newAgentSteps(undo, seen, knownAgentIds());
    seen = keys;
    for (const [id, list] of fresh) {
      const ids: string[] = [];
      for (const s of list) for (const x of s.ids) if (!ids.includes(x)) ids.push(x);
      if (ids.length > 0) showTrail(id, ids);
    }
  };

  const fades = new Map<string, number[]>();
  const showTrail = (id: string, ids: string[]) => {
    for (const t of fades.get(id) ?? []) {
      window.clearTimeout(t);
      timers.delete(t);
    }
    const token = Date.now();
    const st = useAgents.getState();
    st.setTrail(id, { ids, strength: 1, token });
    const steps: number[] = [];
    TRAIL_FADE.forEach((strength, i) => {
      steps.push(later(() => useAgents.getState().trails[id]?.token === token && useAgents.getState().setTrail(id, { ids, strength, token }), TRAIL_HOLD_MS + i * TRAIL_STEP_MS));
    });
    steps.push(later(() => useAgents.getState().trails[id]?.token === token && useAgents.getState().setTrail(id, null), TRAIL_HOLD_MS + TRAIL_FADE.length * TRAIL_STEP_MS));
    fades.set(id, steps);
  };

  stops.push(
    useApp.subscribe((s, p) => {
      const now = s.doc?.project.id ?? null;
      if (now !== project) {
        project = now;
        seen = null;
        useAgents.setState({ trails: {} });
        if (now) readHistorySoon();
        return;
      }
      // A step by another computer's agent arrives with the document (`undo_by`).
      if (s.doc && s.doc !== p.doc && s.doc.revision !== p.doc?.revision && s.doc.undo_by && s.doc.undo_by in useAgents.getState().known) readHistorySoon();
    }),
  );

  // ------------------------------------------------ the 3D view

  // The 3D view re-reads its tints when the live store's peers change
  // (Viewer3D `useLiveTints`); agents' focus, trails and pointers are not in
  // that store, so a change to them nudges it with a new peers object.
  stops.push(
    useAgents.subscribe((s, p) => {
      if (s.trails === p.trails && s.views === p.views) return;
      useLive.setState((l) => ({ peers: { ...l.peers } }));
    }),
  );

  void load();
  if (project) readHistorySoon();

  return () => {
    stopped = true;
    for (const stop of stops) stop();
    for (const id of timers) window.clearTimeout(id);
    timers.clear();
  };
}
