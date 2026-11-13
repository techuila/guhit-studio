// Which part of a selected pipe run is picked (DECISIONS D39): the whole
// run (no pick), one segment, or one point. The plan, the inspector and the
// 3D handle read and set it. Only meaningful while exactly that run is
// selected; it clears itself when the selection changes.
//
// A point pick stands for its plan node: the points a riser joins move
// together in plan, and a height edit moves the whole node (src/editor2d/pipeEdit.ts).

import { create } from "zustand";
import type { PipeEnd } from "../contract/bindings";
import { useApp } from "../state/store";

export interface PipePick {
  pipeId: string;
  /** A segment runs from point `index` to `index + 1`. */
  kind: "segment" | "point";
  index: number;
}

interface PipePickState {
  pick: PipePick | null;
  setPick: (pick: PipePick | null) => void;
  /** The end that keeps its place when a length or a slope is typed, on the plan or in the inspector. */
  hold: PipeEnd;
  setHold: (hold: PipeEnd) => void;
}

export const usePipePick = create<PipePickState>((set) => ({
  pick: null,
  setPick: (pick) => set({ pick }),
  hold: "start",
  setHold: (hold) => set({ hold }),
}));

// A pick belongs to its run's selection: selecting anything else drops it.
useApp.subscribe((s, p) => {
  if (s.selection === p.selection) return;
  const pick = usePipePick.getState().pick;
  if (pick && !(s.selection.length === 1 && s.selection[0] === pick.pipeId)) usePipePick.getState().setPick(null);
});
