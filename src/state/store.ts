// App store. CONTRACT FILE - owned by the orchestrator.
//
// The Rust document is authoritative. `doc` is a read-only mirror of it.
// Nothing in the frontend mutates `doc`: call `dispatch(command)` and the
// returned state replaces the mirror. Propose changes, do not edit.

import { create } from "zustand";
import type {
  ApplyResult,
  Camera,
  CatalogItem,
  Command,
  DocState,
  Element,
  IpcError,
  OpeningStyle,
  PipeMaterial,
  PipeSystem,
  Point,
} from "../contract/bindings";
import { ipc, toIpcError } from "../contract/ipc";

export type Screen = "hub" | "editor";
export type ViewMode = "2d" | "3d" | "split";

export type Tool =
  | "select"
  | "pan"
  | "wall"
  | "rect_room"
  | "door"
  | "window"
  | "column"
  | "stair"
  | "asset"
  | "dimension"
  | "text"
  | "camera"
  | "pipe"
  /** Link a switch (or an aircon outlet) to what it controls: click the
   * device, then its loads. Writes `Asset::links`. */
  | "link";

export interface ToolOptions {
  wallThicknessMm: number | null;
  openingStyle: OpeningStyle | null;
  /** CatalogItem.key placed by the asset tool. */
  assetKey: string | null;
  /** Pipe tool: the system drawn. Material, size and start height default
   * per system (docs/CONTRACT.md, "Pipes") while the fields below are null. */
  pipeSystem: PipeSystem;
  pipeMaterial: PipeMaterial | null;
  pipeDiameterMm: number | null;
  /** Height above the level floor of the next pipe point. */
  pipeElevationMm: number | null;
}

export interface Toast {
  id: number;
  kind: "info" | "success" | "error";
  message: string;
}

/** A capture of the live 3D view, registered by Viewer3D. */
export type CaptureView = () => Promise<{ png: string; camera: Camera }>;
/** Serializes the live 3D scene, registered by Viewer3D. Returns a data URL. */
export type ExportScene = (format: "glb" | "obj" | "dae") => Promise<{ data: string; extension: string }>;
/** A capture of the 2D plan as a PNG data URL, registered by PlanCanvas: the
 * level on screen, or `levelId` when given. */
export type CapturePlan = (levelId?: string | null) => Promise<string>;

/**
 * An undo or redo the engine held back because the step is someone else's
 * (live session, code `other_author`). The shell asks, then resolves it.
 */
export interface UndoConfirm {
  redo: boolean;
  /** The engine's sentence: who made the step and what it was. */
  message: string;
}

const AI_SCOPE_KEY = "guhit.aiScope";

function loadAiScope(): boolean {
  try {
    return globalThis.localStorage?.getItem(AI_SCOPE_KEY) === "1";
  } catch {
    return false;
  }
}

export interface AppState {
  screen: Screen;
  doc: DocState | null;
  /** Hypothetical result shown as a ghost over the model (AI proposal). */
  preview: ApplyResult | null;
  selection: string[];
  hoverId: string | null;
  tool: Tool;
  toolOptions: ToolOptions;
  viewMode: ViewMode;
  activeLevelId: string | null;
  /** Camera element shown by the 3D view, null for the free orbit camera. */
  activeCameraId: string | null;
  /** Plan cursor position in mm, for the status bar. */
  cursor: Point | null;
  snapEnabled: boolean;
  orthoEnabled: boolean;
  gridVisible: boolean;
  catalog: CatalogItem[];
  busy: boolean;
  /** Number of dispatch/undo/redo calls in flight. The backend saves on each. */
  saving: number;
  toasts: Toast[];
  captureView: CaptureView | null;
  capturePlan: CapturePlan | null;
  exportScene: ExportScene | null;
  /**
   * AI edits may change only the selection (DECISIONS D30): the copilot
   * sends it as `AiRequest::scope`, and MCP clients are held to it through
   * this window's presence. Remembered per computer.
   */
  aiScope: boolean;
  /** An undo or redo waiting for the user to confirm, or null. */
  undoConfirm: UndoConfirm | null;

  /** Applies a command in Rust. Returns null and toasts on failure. */
  dispatch: (command: Command) => Promise<ApplyResult | null>;
  /** In a live session, someone else's step asks first (`undoConfirm`). */
  undo: () => Promise<void>;
  redo: () => Promise<void>;
  /** Answers `undoConfirm`: true takes the step back anyway. */
  resolveUndoConfirm: (accept: boolean) => Promise<void>;
  setAiScope: (on: boolean) => void;
  /** Replaces the mirror. Used after restore, AI accept, open. */
  setDoc: (doc: DocState | null) => void;
  setPreview: (preview: ApplyResult | null) => void;
  select: (ids: string[], additive?: boolean) => void;
  setHover: (id: string | null) => void;
  setTool: (tool: Tool, options?: Partial<ToolOptions>) => void;
  setViewMode: (mode: ViewMode) => void;
  setActiveCamera: (id: string | null) => void;
  setActiveLevel: (id: string) => void;
  /** Renames the open project. Not an undo step. */
  renameProject: (name: string) => Promise<void>;
  setCursor: (p: Point | null) => void;
  toggle: (key: "snapEnabled" | "orthoEnabled" | "gridVisible") => void;
  openProject: (id: string) => Promise<void>;
  createProject: (name: string, template?: string) => Promise<void>;
  closeProject: () => Promise<void>;
  loadCatalog: () => Promise<void>;
  toast: (kind: Toast["kind"], message: string) => void;
  dismissToast: (id: number) => void;
  reportError: (e: unknown) => IpcError;
  registerCaptureView: (fn: CaptureView | null) => void;
  registerCapturePlan: (fn: CapturePlan | null) => void;
  registerExportScene: (fn: ExportScene | null) => void;
}

let toastSeq = 1;

export const useApp = create<AppState>((set, get) => ({
  screen: "hub",
  doc: null,
  preview: null,
  selection: [],
  hoverId: null,
  tool: "select",
  toolOptions: {
    wallThicknessMm: null,
    openingStyle: null,
    assetKey: null,
    pipeSystem: "cold_water",
    pipeMaterial: null,
    pipeDiameterMm: null,
    pipeElevationMm: null,
  },
  viewMode: "2d",
  activeLevelId: null,
  activeCameraId: null,
  cursor: null,
  snapEnabled: true,
  orthoEnabled: false,
  gridVisible: true,
  catalog: [],
  busy: false,
  saving: 0,
  toasts: [],
  captureView: null,
  capturePlan: null,
  exportScene: null,
  aiScope: loadAiScope(),
  undoConfirm: null,

  dispatch: async (command) => {
    set((s) => ({ saving: s.saving + 1 }));
    try {
      const result = await ipc.docApply(command);
      get().setDoc(result.state);
      return result;
    } catch (e) {
      get().reportError(e);
      return null;
    } finally {
      set((s) => ({ saving: s.saving - 1 }));
    }
  },

  undo: async () => {
    if (!get().doc?.can_undo) return;
    await stepHistory(false, false);
  },

  redo: async () => {
    if (!get().doc?.can_redo) return;
    await stepHistory(true, false);
  },

  resolveUndoConfirm: async (accept) => {
    const pending = get().undoConfirm;
    set({ undoConfirm: null });
    if (pending && accept) await stepHistory(pending.redo, true);
  },

  setAiScope: (aiScope) => {
    set({ aiScope });
    try {
      globalThis.localStorage?.setItem(AI_SCOPE_KEY, aiScope ? "1" : "0");
    } catch {
      // the choice lasts this session
    }
  },

  setDoc: (doc) =>
    set((s) => {
      if (!doc) return { doc: null, selection: [], hoverId: null, preview: null };
      const ids = new Set(doc.project.elements.map((e) => e.id));
      const levelOk = doc.project.levels.some((l) => l.id === s.activeLevelId);
      return {
        doc,
        selection: s.selection.filter((id) => ids.has(id)),
        hoverId: s.hoverId && ids.has(s.hoverId) ? s.hoverId : null,
        activeLevelId: levelOk ? s.activeLevelId : (doc.project.levels[0]?.id ?? null),
      };
    }),

  setPreview: (preview) => set({ preview }),

  select: (ids, additive = false) =>
    set((s) => {
      if (!additive) return { selection: ids };
      const next = new Set(s.selection);
      for (const id of ids) {
        if (next.has(id)) next.delete(id);
        else next.add(id);
      }
      return { selection: [...next] };
    }),

  setHover: (hoverId) => set({ hoverId }),

  setTool: (tool, options) =>
    set((s) => ({ tool, toolOptions: { ...s.toolOptions, ...options } })),

  setViewMode: (viewMode) => set({ viewMode }),
  setActiveCamera: (activeCameraId) => set({ activeCameraId }),
  setActiveLevel: (id) =>
    set((s) => (s.doc?.project.levels.some((l) => l.id === id) ? { activeLevelId: id, selection: [] } : {})),

  renameProject: async (name) => {
    const doc = get().doc;
    if (!doc) return;
    try {
      await ipc.hubRename(doc.project.id, name);
      const fresh = await ipc.docState();
      if (fresh) get().setDoc(fresh);
    } catch (e) {
      get().reportError(e);
    }
  },
  setCursor: (cursor) => set({ cursor }),
  toggle: (key) => set((s) => ({ [key]: !s[key] }) as Partial<AppState>),

  openProject: async (id) => {
    set({ busy: true });
    try {
      get().setDoc(await ipc.hubOpen(id));
      set({ screen: "editor", tool: "select", selection: [], preview: null });
    } catch (e) {
      get().reportError(e);
    } finally {
      set({ busy: false });
    }
  },

  createProject: async (name, template) => {
    set({ busy: true });
    try {
      get().setDoc(await ipc.hubCreate(name, undefined, template));
      set({ screen: "editor", tool: "select", selection: [], preview: null });
    } catch (e) {
      get().reportError(e);
    } finally {
      set({ busy: false });
    }
  },

  closeProject: async () => {
    try {
      await ipc.hubClose();
    } catch (e) {
      get().reportError(e);
    }
    get().setDoc(null);
    set({ screen: "hub" });
  },

  loadCatalog: async () => {
    try {
      set({ catalog: await ipc.catalogAssets() });
    } catch (e) {
      get().reportError(e);
    }
  },

  toast: (kind, message) =>
    set((s) => ({ toasts: [...s.toasts, { id: toastSeq++, kind, message }] })),
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

  reportError: (e) => {
    const err = toIpcError(e);
    get().toast("error", err.message);
    return err;
  },

  registerCaptureView: (captureView) => set({ captureView }),
  registerCapturePlan: (capturePlan) => set({ capturePlan }),
  registerExportScene: (exportScene) => set({ exportScene }),
}));

/** One undo or redo. `other_author` turns into a question instead of a toast. */
async function stepHistory(redo: boolean, force: boolean): Promise<void> {
  const app = useApp;
  app.setState((s) => ({ saving: s.saving + 1 }));
  try {
    app.getState().setDoc(await (redo ? ipc.docRedo(force) : ipc.docUndo(force)));
  } catch (e) {
    const err = toIpcError(e);
    if (err.code === "other_author" && !force) app.setState({ undoConfirm: { redo, message: err.message } });
    else app.getState().reportError(err);
  } finally {
    app.setState((s) => ({ saving: s.saving - 1 }));
  }
}

/** The state to draw: the AI preview when one is active, else the document. */
export function useVisibleDoc(): DocState | null {
  return useApp((s) => s.preview?.state ?? s.doc);
}

export function findElement(doc: DocState | null, id: string): Element | undefined {
  return doc?.project.elements.find((e) => e.id === id);
}
