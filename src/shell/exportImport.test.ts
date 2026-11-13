// Export opens on the permit set once there is one (U48), its toast names
// what was written and offers "Show in folder" in the desktop app (U10), and
// a CAD import error offers Settings only when the DWG converter is the cause
// (U33). The dock's resize edge moves by keyboard (U23).
import { describe, expect, it } from "vitest";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import type { DocState } from "../contract/bindings";
import { dockRatioKey } from "./EditorShell";
import { defaultExportFormat, exportedMessage } from "./ExportDialog";
import { cadErrorOf } from "./Import";
import { showInFolder } from "./reveal";

const doc = fixture as unknown as DocState;
const withSheets = (n: number): DocState =>
  ({ ...doc, project: { ...doc.project, sheet_set: { ...doc.project.sheet_set, sheets: Array.from({ length: n }, (_, i) => ({ id: `s${i}` })) } } }) as unknown as DocState;

describe("export", () => {
  it("opens on the permit set when the project has one, else on a PDF", () => {
    expect(defaultExportFormat(withSheets(3))).toBe("permit");
    expect(defaultExportFormat(withSheets(0))).toBe("pdf");
    expect(defaultExportFormat(null)).toBe("pdf");
  });

  it("names what was written in sentence case", () => {
    expect(exportedMessage("Export permit set", " at 1:100", "/x/house.pdf")).toBe("Exported permit set at 1:100 to /x/house.pdf");
    expect(exportedMessage("Save .guhit file", "", "/x/house.guhit")).toBe("Saved .guhit file to /x/house.guhit");
  });

  it("offers Show in folder only in the desktop app", () => {
    const action = showInFolder("/x/house.pdf", true);
    expect(action?.label).toBe("Show in folder");
    expect(typeof action?.run).toBe("function");
    expect(showInFolder("/x/house.pdf", false)).toBeUndefined();
    expect(showInFolder("", true)).toBeUndefined();
  });
});

describe("import errors", () => {
  it("offers Settings for a DWG the converter could not handle", () => {
    expect(cadErrorOf({ path: "/a/plan.DWG" }, "DWG needs the ODA File Converter, which is not set up yet.").converter).toBe(true);
    expect(cadErrorOf({ file_name: "plan.dwg", data: "" }, "the DWG converter did not finish in time").converter).toBe(true);
  });

  it("does not for a broken DXF, where Settings cannot help", () => {
    const e = cadErrorOf({ path: "/a/plan.dxf" }, "the drawing has no entities");
    expect(e.converter).toBe(false);
    expect(e.message).toBe("the drawing has no entities");
  });
});

describe("dock resize by keyboard", () => {
  it("ArrowUp grows the dock, ArrowDown shrinks it, other keys do nothing", () => {
    expect(dockRatioKey("ArrowUp", 0.44)).toBeCloseTo(0.46);
    expect(dockRatioKey("ArrowDown", 0.44)).toBeCloseTo(0.42);
    expect(dockRatioKey("ArrowLeft", 0.44)).toBeNull();
  });
});
