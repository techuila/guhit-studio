// Answers the backend's window requests (docs/CONTRACT.md, "App events and
// window requests"; DECISIONS D31). An MCP client asked for a render, a
// capture of the 3D view or a picture of the plan, and only this window has
// the renderer and the plan canvas. Every request gets exactly one
// `window_reply`: the result, or the reason it could not be done.
//
// `startWindowTasks` runs once from App.tsx, so a request is answered from
// the hub too: with `no_document`.

import type { IpcError, WindowReply, WindowRequest } from "../contract/bindings";
import { ipc, onAppEvent, toIpcError } from "../contract/ipc";
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

async function captureView(cameraId: string | null): Promise<WindowReply> {
  await need3dView();
  if (cameraId) {
    bus.emit("apply_camera", cameraOf(cameraId));
    // The camera flies to the saved view; capture once it has landed.
    await sleep(dur("scene") + 250);
  }
  const capture = useApp.getState().captureView;
  if (!capture) fail("invalid", "The 3D view in Guhit Studio closed. Open it and try again.");
  useViewer.getState().flashCapture();
  const { png, camera } = await capture();
  const record = await ipc.renderCapture(camera, png);
  useViewer.getState().bumpRenders();
  return {
    render_ids: [record.id],
    image: await preview(png, PREVIEW_MAX_PX, "photo"),
    note: `Captured the 3D view${camera.name ? ` (${camera.name})` : ""} and saved it to Visuals.`,
  };
}

async function render(task: Extract<WindowRequest["task"], { type: "render" }>): Promise<WindowReply> {
  for (const id of task.views) cameraOf(id);
  await need3dView();
  useApp.getState().toast("info", "Rendering for an MCP client. The images go to Visuals.");
  // Loaded with the 3D view; imported here so the hub bundle stays small.
  const { renderForRequest } = await import("../viewer3d/render/renderQueue");
  const results = await renderForRequest(task.views, task.quality, task.size);
  const first = results[0];
  const data = await ipc.renderData(first.recordId);
  const notes = results.map((r) =>
    r.kind === "path_traced"
      ? `${r.name}: path traced, ${Math.round(r.samples)} samples in ${Math.round(r.seconds)} s`
      : `${r.name}: enhanced capture${r.note ? ` (${r.note})` : ""}`,
  );
  return {
    render_ids: results.map((r) => r.recordId),
    image: await preview(data, PREVIEW_MAX_PX, "photo"),
    note: `Saved to Visuals. ${notes.join("; ")}.`,
  };
}

/** Does one request. Throws an IpcError-shaped object when it cannot. */
export async function runWindowTask(request: WindowRequest): Promise<WindowReply> {
  const app = useApp.getState();
  if (app.screen !== "editor" || !app.doc) {
    fail("no_document", "Guhit Studio shows the project list. Open the project first.");
  }
  const task = request.task;
  switch (task.type) {
    case "capture_plan":
      return capturePlan(task.level_id);
    case "capture_view":
      return captureView(task.camera_id);
    case "render":
      return render(task);
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
  stop = onAppEvent((event) => {
    if (event.type === "window_request") void answer(event.request);
  });
}

if (import.meta.hot) import.meta.hot.dispose(() => stop?.());
