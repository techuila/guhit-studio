// The live 3D view as the render queue and the shadow study see it. Viewer3D
// registers its engine while it is mounted; renders and studies read the
// current camera, the light and the view options from here, and nothing else
// of the view. The app store has no field for it, so it lives in this module.

import { create } from "zustand";
import type { Vec3 } from "../../contract/bindings";
import type { ViewerEngine } from "../engine/ViewerEngine";

interface LiveViewState {
  engine: ViewerEngine | null;
  /** The point under the pointer in the 3D view (plan mm, z up from project
   * zero), null while it is off the model. Live presence sends it
   * (`Presence::pointer_3d`). */
  pointer3d: Vec3 | null;
  /** The name the view goes by ("Exterior corner", "View 2", "Custom view"). */
  label: () => string;
  register: (engine: ViewerEngine, label: () => string) => void;
  /** Set by the 3D view from its hover raycast, only when the point changed (1 mm). */
  setPointer3d: (point: Vec3 | null) => void;
  unregister: (engine: ViewerEngine) => void;
}

export const useLiveView = create<LiveViewState>((set, get) => ({
  engine: null,
  pointer3d: null,
  label: () => "View",
  register: (engine, label) => set({ engine, label }),
  setPointer3d: (pointer3d) => {
    if (get().pointer3d !== pointer3d) set({ pointer3d });
  },
  unregister: (engine) => {
    if (get().engine === engine) set({ engine: null, label: () => "View", pointer3d: null });
  },
}));
