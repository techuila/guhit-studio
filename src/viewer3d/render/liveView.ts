// The live 3D view as the render queue and the shadow study see it. Viewer3D
// registers its engine while it is mounted; renders and studies read the
// current camera, the light and the view options from here, and nothing else
// of the view. The app store has no field for it, so it lives in this module.

import { create } from "zustand";
import type { ViewerEngine } from "../engine/ViewerEngine";

interface LiveViewState {
  engine: ViewerEngine | null;
  /** The name the view goes by ("Exterior corner", "View 2", "Custom view"). */
  label: () => string;
  register: (engine: ViewerEngine, label: () => string) => void;
  unregister: (engine: ViewerEngine) => void;
}

export const useLiveView = create<LiveViewState>((set, get) => ({
  engine: null,
  label: () => "View",
  register: (engine, label) => set({ engine, label }),
  unregister: (engine) => {
    if (get().engine === engine) set({ engine: null, label: () => "View" });
  },
}));
