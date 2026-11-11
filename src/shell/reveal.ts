// "Show in folder" on the toast after a file is written (an export, the
// permit set, a render), so the end of the flow is not a dead end. Desktop
// only: the browser build has no file manager to open.
import { isTauri, toIpcError } from "../contract/ipc";
import { useApp, type ToastAction } from "../state/store";

/** Opens the file manager with the file selected. */
export async function revealFile(path: string): Promise<void> {
  try {
    const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
    await revealItemInDir(path);
  } catch (e) {
    useApp.getState().toast("error", `Could not show the file: ${toIpcError(e).message || String(e)}`);
  }
}

/** The toast action for a written file, or none outside the desktop app. */
export function showInFolder(path: string | null | undefined, desktop: boolean = isTauri): ToastAction | undefined {
  if (!desktop || !path) return undefined;
  return { label: "Show in folder", run: () => void revealFile(path) };
}
