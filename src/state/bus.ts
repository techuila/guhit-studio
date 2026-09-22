// Tiny typed event bus for one-shot view requests that are not state.
// CONTRACT FILE - owned by the orchestrator.

import type { Camera } from "../contract/bindings";

export interface BusEvents {
  /** Fit the whole model in the 2D and 3D views. */
  zoom_to_fit: undefined;
  /** Center the views on these elements. */
  focus_elements: string[];
  /** Move the 3D camera to this pose. */
  apply_camera: Camera;
  /** Open the command palette. */
  open_palette: undefined;
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
