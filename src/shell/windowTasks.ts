// Answers the backend's window requests (docs/CONTRACT.md, "App events and
// window requests"; DECISIONS D31). An MCP client asked for a render, a
// capture of the 3D view or a picture of the plan, and only this window has
// the renderer and the plan canvas. Every request gets exactly one
// `window_reply`: the result, or the reason it could not be done.
//
// `startWindowTasks` runs once from App.tsx, so a request is answered from
// the hub too: with `no_document`.

import type { Camera, IpcError, ViewVisibility, WindowReply, WindowRequest } from "../contract/bindings";
import { call, ipc, onAppEvent, toIpcError } from "../contract/ipc";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { dur } from "../ui/motion";
import { useViewer } from "../viewer3d/viewerStore";

/** Longest side of the preview image an MCP client gets back. Larger images
 * cost the model more and read no better. */
export const PREVIEW_MAX_PX = 1568;

function fail(code: string, message: string): never {
  throw { code, message, element_ids: [] } satisfies IpcError;
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => window.setTimeout(r, ms));
}

/** Longest side of `w` x `h` scaled to fit `max`, never enlarged. */
export function fitSize(w: number, h: number, max: number): { width: number; height: number } {
  const scale = Math.min(1, max / Math.max(1, w, h));
  return { width: Math.max(1, Math.round(w * scale)), height: Math.max(1, Math.round(h * scale)) };
}

/**
 * A copy of an image data URL at most `max` px on its long side. Photos
 * (renders) become JPEG, which is a tenth of the size; line drawings (the
 * plan) stay PNG so thin lines stay crisp.
 */
function preview(dataUrl: string, max: number, kind: "photo" | "drawing"): Promise<string> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      const { width, height } = fitSize(img.naturalWidth, img.naturalHeight, max);
      const canvas = document.createElement("canvas");
      canvas.width = width;
      canvas.height = height;
      const ctx = canvas.getContext("2d");
      if (!ctx) return resolve(dataUrl);
      ctx.imageSmoothingQuality = "high";
      if (kind === "photo") {
        // JPEG has no transparency: keep the sky, not black.
        ctx.fillStyle = "#ffffff";
        ctx.fillRect(0, 0, width, height);
      }
      ctx.drawImage(img, 0, 0, width, height);
      resolve(kind === "photo" ? canvas.toDataURL("image/jpeg", 0.86) : canvas.toDataURL("image/png"));
    };
    img.onerror = () => resolve(dataUrl);
    img.src = dataUrl;
  });
}

/** Waits until `ready()` is true, checking every 60 ms, for at most `ms`. */
async function waitUntil(ready: () => boolean, ms: number): Promise<boolean> {
  const start = Date.now();
  while (!ready()) {
    if (Date.now() - start > ms) return false;
    await sleep(60);
  }
  return true;
}

/** The 3D view on screen (the plan-only view becomes split), mounted and ready. */
async function need3dView(): Promise<void> {
  const app = useApp.getState();
  if (app.viewMode === "2d") app.setViewMode("split");
  if (!(await waitUntil(() => useApp.getState().captureView !== null, 8000))) {
    fail("invalid", "The 3D view in Guhit Studio did not come up. Open it and try again.");
  }
  // Walking or flying owns the camera; a capture is taken from orbit.
  if (useViewer.getState().nav !== "orbit") useViewer.getState().setNav("orbit");
}

function cameraOf(id: string) {
  const el = useApp.getState().doc?.project.elements.find((e) => e.id === id);
  if (!el || el.kind !== "camera") fail("not_found", `No saved view has id "${id}".`);
  return el;
}

async function capturePlan(levelId: string | null): Promise<WindowReply> {
  const app = useApp.getState();
  const before = app.viewMode;
  // The plan canvas registers its capture only while it is on screen.
  if (!app.capturePlan) app.setViewMode("split");
  try {
    if (!(await waitUntil(() => useApp.getState().capturePlan !== null, 8000))) {
      fail("invalid", "The plan view in Guhit Studio did not come up. Open it and try again.");
    }
    const png = await useApp.getState().capturePlan!(levelId);
    const doc = useApp.getState().doc;
    const level = doc?.project.levels.find((l) => l.id === (levelId ?? useApp.getState().activeLevelId));
    return {
      render_ids: [],
      image: await preview(png, PREVIEW_MAX_PX, "drawing"),
      note: level ? `The plan of ${level.name}, as the window draws it.` : "The plan, as the window draws it.",
    };
  } finally {
    if (useApp.getState().viewMode !== before) useApp.getState().setViewMode(before);
  }
}

/** `unsaved`: a camera that is not a saved view (a tour preview, the `look` camera), used when `cameraId` is null. */
async function captureView(cameraId: string | null, visibility: ViewVisibility | null, unsaved: Camera | null = null): Promise<WindowReply> {
  await need3dView();
  const view = cameraId ? cameraOf(cameraId) : unsaved;
  // A tour video exporting holds the view where it is (GR3 P1-1): the
  // picture is taken from the asked camera off screen instead of flying there.
  const { viewHeld } = await import("../viewer3d/tour/tourStore");
  const held = view !== null && viewHeld();
  if (view && !held) {
    bus.emit("apply_camera", view);
    // The camera flies to the view; capture once it has landed.
    await sleep(dur("scene") + 250);
  }
  if (!useApp.getState().captureView) fail("invalid", "The 3D view in Guhit Studio closed. Open it and try again.");
  useViewer.getState().flashCapture();
  // The roof and cutaway the task asks for, or the model's defaults: never
  // this window's toggles, which the client cannot see. The window's own
  // state comes back right after.
  const { captureForTask } = await import("../viewer3d/render/liveView");
  const shot = captureForTask(visibility, held ? view : null);
  if (!shot) fail("invalid", "The 3D view in Guhit Studio closed. Open it and try again.");
  const { png, camera } = shot;
  // The record keeps the roof and cutaway the capture showed, and the AI
  // render's conditioning maps show the same (L6).
  const conditioning = await import("../viewer3d/render/conditioning").catch(() => null);
  const record = await ipc.renderCapture(camera, png, { info: conditioning?.captureInfo(png, shot.visibility) });
  useViewer.getState().bumpRenders();
  void conditioning?.queueMapsFor(record, shot.visibility).catch(() => undefined);
  const { visibilityLabel } = await import("../viewer3d/render/renderQueue");
  return {
    render_ids: [record.id],
    image: await preview(png, PREVIEW_MAX_PX, "photo"),
    note: `Captured the 3D view${camera.name ? ` (${camera.name})` : ""}, ${visibilityLabel(shot.visibility)}, and saved it to Visuals.${held ? " A tour video is exporting, so the window's view stayed where it was and this picture was taken off screen from the camera asked for." : ""}`,
  };
}

// ------------------------------------------------- the render queue's report

/**
 * What the window's render queue is doing, as the backend keeps it for MCP
 * clients (`window_status`, G4 problem 8): `render` status, and a busy
 * window's refusal, say it. The fields of `WindowActivity` (window.rs).
 */
export interface WindowStatus {
  busy: boolean;
  what: string;
  view: string | null;
  index: number | null;
  total: number | null;
  progress_pct: number | null;
  eta_s: number | null;
  job_id: string | null;
}

/** The parts of the render queue's state the report reads. */
export interface QueueView {
  running: boolean;
  current: { index: number; total: number; name: string } | null;
  progress: { samples: number; targetSamples: number; elapsedMs: number; budgetMs: number; etaMs: number | null } | null;
}

/** The report for one state of the render queue. Pure. */
export function queueStatus(q: QueueView, jobId: string | null): WindowStatus {
  if (!q.running) return { busy: false, what: "", view: null, index: null, total: null, progress_pct: null, eta_s: null, job_id: null };
  const p = q.progress;
  const share = p ? Math.max(p.targetSamples > 0 ? p.samples / p.targetSamples : 0, p.budgetMs > 0 ? p.elapsedMs / p.budgetMs : 0) : null;
  return {
    busy: true,
    what: jobId ? "Rendering for an MCP client" : "Rendering",
    view: q.current?.name ?? null,
    index: q.current?.index ?? null,
    total: q.current?.total ?? null,
    progress_pct: share === null ? null : Math.round(Math.min(1, share) * 1000) / 10,
    eta_s: p?.etaMs == null ? null : Math.round(p.etaMs / 100) / 10,
    job_id: jobId,
  };
}

/** The parts of a tour export's state the report reads (L9's `TourExport`). */
export interface ExportView {
  done: number;
  total: number;
  phase: string;
}

/** The report while a tour video exports: frames done of all, and the time
 * left at the rate so far. Pure. */
export function exportStatus(e: ExportView | null, startedMs: number, nowMs: number, jobId: string): WindowStatus {
  const share = e && e.total > 0 ? Math.min(1, e.done / e.total) : null;
  const elapsed = (nowMs - startedMs) / 1000;
  const eta = share !== null && share >= 0.02 ? (elapsed * (1 - share)) / share : null;
  return {
    busy: true,
    what: e?.phase ? `Exporting a tour video (${e.phase.toLowerCase()})` : "Exporting a tour video",
    view: null,
    index: e && e.total > 0 ? e.done : null,
    total: e && e.total > 0 ? e.total : null,
    progress_pct: share === null ? null : Math.round(share * 1000) / 10,
    eta_s: eta === null ? null : Math.round(eta),
    job_id: jobId,
  };
}

/** The window request a render answers, while it runs. */
let renderJob: string | null = null;
/** Least time between two reports while a render goes on, ms. */
const REPORT_EVERY_MS = 2000;

function report(status: WindowStatus): void {
  // Best effort: an older backend without `window_status` just says no.
  call("window_status", status as unknown as Record<string, unknown>).catch(() => undefined);
}

let watching = false;

/**
 * Report the render queue as it goes, whoever started the render: at once
 * when it starts, moves to the next view or stops, else every 2 s. Loaded
 * with the editor, so the hub bundle stays small.
 */
async function watchRenderQueue(): Promise<void> {
  if (watching) return;
  watching = true;
  const { useRenderQueue } = await import("../viewer3d/render/renderQueue");
  let last = 0;
  let lastKey = "";
  useRenderQueue.subscribe((st) => {
    const status = queueStatus(st, renderJob);
    const key = `${status.busy}|${status.index}|${status.view}`;
    const now = Date.now();
    if (key === lastKey && now - last < REPORT_EVERY_MS) return;
    lastKey = key;
    last = now;
    report(status);
  });
}

async function render(task: Extract<WindowRequest["task"], { type: "render" }>, jobId: string): Promise<WindowReply> {
  for (const id of task.views) cameraOf(id);
  await need3dView();
  useApp.getState().toast("info", "Rendering for an MCP client. The images go to Visuals.");
  // Loaded with the 3D view; imported here so the hub bundle stays small.
  const { renderForRequest } = await import("../viewer3d/render/renderQueue");
  void watchRenderQueue();
  renderJob = jobId;
  let results;
  try {
    results = await renderForRequest(task.views, task.quality, task.size, task.visibility ?? null);
  } finally {
    renderJob = null;
  }
  const first = results[0];
  const data = await ipc.renderData(first.recordId);
  const { visibilityLabel } = await import("../viewer3d/render/renderQueue");
  // Each says the camera and the roof and cutaway it used (G4 problem 8).
  const notes = results.map((r) => {
    const name = r.camera?.name || r.name;
    const how = r.kind === "path_traced" ? `path traced, ${Math.round(r.samples)} samples in ${Math.round(r.seconds)} s` : `enhanced capture${r.note ? ` (${r.note})` : ""}`;
    return `${name}: ${how}, ${visibilityLabel(r.visibility)}`;
  });
  return {
    render_ids: results.map((r) => r.recordId),
    image: await preview(data, PREVIEW_MAX_PX, "photo"),
    note: `Saved to Visuals. ${notes.join("; ")}.`,
  };
}

/**
 * The window's copy of the document can trail the engine by a moment: an MCP
 * client that just saved a view and asks for its render at once. Catch up
 * first, so the task sees what the client just changed.
 */
async function catchUp(): Promise<void> {
  const fresh = await ipc.docState();
  const current = useApp.getState().doc;
  if (fresh && current && fresh.project.id === current.project.id && fresh.revision !== current.revision) {
    useApp.getState().setDoc(fresh);
  }
}

/** Does one request. Throws an IpcError-shaped object when it cannot. */
export async function runWindowTask(request: WindowRequest): Promise<WindowReply> {
  const app = useApp.getState();
  if (app.screen !== "editor" || !app.doc) {
    fail("no_document", "Guhit Studio shows the project list. Open the project first.");
  }
  await catchUp();
  const task = request.task;
  switch (task.type) {
    case "capture_plan":
      return capturePlan(task.level_id);
    case "capture_view":
      return captureView(task.camera_id, task.visibility ?? null, task.camera ?? null);
    case "render":
      return render(task, request.id);
    // Wave 2: L6's conditioning maps and L9's tour export, each in its own
    // module, loaded when asked for.
    case "capture_maps":
      return (await import("../viewer3d/render/conditioning")).captureMaps(task.render_id);
    case "export_tour": {
      // Frames done of all, from L9's exporter, every 2 s.
      const { useTours } = await import("../viewer3d/tour/tourStore");
      const started = Date.now();
      let last = 0;
      report(exportStatus(null, started, started, request.id));
      const off = useTours.subscribe((st) => {
        const now = Date.now();
        if (now - last < REPORT_EVERY_MS) return;
        last = now;
        report(exportStatus(st.exporting, started, now, request.id));
      });
      try {
        return await (await import("../viewer3d/tour/export")).exportTour(task.tour_id, task.size);
      } finally {
        off();
        report(queueStatus({ running: false, current: null, progress: null }, null));
      }
    }
  }
}

async function answer(request: WindowRequest): Promise<void> {
  let reply: WindowReply | null = null;
  let error: IpcError | null = null;
  try {
    reply = await runWindowTask(request);
  } catch (e) {
    error = toIpcError(e);
  }
  try {
    await ipc.windowReply(request.id, reply, error);
  } catch {
    // Another window answered first, or the backend gave up waiting.
  }
}

let stop: (() => void) | null = null;

/** Starts answering window requests. Calling it again does nothing. */
export function startWindowTasks(): void {
  if (stop) return;
  const offEvents = onAppEvent((event) => {
    if (event.type === "window_request") void answer(event.request);
  });
  // Renders the user starts are reported too, once the editor is up.
  const offScreen = useApp.subscribe((st) => {
    if (st.screen === "editor") void watchRenderQueue();
  });
  if (useApp.getState().screen === "editor") void watchRenderQueue();
  stop = () => {
    offEvents();
    offScreen();
  };
}

if (import.meta.hot) import.meta.hot.dispose(() => stop?.());
