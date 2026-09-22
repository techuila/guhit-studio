// Typed IPC client. CONTRACT FILE - owned by the orchestrator.
//
// Inside the desktop app every call goes through the single Tauri command
// `ipc`. In a plain browser (dev and tests) the same calls go over HTTP to
// the dev bridge, which runs the same Rust service. See docs/CONTRACT.md.

import type {
  AiRequest,
  AiResolveResult,
  AiSettings,
  AiTurn,
  ApplyResult,
  Camera,
  DwgConverterStatus,
  ImportInspection,
  ImportOptions,
  ImportResult,
  ModelFormat,
  CatalogItem,
  Command,
  DocState,
  ExportResult,
  IpcError,
  PlanExportOptions,
  PlanFormat,
  ProjectMeta,
  ProjectSettings,
  Query,
  RenderAiRequest,
  RenderAiResult,
  RenderAiSettings,
  RenderRecord,
  RenderStyle,
  SnapshotMeta,
} from "./bindings";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const BRIDGE_URL: string =
  (import.meta.env.VITE_BRIDGE_URL as string | undefined) ?? "http://localhost:1430";

export function isIpcError(e: unknown): e is IpcError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

/** Normalizes anything thrown by `call` into an IpcError. */
export function toIpcError(e: unknown): IpcError {
  if (isIpcError(e)) return e;
  return { code: "io", message: e instanceof Error ? e.message : String(e), element_ids: [] };
}

export async function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>("ipc", { cmd, args });
  }
  let res: Response;
  try {
    res = await fetch(`${BRIDGE_URL}/ipc/${cmd}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(args),
    });
  } catch {
    throw {
      code: "io",
      message: `Dev bridge not reachable at ${BRIDGE_URL}. Start it with: pnpm bridge`,
      element_ids: [],
    } satisfies IpcError;
  }
  const body: unknown = await res.json();
  if (!res.ok) throw body;
  return body as T;
}

/**
 * Subscribe to changes made to the open document by something other than this
 * window: the MCP server driven by Claude Code, or another client. The handler
 * receives the new revision; call `ipc.docState()` to refresh. Returns an
 * unsubscribe function. In the desktop app this is a Tauri event; over the dev
 * bridge it polls `doc_revision` once a second.
 */
export function onDocChanged(handler: (revision: number) => void): () => void {
  if (isTauri) {
    let stop: (() => void) | null = null;
    let cancelled = false;
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<{ revision: number }>("doc_changed", (e) => handler(e.payload.revision)).then((un) => {
        if (cancelled) un();
        else stop = un;
      }),
    );
    return () => {
      cancelled = true;
      stop?.();
    };
  }
  let last: number | null = null;
  const id = window.setInterval(async () => {
    try {
      const r = await call<{ revision: number; project_id: string | null }>("doc_revision");
      if (last !== null && r.revision !== last) handler(r.revision);
      last = r.revision;
    } catch {
      // bridge down: keep polling quietly
    }
  }, 1000);
  return () => window.clearInterval(id);
}

export type FileSource = { path: string } | { file_name: string; data: string };

export const ipc = {
  // project hub
  hubList: () => call<ProjectMeta[]>("hub_list"),
  /** Creates and opens a project. `template`: "blank" | "sample-bungalow". */
  hubCreate: (name: string, settings?: ProjectSettings, template?: string) =>
    call<DocState>("hub_create", { name, settings: settings ?? null, template: template ?? null }),
  hubOpen: (id: string) => call<DocState>("hub_open", { id }),
  hubRename: (id: string, name: string) => call<ProjectMeta>("hub_rename", { id, name }),
  hubDuplicate: (id: string) => call<ProjectMeta>("hub_duplicate", { id }),
  hubDelete: (id: string) => call<null>("hub_delete", { id }),
  /** `png` is a data URL. */
  hubSetThumbnail: (id: string, png: string) => call<null>("hub_set_thumbnail", { id, png }),
  hubClose: () => call<null>("hub_close"),

  // document
  docState: () => call<DocState | null>("doc_state"),
  docApply: (command: Command) => call<ApplyResult>("doc_apply", { command }),
  docPreview: (command: Command) => call<ApplyResult>("doc_preview", { command }),
  docUndo: () => call<DocState>("doc_undo"),
  docRedo: () => call<DocState>("doc_redo"),
  docQuery: (query: Query) => call<unknown>("doc_query", { query }),

  // versions
  snapshotCreate: (label: string) => call<SnapshotMeta>("snapshot_create", { label }),
  snapshotList: () => call<SnapshotMeta[]>("snapshot_list"),
  snapshotRestore: (id: string) => call<DocState>("snapshot_restore", { id }),

  // library
  catalogAssets: () => call<CatalogItem[]>("catalog_assets"),

  // export. `path` null writes into the app's exports folder.
  exportPlan: (format: PlanFormat, options: PlanExportOptions, path: string | null) =>
    call<ExportResult>("export_plan", { format, options, path }),
  /** `png` is a data URL. */
  exportImage: (png: string, name: string, path: string | null) =>
    call<ExportResult>("export_image", { png, name, path }),

  // underlay (plan image import)
  /** `data` is a data URL. Returns the stored file name. */
  underlayStore: (fileName: string, data: string) =>
    call<{ file_name: string }>("underlay_store", { file_name: fileName, data }),
  /** Returns a data URL. */
  underlayData: (fileName: string) => call<string>("underlay_data", { file_name: fileName }),

  // renders
  renderStyles: () => call<RenderStyle[]>("render_styles"),
  renderList: () => call<RenderRecord[]>("render_list"),
  /** Saves a deterministic capture of the 3D view (Tier 1). `png` is a data URL. */
  renderCapture: (camera: Camera, png: string) =>
    call<RenderRecord>("render_capture", { camera, png }),
  /** Returns a data URL. */
  renderData: (id: string) => call<string>("render_data", { id }),
  renderDelete: (id: string) => call<null>("render_delete", { id }),

  // interchange. A file arrives either as a native path (desktop, from the
  // open dialog) or as a data URL (browser). Exactly one of the two.
  /** Reads a DXF or DWG (DWG needs the converter) and reports what it contains. */
  importInspect: (source: FileSource) => call<ImportInspection>("import_inspect", source),
  /** Imports the inspected file with the chosen options as one undo step. */
  importCommit: (source: FileSource, options: ImportOptions) =>
    call<ImportResult>("import_commit", { ...source, options }),
  /** Copies a glTF/GLB/OBJ into the project's models folder. Returns the stored file name. */
  modelStore: (source: FileSource) => call<{ file_name: string; size: number }>("model_store", source),
  /** Returns a data URL of a stored reference model. */
  modelData: (fileName: string) => call<string>("model_data", { file_name: fileName }),
  /** Backend-produced whole-model export. Null path writes to the exports folder. */
  exportModel: (format: ModelFormat, path: string | null) =>
    call<ExportResult>("export_model", { format, path }),
  /** Writes bytes the frontend produced (GLB, OBJ, DAE). `data` is a data URL. */
  exportBytes: (name: string, data: string, path: string | null) =>
    call<ExportResult>("export_bytes", { name, data, path }),
  /** Saves the open project as one .guhit bundle. */
  bundleSave: (path: string | null) => call<ExportResult>("bundle_save", { path }),
  /** Opens a .guhit bundle: copies it into the projects folder and opens it. */
  bundleOpen: (source: FileSource) => call<DocState>("bundle_open", source),
  dwgStatus: () => call<DwgConverterStatus>("dwg_status"),
  /** Sets the ODA File Converter path. Empty string clears it. */
  dwgSetPath: (path: string) => call<DwgConverterStatus>("dwg_set_path", { path }),

  // AI visualization (Tier 2). Every result is labelled and tied to its source capture.
  renderAiSettingsGet: () => call<RenderAiSettings>("render_ai_settings_get"),
  /** `apiKey` "" removes the key; null leaves it. */
  renderAiSettingsSet: (apiKey: string | null, model: string | null) =>
    call<RenderAiSettings>("render_ai_settings_set", { api_key: apiKey, model }),
  /** Long call (10 to 60 s). The UI shows progress and may abandon the promise. */
  renderAiGenerate: (request: RenderAiRequest) => call<RenderAiResult>("render_ai_generate", { request }),

  // AI copilot
  aiSettingsGet: () => call<AiSettings>("ai_settings_get"),
  /** Pass `apiKey` "" to remove the stored key. The key is never read back. */
  aiSettingsSet: (apiKey: string | null, model: string | null) =>
    call<AiSettings>("ai_settings_set", { api_key: apiKey, model }),
  aiChat: (request: AiRequest) => call<AiTurn>("ai_chat", { request }),
  aiResolve: (proposalId: string, accept: boolean) =>
    call<AiResolveResult>("ai_resolve", { proposal_id: proposalId, accept }),
};
