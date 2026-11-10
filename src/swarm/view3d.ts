// Hands the swarm draft to a 3D view (DECISIONS D56, D57): the draft group
// when the draft or its toggle changes (the engine rebuilds it at most 4
// times a second), and which of its elements show at each reveal boundary.
// No paced drawing in 3D (v1): an element simply rises and fades in when the
// reveal reaches it, the same moment it starts drawing on the plan.

import type { DocState } from "../contract/bindings";
import type { DraftOverlay } from "./draft3d";
import { peerColor } from "./draw2d";
import { onRevealTick, reveal, useSwarm } from "./swarmStore";
import type { SwarmDraftState } from "./types";

export interface DraftView3D {
  setDraftOverlay(overlay: DraftOverlay | null): void;
  setDraftShown(shown: (id: string) => boolean): void;
}

/** What the 3D view draws of a draft. `main` is the model: its elements the draft changes hide. */
export function overlayOf(draft: SwarmDraftState, main: DocState | null): DraftOverlay {
  const inModel = new Set((main?.project.elements ?? []).map((e) => e.id));
  const changed = draft.parts.flatMap((p) => p.ids).filter((id) => inModel.has(id));
  return {
    key: `${draft.swarm_id}:${draft.draft_revision}`,
    state: draft.state,
    parts: draft.parts.map((p) => ({ color: peerColor(p.color), ids: p.ids })),
    context: draft.display_modified ?? [],
    hide: [...changed, ...(draft.display_modified ?? [])],
  };
}

/** The reveal's answer, asked when the view asks: the same as the plan's (progress above 0). */
export const shownNow = (id: string): boolean => reveal.progress(id, performance.now()) > 0;

/** Keeps a 3D view's draft group current. Returns the disconnect, which takes the draft away. */
export function connectDraft3D(view: DraftView3D, main: () => DocState | null): () => void {
  const push = () => {
    const st = useSwarm.getState();
    view.setDraftOverlay(st.draft && st.showDraft ? overlayOf(st.draft, main()) : null);
    view.setDraftShown(shownNow);
  };
  push();
  const offStore = useSwarm.subscribe((s, p) => {
    if (s.draft !== p.draft || s.showDraft !== p.showDraft) push();
  });
  const offTick = onRevealTick(() => view.setDraftShown(shownNow));
  return () => {
    offStore();
    offTick();
    view.setDraftOverlay(null);
  };
}
