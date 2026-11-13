// State for the "Connect an AI agent" button and dialog: whether the dialog is
// open, and the app's MCP status, refreshed by polling while something shows it.
import { useEffect } from "react";
import { create } from "zustand";
import type { McpStatus } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";

interface ConnectState {
  open: boolean;
  status: McpStatus | null;
  /** True once a status request failed and none has succeeded since. */
  unreachable: boolean;
  /**
   * The extended toolsets every agent gets by default (MCP v2,
   * `mcp_toolsets`), null until read or when the app cannot say.
   */
  toolsets: string[] | null;
  openDialog: () => void;
  closeDialog: () => void;
  refresh: () => Promise<void>;
  setEnabled: (enabled: boolean) => Promise<void>;
  loadToolsets: () => Promise<void>;
  /** Saves the defaults; shown at once, put back when the app refuses. Throws its error. */
  setToolsets: (toolsets: string[]) => Promise<void>;
}

export const useConnect = create<ConnectState>((set, get) => ({
  open: false,
  status: null,
  unreachable: false,
  toolsets: null,
  openDialog: () => set({ open: true }),
  closeDialog: () => set({ open: false }),
  refresh: async () => {
    try {
      set({ status: await ipc.mcpStatus(), unreachable: false });
    } catch {
      set({ unreachable: true });
    }
  },
  setEnabled: async (enabled) => {
    set({ status: await ipc.mcpSetEnabled(enabled), unreachable: false });
  },
  loadToolsets: async () => {
    try {
      set({ toolsets: (await ipc.mcpToolsets()).toolsets });
    } catch {
      // An app before MCP v2: the section stays hidden.
    }
  },
  setToolsets: async (toolsets) => {
    const before = get().toolsets;
    set({ toolsets });
    try {
      set({ toolsets: (await ipc.mcpSetToolsets(toolsets)).toolsets });
    } catch (e) {
      set({ toolsets: before });
      throw e;
    }
  },
}));

/** Keeps the status fresh while the caller is mounted. */
export function useMcpPolling(ms: number): void {
  useEffect(() => {
    void useConnect.getState().refresh();
    const id = window.setInterval(() => void useConnect.getState().refresh(), ms);
    return () => window.clearInterval(id);
  }, [ms]);
}

export type DotState = "on" | "off" | "warn";

export function dotState(status: McpStatus | null): DotState {
  if (!status || !status.enabled) return "off";
  return status.listening ? "on" : "warn";
}
