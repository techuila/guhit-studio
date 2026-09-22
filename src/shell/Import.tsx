// Import flow: DXF/DWG (recognized walls or linework), glTF/GLB/OBJ reference
// models, and .guhit bundles (DECISIONS D16). File picking branches on
// platform: the native dialog on desktop, a hidden <input type=file> in the
// browser (docs/CONTRACT.md `FileSource`). Mounted once at the app root so
// it works the same from the hub and from the editor.
import { useEffect, useRef, useState } from "react";
import type { Element, ImportInspection, ImportMode, ImportOptions, Point, ReferenceModel } from "../contract/bindings";
import { ipc, isTauri, toIpcError, type FileSource } from "../contract/ipc";
import { rectIsEmpty } from "../editor2d/geom";
import { buildIndex, modelBounds } from "../editor2d/model";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { Dialog } from "../ui/Dialog";
import { Button, CheckRow, Field, NumberField, Segmented, Select } from "../ui/controls";
import { setBusyLabel } from "../ui/feedback";
import type { PresenceStage } from "../ui/motion";
import { Presence, useLastTruthy } from "../ui/motionDom";
import { formatLength } from "../ui/units";
import { MODEL_UNITS } from "./ElementFields";
import { useShell } from "./shellStore";
import s from "./overlays.module.css";

function readAsDataURL(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("Could not read the file"));
    reader.readAsDataURL(file);
  });
}

const CAD_UNITS = [...MODEL_UNITS, { value: "custom", label: "Custom", mm: 0 }] as const;
type CadUnit = (typeof CAD_UNITS)[number]["value"];

const MODEL_SCALE_DEFAULT: Record<string, number> = { glb: 1000, gltf: 1000, obj: 1 };

interface CadState {
  source: FileSource;
  inspection: ImportInspection;
}

interface ModelState {
  fileName: string;
  extension: string;
}

/** Mount once (App.tsx). Reacts to file-menu clicks and to palette/shortcut requests routed through `useShell().requestImport`. */
export function ImportController() {
  const cadInputRef = useRef<HTMLInputElement>(null);
  const modelInputRef = useRef<HTMLInputElement>(null);
  const bundleInputRef = useRef<HTMLInputElement>(null);
  const importRequest = useShell((st) => st.importRequest);
  const toast = useApp((st) => st.toast);
  const reportError = useApp((st) => st.reportError);

  const [cad, setCad] = useState<CadState | null>(null);
  const [cadError, setCadError] = useState<string | null>(null);
  const [model, setModel] = useState<ModelState | null>(null);

  const lastCad = useLastTruthy(cad);
  const lastModel = useLastTruthy(model);

  const inspectCad = async (source: FileSource) => {
    setBusyLabel("Reading the drawing");
    useApp.setState({ busy: true });
    setCadError(null);
    try {
      const inspection = await ipc.importInspect(source);
      setCad({ source, inspection });
    } catch (e) {
      setCadError(toIpcError(e).message);
    } finally {
      useApp.setState({ busy: false });
    }
  };

  const storeModel = async (source: FileSource, originalName: string) => {
    setBusyLabel("Preparing the model");
    useApp.setState({ busy: true });
    try {
      const { file_name } = await ipc.modelStore(source);
      const extension = (originalName.split(".").pop() ?? "").toLowerCase();
      setModel({ fileName: file_name, extension });
    } catch (e) {
      reportError(e);
    } finally {
      useApp.setState({ busy: false });
    }
  };

  const openBundle = async (source: FileSource) => {
    setBusyLabel("Opening the bundle");
    useApp.setState({ busy: true });
    try {
      const state = await ipc.bundleOpen(source);
      useApp.getState().setDoc(state);
      useApp.setState({ screen: "editor", tool: "select", selection: [], preview: null });
      toast("success", `Opened "${state.project.name}"`);
    } catch (e) {
      reportError(e);
    } finally {
      useApp.setState({ busy: false });
    }
  };

  const startCad = async () => {
    if (isTauri) {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ multiple: false, filters: [{ name: "CAD drawing", extensions: ["dxf", "dwg"] }] });
      if (typeof picked === "string") void inspectCad({ path: picked });
    } else {
      cadInputRef.current?.click();
    }
  };

  const startModel = async () => {
    if (isTauri) {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ multiple: false, filters: [{ name: "3D model", extensions: ["glb", "gltf", "obj"] }] });
      if (typeof picked === "string") void storeModel({ path: picked }, picked);
    } else {
      modelInputRef.current?.click();
    }
  };

  const startBundle = async () => {
    if (isTauri) {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ multiple: false, filters: [{ name: "Guhit project bundle", extensions: ["guhit"] }] });
      if (typeof picked === "string") void openBundle({ path: picked });
    } else {
      bundleInputRef.current?.click();
    }
  };

  // A click on the File menu's items call these directly; a palette entry or
  // shortcut goes through the shell store (same token pattern as the O
  // shortcut opening the object flyout in ToolRail).
  useEffect(() => {
    if (!importRequest) return;
    if (importRequest.kind === "cad") void startCad();
    else if (importRequest.kind === "model") void startModel();
    else void startBundle();
    // startCad/startModel/startBundle read no reactive state; re-running the
    // effect only on a new token (a fresh click) is what we want here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [importRequest]);

  return (
    <>
      <input
        ref={cadInputRef}
        type="file"
        accept=".dxf,.dwg"
        data-testid="import-cad-input"
        style={{ display: "none" }}
        onChange={async (e) => {
          const file = e.target.files?.[0];
          e.target.value = "";
          if (file) void inspectCad({ file_name: file.name, data: await readAsDataURL(file) });
        }}
      />
      <input
        ref={modelInputRef}
        type="file"
        accept=".glb,.gltf,.obj"
        data-testid="import-model-input"
        style={{ display: "none" }}
        onChange={async (e) => {
          const file = e.target.files?.[0];
          e.target.value = "";
          if (file) void storeModel({ file_name: file.name, data: await readAsDataURL(file) }, file.name);
        }}
      />
      <input
        ref={bundleInputRef}
        type="file"
        accept=".guhit"
        data-testid="import-bundle-input"
        style={{ display: "none" }}
        onChange={async (e) => {
          const file = e.target.files?.[0];
          e.target.value = "";
          if (file) void openBundle({ file_name: file.name, data: await readAsDataURL(file) });
        }}
      />

      <Presence open={cad !== null || cadError !== null} exit="panel">
        {(stage) =>
          cadError ? (
            <CadErrorDialog message={cadError} onClose={() => setCadError(null)} stage={stage} />
          ) : lastCad ? (
            <CadImportDialog cad={lastCad} onClose={() => setCad(null)} stage={stage} />
          ) : null
        }
      </Presence>

      <Presence open={model !== null} exit="panel">
        {(stage) => (lastModel ? <ModelScaleDialog model={lastModel} onClose={() => setModel(null)} stage={stage} /> : null)}
      </Presence>
    </>
  );
}

function CadErrorDialog({ message, onClose, stage }: { message: string; onClose: () => void; stage?: PresenceStage }) {
  return (
    <Dialog
      title="Could not read the file"
      onClose={onClose}
      width={420}
      stage={stage}
      footer={
        <>
          <Button onClick={onClose}>Close</Button>
          <Button
            variant="primary"
            data-autofocus
            onClick={() => {
              onClose();
              useShell.getState().open("settings");
            }}
          >
            Open Settings
          </Button>
        </>
      }
    >
      <p className={s.importError}>{message}</p>
    </Dialog>
  );
}

function BoundsPreview({ min, max }: { min: Point; max: Point }) {
  const w = Math.max(1e-6, max.x - min.x);
  const h = Math.max(1e-6, max.y - min.y);
  const box = 190;
  const pad = 14;
  const scale = Math.min((box - pad * 2) / w, (box - pad * 2) / h);
  const rw = Math.max(1, w * scale);
  const rh = Math.max(1, h * scale);
  return (
    <svg viewBox={`0 0 ${box} ${box}`} width={box} height={box} className={s.importBounds} aria-hidden>
      <rect x={(box - rw) / 2} y={(box - rh) / 2} width={rw} height={rh} fill="var(--accent-soft)" stroke="var(--accent)" strokeWidth={1.5} />
    </svg>
  );
}

function CadImportDialog({ cad, onClose, stage }: { cad: CadState; onClose: () => void; stage?: PresenceStage }) {
  const doc = useApp((st) => st.doc);
  const activeLevelId = useApp((st) => st.activeLevelId);
  const toast = useApp((st) => st.toast);
  const reportError = useApp((st) => st.reportError);
  const { inspection, source } = cad;

  const suggestedChoice = CAD_UNITS.find((u) => u.value !== "custom" && Math.abs(u.mm - inspection.suggested_mm_per_unit) < 1e-6)?.value ?? "custom";
  const [unitChoice, setUnitChoice] = useState<CadUnit>(suggestedChoice);
  const [mmPerUnit, setMmPerUnit] = useState(inspection.suggested_mm_per_unit);
  const [enabled, setEnabled] = useState<Set<string>>(() => new Set(inspection.layers.map((l) => l.name)));
  const [mode, setMode] = useState<ImportMode>("walls");
  const [levelId, setLevelId] = useState(activeLevelId ?? doc?.project.levels[0]?.id ?? "");
  const [working, setWorking] = useState(false);

  if (!doc) return null;

  const totalWalls = inspection.layers.filter((l) => enabled.has(l.name)).reduce((n, l) => n + l.detected_walls, 0);
  const totalEntities = inspection.layers.filter((l) => enabled.has(l.name)).reduce((n, l) => n + l.entity_count, 0);
  const wMm = (inspection.max.x - inspection.min.x) * mmPerUnit;
  const hMm = (inspection.max.y - inspection.min.y) * mmPerUnit;

  const run = async () => {
    if (enabled.size === 0) return;
    setWorking(true);
    try {
      const options: ImportOptions = {
        mm_per_unit: mmPerUnit,
        layers: [...enabled],
        mode,
        offset: { x: -inspection.min.x * mmPerUnit, y: -inspection.min.y * mmPerUnit },
        level_id: levelId || null,
      };
      const result = await ipc.importCommit(source, options);
      useApp.getState().setDoc(result.state);
      toast("success", `Imported ${result.walls_added} wall${result.walls_added === 1 ? "" : "s"} and ${result.linework_added} linework layer${result.linework_added === 1 ? "" : "s"}`);
      if (result.skipped.length > 0) toast("info", `Skipped: ${result.skipped.join(", ")}`);
      bus.emit("zoom_to_fit");
      onClose();
    } catch (e) {
      reportError(e);
    } finally {
      setWorking(false);
    }
  };

  return (
    <Dialog
      title={`Import ${inspection.file_name}`}
      onClose={onClose}
      width={660}
      stage={stage}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={working || enabled.size === 0} onClick={() => void run()} data-autofocus>
            {working ? "Importing" : "Import"}
          </Button>
        </>
      }
    >
      <div className={s.importBody}>
        <div className={s.importPreview}>
          <BoundsPreview min={inspection.min} max={inspection.max} />
          <p className={s.exportNote}>
            {formatLength(wMm, "m")} x {formatLength(hMm, "m")}
            <br />
            {inspection.layers.length} layer{inspection.layers.length === 1 ? "" : "s"}, {totalEntities} entities
            {mode === "walls" ? `, ${totalWalls} wall${totalWalls === 1 ? "" : "s"} detected` : ""}
          </p>
        </div>
        <div className={s.importOptions}>
          <Field label="Unit" hint={inspection.declared_unit ? `The file declares ${inspection.declared_unit}` : "The file does not declare a unit"}>
            <Segmented
              label="Drawing unit"
              stretch
              value={unitChoice}
              onChange={(v) => {
                setUnitChoice(v);
                if (v !== "custom") setMmPerUnit(CAD_UNITS.find((u) => u.value === v)!.mm);
              }}
              options={[...CAD_UNITS]}
            />
          </Field>
          {unitChoice === "custom" ? (
            <Field label="mm / unit">
              <NumberField label="Millimeters per drawing unit" suffix="mm" min={0.0001} decimals={4} value={mmPerUnit} onCommit={setMmPerUnit} />
            </Field>
          ) : null}
          <Field
            label="Bring in as"
            hint={mode === "walls" ? "Straight double lines on the checked layers become walls you can edit like any other." : "Everything comes in as reference tracing on the Underlays layer: move, rotate or delete it, but the lines stay as drawn."}
          >
            <Segmented
              label="Import mode"
              stretch
              value={mode}
              onChange={setMode}
              options={[
                { value: "walls", label: "Recognize walls" },
                { value: "linework", label: "Linework only" },
              ]}
            />
          </Field>
          {doc.project.levels.length > 1 ? (
            <Field label="Level">
              <Select label="Level" value={levelId} onChange={setLevelId} options={doc.project.levels.map((l) => ({ value: l.id, label: l.name }))} />
            </Field>
          ) : null}
          <div className={s.importLayers}>
            <span className={s.importLayersHead}>Layers</span>
            <ul className={s.importLayerList}>
              {inspection.layers.map((l) => (
                <li key={l.name} className={s.importLayer}>
                  <CheckRow
                    checked={enabled.has(l.name)}
                    onChange={(v) =>
                      setEnabled((prev) => {
                        const next = new Set(prev);
                        if (v) next.add(l.name);
                        else next.delete(l.name);
                        return next;
                      })
                    }
                  >
                    <span className={s.importLayerSwatch} style={{ background: l.color }} />
                    {l.name}
                  </CheckRow>
                  <span className={s.importLayerCounts}>
                    {l.entity_count} {l.entity_count === 1 ? "entity" : "entities"}
                    {mode === "walls" ? `, ${l.detected_walls} walls` : ""}
                  </span>
                </li>
              ))}
              {inspection.layers.length === 0 ? <li className={s.importLayerCounts}>No layers found</li> : null}
            </ul>
          </div>
        </div>
      </div>
    </Dialog>
  );
}

function ModelScaleDialog({ model, onClose, stage }: { model: ModelState; onClose: () => void; stage?: PresenceStage }) {
  const doc = useApp((st) => st.doc);
  const activeLevelId = useApp((st) => st.activeLevelId);
  const toast = useApp((st) => st.toast);
  const reportError = useApp((st) => st.reportError);
  const defaultScale = MODEL_SCALE_DEFAULT[model.extension] ?? 1;
  const [unitChoice, setUnitChoice] = useState(MODEL_UNITS.find((u) => Math.abs(u.mm - defaultScale) < 1e-6)?.value ?? "m");
  const [scale, setScale] = useState(defaultScale);
  const [working, setWorking] = useState(false);

  if (!doc) return null;
  const levelId = activeLevelId ?? doc.project.levels[0]?.id ?? null;

  const run = async () => {
    if (!levelId) return;
    setWorking(true);
    try {
      const index = buildIndex(doc, levelId);
      const bounds = doc.project.elements.length > 0 ? modelBounds(index) : null;
      const center = bounds && !rectIsEmpty(bounds) ? { x: (bounds.minX + bounds.maxX) / 2, y: (bounds.minY + bounds.maxY) / 2 } : { x: 0, y: 0 };
      const element: Element = {
        kind: "reference_model",
        id: "",
        level_id: levelId,
        name: model.fileName.replace(/\.[^./\\]+$/, ""),
        file_name: model.fileName,
        position: center,
        rotation_deg: 0,
        elevation_mm: 0,
        scale_to_mm: scale,
        locked: false,
      } satisfies { kind: "reference_model" } & ReferenceModel;
      const result = await useApp.getState().dispatch({ type: "add_element", element });
      if (result) {
        toast("success", `Placed "${element.kind === "reference_model" ? element.name : ""}"`);
        if (result.diff.added.length > 0) {
          useApp.getState().select(result.diff.added);
          bus.emit("focus_elements", result.diff.added);
        }
        onClose();
      }
    } catch (e) {
      reportError(e);
    } finally {
      setWorking(false);
    }
  };

  return (
    <Dialog
      title={`Place ${model.fileName}`}
      onClose={onClose}
      width={420}
      stage={stage}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={working || !levelId} onClick={() => void run()} data-autofocus>
            {working ? "Placing" : "Place"}
          </Button>
        </>
      }
    >
      <p className={s.exportNote}>
        Added to the plan center as a reference model. Its real shape only shows in the 3D view; in 2D it is a small marker
        you can move, rotate and delete.
      </p>
      <Field label="Model unit" hint="What one unit in the file means, so it lands at the right size">
        <Segmented
          label="Model unit"
          stretch
          value={unitChoice}
          onChange={(v) => {
            setUnitChoice(v);
            setScale(MODEL_UNITS.find((u) => u.value === v)!.mm);
          }}
          options={[...MODEL_UNITS]}
        />
      </Field>
      <Field label="Scale to mm">
        <NumberField label="Scale to millimeters" min={0.0001} decimals={4} value={scale} onCommit={setScale} />
      </Field>
    </Dialog>
  );
}

