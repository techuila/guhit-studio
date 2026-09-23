// Live 3D view of the model (Tier 1: deterministic, always in sync with the
// plan). Fills its parent. The three.js work lives in engine/ViewerEngine.

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { ipc } from "../contract/ipc";
import { bus } from "../state/bus";
import { useApp, useVisibleDoc, type ExportScene } from "../state/store";
import { isModalOpen } from "../ui/Dialog";
import { usePresence } from "../ui/motion";
import type { HiddenReason } from "./geom/walkStart";
import { liveEngineCount, ViewerEngine, type PresetKind } from "./engine/ViewerEngine";
import { useViewer, type NavMode, type ShellMode } from "./viewerStore";
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

interface SegOption<T extends string> {
  value: T;
  label: string;
  title: string;
  icon: ReactNode;
}

const NAV_OPTIONS: SegOption<NavMode>[] = [
  {
    value: "orbit",
    label: "Orbit",
    title: "Orbit around the model (Esc)",
    icon: (
      <>
        <ellipse {...stroke} cx="8" cy="8" rx="6.3" ry="2.9" transform="rotate(-22 8 8)" />
        <circle cx="8" cy="8" r="1.7" fill="currentColor" />
      </>
    ),
  },
  {
    value: "walk",
    label: "Walk",
    title: "Walk at eye height, walls stop you (Shift+W)",
    icon: (
      <>
        <circle {...stroke} cx="9" cy="2.9" r="1.4" />
        <path {...stroke} d="M8.6 5.4 7.2 9.5l2.3 1.7.8 3.3M7.2 9.5l-1.7 4.3M8.6 5.4l2.3 2.1 1.7.3M8.3 6.3 5.8 7.6l-.7 1.9" />
      </>
    ),
  },
  {
    value: "fly",
    label: "Fly",
    title: "Fly freely, nothing stops you (F while walking)",
    icon: <path {...stroke} d="M1.8 8.7 14.2 3.1 10.2 13.4 7.7 9.3zM7.7 9.3 14.2 3.1" />,
  },
];

const SHELL_OPTIONS: SegOption<ShellMode>[] = [
  {
    value: "solid",
    label: "Solid",
    title: "Solid building (X cycles Solid, X-ray, Hidden)",
    icon: (
      <>
        <path d="M8 2.2 13.5 5.2v6L8 14.2l-5.5-3v-6z" fill="currentColor" opacity="0.18" />
        <path {...stroke} d="M8 2.2 13.5 5.2v6L8 14.2l-5.5-3v-6zM2.5 5.2 8 8.2l5.5-3M8 8.2v6" />
      </>
    ),
  },
  {
    value: "xray",
    label: "X-ray",
    title: "See through the building, pipes stay solid (X)",
    icon: (
      <>
        <path {...stroke} strokeDasharray="1.6 1.6" d="M8 2.2 13.5 5.2v6L8 14.2l-5.5-3v-6zM2.5 5.2 8 8.2l5.5-3M8 8.2v6" />
        <path {...stroke} d="M4.2 10.2h7.6" />
      </>
    ),
  },
  {
    value: "hidden",
    label: "Hidden",
    title: "Hide the building, keep floors and pipes (X)",
    icon: (
      <>
        <path {...stroke} d="M2.5 10.6 8 13.6l5.5-3L8 7.6z" />
        <path {...stroke} d="M4.2 7.2h7.6" />
      </>
    ),
  },
];

/** Where a finding is, in the words of the X-ray toast. */
const HIDDEN_WHERE: Record<HiddenReason, string> = {
  wall: "through the wall",
  column: "through the column",
  below: "under the floor",
  above: "above the ceiling",
};

function switchToXray(message: string): void {
  const viewer = useViewer.getState();
  if (viewer.shell !== "solid") return;
  viewer.setShell("xray");
  useApp.getState().toast("info", message);
}

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
  const nav = useViewer((s) => s.nav);
  const shell = useViewer((s) => s.shell);
  const walkRequest = useViewer((s) => s.walkRequest);
  const [contextLost, setContextLost] = useState(false);
  const [activePreset, setActivePreset] = useState<PresetKind | null>("exterior_corner");
  const [empty, setEmpty] = useState(true);
  const [saving, setSaving] = useState(false);
  // The canvas fades in once the first frame is on screen, so a pane that
  // just appeared never shows empty sky before the model.
  const [ready, setReady] = useState(false);
  const [locked, setLocked] = useState(false);
  const [lockRefused, setLockRefused] = useState(false);
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
        onNavChange: (next) => {
          if (useViewer.getState().nav !== next) useViewer.getState().setNav(next);
          if (next !== "orbit") return;
          viewLabel.current = "Custom view";
          setActivePreset(null);
        },
        onCycleShell: () => useViewer.getState().cycleShell(),
        keysBlocked: isModalOpen,
        onPointerLock: setLocked,
        onPointerLockError: () => {
          setLockRefused(true);
          useApp.getState().toast("info", "Mouse lock is not available here. Drag to look around instead.");
        },
        onWalkTo: (reason) => {
          if (reason) switchToXray(`X-ray is on so the finding shows ${HIDDEN_WHERE[reason]}.`);
        },
      });
    } catch (e) {
      console.error("viewer3d: WebGL is not available", e);
      setContextLost(true);
      return;
    }
    engineRef.current = engine;
    if (import.meta.env.DEV) (window as unknown as { __viewer3d?: ViewerEngine }).__viewer3d = engine;
    // A remount keeps the shell the user chose, without replaying its fade.
    engine.setShell(useViewer.getState().shell, false);

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
    const offFocus = bus.on("focus_elements", (ids) => {
      if (engine.focusElements(ids)) switchToXray("X-ray is on so the pipe shows through the wall and floor.");
    });
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
      // Keys go back to the app when the 3D view is really gone: the global
      // shortcuts are quiet while walking. A remount in the same tick (React
      // StrictMode in dev) keeps walking.
      window.setTimeout(() => {
        if (liveEngineCount() === 0 && useViewer.getState().nav !== "orbit") useViewer.getState().setNav("orbit");
      }, 0);
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

  // Walk, fly or orbit, and `walk_to` requests. After the document effect, so
  // the engine has a model to stand in when it can.
  useEffect(() => {
    const engine = engineRef.current;
    if (!engine) return;
    if (walkRequest) {
      useViewer.setState({ walkRequest: null });
      engine.walkTo(walkRequest.ids, walkRequest.location);
      return;
    }
    engine.setNav(nav);
  }, [nav, walkRequest]);

  useEffect(() => {
    engineRef.current?.setShell(shell);
  }, [shell]);

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

  const walking = nav !== "orbit";

  return (
    <div className={styles.root} data-testid="viewer3d" data-nav={nav} data-shell={shell}>
      <div ref={hostRef} className={styles.canvasHost} data-ready={ready} data-testid="viewer3d-canvas" />
      {captureFlash > 0 && <div key={captureFlash} className={styles.flash} data-testid="capture-flash" aria-hidden="true" />}

      <WalkOverlay
        open={walking && !contextLost}
        nav={nav}
        engineRef={engineRef}
        locked={locked}
        lockable={!lockRefused && (engineRef.current?.pointerLockAvailable() ?? false)}
      />

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
              data-active={!walking && activePreset === p.kind}
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

        <Segmented label="Navigation" testId="nav" value={nav} options={NAV_OPTIONS} onChange={(v) => useViewer.getState().setNav(v)} />
        <Segmented label="Building shell" testId="shell" value={shell} options={SHELL_OPTIONS} onChange={(v) => useViewer.getState().setShell(v)} />

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

      {doc && empty && !contextLost && !walking && (
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
      <div className={styles.hint} data-hidden={walking}>
        Drag to orbit. Right drag to pan. Scroll to zoom. Click to select. Shift+W to walk.
      </div>
    </div>
  );
}

/** A segmented control whose active pill slides and resizes to the chosen item. */
function Segmented<T extends string>(props: {
  label: string;
  testId: string;
  value: T;
  options: SegOption<T>[];
  onChange: (value: T) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pill, setPill] = useState<{ x: number; w: number } | null>(null);
  useLayoutEffect(() => {
    const group = ref.current;
    if (!group) return;
    const measure = () => {
      const active = group.querySelector<HTMLElement>('[data-active="true"]');
      setPill(active ? { x: active.offsetLeft, w: active.offsetWidth } : null);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(group);
    return () => ro.disconnect();
  }, [props.value]);

  return (
    <div className={styles.group} data-slider="true" role="radiogroup" aria-label={props.label} ref={ref} data-testid={`${props.testId}-group`}>
      {pill && (
        <span className={styles.indicator} aria-hidden="true" style={{ transform: `translateX(${pill.x}px)`, width: `${pill.w}px` }} />
      )}
      {props.options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={props.value === o.value}
          className={styles.btn}
          data-active={props.value === o.value}
          data-testid={`${props.testId}-${o.value}`}
          title={o.title}
          onClick={() => props.onChange(o.value)}
        >
          <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
            {o.icon}
          </svg>
          <span className={styles.label}>{o.label}</span>
        </button>
      ))}
    </div>
  );
}

/** How long the key hint stays before it fades, and how long after the pointer leaves it. */
const HINT_SHOW_MS = 4200;
const HINT_LINGER_MS = 2200;

/**
 * Walk and fly overlay: a minimap of the level, a crosshair, the key hint and
 * a mouse lock button where the browser supports pointer lock.
 */
function WalkOverlay(props: {
  open: boolean;
  nav: NavMode;
  engineRef: RefObject<ViewerEngine | null>;
  locked: boolean;
  lockable: boolean;
}) {
  const presence = usePresence(props.open, "base");
  const [hintOn, setHintOn] = useState(true);
  const timer = useRef<number | undefined>(undefined);
  const { engineRef } = props;

  const fadeLater = useCallback((ms: number) => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setHintOn(false), ms);
  }, []);

  // The hint comes back on every entry and on every walk and fly switch.
  useEffect(() => {
    if (!props.open) return;
    setHintOn(true);
    fadeLater(HINT_SHOW_MS);
    return () => window.clearTimeout(timer.current);
  }, [props.open, props.nav, fadeLater]);

  const minimapRef = useCallback(
    (el: HTMLCanvasElement | null) => {
      engineRef.current?.setMinimap(el);
    },
    [engineRef],
  );

  if (!presence.mounted) return null;
  const fly = props.nav === "fly";
  return (
    <div className={styles.walkOverlay} data-stage={presence.stage} data-testid="walk-overlay" aria-hidden={!props.open}>
      <div className={styles.crosshair} aria-hidden="true" />
      <canvas ref={minimapRef} className={styles.minimap} data-testid="walk-minimap" aria-label="Minimap of this level" role="img" />
      <div className={styles.walkHintRow}>
      <div
        className={styles.walkHint}
        data-on={hintOn}
        data-testid="walk-hint"
        onPointerEnter={() => {
          window.clearTimeout(timer.current);
          setHintOn(true);
        }}
        onPointerLeave={() => fadeLater(HINT_LINGER_MS)}
      >
        {fly ? (
          <>
            <kbd>W</kbd>
            <kbd>A</kbd>
            <kbd>S</kbd>
            <kbd>D</kbd> fly · <kbd>E</kbd> up · <kbd>Q</kbd> down · drag to look · <kbd>Shift</kbd> faster · <kbd>F</kbd> walk · <kbd>X</kbd> shell ·{" "}
            <kbd>Esc</kbd> orbit
          </>
        ) : (
          <>
            <kbd>W</kbd>
            <kbd>A</kbd>
            <kbd>S</kbd>
            <kbd>D</kbd> walk · drag to look · <kbd>Shift</kbd> run · <kbd>F</kbd> fly · <kbd>X</kbd> shell · <kbd>Esc</kbd> orbit
          </>
        )}
      </div>
      </div>
      {props.lockable && (
        <div className={`${styles.group} ${styles.lockGroup}`}>
          <button
            type="button"
            className={styles.btn}
            data-active={props.locked}
            data-testid="walk-lock"
            title={props.locked ? "Press Esc to free the mouse" : "Lock the mouse to look without dragging"}
            onClick={() => (props.locked ? engineRef.current?.exitPointerLock() : engineRef.current?.requestPointerLock())}
          >
            <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
              <path {...stroke} d="M4.5 7.2h7v6.3h-7zM6 7.2V5.3a2 2 0 0 1 4 0v1.9" />
            </svg>
            <span className={styles.labelAlways}>{props.locked ? "Esc to unlock" : "Lock mouse"}</span>
          </button>
        </div>
      )}
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
