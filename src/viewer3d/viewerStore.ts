// View-only state of the 3D viewer. Lives here because the app store has no
// fields for it (see "Contract requests" in the viewer agent report).

import { create } from "zustand";
import type { SkyKind, Vec3 } from "../contract/bindings";
import { bus } from "../state/bus";
import { useApp } from "../state/store";

/** How the 3D camera moves. Walk: eye height on the active level, walls
 * block. Fly: free, nothing blocks. While not "orbit" the 3D view owns the
 * keyboard (WASD, arrows, Shift, Escape): the global shortcut handler ignores
 * every key without MOD, and Escape returns to orbit. */
export type NavMode = "orbit" | "walk" | "fly";
/** How the building shell (walls, roof, slabs, openings, columns, stairs,
 * objects) is drawn, so pipes stay visible. Pipes are always solid. */
export type ShellMode = "solid" | "xray" | "hidden";

const SHELL_ORDER: ShellMode[] = ["solid", "xray", "hidden"];

/** Fixtures: on after sunset (auto), always on, or off. */
/** Auto (lit from dusk to dawn), on or off: the contract's `LampMode`. */
export type { LampMode } from "../contract/bindings";
import type { LampMode } from "../contract/bindings";

/** The live light of the 3D view: local time at the project's site
 * (`ProjectSettings::site`, Manila by default). A saved view carries its own
 * (`Camera::light`) and applying it copies it here. Never saved in the project
 * and never an undo step. docs/CONTRACT.md, "Sun and light". */
export interface LiveLight {
  /** 1 to 12. */
  month: number;
  /** 1 to 31. */
  day: number;
  /** Minutes after local midnight, 0 to 1439. */
  minutes: number;
  sky: SkyKind;
  /** Exposure offset in EV. null: auto exposure. */
  exposureEv: number | null;
  lamps: LampMode;
}

function todayAt(minutes: number): LiveLight {
  // Philippine time (UTC+8) is the default site's; the 3D view corrects the
  // clock for other sites when it computes the sun.
  const ph = new Date(Date.now() + 8 * 3600e3);
  return { month: ph.getUTCMonth() + 1, day: ph.getUTCDate(), minutes, sky: "clear", exposureEv: null, lamps: "auto" };
}

/** Walk settings, remembered on this computer (localStorage), not in projects. */
export interface WalkSettings {
  /** Eye height above the floor, 800 to 2500 mm. */
  eyeHeightMm: number;
  /** Walking speed in m/s; Shift runs faster. */
  speed: number;
}

const WALK_KEY = "guhit.walk";
const WALK_DEFAULT: WalkSettings = { eyeHeightMm: 1600, speed: 1.4 };
function loadWalk(): WalkSettings {
  try {
    const raw = globalThis.localStorage?.getItem(WALK_KEY);
    if (!raw) return WALK_DEFAULT;
    const v = JSON.parse(raw) as Partial<WalkSettings>;
    return {
      eyeHeightMm: Math.min(2500, Math.max(800, Number(v.eyeHeightMm) || WALK_DEFAULT.eyeHeightMm)),
      speed: Math.min(6, Math.max(0.3, Number(v.speed) || WALK_DEFAULT.speed)),
    };
  } catch {
    return WALK_DEFAULT;
  }
}

/** A `walk_to` waiting for the 3D view. `seq` tells two requests for the same finding apart. */
export interface WalkRequest {
  seq: number;
  ids: string[];
  location: Vec3 | null;
}

export interface ViewerState {
  roofVisible: boolean;
  cutaway: boolean;
  shadows: boolean;
  nav: NavMode;
  shell: ShellMode;
  setNav: (nav: NavMode) => void;
  setShell: (shell: ShellMode) => void;
  /** Solid, then X-ray, then hidden, then solid. */
  cycleShell: () => void;
  /** The latest `walk_to`, consumed by the 3D view when it has a model. */
  walkRequest: WalkRequest | null;
  /** Enters walk mode near these elements (the `walk_to` bus event lands here). */
  requestWalk: (ids: string[], location: Vec3 | null) => void;
  /** Live sun, sky, exposure and lamps. */
  light: LiveLight;
  setLight: (patch: Partial<LiveLight>) => void;
  /** Refine the picture when the camera rests: soft sun shadows and clean
   * edges, then stop drawing. On by default. */
  refine: boolean;
  setRefine: (on: boolean) => void;
  /** Draw the sun path over the 3D view (the Jun 21 and Dec 21 arcs, the day's arc, a compass). */
  sunPath: boolean;
  toggleSunPath: () => void;
  walk: WalkSettings;
  setWalk: (patch: Partial<WalkSettings>) => void;
  /** The walk overlay's settings popover (eye height and speed) is open. It
   * closes when walking ends. */
  walkSettingsOpen: boolean;
  setWalkSettingsOpen: (open: boolean) => void;
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
  nav: "orbit",
  shell: "solid",
  setNav: (nav) => set({ nav }),
  setShell: (shell) => set({ shell }),
  cycleShell: () => set((s) => ({ shell: SHELL_ORDER[(SHELL_ORDER.indexOf(s.shell) + 1) % SHELL_ORDER.length] })),
  light: todayAt(600),
  setLight: (patch) => set((s) => ({ light: { ...s.light, ...patch } })),
  refine: true,
  setRefine: (on) => set({ refine: on }),
  sunPath: false,
  toggleSunPath: () => set((s) => ({ sunPath: !s.sunPath })),
  walk: loadWalk(),
  setWalk: (patch) =>
    set((s) => {
      const walk = { ...s.walk, ...patch };
      try {
        globalThis.localStorage?.setItem(WALK_KEY, JSON.stringify(walk));
      } catch {
        /* private mode or blocked storage: the setting lasts this session */
      }
      return { walk };
    }),
  walkSettingsOpen: false,
  setWalkSettingsOpen: (open) => set({ walkSettingsOpen: open }),
  walkRequest: null,
  requestWalk: (ids, location) =>
    set((s) => ({ nav: "walk", walkRequest: { seq: (s.walkRequest?.seq ?? 0) + 1, ids: [...ids], location } })),
  rendersVersion: 0,
  captureFlash: 0,
  toggleRoof: () => set((s) => ({ roofVisible: !s.roofVisible })),
  toggleCutaway: () => set((s) => ({ cutaway: !s.cutaway })),
  toggleShadows: () => set((s) => ({ shadows: !s.shadows })),
  bumpRenders: () => set((s) => ({ rendersVersion: s.rendersVersion + 1 })),
  flashCapture: () => set((s) => ({ captureFlash: s.captureFlash + 1 })),
}));

// `walk_to` is listened to here, not in the 3D view: the view is not mounted
// while the app shows the plan only. The request waits in the store, the view
// switches to split, and the 3D view walks there once it has a model.
const offWalkTo = bus.on("walk_to", ({ ids, location }) => {
  const app = useApp.getState();
  if (app.viewMode === "2d") app.setViewMode("split");
  useViewer.getState().requestWalk(ids, location);
});

if (import.meta.hot) import.meta.hot.dispose(() => offWalkTo());
