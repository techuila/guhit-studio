// Typed IPC client. CONTRACT FILE - owned by the orchestrator.
//
// Inside the desktop app every call goes through the single Tauri command
// `ipc`. In a plain browser (dev and tests) the same calls go over HTTP to
// the dev bridge, which runs the same Rust service. See docs/CONTRACT.md.

import type {
  AiRequest,
  AiResolveResult,
  AgentLimit,
  AgentSummary,
  AgentUsage,
  Draft,
  Estimate,
  MapKind,
  MapsMeta,
  AiSettings,
  AiTurn,
  AppEvent,
  ApplyResult,
  Camera,
  ChatMessage,
  HistoryList,
  LiveStatus,
  Point,
  Presence,
  PresenceEntry,
  Profile,
  WindowReply,
  DwgConverterStatus,
  McpStatus,
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
  RenderInfo,
  RenderRecord,
  RenderStyle,
  SheetExportOptions,
  Signatory,
  SnapshotMeta,
  SvgDrawing,
  VideoInfo,
  ViewRef,
} from "./bindings";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * Development only: `?bridge=http://localhost:1431` points this browser tab at
 * another dev bridge, so a live session can be tried with two tabs on one
 * computer (docs/CONTRACT.md, "Dev bridge"). Loopback addresses only.
 */
function devBridgeOverride(): string | null {
  if (!import.meta.env.DEV || typeof window === "undefined") return null;
  const asked = new URLSearchParams(window.location.search).get("bridge");
  return asked && /^http:\/\/(localhost|127\.0\.0\.1):\d{1,5}$/.test(asked) ? asked : null;
}

const BRIDGE_URL: string =
  devBridgeOverride() ?? (import.meta.env.VITE_BRIDGE_URL as string | undefined) ?? "http://localhost:1430";

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

type AppEventHandler = (event: AppEvent) => void;
const appEventHandlers = new Set<AppEventHandler>();
let appEventStop: (() => void) | null = null;

function openAppEvents(): () => void {
  const deliver = (event: AppEvent) => {
    for (const h of [...appEventHandlers]) {
      try {
        h(event);
      } catch (e) {
        console.error("app_event handler failed", e);
      }
    }
  };
  if (isTauri) {
    let stop: (() => void) | null = null;
    let cancelled = false;
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<AppEvent>("app_event", (e) => deliver(e.payload)).then((un) => {
        if (cancelled) un();
        else stop = un;
      }),
    );
    return () => {
      cancelled = true;
      stop?.();
    };
  }
  // Dev bridge: server-sent events. EventSource reconnects by itself. Unit
  // tests run without one: nothing to listen to there.
  if (typeof EventSource === "undefined") return () => {};
  const source = new EventSource(`${BRIDGE_URL}/events`);
  source.onmessage = (m) => {
    try {
      deliver(JSON.parse(m.data as string) as AppEvent);
    } catch {
      // not an event we understand
    }
  };
  return () => source.close();
}

/**
 * Subscribe to what the backend pushes to the window (docs/CONTRACT.md,
 * "Live sessions" and "Window requests"): live session changes, other
 * people's presence, chat messages and window requests. One connection is
 * shared by every subscriber. Returns an unsubscribe function.
 */
export function onAppEvent(handler: AppEventHandler): () => void {
  appEventHandlers.add(handler);
  if (!appEventStop) appEventStop = openAppEvents();
  return () => {
    appEventHandlers.delete(handler);
    if (appEventHandlers.size === 0 && appEventStop) {
      appEventStop();
      appEventStop = null;
    }
  };
}

export type FileSource = { path: string } | { file_name: string; data: string };

/** An open's answer: the state, plus what opening the project mended
 * (`Document::repairs`), one sentence each. Absent when nothing was. */
export type Opened = DocState & { repairs?: string[] };

export const ipc = {
  // project hub
  hubList: () => call<ProjectMeta[]>("hub_list"),
  /** Creates and opens a project. `template`: "blank" | "sample-bungalow" | "plumbing-demo". */
  hubCreate: (name: string, settings?: ProjectSettings, template?: string) =>
    call<DocState>("hub_create", { name, settings: settings ?? null, template: template ?? null }),
  hubOpen: (id: string) => call<Opened>("hub_open", { id }),
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
  /**
   * In a live session, undoing someone else's step fails with `other_author` unless `force`.
   * `steps` takes back (or brings back) that many steps at once, as one revision.
   * `expectedRevision`: fail with `stale` and change nothing once the document moved on
   * from it, so "Undo anyway" takes only the steps it asked about.
   */
  docUndo: (force = false, steps = 1, expectedRevision?: number) =>
    call<DocState>("doc_undo", { force, steps, expected_revision: expectedRevision ?? null }),
  docRedo: (force = false, steps = 1, expectedRevision?: number) =>
    call<DocState>("doc_redo", { force, steps, expected_revision: expectedRevision ?? null }),
  /** The undo and redo stacks with who made each step and when, newest first. */
  historyList: () => call<HistoryList>("history_list"),
  docQuery: (query: Query) => call<unknown>("doc_query", { query }),

  // versions
  snapshotCreate: (label: string) => call<SnapshotMeta>("snapshot_create", { label }),
  snapshotList: () => call<SnapshotMeta[]>("snapshot_list"),
  snapshotRestore: (id: string) => call<Opened>("snapshot_restore", { id }),

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

  // Wave 2 (DECISIONS D53, D54). Until the lane that owns a call builds it,
  // the engine answers with the error code `not_built`.

  // the permit set (lane L4) and drawings computed on demand (lane L3)
  /** Writes the permit set, or the sheets in `sheet_ids`: PDF one file, SVG one per sheet, DXF side by side. */
  exportSheets: (opts: SheetExportOptions) => call<ExportResult>("export_sheets", { ...opts }),
  /** One sheet as SVG, title block included. */
  sheetPreview: (sheetId: string) => call<SvgDrawing>("sheet_preview", { sheet_id: sheetId }),
  /** One view as SVG: a plan, an elevation, a section, the roof or site plan, a schedule. `scale` 1:N, default the view's own. */
  viewSvg: (view: ViewRef, scale?: number) => call<SvgDrawing>("view_svg", { view, scale: scale ?? null }),

  // project files: images placed on sheets, the vicinity map (D53)
  /** `dataUrl` is an image data URL, as for `underlayStore`. Returns the stored file name. */
  fileStore: (fileName: string, dataUrl: string) => call<{ file_name: string }>("file_store", { file_name: fileName, data: dataUrl }),
  /** Returns a data URL. */
  fileData: (fileName: string) => call<string>("file_data", { file_name: fileName }),

  // the architect's license numbers on this computer (D53). Only people set
  // them; agents and the copilot are refused.
  professionalGet: () => call<Signatory>("professional_get"),
  professionalSet: (signatory: Signatory) => call<Signatory>("professional_set", { signatory }),

  // renders
  renderStyles: () => call<RenderStyle[]>("render_styles"),
  renderList: () => call<RenderRecord[]>("render_list"),
  /**
   * Saves a model view image (Tier 1): a capture of the 3D view, or a render
   * of it. `png` is a data URL. A render passes the revision it started from
   * and how it was made.
   */
  renderCapture: (camera: Camera, png: string, opts: { revision?: number; info?: RenderInfo } = {}) =>
    call<RenderRecord>("render_capture", { camera, png, revision: opts.revision ?? null, info: opts.info ?? null }),
  /** Returns a data URL. */
  renderData: (id: string) => call<string>("render_data", { id }),
  renderDelete: (id: string) => call<null>("render_delete", { id }),
  /** Makes a render the project's look, the style reference for AI renders (lane L6). `id` null clears it. Returns every record. */
  renderSetLook: (id: string | null) => call<RenderRecord[]>("render_set_look", { id }),

  // tour videos (lane L9): the window encodes, the backend writes the file.
  /** Starts a video file for a tour at a revision. Returns the upload id. */
  videoBegin: (tourId: string, revision: number) => call<{ id: string }>("video_begin", { tour_id: tourId, revision }),
  /** Appends encoded bytes, base64, at most 4 MiB per call. */
  videoAppend: (id: string, base64: string) => call<null>("video_append", { id, data: base64 }),
  /** Closes the file and saves it to Visuals. `poster` is an image data URL. */
  videoFinish: (id: string, poster: string, info: VideoInfo) => call<RenderRecord>("video_finish", { id, poster, info }),
  /** Drops an unfinished video. */
  videoAbort: (id: string) => call<null>("video_abort", { id }),

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
  /** `videos`: also pack tour videos and their frames (left out by default, D54). */
  bundleSave: (path: string | null, videos = false) => call<ExportResult>("bundle_save", { path, videos }),
  /** Opens a .guhit bundle: copies it into the projects folder and opens it. */
  bundleOpen: (source: FileSource) => call<Opened>("bundle_open", source),
  dwgStatus: () => call<DwgConverterStatus>("dwg_status"),
  /** Sets the ODA File Converter path. Empty string clears it. */
  dwgSetPath: (path: string) => call<DwgConverterStatus>("dwg_set_path", { path }),

  // MCP server for agents (Connect agent dialog, DECISIONS D35).
  mcpStatus: () => call<McpStatus>("mcp_status"),
  /** The "Allow agents" switch. Off: every MCP request gets HTTP 503. */
  mcpSetEnabled: (enabled: boolean) => call<McpStatus>("mcp_set_enabled", { enabled }),
  /** Writes the Claude Desktop extension (.mcpb). Desktop app only. */
  mcpClaudeDesktopBundle: () => call<{ path: string }>("mcp_claude_desktop_bundle"),
  /** The extended toolsets every agent gets by default, and the names there are. */
  mcpToolsets: () => call<{ toolsets: string[]; available: string[] }>("mcp_toolsets"),
  /** Sets the default extended toolsets. An agent whose URL or command names its own (`?toolsets=`, `--toolsets`) gets those instead. */
  mcpSetToolsets: (toolsets: string[]) => call<{ toolsets: string[]; available: string[] }>("mcp_set_toolsets", { toolsets }),

  // Agents working on the project through MCP (DECISIONS D42, D47). Changes
  // also arrive as the `agents` app event.
  /** Every agent this computer knows, newest call first. */
  agentsList: () => call<AgentSummary[]>("agents_list"),
  /** Renames an agent, or sets or clears its limit (null clears). */
  agentUpdate: (id: string, patch: { name?: string; limit?: AgentLimit | null }) =>
    call<AgentSummary>("agent_update", { id, ...patch }),
  /** Pause: its edits are refused, reads still work. `id` null: every agent. */
  agentPause: (id: string | null) => call<AgentSummary[]>("agent_pause", { id }),
  /** Stop: every call is refused until resumed. `id` null: every agent. */
  agentStop: (id: string | null) => call<AgentSummary[]>("agent_stop", { id }),
  /** Ends a pause or a stop. `id` null: every agent. */
  agentResume: (id: string | null) => call<AgentSummary[]>("agent_resume", { id }),

  // AI visualization (Tier 2). Every result is labelled and tied to its source capture.
  renderAiSettingsGet: () => call<RenderAiSettings>("render_ai_settings_get"),
  /** `apiKey` "" removes the key; null leaves it. */
  renderAiSettingsSet: (apiKey: string | null, model: string | null) =>
    call<RenderAiSettings>("render_ai_settings_set", { api_key: apiKey, model }),
  /** Long call (10 to 60 s). The UI shows progress and may abandon the promise. */
  renderAiGenerate: (request: RenderAiRequest) => call<RenderAiResult>("render_ai_generate", { request }),

  // AI render v2 (lane L6, D53): the price before every call, the drift
  // check before a person's image is saved, the maps, the agents' cap.
  /** What the request will cost and send. Spends nothing. */
  renderAiEstimate: (request: RenderAiRequest) => call<Estimate>("render_ai_estimate", { request }),
  /** Long call. The image comes back checked and is saved only by `renderAiKeep`. */
  renderAiDraft: (request: RenderAiRequest) => call<Draft>("render_ai_draft", { request }),
  /** Saves a draft to Visuals with its drift report. Drafts last 30 minutes. */
  renderAiKeep: (pendingId: string) => call<RenderRecord>("render_ai_keep", { pending_id: pendingId }),
  renderAiDiscard: (pendingId: string) => call<null>("render_ai_discard", { pending_id: pendingId }),
  /** Attaches a model view's conditioning maps (PNG data URLs) and what they were drawn with. */
  renderAiMapsSave: (renderId: string, maps: Array<{ kind: MapKind; png: string }>, meta: MapsMeta | null) =>
    call<RenderRecord>("render_ai_maps_save", { render_id: renderId, maps, meta }),
  /** One saved map as a data URL. */
  renderAiMapData: (id: string, kind: MapKind) => call<string>("render_ai_map_data", { id, kind }),
  /** An AI image's unmatched model edges as a data URL, or null. */
  renderAiDriftOverlay: (id: string) => call<string | null>("render_ai_drift_overlay", { id }),
  /** Agents' AI images in this project today, against the cap. */
  renderAiUsage: () => call<AgentUsage>("render_ai_usage"),
  /** 0 to 1000; 0 keeps agents from making AI images. */
  renderAiUsageSet: (agentDailyCap: number) => call<AgentUsage>("render_ai_usage_set", { agent_daily_cap: agentDailyCap }),

  // AI copilot
  aiSettingsGet: () => call<AiSettings>("ai_settings_get"),
  /** Pass `apiKey` "" to remove the stored key. The key is never read back. */
  aiSettingsSet: (apiKey: string | null, model: string | null) =>
    call<AiSettings>("ai_settings_set", { api_key: apiKey, model }),
  aiChat: (request: AiRequest) => call<AiTurn>("ai_chat", { request }),
  aiResolve: (proposalId: string, accept: boolean) =>
    call<AiResolveResult>("ai_resolve", { proposal_id: proposalId, accept }),

  // presence: this window's pointer, selection, cursor chat and AI scope.
  // MCP `get_selection` reads it; a live session sends it to the others.
  presenceSet: (presence: Presence) => call<null>("presence_set", { presence }),
  /** Everyone else's latest presence in the live session. */
  presenceList: () => call<PresenceEntry[]>("presence_list"),

  // live sessions (docs/CONTRACT.md, "Live sessions")
  profileGet: () => call<Profile>("profile_get"),
  /** 1 to 40 characters. */
  profileSet: (name: string) => call<Profile>("profile_set", { name }),
  liveStatus: () => call<LiveStatus>("live_status"),
  /** Shares the open project. Needs a profile name. */
  liveHost: (port: number | null = null) => call<LiveStatus>("live_host", { port }),
  /** Opens the project an invite shares. Needs a profile name. */
  liveJoin: (invite: string) => call<DocState>("live_join", { invite }),
  /** Host: ends the session for everyone. Guest: leaves it; the window goes to the hub. */
  liveLeave: () => call<LiveStatus>("live_leave"),
  /** Host only. */
  liveRemove: (participantId: string) => call<LiveStatus>("live_remove", { participant_id: participantId }),
  /** Guest: a copy of the shared project in this computer's projects, with a new id. */
  liveSaveCopy: () => call<ProjectMeta>("live_save_copy"),
  /** Live session only. `at` and `levelId` place a cursor chat message on the plan. */
  chatSend: (text: string, at: Point | null = null, levelId: string | null = null) =>
    call<ChatMessage>("chat_send", { text, at, level_id: levelId }),
  /** The open project's chat, oldest first. */
  chatList: () => call<ChatMessage[]>("chat_list"),

  /** The window's answer to a `WindowRequest` (docs/CONTRACT.md, "Window requests"). */
  windowReply: (id: string, reply: WindowReply | null, error: IpcError | null) =>
    call<null>("window_reply", { id, reply, error }),
};
