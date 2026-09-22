// View-only state of the 3D viewer. Lives here because the app store has no
// fields for it (see "Contract requests" in the viewer agent report).

import { create } from "zustand";

export interface ViewerState {
  roofVisible: boolean;
  cutaway: boolean;
  shadows: boolean;
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
  rendersVersion: 0,
  captureFlash: 0,
  toggleRoof: () => set((s) => ({ roofVisible: !s.roofVisible })),
  toggleCutaway: () => set((s) => ({ cutaway: !s.cutaway })),
  toggleShadows: () => set((s) => ({ shadows: !s.shadows })),
  bumpRenders: () => set((s) => ({ rendersVersion: s.rendersVersion + 1 })),
  flashCapture: () => set((s) => ({ captureFlash: s.captureFlash + 1 })),
}));
