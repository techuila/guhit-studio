// View-only state of the 3D viewer. Lives here because the app store has no
// fields for it (see "Contract requests" in the viewer agent report).

import { create } from "zustand";
import type { Vec3 } from "../contract/bindings";
import { bus } from "../state/bus";
import { useApp } from "../state/store";

/** How the 3D camera moves. Walk: eye height on the active level, walls
 * block. Fly: free, nothing blocks. While not "orbit" the 3D view owns the
 * keyboard (WASD, arrows, Shift, Escape): the global shortcut handler ignores
 * every key without MOD, and Escape returns to orbit. */
export type NavMode = "orbit" | "walk" | "fly";
/** How the building shell (walls, roof, slabs, openings, columns, stairs,
 * objects) is drawn, so pipes stay visible. Pipes are always solid. */
export type ShellMode = "solid" | "xray" | "hidden";

const SHELL_ORDER: ShellMode[] = ["solid", "xray", "hidden"];

/** A `walk_to` waiting for the 3D view. `seq` tells two requests for the same finding apart. */
export interface WalkRequest {
  seq: number;
  ids: string[];
  location: Vec3 | null;
}

export interface ViewerState {
  roofVisible: boolean;
  cutaway: boolean;
  shadows: boolean;
  nav: NavMode;
  shell: ShellMode;
  setNav: (nav: NavMode) => void;
  setShell: (shell: ShellMode) => void;
  /** Solid, then X-ray, then hidden, then solid. */
  cycleShell: () => void;
  /** The latest `walk_to`, consumed by the 3D view when it has a model. */
  walkRequest: WalkRequest | null;
  /** Enters walk mode near these elements (the `walk_to` bus event lands here). */
  requestWalk: (ids: string[], location: Vec3 | null) => void;
  /** Bumped after a capture is saved so the Visuals gallery reloads. */
  rendersVersion: number;
  /** Bumped by the Visuals panel to flash the 3D view when it grabs a frame. */
  captureFlash: number;
  toggleRoof: () => void;
  toggleCutaway: () => void;
  toggleShadows: () => void;
  bumpRenders: () => void;
  flashCapture: () => void;
}

export const useViewer = create<ViewerState>((set) => ({
  roofVisible: true,
  cutaway: false,
  shadows: true,
  nav: "orbit",
  shell: "solid",
  setNav: (nav) => set({ nav }),
  setShell: (shell) => set({ shell }),
  cycleShell: () => set((s) => ({ shell: SHELL_ORDER[(SHELL_ORDER.indexOf(s.shell) + 1) % SHELL_ORDER.length] })),
  walkRequest: null,
  requestWalk: (ids, location) =>
    set((s) => ({ nav: "walk", walkRequest: { seq: (s.walkRequest?.seq ?? 0) + 1, ids: [...ids], location } })),
  rendersVersion: 0,
  captureFlash: 0,
  toggleRoof: () => set((s) => ({ roofVisible: !s.roofVisible })),
  toggleCutaway: () => set((s) => ({ cutaway: !s.cutaway })),
  toggleShadows: () => set((s) => ({ shadows: !s.shadows })),
  bumpRenders: () => set((s) => ({ rendersVersion: s.rendersVersion + 1 })),
  flashCapture: () => set((s) => ({ captureFlash: s.captureFlash + 1 })),
}));

// `walk_to` is listened to here, not in the 3D view: the view is not mounted
// while the app shows the plan only. The request waits in the store, the view
// switches to split, and the 3D view walks there once it has a model.
const offWalkTo = bus.on("walk_to", ({ ids, location }) => {
  const app = useApp.getState();
  if (app.viewMode === "2d") app.setViewMode("split");
  useViewer.getState().requestWalk(ids, location);
});

if (import.meta.hot) import.meta.hot.dispose(() => offWalkTo());
