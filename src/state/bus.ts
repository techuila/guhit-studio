// Tiny typed event bus for one-shot view requests that are not state.
// CONTRACT FILE - owned by the orchestrator.

import type { Camera, Point, Vec3, ViewRef } from "../contract/bindings";

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
  /** The user moved a view by hand (panned, zoomed, orbited, walked, flew,
   * picked a camera preset) or pressed on it. Never sent for programmatic
   * moves (fits and focus from code, follow) or for hovering. Following
   * someone in a live session stops on it (DECISIONS D37). */
  user_view: { view: "plan" | "3d" };
  /** Open the Sheets panel (shell/sheets/SheetsPanel.tsx), at this sheet, or
   * at the whole set when null. */
  open_sheets: { sheet_id: string | null };
  /** Open the views viewer (shell/views/ViewsPanel.tsx) on one drawing: an
   * elevation, a section, the roof or site plan, a schedule. */
  open_view: { view: ViewRef };
  /** Play a tour in the 3D view (viewer3d/tour/player.ts), from its first shot. */
  play_tour: { tour_id: string };
  /** Show the swarm (DECISIONS D56): open the AI dock on its Swarm tab, at
   * this part's row when `part` is a part key. Sent by the draft bar, the
   * main-lock banner and `swarm_running` refusal toasts; src/swarm answers. */
  swarm: { part: string | null };
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
