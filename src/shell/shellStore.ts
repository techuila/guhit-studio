// Shell-local UI state. Not part of the contract store: only the shell reads it.
import { create } from "zustand";
import { useApp } from "../state/store";

export type DockTab = "copilot" | "visuals";
export type Overlay = "palette" | "export" | "versions" | "shortcuts" | "settings" | null;
export type FlyoutKind = "wall" | "door" | "window" | "asset";
export type ImportKind = "cad" | "model" | "bundle";

interface ShellState {
  overlay: Overlay;
  dockTab: DockTab;
  dockCollapsed: boolean;
  /** Dock height as a fraction of the right column. */
  dockRatio: number;
  /** Width share of the 2D pane in split view. */
  splitRatio: number;
  inspectorSections: Record<string, boolean>;
  /**
   * A request from outside the tool rail (a keyboard shortcut) to open one
   * of its flyouts, for example O opening the object library. The token
   * makes the same flyout requested twice in a row still fire the effect.
   */
  flyoutRequest: { flyout: FlyoutKind; token: number } | null;
  /**
   * A request to start an import pick, from the palette or a shortcut, for
   * the hidden file inputs mounted once by `ImportController` to react to.
   */
  importRequest: { kind: ImportKind; token: number } | null;

  open: (overlay: Exclude<Overlay, null>) => void;
  close: () => void;
  setDockTab: (tab: DockTab) => void;
  setDockCollapsed: (collapsed: boolean) => void;
  setDockRatio: (ratio: number) => void;
  setSplitRatio: (ratio: number) => void;
  toggleSection: (key: string, fallback: boolean) => void;
  requestFlyout: (flyout: FlyoutKind) => void;
  requestImport: (kind: ImportKind) => void;
}

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n));

export const useShell = create<ShellState>((set) => ({
  overlay: null,
  dockTab: "copilot",
  dockCollapsed: false,
  dockRatio: 0.44,
  splitRatio: 0.5,
  inspectorSections: {},
  flyoutRequest: null,
  importRequest: null,

  open: (overlay) => set({ overlay }),
  close: () => set({ overlay: null }),
  setDockTab: (dockTab) => set({ dockTab, dockCollapsed: false }),
  setDockCollapsed: (dockCollapsed) => set({ dockCollapsed }),
  setDockRatio: (ratio) => set({ dockRatio: clamp(ratio, 0.2, 0.75) }),
  setSplitRatio: (ratio) => set({ splitRatio: clamp(ratio, 0.2, 0.8) }),
  toggleSection: (key, fallback) =>
    set((s) => ({ inspectorSections: { ...s.inspectorSections, [key]: !(s.inspectorSections[key] ?? fallback) } })),
  requestFlyout: (flyout) => set((s) => ({ flyoutRequest: { flyout, token: (s.flyoutRequest?.token ?? 0) + 1 } })),
  requestImport: (kind) => set((s) => ({ importRequest: { kind, token: (s.importRequest?.token ?? 0) + 1 } })),
}));

/** The name to show for the open project. */
export function useProjectName(): string {
  return useApp((s) => s.doc?.project.name ?? "");
}

/** Open state of a collapsible inspector section, remembered for the session. */
export function useSection(key: string, fallback: boolean): [boolean, () => void] {
  const open = useShell((s) => s.inspectorSections[key] ?? fallback);
  const toggle = useShell((s) => s.toggleSection);
  return [open, () => toggle(key, fallback)];
}
