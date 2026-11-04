// The level below as a faint underlay on the plan (docs/CONTRACT.md "Slabs
// and railings"): view state only, on by default, remembered per project in
// local storage. Never saved in the project, never exported.
//
// The project it belongs to always comes from the open document in
// `useApp`, never from a caller, so a switch can only be saved to the
// project on screen.

import { create } from "zustand";
import { useApp } from "../state/store";
import { readUnderlay, writeUnderlay } from "./slab";

interface UnderlayState {
  on: boolean;
  projectId: string | null;
  /** Reads the open project's switch when another project opens. */
  sync: () => void;
  set: (on: boolean) => void;
  toggle: () => void;
}

function openProjectId(): string | null {
  return useApp.getState().doc?.project.id ?? null;
}

export const useUnderlay = create<UnderlayState>((set, get) => ({
  on: readUnderlay(openProjectId()),
  projectId: openProjectId(),
  sync: () => {
    const projectId = openProjectId();
    if (projectId === get().projectId) return;
    set({ projectId, on: readUnderlay(projectId) });
  },
  set: (on) => {
    // Catch up first, so the value lands on the project that is open now.
    get().sync();
    writeUnderlay(get().projectId, on);
    set({ on });
  },
  toggle: () => {
    get().sync();
    get().set(!get().on);
  },
}));

// Opening another project brings its own switch.
useApp.subscribe((s, prev) => {
  if (s.doc?.project.id !== prev.doc?.project.id) useUnderlay.getState().sync();
});
