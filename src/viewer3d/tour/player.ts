// Plays a tour in the live 3D view (lane L9). The engine calls in here and
// never the other way round: `startTour` on the `play_tour` bus event
// (ViewerEngine.playTour), `stepTour` once per frame inside the engine's
// one frame loop, `stopTour` when the engine goes away. Frames are asked
// for only through the engine (AGENTS.md, the 3D frame loop invariant):
// nothing here calls requestAnimationFrame. `engine.invalidate()` asks for
// the first frame; while a tour plays `stepTour` answers true and the loop
// keeps going.
//
// The camera comes from the engine's frames (`Query::TourFrames`), blended
// between two frames for a display faster than the tour; transitions lie
// over the view (tour/overlay.ts); the doors the camera passes through open
// in the view only, as the frames say (`doorsAt`). Any click, wheel or Escape in the view
// stops the tour where it is.

import type { ViewLight } from "../../contract/bindings";
import { useApp } from "../../state/store";
import { dur } from "../../ui/motion";
import type { ViewerEngine } from "../engine/ViewerEngine";
import { cameraToPose } from "../geom/coords";
import { fromViewLight } from "../light/model";
import { useLiveView } from "../render/liveView";
import { useViewer, type LiveLight } from "../viewerStore";
import { doorsAt, fxLayers, poseAt, tourFrames, type TourFrames } from "./frames";
import { TourOverlay } from "./overlay";
import { hideTourPath, markTourPath } from "./pathPreview";
import { useTours } from "./tourStore";

interface Playing {
  engine: ViewerEngine;
  tourId: string;
  data: TourFrames | null;
  /** performance.now() at the tour's time 0; set by the first frame. */
  t0: number;
  /** Where it starts, seconds. */
  from: number;
  /** Not before this time (a walk to leave first). */
  notBefore: number;
  savedLight: LiveLight;
  lightKey: string;
  savedPolar: number;
  overlay: TourOverlay | null;
  /** The frame the overlay holds, -1 for none. */
  held: number;
  showPath: () => void;
  off: () => void;
  lastPlayheadAt: number;
}

let playing: Playing | null = null;

function lightKey(l: ViewLight): string {
  return `${l.month}/${l.day}/${l.minutes}/${l.sky}/${l.lamps}/${l.exposure_ev ?? "auto"}`;
}

/**
 * Puts the view's camera at a pose (plan mm) in this frame, ready for the
 * render that follows, dropping any fly on its way. Playback only: a video's
 * frames draw from a camera of their own (render/tourJob.ts).
 */
export function setCamera(engine: ViewerEngine, position: { x: number; y: number; z: number }, target: { x: number; y: number; z: number }, fov: number): void {
  engine.placeCamera(cameraToPose({ position, target, fov_deg: fov }));
}

/** Starts tour `tourId` from the playhead (or the start), stopping any tour that plays. */
export function startTour(engine: ViewerEngine, tourId: string): void {
  stopTour();
  const tours = useTours.getState();
  const fromPlayhead = tours.tourId === tourId ? tours.playhead : 0;
  tours.select(tourId);
  const viewer = useViewer.getState();
  // Walking or flying owns the camera: back to orbit first, then play.
  const leaving = viewer.nav !== "orbit";
  if (leaving) viewer.setNav("orbit");
  const canvas = engine.renderer.domElement;
  const host = canvas.parentElement;
  const stop = () => stopTour();
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") stopTour();
  };
  canvas.addEventListener("pointerdown", stop);
  canvas.addEventListener("wheel", stop, { passive: true });
  window.addEventListener("keydown", onKey);
  const p: Playing = {
    engine,
    tourId,
    data: null,
    t0: 0,
    from: fromPlayhead,
    notBefore: leaving ? performance.now() + dur("scene") + 60 : 0,
    savedLight: viewer.light,
    lightKey: "",
    savedPolar: engine.controls.maxPolarAngle,
    overlay: host ? new TourOverlay(host, canvas) : null,
    held: -1,
    showPath: hideTourPath(),
    off: () => {
      canvas.removeEventListener("pointerdown", stop);
      canvas.removeEventListener("wheel", stop);
      window.removeEventListener("keydown", onKey);
    },
    lastPlayheadAt: 0,
  };
  playing = p;
  // A tour looks up a stair or down from the drone: no orbit polar limit while it plays.
  engine.controls.maxPolarAngle = Math.PI - 0.01;
  tours.setPlaying(true);
  void tourFrames(tourId).then(
    (data) => {
      if (playing !== p) return;
      if (data.frame_count === 0) {
        useApp.getState().toast("info", "This tour has no shots to play yet.");
        stopTour();
        return;
      }
      p.data = data;
      if (p.from >= data.seconds - 1 / data.fps) p.from = 0;
      if (data.frames.some((f) => f.fx?.kind === "plan_lift")) void loadPlan(p);
      engine.invalidate();
    },
    (e) => {
      if (playing !== p) return;
      useApp.getState().toast("error", `The tour cannot play: ${e?.message ?? e}`);
      stopTour();
    },
  );
}

/** The plan of the lowest level, for a plan lift, when the plan view can draw it. */
async function loadPlan(p: Playing): Promise<void> {
  const capture = useApp.getState().capturePlan;
  const doc = useApp.getState().doc;
  if (!capture || !doc || !p.overlay) return;
  const lowest = [...doc.project.levels].sort((a, b) => a.elevation_mm - b.elevation_mm)[0];
  try {
    const url = await capture(lowest?.id ?? null);
    if (playing === p) p.overlay.setPlan(url);
  } catch {
    // No plan view on screen: the lift is the camera alone.
  }
}

/** Stops the tour that is playing, if any. The camera stays where the tour left it. */
export function stopTour(): void {
  const p = playing;
  if (!p) return;
  playing = null;
  p.off();
  p.overlay?.dispose();
  p.showPath();
  p.engine.setTourDoors(null);
  if (p.lightKey) useViewer.getState().setLight(p.savedLight);
  // Hand the camera back through a saved-view jump, which opens the polar
  // limit for a view that looks up and closes it once the user orbits back.
  const engine = p.engine;
  engine.controls.maxPolarAngle = p.savedPolar;
  const pos = engine.camera.position;
  const tgt = engine.controls.target;
  engine.flyToCamera(
    {
      position: { x: pos.x * 1000, y: -pos.z * 1000, z: pos.y * 1000 },
      target: { x: tgt.x * 1000, y: -tgt.z * 1000, z: tgt.y * 1000 },
      fov_deg: engine.camera.fov,
    },
    0,
  );
  const tours = useTours.getState();
  tours.setPlaying(false);
  markTourPath(tours.playhead);
}

/** True while a tour plays in this engine. */
export function tourPlaying(engine: ViewerEngine): boolean {
  return playing?.engine === engine;
}

/**
 * Moves the playing tour to `now` (the frame's timestamp, ms). True while a
 * tour still moves, so the engine asks for the next frame. False: nothing
 * is playing.
 */
export function stepTour(engine: ViewerEngine, now: number): boolean {
  const p = playing;
  if (!p || p.engine !== engine) return false;
  // Still loading, or leaving a walk: the load or the next input asks again.
  if (!p.data) return false;
  if (now < p.notBefore) return true;
  const data = p.data;
  if (p.t0 === 0) p.t0 = now - p.from * 1000;
  const t = Math.min((now - p.t0) / 1000, data.seconds);
  const at = poseAt(data, t);
  const frame = at.frame;

  // A dissolve or dip holds the last frame of the shot before: draw it once.
  const layers = fxLayers(frame.fx);
  const from = frame.fx?.from_frame;
  if (layers.held > 0 && from !== undefined && from !== p.held && p.overlay) {
    const f = data.frames[from];
    setCamera(engine, f.position, f.target, f.fov_deg);
    engine.setTourDoors(doorsAt(data, f.t));
    const { width, height } = engine.viewSize();
    try {
      engine.captureCanvas(Math.max(2, width), Math.max(2, height), (c) => p.overlay?.hold(c, width, height));
    } catch {
      // The view went away mid tour: nothing to hold.
    }
    p.held = from;
  }
  setCamera(engine, at.position, at.target, at.fov);
  engine.setTourDoors(doorsAt(data, t));

  // Day to dusk: the frame's light, while it has one; the live light again after.
  if (frame.light) {
    const key = lightKey(frame.light);
    if (key !== p.lightKey) {
      p.lightKey = key;
      useViewer.getState().setLight(fromViewLight(frame.light));
    }
  } else if (p.lightKey) {
    p.lightKey = "";
    useViewer.getState().setLight(p.savedLight);
  }
  p.overlay?.apply(layers, data.seconds > 0 ? t / data.seconds : 1);

  // The panel's playhead, about 20 times a second.
  if (now - p.lastPlayheadAt > 48 || t >= data.seconds) {
    p.lastPlayheadAt = now;
    useTours.getState().setPlayhead(t);
  }
  if (t >= data.seconds) {
    // The last frame is drawn this frame; the tour ends with it.
    useTours.getState().setPlayhead(0);
    queueMicrotask(() => {
      if (playing === p) stopTour();
    });
    return true;
  }
  return true;
}

/**
 * Shows the frame at `t` seconds without playing: the timeline's scrub bar.
 * A tour that plays jumps there and plays on.
 */
export async function seekTour(tourId: string, t: number): Promise<void> {
  const p = playing;
  if (p && p.tourId === tourId && p.data) {
    p.t0 = performance.now() - t * 1000;
    p.engine.invalidate();
    return;
  }
  markTourPath(t);
  const engine = useLiveView.getState().engine;
  if (!engine) return;
  const data = await tourFrames(tourId);
  if (data.frame_count === 0 || playing) return;
  const at = poseAt(data, Math.min(Math.max(t, 0), data.seconds));
  // Looking through the tour's camera: its own path would only be in the
  // way, and its doors stand as the tour opens them. Both come back once the
  // user takes the camera back.
  if (!scrubShow) {
    const show = hideTourPath();
    const canvas = engine.renderer.domElement;
    const back = () => {
      canvas.removeEventListener("pointerdown", back);
      canvas.removeEventListener("wheel", back);
      scrubShow = null;
      engine.setTourDoors(null);
      show();
    };
    canvas.addEventListener("pointerdown", back);
    canvas.addEventListener("wheel", back, { passive: true });
    scrubShow = back;
  }
  // A scrub may look up a stair: the polar limit opens for it, as for a saved view.
  engine.flyToCamera({ position: at.position, target: at.target, fov_deg: at.fov }, 0);
  engine.setTourDoors(doorsAt(data, Math.min(Math.max(t, 0), data.seconds)));
}

/** Shows the path again after a scrub hid it; null while it shows. */
let scrubShow: (() => void) | null = null;

/** The panel closed or another tour opened: the path's scrub hide ends. */
export function endScrub(): void {
  scrubShow?.();
}
