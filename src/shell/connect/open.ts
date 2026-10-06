// Opens an agent's install link, or the Claude Desktop extension file.
import { ipc, isTauri, toIpcError } from "../../contract/ipc";

export async function openLink(link: string): Promise<void> {
  if (isTauri) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(link);
    return;
  }
  window.open(link, "_blank");
}

export async function openClaudeDesktopBundle(): Promise<void> {
  if (!isTauri) throw new Error("Installing in Claude Desktop needs the Guhit Studio desktop app. Open the app and try again.");
  const { path } = await ipc.mcpClaudeDesktopBundle();
  const { openPath } = await import("@tauri-apps/plugin-opener");
  await openPath(path);
}

/** Plain-language text for a failed open. */
export function openErrorText(e: unknown, app: string): string {
  const msg = e instanceof Error ? e.message : toIpcError(e).message;
  if (e instanceof Error && msg.startsWith("Installing in")) return msg;
  return `Could not open ${app}. Is it installed? ${msg ? `(${msg})` : ""}`.trim();
}
