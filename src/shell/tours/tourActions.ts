// What the Tours panel, the tour fields and the tours section do (lane L9).
// Every edit is one typed command: `make_tour`, or `update_element` with the
// whole tour (shots carry their own ids).

import type { AutoTour, Element, RenderSize, Shot, Tour, TransitionKind } from "../../contract/bindings";
import { bus } from "../../state/bus";
import { useApp } from "../../state/store";
import { useLiveView } from "../../viewer3d/render/liveView";
import { useTours } from "../../viewer3d/tour/tourStore";
import { updateElement } from "../commitLatest";

export type TourEl = Extract<Element, { kind: "tour" }>;

export const TRANSITIONS: Array<{ value: TransitionKind; label: string; short: string }> = [
  { value: "cut", label: "Cut", short: "Cut" },
  { value: "dissolve", label: "Dissolve", short: "Dis" },
  { value: "dip_black", label: "Dip to black", short: "Blk" },
  { value: "dip_white", label: "Dip to white", short: "Wht" },
  { value: "whip_pan", label: "Whip pan", short: "Whip" },
  { value: "plan_lift", label: "Plan lift", short: "Plan" },
];

export const MOVES: Array<{ value: Shot["kind"]; label: string }> = [
  { value: "fly_in", label: "Fly in" },
  { value: "orbit", label: "Orbit" },
  { value: "push_in", label: "Push in" },
  { value: "pull_out", label: "Pull out" },
  { value: "truck", label: "Truck" },
  { value: "pedestal", label: "Pedestal" },
  { value: "walk", label: "Walk" },
  { value: "hold", label: "Hold" },
  { value: "path", label: "Path" },
];

export function toursOf(elements: Element[]): TourEl[] {
  return elements.filter((e): e is TourEl => e.kind === "tour");
}

export function tourSeconds(t: Pick<Tour, "shots">): number {
  return t.shots.reduce((sum, s) => sum + s.seconds, 0);
}

/** "1:05", "0:08.5" for a timeline. */
export function clock(seconds: number, tenths = false): string {
  const s = Math.max(0, seconds);
  const m = Math.floor(s / 60);
  const rest = s - m * 60;
  const sec = tenths ? rest.toFixed(1).padStart(4, "0") : String(Math.floor(rest)).padStart(2, "0");
  return `${m}:${sec}`;
}

/** A shot length as typed or dragged: 0.1 s steps, 0.1 to 600 s. */
export function cleanSeconds(v: number): number {
  if (!Number.isFinite(v)) return 0.1;
  return Math.min(600, Math.max(0.1, Math.round(v * 10) / 10));
}

/** The shot length a drag of `dx` pixels gives at `pxPerSecond`, the drag tracking the pointer 1:1. */
export function draggedSeconds(start: number, dx: number, pxPerSecond: number): number {
  const raw = start + dx / Math.max(pxPerSecond, 1e-6);
  return Math.min(600, Math.max(0.1, raw));
}

/** The tour with one shot replaced. */
export function withShot(t: TourEl, index: number, patch: Partial<Shot>): TourEl {
  return { ...t, shots: t.shots.map((s, i) => (i === index ? { ...s, ...patch } : s)) };
}

export function saveTour(before: TourEl, after: TourEl): void {
  void updateElement(before, after);
}

/** `make_tour` with the panel's options; selects the new tour. */
export async function makeTour(options: AutoTour, name: string | null = null): Promise<string | null> {
  const app = useApp.getState();
  const before = new Set(toursOf(app.doc?.project.elements ?? []).map((t) => t.id));
  const result = await app.dispatch({ type: "make_tour", name, options });
  if (!result) return null;
  const made = toursOf(result.state.project.elements).find((t) => !before.has(t.id));
  if (made) {
    useTours.getState().select(made.id);
    if (result.diff.summary) useApp.getState().toast("success", result.diff.summary);
  }
  return made?.id ?? null;
}

/** Plays a tour in the 3D view, opening it when only the plan shows. */
export function playTour(tourId: string): void {
  const app = useApp.getState();
  useTours.getState().select(tourId);
  if (useLiveView.getState().engine) {
    bus.emit("play_tour", { tour_id: tourId });
    return;
  }
  if (app.viewMode === "2d") app.setViewMode("split");
  const start = Date.now();
  const wait = () => {
    if (useLiveView.getState().engine) bus.emit("play_tour", { tour_id: tourId });
    else if (Date.now() - start < 8000) window.setTimeout(wait, 60);
  };
  wait();
}

/** Exports a tour from the panel; failures and the result are toasts. */
export async function exportFromPanel(tourId: string, size: RenderSize): Promise<void> {
  try {
    const { exportTour } = await import("../../viewer3d/tour/export");
    await exportTour(tourId, size);
  } catch (e) {
    const err = e as { code?: string; message?: string };
    if (err?.code === "cancelled") useApp.getState().toast("info", "The tour video export was stopped.");
    else useApp.getState().toast("error", `The tour video did not export: ${err?.message ?? String(e)}`);
  }
}

/** A still shot from the 3D view as it is now, after shot `after` (or at the end). */
export function shotFromView(t: TourEl, after: number | null): TourEl | null {
  const engine = useLiveView.getState().engine;
  if (!engine) return null;
  const cam = engine.currentCamera("View");
  const shot: Shot = {
    id: "",
    name: `Shot ${t.shots.length + 1}`,
    kind: "hold",
    keys: [{ at: 0, position: cam.position, target: cam.target, fov_deg: cam.fov_deg }],
    seconds: 3,
    ease: "in_out",
    light: null,
    light_end: null,
    transition: { kind: "dissolve", seconds: 0.8 },
  };
  const at = after === null ? t.shots.length : after + 1;
  return { ...t, shots: [...t.shots.slice(0, at), shot, ...t.shots.slice(at)] };
}
