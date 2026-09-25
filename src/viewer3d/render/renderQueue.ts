// The Render button's queue: answers `bus.emit("render", { views })` (the 3D
// toolbar, the Visuals panel, the palette), renders the views one after the
// other with render/renderJob.ts and saves each result to the Visuals gallery
// as a `RenderRecord` with the camera and light it used (docs/CONTRACT.md,
// "Render"). Esc cancels; "Stop and save" keeps what is there.
//
// The listener is installed when this module loads, which the 3D view and the
// Visuals panel both do: whichever is mounted, a render request lands.

import { create } from "zustand";
import type { Camera, DocState } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";
import { bus } from "../../state/bus";
import { useApp } from "../../state/store";
import { isModalOpen } from "../../ui/Dialog";
import { fromViewLight, toViewLight } from "../light/model";
import { useViewer, type LiveLight } from "../viewerStore";
import { useLiveView } from "./liveView";
import type { RenderKind } from "../../contract/bindings";
import { QUALITIES, RENDER_SIZES, RenderJob, type RenderProgress, type TraceQuality, type RenderSizeKey } from "./renderJob";

const PREFS_KEY = "guhit.render";

interface Prefs {
  size: RenderSizeKey;
  quality: TraceQuality;
}

function loadPrefs(): Prefs {
  try {
    const raw = globalThis.localStorage?.getItem(PREFS_KEY);
    const v = raw ? (JSON.parse(raw) as Partial<Prefs>) : {};
    return {
      size: RENDER_SIZES.some((s) => s.key === v.size) ? (v.size as RenderSizeKey) : "hd",
      quality: v.quality === "final" ? "final" : "quick",
    };
  } catch {
    return { size: "hd", quality: "quick" };
  }
}

function savePrefs(p: Prefs): void {
  try {
    globalThis.localStorage?.setItem(PREFS_KEY, JSON.stringify(p));
  } catch {
    /* the choice lasts this session */
  }
}

export interface RenderTask {
  camera: Camera;
  light: LiveLight;
  lockedEv: number;
  /** The live view's walk switches, for the current view only. */
  overrides: ReadonlyMap<string, boolean>;
}

export interface RenderResult {
  recordId: string;
  kind: RenderKind;
  name: string;
  samples: number;
  seconds: number;
  /** Why it is an Enhanced capture, when it is one. */
  note: string;
}

interface RenderQueueState extends Prefs {
  running: boolean;
  /** Which view of how many, and its name. */
  current: { index: number; total: number; name: string } | null;
  progress: RenderProgress | null;
  /** The job's preview canvas, redrawn as the image refines. */
  preview: HTMLCanvasElement | null;
  previewVersion: number;
  last: RenderResult | null;
  error: string | null;
  setSize: (size: RenderSizeKey) => void;
  setQuality: (quality: TraceQuality) => void;
  start: (views: "current" | "all" | string[]) => void;
  /** Stops the render in progress: `save` keeps what is there, and the queue stops too. */
  stop: (save: boolean) => void;
  dismissLast: () => void;
}

let job: RenderJob | null = null;
let queueCancelled = false;

/** Cameras for a request, with the light each one renders with. */
function resolveTasks(views: "current" | "all" | string[], doc: DocState): RenderTask[] | string {
  const viewer = useViewer.getState();
  const live = useLiveView.getState().engine;
  const liveLocked = live?.lightRig().lockedEv() ?? viewer.light.exposureEv ?? 0;
  const savedTask = (el: Camera): RenderTask => {
    const light = el.light ? fromViewLight(el.light) : viewer.light;
    const lockedEv = el.light?.exposure_ev ?? liveLocked;
    const camera: Camera = { ...el, light: el.light ?? toViewLight(viewer.light, lockedEv) };
    return { camera, light, lockedEv, overrides: new Map() };
  };
  const cameras = doc.project.elements.filter((e): e is Extract<typeof e, { kind: "camera" }> => e.kind === "camera");
  if (views === "current") {
    if (!live) {
      const active = cameras.find((c) => c.id === useApp.getState().activeCameraId);
      if (active) return [savedTask(active)];
      return "Open the 3D view to render it.";
    }
    const camera = live.currentCamera(useLiveView.getState().label());
    return [
      {
        camera,
        light: viewer.light,
        lockedEv: liveLocked,
        overrides: live.lightRig().fixtureOverrides(),
      },
    ];
  }
  const picked = views === "all" ? cameras : cameras.filter((c) => views.includes(c.id));
  if (picked.length === 0) return views === "all" ? "Save a view first: Render all views renders every saved view." : "Those views are not in the project.";
  return picked.map(savedTask);
}

export const useRenderQueue = create<RenderQueueState>((set, get) => ({
  ...loadPrefs(),
  running: false,
  current: null,
  progress: null,
  preview: null,
  previewVersion: 0,
  last: null,
  error: null,

  setSize: (size) => {
    set({ size });
    savePrefs({ size, quality: get().quality });
  },
  setQuality: (quality) => {
    set({ quality });
    savePrefs({ size: get().size, quality });
  },

  start: (views) => {
    const app = useApp.getState();
    const doc = app.doc;
    if (!doc) return;
    if (get().running) {
      app.toast("info", "A render is already running. Stop it or wait for it to finish.");
      return;
    }
    const tasks = resolveTasks(views, doc);
    if (typeof tasks === "string") {
      app.toast("info", tasks);
      return;
    }
    void runQueue(tasks);
  },

  stop: (save) => {
    queueCancelled = true;
    job?.stop(save);
  },

  dismissLast: () => set({ last: null, error: null }),
}));

/** What one pass of the queue produced. */
interface QueueOutcome {
  results: RenderResult[];
  /** Set when a render failed; the queue stopped there. */
  error: string | null;
  /** The user pressed Esc (or Stop without saving) before the queue finished. */
  cancelled: boolean;
}

/** Size and quality for one pass, instead of the user's own choices. */
interface QueueOptions {
  size?: RenderSizeKey;
  quality?: TraceQuality;
}

/**
 * Renders for a window request (an MCP client, DECISIONS D31) with the same
 * queue the Render button uses, at the size and quality asked, without
 * changing the user's own choices. `views` are camera ids of saved views;
 * empty renders the current 3D view. Resolves with what was saved, in order;
 * throws an IpcError-shaped object when nothing could be rendered.
 */
export async function renderForRequest(views: string[], quality: TraceQuality, size: RenderSizeKey): Promise<RenderResult[]> {
  const doc = useApp.getState().doc;
  if (!doc) throw { code: "no_document", message: "No project is open in Guhit Studio.", element_ids: [] };
  if (useRenderQueue.getState().running) {
    throw { code: "invalid", message: "A render is already running in Guhit Studio. Wait for it or stop it, then try again.", element_ids: [] };
  }
  const tasks = resolveTasks(views.length === 0 ? "current" : views, doc);
  if (typeof tasks === "string") throw { code: "invalid", message: tasks, element_ids: [] };
  const outcome = await runQueue(tasks, { size, quality });
  if (outcome.results.length === 0) {
    const message = outcome.error ?? (outcome.cancelled ? "The render was cancelled in Guhit Studio." : "Nothing was rendered.");
    throw { code: "invalid", message, element_ids: [] };
  }
  return outcome.results;
}

async function runQueue(tasks: RenderTask[], opts: QueueOptions = {}): Promise<QueueOutcome> {
  const q = useRenderQueue;
  const app = useApp.getState();
  const outcome: QueueOutcome = { results: [], error: null, cancelled: false };
  queueCancelled = false;
  q.setState({ running: true, error: null, last: null, progress: null, preview: null });
  window.addEventListener("keydown", onKey, true);
  try {
    for (let i = 0; i < tasks.length; i++) {
      if (queueCancelled) break;
      const task = tasks[i];
      const doc = useApp.getState().doc;
      if (!doc) break;
      const size = RENDER_SIZES.find((s) => s.key === (opts.size ?? q.getState().size)) ?? RENDER_SIZES[0];
      const quality = opts.quality ?? q.getState().quality;
      const viewer = useViewer.getState();
      const startRevision = doc.revision;
      q.setState({ current: { index: i + 1, total: tasks.length, name: task.camera.name || "View" }, progress: null });
      const thisJob = new RenderJob(
        {
          doc,
          camera: task.camera,
          light: task.light,
          lockedEv: task.lockedEv,
          width: size.width,
          height: size.height,
          view: { cutaway: viewer.cutaway, roofVisible: viewer.roofVisible, activeLevelId: useApp.getState().activeLevelId },
          packModels: true,
          lampOverrides: task.overrides,
        },
        quality,
        (progress) => q.setState({ progress }),
        () => q.setState((st) => ({ previewVersion: st.previewVersion + 1 })),
      );
      job = thisJob;
      q.setState({ preview: thisJob.preview });
      let rendered;
      try {
        rendered = await thisJob.run();
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        q.setState({ error: `The render could not finish: ${message}` });
        app.toast("error", `The render could not finish: ${message}`);
        outcome.error = `The render could not finish: ${message}`;
        break;
      }
      if (!rendered) {
        app.toast("info", "Render cancelled");
        outcome.cancelled = true;
        break;
      }
      // The record keeps how it was made and the model it shows: the
      // revision the render started from.
      const record = await ipc.renderCapture(task.camera, rendered.png, {
        revision: startRevision,
        info: {
          kind: rendered.kind,
          width: rendered.width,
          height: rendered.height,
          samples: Math.max(0, Math.round(rendered.samples)),
          seconds: rendered.seconds,
          quality: rendered.kind === "path_traced" ? quality : null,
          gpu: rendered.gpu ?? "",
        },
      });
      useViewer.getState().bumpRenders();
      const result: RenderResult = {
        recordId: record.id,
        kind: rendered.kind,
        name: task.camera.name || "View",
        samples: rendered.samples,
        seconds: rendered.seconds,
        note: rendered.fallbackReason ?? "",
      };
      outcome.results.push(result);
      q.setState({ last: result });
      app.toast(
        "success",
        rendered.kind === "path_traced"
          ? `Render saved to Visuals: ${result.name}, ${rendered.samples} samples in ${Math.round(rendered.seconds)} s`
          : `Enhanced capture saved to Visuals: ${result.name}`,
      );
    }
  } finally {
    window.removeEventListener("keydown", onKey, true);
    job = null;
    q.setState({ running: false, current: null, progress: null });
  }
  return outcome;
}

/** Esc cancels the render, unless a dialog or walk mode has the key. */
function onKey(e: KeyboardEvent): void {
  if (e.key !== "Escape" || isModalOpen()) return;
  if (useViewer.getState().nav !== "orbit") return;
  const t = e.target as HTMLElement | null;
  if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
  e.preventDefault();
  e.stopPropagation();
  useRenderQueue.getState().stop(false);
}

/** Time left as "1 min 20 s" or "42 s". */
export function timeLeftLabel(ms: number | null): string {
  if (ms === null || !Number.isFinite(ms)) return "working out the time left";
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s left`;
  return `${Math.floor(s / 60)} min ${String(s % 60).padStart(2, "0")} s left`;
}

export { QUALITIES, RENDER_SIZES };

const offRender = bus.on("render", ({ views }) => useRenderQueue.getState().start(views));
if (import.meta.hot) import.meta.hot.dispose(() => offRender());
