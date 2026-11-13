// The swarm in the window (DECISIONS D56, D57): the status the backend sends,
// the draft it serves, the reveal that paces it, and the panel's own state.
// Filled by wiring.ts; actions in actions.ts; pure words in view.ts.
//
// The reveal is one object for the whole window (`reveal`). A ticker fires
// one setTimeout per segment boundary (`onRevealTick`): the agent cursors
// set their CSS glide there, the plan wakes its loop, the 3D view shows what
// started. Nothing here runs a frame loop, and the ticker sleeps at rest.

import { useSyncExternalStore } from "react";
import { create } from "zustand";
import type { DocState } from "../contract/bindings";
import { buildIndex, type DocIndex } from "../editor2d/model";
import { motionOK } from "../ui/motion";
import { stepItems } from "./items";
import { Reveal, SPEED_DEFAULT, clampSpeed } from "./reveal";
import type { SwarmCost, SwarmDraftState, SwarmStatus, SwarmStep } from "./types";
import type { Ask } from "./view";

/** The AI dock's switch. */
export type DockMode = "chat" | "swarm";

export interface SwarmNote {
  tone: "info" | "error";
  text: string;
}

export interface SwarmUiState {
  /** The swarm of the open project, null when there is none. */
  status: SwarmStatus | null;
  /** The latest draft the window fetched. */
  draft: SwarmDraftState | null;
  /** The newest step the reveal has seen. */
  lastSeq: number;
  /**
   * Per part, its newest step as the drawn view shows it: the status of the
   * drawn revision when the window has it, else the newest step fetched.
   * Accept pins to it.
   */
  seen: Record<string, number>;
  /**
   * The window saw this swarm start, so its first steps are drawn in. A
   * window that opens on a swarm already drafting shows what is there at once.
   */
  watching: boolean;
  mode: DockMode;
  /** The draft's own view toggle (not a layer). */
  showDraft: boolean;
  speed: number;
  brief: string;
  /** The spending cap typed in the panel, null for the setting's (default US$3). */
  capUsd: number | null;
  estimate: SwarmCost | null;
  /** An Accept or Discard waiting for the person to confirm. */
  ask: Ask | null;
  /** What the panel says under its controls. */
  note: SwarmNote | null;
  /** The call in flight: "start", "accept", "discard", "pause", "stop:<key>". */
  acting: string | null;
  /** Bumps when the reveal changes, for the rows' revealed fill. */
  revealSeq: number;
  /** A part row asked to come into view (the `swarm` bus event); the token repeats a request. */
  focus: { part: string; token: number } | null;

  setMode: (mode: DockMode) => void;
  setShowDraft: (on: boolean) => void;
  setBrief: (brief: string) => void;
  setCap: (usd: number | null) => void;
  setEstimate: (cost: SwarmCost | null) => void;
  setAsk: (ask: Ask | null) => void;
  setNote: (note: SwarmNote | null) => void;
  setActing: (acting: string | null) => void;
  setSpeed: (speed: number) => void;
  /** Shows the Swarm tab, at this part's row when given. */
  showSwarm: (part: string | null) => void;
  /** Everything still to come shows now. */
  skip: () => void;
  /** A status from the backend. `event`: pushed (the window watches it happen), not read at start. */
  applyStatus: (status: SwarmStatus | null, event: boolean) => void;
  /** A draft from the backend: its new steps go to the reveal. `main` is the model, for removed elements. */
  applyDraft: (draft: SwarmDraftState, main: DocState | null) => void;
}

/** The one reveal: the plan, the 3D view and the cursors all read it. */
export const reveal = new Reveal(() => !motionOK());

const clock = (): number => performance.now();

export const useSwarm = create<SwarmUiState>((set, get) => ({
  status: null,
  draft: null,
  lastSeq: 0,
  seen: {},
  watching: false,
  mode: "chat",
  showDraft: true,
  speed: SPEED_DEFAULT,
  brief: "",
  capUsd: null,
  estimate: null,
  ask: null,
  note: null,
  acting: null,
  revealSeq: 0,
  focus: null,

  setMode: (mode) => set({ mode }),
  setShowDraft: (showDraft) => set({ showDraft }),
  setBrief: (brief) => set({ brief }),
  setCap: (capUsd) => set({ capUsd }),
  setEstimate: (estimate) => set({ estimate }),
  setAsk: (ask) => set({ ask }),
  setNote: (note) => set({ note }),
  setActing: (acting) => set({ acting }),

  setSpeed: (speed) => {
    const next = clampSpeed(speed);
    reveal.setSpeed(next, clock());
    set((s) => ({ speed: next, revealSeq: s.revealSeq + 1 }));
    kickReveal();
  },

  showSwarm: (part) => set((s) => ({ mode: "swarm", focus: part ? { part, token: (s.focus?.token ?? 0) + 1 } : s.focus })),

  skip: () => {
    reveal.skip(clock());
    set((s) => ({ revealSeq: s.revealSeq + 1 }));
    kickReveal();
  },

  applyStatus: (status, event) => {
    const prev = get().status;
    if (!status) {
      if (!prev && !get().draft) return;
      reveal.reset();
      set((s) => ({ status: null, draft: null, lastSeq: 0, seen: {}, watching: false, ask: null, acting: null, revealSeq: s.revealSeq + 1 }));
      kickReveal();
      return;
    }
    const fresh = !prev || prev.id !== status.id;
    if (fresh) {
      reveal.reset();
      set((s) => ({ status, draft: null, lastSeq: 0, seen: {}, watching: event, ask: null, revealSeq: s.revealSeq + 1 }));
      kickReveal();
      return;
    }
    // The status of the revision drawn says exactly what the person sees, undos included.
    const drawn = get().draft;
    if (drawn && drawn.swarm_id === status.id && drawn.draft_revision === status.draft_revision) set({ status, seen: lastSeqs(status) });
    else set({ status });
  },

  applyDraft: (draft, main) => {
    const st = get();
    // A late answer for a swarm that ended or was replaced paints nothing.
    if (!st.status || st.status.id !== draft.swarm_id) return;
    const now = clock();
    const steps = draft.steps.filter((x) => x.seq > st.lastSeq).sort((a, b) => a.seq - b.seq);
    if (st.draft === null && !st.watching) {
      // Opened on a swarm already drafting: what is there shows at once.
      const byPart = new Map<string, { n: number; ids: string[] }>();
      for (const x of steps) {
        const e = byPart.get(x.part) ?? { n: 0, ids: [] };
        e.n++;
        e.ids.push(...x.added, ...x.modified, ...x.removed);
        byPart.set(x.part, e);
      }
      for (const p of draft.parts) {
        const e = byPart.get(p.part) ?? { n: 0, ids: [] };
        e.ids.push(...p.ids);
        byPart.set(p.part, e);
      }
      for (const [part, e] of byPart) reveal.known(part, e.n, e.ids);
    } else if (steps.length > 0) {
      const draftIndex = draftIndexOf(draft);
      const mainIndex = needsMain(steps) && main ? buildIndex(main, null) : null;
      const lines = new Map((st.status?.parts ?? []).map((p) => [p.key, p.status]));
      for (const x of steps) reveal.arrive(x.part, stepItems(x, draftIndex, mainIndex, lines.get(x.part) ?? null), now);
    }
    const lastSeq = steps.length > 0 ? steps[steps.length - 1].seq : st.lastSeq;
    // What the person sees of each part: exactly the status of this revision
    // when the window has it, else the newest step fetched (an undo since
    // then makes the accept `stale`, so the person looks again).
    let seen = st.seen;
    if (st.status.draft_revision === draft.draft_revision) seen = lastSeqs(st.status);
    else for (const x of steps) if ((seen[x.part] ?? 0) < x.seq) seen = { ...seen, [x.part]: x.seq };
    set((s) => ({ draft, lastSeq, seen, revealSeq: s.revealSeq + 1 }));
    kickReveal();
  },
}));

/** Each part's newest step, as the status says. */
function lastSeqs(status: SwarmStatus): Record<string, number> {
  return Object.fromEntries(status.parts.map((p) => [p.key, p.last_seq]));
}

function needsMain(steps: readonly SwarmStep[]): boolean {
  return steps.some((x) => x.removed.length > 0);
}

// ---------------------------------------------------------------- draft index

let indexFor: { draft: SwarmDraftState; index: DocIndex } | null = null;

/** Every element of the draft view by id, built once per draft. */
export function draftIndexOf(draft: SwarmDraftState): DocIndex {
  if (indexFor?.draft !== draft) indexFor = { draft, index: buildIndex(draft.state, null) };
  return indexFor.index;
}

/** True while a draft is open: the model is locked for people (D57). */
export function swarmLocked(): boolean {
  return useSwarm.getState().status !== null;
}

/** For components: the model is locked while a draft is open (D57). */
export function useSwarmLocked(): boolean {
  return useSwarm((s) => s.status !== null);
}

// ---------------------------------------------------------------- ticker

type TickListener = (now: number) => void;
const tickListeners = new Set<TickListener>();
let timer: ReturnType<typeof setTimeout> | null = null;
let timerAt = Infinity;

/**
 * Called at every segment boundary of the reveal (and when it changes): a
 * cursor starts a glide or a draw, an element starts or ends. Returns the
 * unsubscribe.
 */
export function onRevealTick(fn: TickListener): () => void {
  tickListeners.add(fn);
  return () => tickListeners.delete(fn);
}

let lastTick = 0;

/** Tells every listener the reveal changed now, and sets the timer for the next boundary. */
export function kickReveal(): void {
  const now = clock();
  lastTick = now;
  for (const fn of [...tickListeners]) {
    try {
      fn(now);
    } catch (e) {
      console.error("swarm reveal listener failed", e);
    }
  }
  arm(now);
}

function arm(now: number): void {
  const next = reveal.nextBoundary(now);
  if (next === null) {
    disarm();
    return;
  }
  if (timer !== null && timerAt <= next) return;
  disarm();
  timerAt = next;
  // A millisecond late, so the boundary has passed when it fires.
  timer = setTimeout(fire, Math.max(0, next - now) + 1);
}

function disarm(): void {
  if (timer !== null) clearTimeout(timer);
  timer = null;
  timerAt = Infinity;
}

function fire(): void {
  timer = null;
  timerAt = Infinity;
  kickReveal();
}

/** For the dev probe and tests: whether the ticker holds a timer. */
export function revealTimerArmed(): boolean {
  return timer !== null;
}

/** The moment of the latest tick: React readers of the reveal re-render on each one. */
export function useRevealNow(): number {
  return useSyncExternalStore(onRevealTick, () => lastTick, () => lastTick);
}

// Dev builds: the store and the reveal for the console, the dev probe and scripts/swarm-check.mjs.
if (import.meta.env?.DEV && typeof window !== "undefined") {
  (window as unknown as { __swarm: unknown }).__swarm = { useSwarm, reveal, revealTimerArmed };
}
