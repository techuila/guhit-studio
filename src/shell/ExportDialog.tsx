import { useEffect, useState } from "react";
import type { DwgConverterStatus, ExportResult, Orientation, PaperSize, PlanExportOptions, PlanFormat } from "../contract/bindings";
import { ipc, isTauri } from "../contract/ipc";
import { useApp } from "../state/store";
import { Dialog } from "../ui/Dialog";
import { Button, CheckRow, Field, Segmented, Select, Spinner, cx } from "../ui/controls";
import { Icon, type IconName } from "../ui/icons";
import type { PresenceStage } from "../ui/motion";
import { useProjectName, useShell } from "./shellStore";
import s from "./overlays.module.css";

type SceneFormat = "glb" | "obj" | "dae";
type Format = PlanFormat | "dwg2d" | "png_plan" | "png_3d" | "ifc" | "dxf3d" | SceneFormat | "bundle";
type Group = "Drawings" | "Model" | "Images" | "Project bundle";

interface FormatDef {
  value: Format;
  group: Group;
  name: string;
  /** One sentence naming the apps that open this format. */
  apps: string;
  icon: IconName;
  ext: string;
}

const GROUPS: Group[] = ["Drawings", "Model", "Images", "Project bundle"];

const FORMATS: FormatDef[] = [
  { value: "pdf", group: "Drawings", name: "PDF", apps: "Any PDF viewer, for printing or sending to a client", icon: "export", ext: "pdf" },
  { value: "svg", group: "Drawings", name: "SVG", apps: "Illustrator, Inkscape, Figma", icon: "pencil", ext: "svg" },
  { value: "dxf", group: "Drawings", name: "DXF (2D)", apps: "AutoCAD, BricsCAD, LibreCAD, SketchUp Pro", icon: "layers", ext: "dxf" },
  { value: "dwg2d", group: "Drawings", name: "DWG", apps: "AutoCAD, BricsCAD", icon: "layers", ext: "dwg" },
  { value: "ifc", group: "Model", name: "IFC", apps: "Archicad, Revit, SketchUp Pro, BIM viewers", icon: "model", ext: "ifc" },
  { value: "dxf3d", group: "Model", name: "DXF (3D)", apps: "AutoCAD, BricsCAD, SketchUp Pro", icon: "model", ext: "dxf" },
  { value: "glb", group: "Model", name: "GLB", apps: "SketchUp, Blender, web and AR viewers", icon: "view3d", ext: "glb" },
  { value: "obj", group: "Model", name: "OBJ", apps: "SketchUp, Blender, 3ds Max, most 3D apps", icon: "view3d", ext: "obj" },
  { value: "dae", group: "Model", name: "DAE (Collada)", apps: "SketchUp Free and Pro", icon: "view3d", ext: "dae" },
  { value: "png_plan", group: "Images", name: "Plan image", apps: "Any image viewer", icon: "view2d", ext: "png" },
  { value: "png_3d", group: "Images", name: "3D image", apps: "Any image viewer", icon: "view3d", ext: "png" },
  { value: "bundle", group: "Project bundle", name: ".guhit", apps: "Guhit Studio, on this computer or another", icon: "folder", ext: "guhit" },
];

const SCALES = [20, 25, 50, 75, 100, 125, 150, 200, 250, 500];

function safeFileName(name: string): string {
  const cleaned = name.replace(/[\\/:*?"<>|]+/g, " ").replace(/\s+/g, " ").trim();
  return cleaned === "" ? "plan" : cleaned;
}

/** Waits for a capture or export function to register after a view mode switch. */
async function waitFor<T>(read: () => T | null, timeoutMs: number): Promise<T | null> {
  const start = Date.now();
  for (;;) {
    const v = read();
    if (v) return v;
    if (Date.now() - start > timeoutMs) return null;
    await new Promise((r) => window.setTimeout(r, 80));
  }
}

export function ExportDialog({ onClose, stage }: { onClose: () => void; stage?: PresenceStage }) {
  const doc = useApp((st) => st.doc);
  const name = useProjectName();
  const activeLevelId = useApp((st) => st.activeLevelId);
  const hasPlanCapture = useApp((st) => st.capturePlan !== null);
  const hasViewCapture = useApp((st) => st.captureView !== null);
  const hasExportScene = useApp((st) => st.exportScene !== null);
  const toast = useApp((st) => st.toast);
  const reportError = useApp((st) => st.reportError);

  const settings = doc?.project.settings;
  const [format, setFormat] = useState<Format>("pdf");
  const [paper, setPaper] = useState<PaperSize>(settings?.paper ?? "a3");
  const [orientation, setOrientation] = useState<Orientation>("landscape");
  const [scale, setScale] = useState<string>("auto");
  const [showDimensions, setShowDimensions] = useState(true);
  const [showRoomLabels, setShowRoomLabels] = useState(true);
  const [showAssets, setShowAssets] = useState(true);
  const [titleBlock, setTitleBlock] = useState(true);
  const [working, setWorking] = useState(false);
  const [dwg, setDwg] = useState<DwgConverterStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    void ipc
      .dwgStatus()
      .then((st) => {
        if (!cancelled) setDwg(st);
      })
      .catch(() => {
        // Unknown until the bridge answers; the row just stays disabled.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (!doc || !settings) return null;

  const dwgReady = dwg?.configured === true;
  const def = FORMATS.find((f) => f.value === format)!;
  const isPlanFile = format === "pdf" || format === "svg" || format === "dxf";
  const usesSheet = format === "pdf" || format === "svg";
  const isScene = format === "glb" || format === "obj" || format === "dae";
  const scaleOptions = [
    { value: "auto", label: "Best fit for the sheet" },
    ...[...new Set([...SCALES, settings.scale_denominator])].sort((a, b) => a - b).map((n) => ({
      value: String(n),
      label: n === settings.scale_denominator ? `1:${n} (project scale)` : `1:${n}`,
    })),
  ];

  const choosePath = async (): Promise<string | null | "cancelled"> => {
    if (!isTauri) return null;
    const { save } = await import("@tauri-apps/plugin-dialog");
    const picked = await save({
      title: `Export ${def.name}`,
      defaultPath: `${safeFileName(name)}.${def.ext}`,
      filters: [{ name: def.name, extensions: [def.ext] }],
    });
    return picked ?? "cancelled";
  };

  const run = async () => {
    if (format === "dwg2d" && !dwgReady) return;
    setWorking(true);
    try {
      const path = await choosePath();
      if (path === "cancelled") return;

      let result: ExportResult;
      if (isPlanFile) {
        const options: PlanExportOptions = {
          level_id: activeLevelId,
          paper,
          orientation,
          scale_denominator: scale === "auto" ? null : Number(scale),
          show_dimensions: showDimensions,
          show_room_labels: showRoomLabels,
          show_assets: showAssets,
          title_block: titleBlock,
        };
        result = await ipc.exportPlan(format as PlanFormat, options, path);
      } else if (format === "dwg2d") {
        result = await ipc.exportModel("dwg", path);
      } else if (format === "ifc" || format === "dxf3d") {
        result = await ipc.exportModel(format, path);
      } else if (isScene) {
        const app = useApp.getState();
        const previousMode = app.viewMode;
        const mounted = app.exportScene !== null;
        if (!mounted) app.setViewMode("split");
        const restoreMode = () => {
          if (!mounted) useApp.getState().setViewMode(previousMode);
        };
        const exportScene = await waitFor(() => useApp.getState().exportScene, 4000);
        if (!exportScene) {
          restoreMode();
          toast("error", "The 3D view is not ready to export yet. Open the 3D view and try again.");
          return;
        }
        if (!mounted) await new Promise((r) => window.setTimeout(r, 400));
        let scene: { data: string; extension: string };
        try {
          scene = await exportScene(format);
        } finally {
          restoreMode();
        }
        result = await ipc.exportBytes(`${safeFileName(name)}.${scene.extension}`, scene.data, path);
      } else if (format === "bundle") {
        result = await ipc.bundleSave(path);
      } else {
        const app = useApp.getState();
        const wantPlan = format === "png_plan";
        const mounted = wantPlan ? app.capturePlan : app.captureView;
        const previousMode = app.viewMode;
        if (!mounted) app.setViewMode("split");
        const restoreMode = () => {
          if (!mounted) useApp.getState().setViewMode(previousMode);
        };
        const capture = await waitFor(() => (wantPlan ? useApp.getState().capturePlan : useApp.getState().captureView), 4000);
        if (!capture) {
          restoreMode();
          toast("error", wantPlan ? "The plan view is not ready to capture yet. Open the 2D view and try again." : "The 3D view is not ready to capture yet. Open the 3D view and try again.");
          return;
        }
        // Give a freshly mounted view a moment to draw its first frame.
        if (!mounted) await new Promise((r) => window.setTimeout(r, 400));
        let png: string;
        try {
          const shot = await capture();
          png = typeof shot === "string" ? shot : shot.png;
        } finally {
          restoreMode();
        }
        result = await ipc.exportImage(png, `${safeFileName(name)} ${wantPlan ? "plan" : "3D"}`, path);
      }

      const usedScale = result.scale_denominator ? ` at 1:${result.scale_denominator}` : "";
      toast("success", `Exported ${def.name}${usedScale} to ${result.path}`);
      onClose();
    } catch (e) {
      reportError(e);
    } finally {
      setWorking(false);
    }
  };

  return (
    <Dialog
      title="Export"
      onClose={onClose}
      width={640}
      stage={stage}
      footer={
        <>
          <span className={s.exportWhere}>{isTauri ? "You choose where to save next." : "Saved to the app's exports folder."}</span>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={working || (format === "dwg2d" && !dwgReady)} onClick={() => void run()} data-autofocus>
            {working ? "Exporting" : `Export ${def.name}`}
          </Button>
        </>
      }
    >
      <div className={s.export}>
        <div className={s.formats} role="radiogroup" aria-label="Format">
          {GROUPS.map((group) => (
            <div key={group} className={s.formatGroup}>
              <span className={s.formatGroupLabel}>{group}</span>
              {FORMATS.filter((f) => f.group === group).map((f) => {
                const disabled = f.value === "dwg2d" && !dwgReady;
                return (
                  <button
                    key={f.value}
                    type="button"
                    role="radio"
                    aria-checked={format === f.value}
                    aria-disabled={disabled}
                    className={cx(s.format, format === f.value && s.formatOn, disabled && s.formatDisabled)}
                    onClick={() => setFormat(f.value)}
                  >
                    <Icon name={f.icon} size={18} />
                    <span className={s.formatText}>
                      <strong>{f.name}</strong>
                      <small>{disabled ? "Needs the ODA File Converter, set it in Settings" : f.apps}</small>
                    </span>
                  </button>
                );
              })}
            </div>
          ))}
        </div>

        <div className={s.exportOptions}>
          {isPlanFile ? (
            <>
              {usesSheet ? (
                <>
                  <Field label="Paper">
                    <Select
                      label="Paper size"
                      value={paper}
                      onChange={setPaper}
                      options={[
                        { value: "a4", label: "A4" },
                        { value: "a3", label: "A3" },
                        { value: "a2", label: "A2" },
                        { value: "a1", label: "A1" },
                      ]}
                    />
                  </Field>
                  <Field label="Orientation">
                    <Segmented
                      label="Orientation"
                      stretch
                      value={orientation}
                      onChange={setOrientation}
                      options={[
                        { value: "landscape", label: "Landscape" },
                        { value: "portrait", label: "Portrait" },
                      ]}
                    />
                  </Field>
                  <Field label="Scale">
                    <Select label="Scale" value={scale} onChange={setScale} options={scaleOptions} />
                  </Field>
                </>
              ) : (
                <p className={s.exportNote}>{format === "dxf" ? "DXF" : "DWG"} is written at full size in millimeters, so it opens to scale in CAD.</p>
              )}
              <div className={s.exportChecks}>
                <CheckRow checked={showDimensions} onChange={setShowDimensions}>
                  Dimensions
                </CheckRow>
                <CheckRow checked={showRoomLabels} onChange={setShowRoomLabels}>
                  Room names and areas
                </CheckRow>
                <CheckRow checked={showAssets} onChange={setShowAssets}>
                  Furniture and fixtures
                </CheckRow>
                {usesSheet ? (
                  <CheckRow checked={titleBlock} onChange={setTitleBlock} hint="Project, client, location, designer, scale">
                    Title block
                  </CheckRow>
                ) : null}
              </div>
            </>
          ) : format === "dwg2d" ? (
            dwgReady ? (
              <p className={s.exportNote}>DWG is written at full size in millimeters, converted from DXF by the ODA File Converter.</p>
            ) : (
              <>
                <p className={s.exportNote}>DWG needs the free ODA File Converter installed and pointed to in Settings.</p>
                <Button
                  size="sm"
                  icon="settings"
                  onClick={() => {
                    onClose();
                    useShell.getState().open("settings");
                  }}
                >
                  Open Settings
                </Button>
              </>
            )
          ) : format === "ifc" ? (
            <p className={s.exportNote}>A complete BIM model: walls, openings, rooms, roof and materials, ready for Archicad, Revit or a BIM viewer.</p>
          ) : format === "dxf3d" ? (
            <p className={s.exportNote}>3D linework of the whole model, at full size in millimeters.</p>
          ) : isScene ? (
            <p className={s.exportNote}>
              Saves the current 3D scene, materials included.
              {hasExportScene ? "" : " The 3D view opens next to the plan to prepare the export."}
            </p>
          ) : format === "bundle" ? (
            <p className={s.exportNote}>A single .guhit file with the whole project: model, materials, snapshots and traced images. Open it on any computer running Guhit Studio.</p>
          ) : format === "png_plan" ? (
            <p className={s.exportNote}>
              Saves the plan as it looks now, with the current layers and zoom.
              {hasPlanCapture ? "" : " The plan view opens next to the 3D view to take the picture."}
            </p>
          ) : (
            <p className={s.exportNote}>
              Saves the 3D view from its current camera. It is a picture of the model, not an AI image.
              {hasViewCapture ? "" : " The 3D view opens next to the plan to take the picture."}
            </p>
          )}
        </div>
      </div>
      {working ? (
        <div className={s.exportBusy}>
          <Spinner />
        </div>
      ) : null}
    </Dialog>
  );
}
