// Making and exporting the permit set (lane L4), shared by the Sheets panel
// and the export dialog. `MakeSheetSet` is the engine's (lane L1); until it
// is built the panel makes the same set as one batch of `UpsertSheet`.
import type { ExportResult, PaperSize, PlanFormat } from "../../contract/bindings";
import { ipc, isTauri, toIpcError } from "../../contract/ipc";
import { useApp } from "../../state/store";
import { defaultSetCommand, today } from "./sheetModel";

/**
 * Makes the permit set on `paper`: missing sheets only when there is a set
 * already. True when the set changed.
 */
export async function makePermitSet(paper: PaperSize): Promise<boolean> {
  const app = useApp.getState();
  const doc = app.doc;
  if (!doc) return false;
  try {
    const result = await ipc.docApply({ type: "make_sheet_set", paper, replace: false });
    app.setDoc(result.state);
    return true;
  } catch (e) {
    const err = toIpcError(e);
    if (err.code !== "not_built") {
      app.reportError(e);
      return false;
    }
  }
  if (doc.project.sheet_set.sheets.length > 0) {
    app.toast("info", "The set already has sheets. Add more with Add sheet.");
    return false;
  }
  return (await app.dispatch(defaultSetCommand(doc, paper))) !== null;
}

function safeName(name: string): string {
  const cleaned = name.replace(/[\\/:*?"<>|]+/g, " ").replace(/\s+/g, " ").trim();
  return cleaned === "" ? "permit set" : cleaned;
}

/**
 * Writes the set, or `sheetIds`, as one PDF (or one SVG per sheet). In the
 * desktop app the user picks where; null when they cancel.
 */
export async function exportPermitSet(sheetIds: string[], format: Extract<PlanFormat, "pdf" | "svg"> = "pdf"): Promise<ExportResult | null> {
  const name = useApp.getState().doc?.project.name ?? "";
  let path: string | null = null;
  if (isTauri) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const picked = await save({
      title: format === "pdf" ? "Export the permit set" : "Export the sheets as SVG",
      defaultPath: `${safeName(name)} permit set.${format}`,
      filters: [{ name: format.toUpperCase(), extensions: [format] }],
    });
    if (!picked) return null;
    path = picked;
  }
  return ipc.exportSheets({ format, sheet_ids: sheetIds, path, date: today() });
}
