// The live 3D view as the render queue and the shadow study see it. Viewer3D
// registers its engine while it is mounted; renders and studies read the
// current camera, the light and the view options from here, and nothing else
// of the view. The app store has no field for it, so it lives in this module.

import { create } from "zustand";
import type { Camera, Vec3, ViewVisibility } from "../../contract/bindings";
import type { ViewerEngine } from "../engine/ViewerEngine";

/** Size of a capture of the 3D view, device pixels. */
export const CAPTURE_SIZE = { width: 1920, height: 1080 } as const;

/** The model's own visibility: roof on, no cutaway. */
export const MODEL_VISIBILITY: ViewVisibility = { roof: true, cutaway: false };

/**
 * A capture of the live 3D view for a window task (an MCP client): the
 * roof and cutaway as the task asks, or the model's defaults when it does not
 * (`WindowTask::CaptureView::visibility`), never this window's toggles, which
 * the client cannot see. Null when no 3D view is mounted.
 */
export function captureForTask(visibility: ViewVisibility | null): { png: string; camera: Camera } | null {
  const { engine, label } = useLiveView.getState();
  if (!engine) return null;
  const png = engine.captureWith(CAPTURE_SIZE.width, CAPTURE_SIZE.height, visibility ?? MODEL_VISIBILITY);
  return { png, camera: engine.currentCamera(label()) };
}

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
