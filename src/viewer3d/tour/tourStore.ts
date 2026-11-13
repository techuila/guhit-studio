// What the Tours panel, the player and the export share (lane L9): which
// tour is open, whether its path shows in 3D, the playhead, and an export in
// progress. View state only: tours themselves live in the document. The app
// store has no field for it (panels open themselves, C0), so it lives here.

import { create } from "zustand";
import type { RenderSize } from "../../contract/bindings";

export interface TourExport {
  tourId: string;
  size: RenderSize;
  /** Frames done and in all. */
  done: number;
  total: number;
  /** "avc", "vp9" or "png" once chosen. */
  codec: string | null;
  /** What it is doing, for the panel: "Rendering", "Saving". */
  phase: string;
  /** Asked to stop. */
  cancelled: boolean;
}

interface TourView {
  /** The Tours panel. */
  open: boolean;
  /** The tour the panel and the path preview show. */
  tourId: string | null;
  /** Its path in the 3D view. */
  showPath: boolean;
  /** Playing in the 3D view. */
  playing: boolean;
  /** Seconds into the tour, while playing or scrubbing. */
  playhead: number;
  exporting: TourExport | null;
  setOpen: (open: boolean) => void;
  select: (tourId: string | null) => void;
  setShowPath: (on: boolean) => void;
  setPlaying: (on: boolean) => void;
  setPlayhead: (t: number) => void;
  setExport: (e: TourExport | null) => void;
  patchExport: (patch: Partial<TourExport>) => void;
}

export const useTours = create<TourView>((set) => ({
  open: false,
  tourId: null,
  showPath: true,
  playing: false,
  playhead: 0,
  exporting: null,
  setOpen: (open) => set({ open }),
  select: (tourId) => set((s) => (s.tourId === tourId ? s : { tourId, playhead: 0 })),
  setShowPath: (showPath) => set({ showPath }),
  setPlaying: (playing) => set({ playing }),
  setPlayhead: (playhead) => set({ playhead }),
  setExport: (exporting) => set({ exporting }),
  patchExport: (patch) => set((s) => (s.exporting ? { exporting: { ...s.exporting, ...patch } } : s)),
}));

/** Stops the video export that is running (tour/export.ts checks between frames). */
export function cancelTourExport(): void {
  useTours.getState().patchExport({ cancelled: true });
}

/** Said when a camera move is refused while a video exports. */
export const VIEW_HELD_NOTE = "The 3D view stays where it is while the tour video exports. Stop the export in the Tours panel to move it.";

/**
 * True while a tour video exports. The video draws from its own camera, so
 * a move could not spoil it, but presets, saved views and agents' looks
 * still leave the view where it is until the export ends (GR3 P1-1): no fly
 * or saved light competes with the frames for the GPU or the live light.
 */
export function viewHeld(): boolean {
  return useTours.getState().exporting !== null;
}
