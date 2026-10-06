// Tiny typed event bus for one-shot view requests that are not state.
// CONTRACT FILE - owned by the orchestrator.

import type { Camera, Point, Vec3 } from "../contract/bindings";

export interface BusEvents {
  /** Fit the whole model in the 2D and 3D views. */
  zoom_to_fit: undefined;
  /** Center the views on these elements. */
  focus_elements: string[];
  /** Center the plan on a point (mm), switching to its level first when one
   * is given: a live session participant's pointer, a cursor chat message. */
  focus_point: { point: Point; level_id: string | null };
  /** Move the 3D camera to this pose. */
  apply_camera: Camera;
  /** Open the command palette. */
  open_palette: undefined;
  /** Enter walk mode standing near these elements, looking at `location` when
   * given (plan x and y, z above the floor of the first element's level, the
   * `Issue::location` convention). Switches the view to 3D when needed. */
  walk_to: { ids: string[]; location: Vec3 | null };
  /** Render with the path tracer: the current view, every saved view (a
   * batch), or these camera ids. Results land in the Visuals gallery. */
  render: { views: "current" | "all" | string[] };
  /** Open the shadow study export (frames from the live 3D view). */
  shadow_study: undefined;
}

type Handler<K extends keyof BusEvents> = (payload: BusEvents[K]) => void;
const handlers = new Map<keyof BusEvents, Set<Handler<never>>>();

export const bus = {
  on<K extends keyof BusEvents>(event: K, handler: Handler<K>): () => void {
    let set = handlers.get(event);
    if (!set) handlers.set(event, (set = new Set()));
    set.add(handler as Handler<never>);
    return () => set.delete(handler as Handler<never>);
  },
  emit<K extends keyof BusEvents>(event: K, ...payload: BusEvents[K] extends undefined ? [] : [BusEvents[K]]) {
    handlers.get(event)?.forEach((h) => (h as Handler<K>)(payload[0] as BusEvents[K]));
  },
};
