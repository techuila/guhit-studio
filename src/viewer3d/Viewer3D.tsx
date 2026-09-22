// Live 3D view of the model (Tier 1: deterministic, always in sync with the
// plan). Fills its parent. The three.js work lives in engine/ViewerEngine.

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ipc } from "../contract/ipc";
import { bus } from "../state/bus";
import { useApp, useVisibleDoc, type ExportScene } from "../state/store";
import { ViewerEngine, type PresetKind } from "./engine/ViewerEngine";
import { useViewer } from "./viewerStore";
import styles from "./Viewer3D.module.css";

const CAPTURE_W = 1920;
const CAPTURE_H = 1080;

interface PresetButton {
  kind: PresetKind;
  label: string;
  title: string;
  icon: ReactNode;
}

const stroke = { fill: "none", stroke: "currentColor", strokeWidth: 1.4, strokeLinecap: "round", strokeLinejoin: "round" } as const;

const PRESETS: PresetButton[] = [
  {
    kind: "exterior_corner",
    label: "Corner",
    title: "Exterior corner view",
    icon: <path {...stroke} d="M2.5 6.5 8 3.5l5.5 3v6L8 15.5l-5.5-3zM8 9.5v6M8 9.5l5.5-3M8 9.5l-5.5-3" transform="translate(0 -1.5)" />,
  },
  {
    kind: "eye_level",
    label: "Eye level",
    title: "Eye level exterior, 1600 mm",
    icon: (
      <>
        <path {...stroke} d="M1.5 8S4 3.8 8 3.8 14.5 8 14.5 8 12 12.2 8 12.2 1.5 8 1.5 8z" />
        <circle {...stroke} cx="8" cy="8" r="1.9" />
      </>
    ),
  },
  {
    kind: "top",
    label: "Top",
    title: "Top view, north up",
    icon: <path {...stroke} d="M2.5 2.5h11v11h-11zM2.5 8h6M8.5 2.5v11" />,
  },
  {
    kind: "axonometric",
    label: "Axo",
    title: "Axonometric view",
    icon: <path {...stroke} d="M8 1.8 13.8 5v6.2L8 14.4 2.2 11.2V5zM2.2 5 8 8.2 13.8 5M8 8.2v6.2" />,
  },
  {
    kind: "room_interior",
    label: "Room",
    title: "Stand inside the selected room",
    icon: <path {...stroke} d="M2 13.5V3.2l5-1.4v13.4zM7 3h7v10.5H7M5.2 8.4v.9" />,
  },
];

export function Viewer3D() {
  const hostRef = useRef<HTMLDivElement>(null);
  const engineRef = useRef<ViewerEngine | null>(null);
  const viewLabel = useRef("Exterior corner");

  const doc = useVisibleDoc();
  // The committed document: while a preview is active, `doc` above is the
  // hypothetical post-proposal state, so removed elements only exist here.
  const committedProject = useApp((s) => s.doc?.project ?? null);
  const selection = useApp((s) => s.selection);
  const hoverId = useApp((s) => s.hoverId);
  const previewDiff = useApp((s) => s.preview?.diff ?? null);
  const activeLevelId = useApp((s) => s.activeLevelId);
  const activeCameraId = useApp((s) => s.activeCameraId);

  const { roofVisible, cutaway, shadows, toggleRoof, toggleCutaway, toggleShadows } = useViewer();
  const captureFlash = useViewer((s) => s.captureFlash);
  const [contextLost, setContextLost] = useState(false);
  const [activePreset, setActivePreset] = useState<PresetKind | null>("exterior_corner");
  const [empty, setEmpty] = useState(true);
  const [saving, setSaving] = useState(false);
  // The canvas fades in once the first frame is on screen, so a pane that
  // just appeared never shows empty sky before the model.
  const [ready, setReady] = useState(false);
  const presetsRef = useRef<HTMLDivElement>(null);
  const [indicator, setIndicator] = useState<{ x: number; w: number } | null>(null);

  // Engine lifecycle. StrictMode mounts twice: the first engine is fully
  // disposed by the cleanup before the second one is created.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let engine: ViewerEngine;
    try {
      engine = new ViewerEngine(host, {
        onPick: (id, additive) => {
          const app = useApp.getState();
          if (id) app.select([id], additive);
          else if (!additive) app.select([]);
        },
        onHover: (id) => useApp.getState().setHover(id),
        onUserOrbit: () => {
          viewLabel.current = "Custom view";
          setActivePreset(null);
          if (useApp.getState().activeCameraId) useApp.getState().setActiveCamera(null);
        },
        onContextLost: setContextLost,
        onFirstRender: () => setReady(true),
        fetchReferenceModel: (name) => ipc.modelData(name),
        onReferenceModelWarning: (message) => useApp.getState().toast("info", message),
      });
    } catch (e) {
      console.error("viewer3d: WebGL is not available", e);
      setContextLost(true);
      return;
    }
    engineRef.current = engine;
    if (import.meta.env.DEV) (window as unknown as { __viewer3d?: ViewerEngine }).__viewer3d = engine;

    const capture = async () => ({
      png: engine.capture(CAPTURE_W, CAPTURE_H),
      camera: engine.currentCamera(viewLabel.current),
    });
    useApp.getState().registerCaptureView(capture);

    const exportScene: ExportScene = (format) => engine.exportScene(format);
    useApp.getState().registerExportScene(exportScene);

    const offFit = bus.on("zoom_to_fit", () => {
      engine.goPreset("fit", []);
    });
    const offFocus = bus.on("focus_elements", (ids) => engine.focusElements(ids));
    const offCamera = bus.on("apply_camera", (camera) => {
      viewLabel.current = camera.name || "Saved view";
      setActivePreset(null);
      engine.flyToCamera(camera);
    });

    return () => {
      offFit();
      offFocus();
      offCamera();
      if (useApp.getState().captureView === capture) useApp.getState().registerCaptureView(null);
      if (useApp.getState().exportScene === exportScene) useApp.getState().registerExportScene(null);
      if (useApp.getState().hoverId) useApp.getState().setHover(null);
      engine.dispose();
      if (engineRef.current === engine) engineRef.current = null;
    };
  }, []);

  // A proposal's removed elements are gone from `doc` (the preview state).
  // Draw them from the committed project so a delete is visible in 3D.
  const ghostsRemoved = useMemo(() => {
    if (!previewDiff || previewDiff.removed.length === 0 || !committedProject) return null;
    return { project: committedProject, ids: previewDiff.removed };
  }, [previewDiff, committedProject]);

  useEffect(() => {
    const engine = engineRef.current;
    if (!engine) return;
    engine.setDoc(doc, { cutaway, roofVisible, activeLevelId, ghostsRemoved });
    setEmpty(engine.isEmpty());
  }, [doc, cutaway, roofVisible, activeLevelId, ghostsRemoved]);

  const previewIds = useMemo(
    () => (previewDiff ? [...previewDiff.added, ...previewDiff.modified] : []),
    [previewDiff],
  );
  useEffect(() => {
    engineRef.current?.setHighlights({ selection, hoverId, previewIds });
  }, [selection, hoverId, previewIds, doc, cutaway, activeLevelId]);

  useEffect(() => {
    engineRef.current?.setShadows(shadows);
  }, [shadows]);

  // Fly to the active Camera element.
  useEffect(() => {
    if (!activeCameraId) return;
    const el = useApp.getState().doc?.project.elements.find((e) => e.id === activeCameraId);
    if (!el || el.kind !== "camera") return;
    viewLabel.current = el.name || "Saved view";
    setActivePreset(null);
    engineRef.current?.flyToCamera(el);
  }, [activeCameraId]);

  // The active pill slides between camera presets. It is measured, because
  // the labels collapse at narrow widths and the buttons change size.
  useLayoutEffect(() => {
    const group = presetsRef.current;
    if (!group) return;
    const measure = () => {
      const active = group.querySelector<HTMLElement>('[data-active="true"]');
      setIndicator(active ? { x: active.offsetLeft, w: active.offsetWidth } : null);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(group);
    return () => ro.disconnect();
  }, [activePreset]);

  const goPreset = useCallback((p: PresetButton) => {
    const engine = engineRef.current;
    if (!engine) return;
    const app = useApp.getState();
    if (app.activeCameraId) app.setActiveCamera(null);
    const pose = engine.goPreset(p.kind, app.selection);
    if (!pose) {
      app.toast("info", "No room to stand in yet. Close a room with walls first.");
      return;
    }
    viewLabel.current = pose.name;
    setActivePreset(p.kind);
  }, []);

  const saveView = useCallback(async () => {
    const engine = engineRef.current;
    const app = useApp.getState();
    if (!engine || !app.doc || saving) return;
    const count = app.doc.project.elements.filter((e) => e.kind === "camera").length;
    const name = `View ${count + 1}`;
    const pose = engine.currentCamera(name);
    setSaving(true);
    const result = await app.dispatch({ type: "add_element", element: { kind: "camera", ...pose, id: "" } });
    setSaving(false);
    if (result) {
      viewLabel.current = name;
      app.toast("success", `Saved ${name}`);
    }
  }, [saving]);

  return (
    <div className={styles.root} data-testid="viewer3d">
      <div ref={hostRef} className={styles.canvasHost} data-ready={ready} data-testid="viewer3d-canvas" />
      {captureFlash > 0 && <div key={captureFlash} className={styles.flash} data-testid="capture-flash" aria-hidden="true" />}

      <div className={styles.toolbar} role="toolbar" aria-label="3D view">
        <div className={styles.group} data-slider="true" ref={presetsRef}>
          {indicator && (
            <span
              className={styles.indicator}
              data-testid="preset-indicator"
              aria-hidden="true"
              style={{ transform: `translateX(${indicator.x}px)`, width: `${indicator.w}px` }}
            />
          )}
          {PRESETS.map((p) => (
            <button
              key={p.kind}
              type="button"
              className={styles.btn}
              data-active={activePreset === p.kind}
              data-testid={`preset-${p.kind}`}
              title={p.title}
              onClick={() => goPreset(p)}
            >
              <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
                {p.icon}
              </svg>
              <span className={styles.label}>{p.label}</span>
            </button>
          ))}
          <button
            type="button"
            className={styles.btn}
            data-testid="preset-fit"
            title="Fit the whole model"
            onClick={() => engineRef.current?.goPreset("fit", [])}
          >
            <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
              <path {...stroke} d="M2.5 6V2.5H6M10 2.5h3.5V6M13.5 10v3.5H10M6 13.5H2.5V10" />
            </svg>
            <span className={styles.label}>Fit</span>
          </button>
        </div>

        <div className={styles.group}>
          <Toggle id="toggle-roof" on={roofVisible && !cutaway} disabled={cutaway} onClick={toggleRoof} title="Show or hide the roof">
            Roof
          </Toggle>
          <Toggle id="toggle-cutaway" on={cutaway} onClick={toggleCutaway} title="Cut the walls at 1200 mm to see inside">
            Cutaway
          </Toggle>
          <Toggle id="toggle-shadows" on={shadows} onClick={toggleShadows} title="Sun shadows">
            Shadows
          </Toggle>
        </div>

        <div className={styles.group}>
          <button
            type="button"
            className={styles.btn}
            data-testid="save-view"
            title="Save this camera position as a view"
            disabled={!doc || saving}
            onClick={saveView}
          >
            <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
              <path {...stroke} d="M2 5.5h2.4l1-1.6h5.2l1 1.6H14v7.2H2z" />
              <circle {...stroke} cx="8" cy="8.9" r="2.2" />
            </svg>
            <span className={styles.label}>Save view</span>
          </button>
        </div>
      </div>

      {doc && empty && !contextLost && (
        <div className={styles.notice}>
          <strong>Nothing to show yet</strong>
          <span>Draw walls in the plan and they appear here right away.</span>
        </div>
      )}
      {contextLost && (
        <div className={styles.notice} data-testid="context-lost">
          <strong>3D view paused</strong>
          <span>The graphics context was lost. It comes back on its own when the system frees it.</span>
        </div>
      )}
      <div className={styles.hint}>Drag to orbit. Right drag to pan. Scroll to zoom. Click to select.</div>
    </div>
  );
}

function Toggle(props: {
  id: string;
  on: boolean;
  disabled?: boolean;
  title: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      className={styles.btn}
      data-testid={props.id}
      data-active={props.on}
      aria-pressed={props.on}
      disabled={props.disabled}
      title={props.title}
      onClick={props.onClick}
    >
      <span className={styles.dot} aria-hidden="true" />
      <span className={styles.labelAlways}>{props.children}</span>
    </button>
  );
}
