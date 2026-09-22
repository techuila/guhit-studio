// "Visuals" panel: the render studio. Tier 1 captures of the live 3D view,
// tied to the model revision and camera they came from, and Tier 2 AI
// visualizations made from a capture (DECISIONS D17), always offered next to
// the model view in a compare slider.
//
// Geometry is the authority, imagery is derived from it. Nothing here writes
// back into the model.

import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { RenderAiSettings, RenderRecord, RenderStyle } from "../contract/bindings";
import { ipc, isTauri, toIpcError } from "../contract/ipc";
import { bus } from "../state/bus";
import { useShell } from "../shell/shellStore";
import { useApp } from "../state/store";
import { usePresence } from "../ui/motion";
import { collapseOut } from "../ui/motionWaapi";
import { RenderLightbox } from "./render/RenderLightbox";
import { RenderStudio } from "./render/RenderStudio";
import { formatResolution, useRenderUi } from "./render/renderStore";
import { useViewer } from "./viewerStore";
import styles from "./RenderPanel.module.css";
import rs from "./render/render.module.css";

function fileName(r: RenderRecord): string {
  const base = (r.camera?.name || "view").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  const kind = r.source === "ai_visualization" ? "ai" : "view";
  return `${base || "view"}-${kind}-rev${r.revision}`;
}

export function RenderPanel() {
  const hasDoc = useApp((s) => s.doc !== null);
  const projectId = useApp((s) => s.doc?.project.id ?? null);
  const revision = useApp((s) => s.doc?.revision ?? 0);
  const canCapture = useApp((s) => s.captureView !== null);
  const rendersVersion = useViewer((s) => s.rendersVersion);

  const [records, setRecords] = useState<RenderRecord[]>([]);
  const [thumbs, setThumbs] = useState<Record<string, string>>({});
  const [renderStyles, setRenderStyles] = useState<RenderStyle[]>([]);
  const [settings, setSettings] = useState<RenderAiSettings | null>(null);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const [resolution, setResolution] = useState("");

  const fullOpen = useRenderUi((s) => s.fullOpen);
  const setFullOpen = useRenderUi((s) => s.setFullOpen);
  const compareId = useRenderUi((s) => s.compareId);
  const setCompareId = useRenderUi((s) => s.setCompareId);
  const sourceId = useRenderUi((s) => s.sourceId);
  const lightboxId = useRenderUi((s) => s.lightboxId);
  const setLightboxId = useRenderUi((s) => s.setLightboxId);
  const focusSettings = useRenderUi((s) => s.focusSettings);

  const alive = useRef(true);
  const requested = useRef(new Set<string>());
  const rootRef = useRef<HTMLDivElement>(null);
  const overlay = usePresence(fullOpen, "panel");

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  // Dev only: the AI commands can be faked with ?renderai=mock while the
  // backend provider is being built. Guarded so it never ships.
  const [mockReady, setMockReady] = useState(!import.meta.env.DEV);
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    void import("./render/renderMock")
      .then((m) => m.installRenderMock())
      .finally(() => setMockReady(true));
  }, []);

  const reload = useCallback(async () => {
    if (!useApp.getState().doc) {
      setRecords([]);
      return;
    }
    setLoading(true);
    try {
      const list = await ipc.renderList();
      if (!alive.current) return;
      const sorted = [...(Array.isArray(list) ? list : [])].sort((a, b) => b.created_at.localeCompare(a.created_at));
      setRecords(sorted);
      setError(null);
    } catch (e) {
      if (alive.current) setError(toIpcError(e).message);
    } finally {
      if (alive.current) setLoading(false);
    }
  }, []);

  const lastProject = useRef<string | null>(null);
  useEffect(() => {
    if (lastProject.current !== projectId) {
      lastProject.current = projectId;
      requested.current.clear();
      setThumbs({});
    }
    if (mockReady) void reload();
  }, [projectId, rendersVersion, reload, mockReady]);

  // The panel stays mounted but hidden while another dock tab is active, so a
  // capture made out of view would leave a stale list. Reload when it shows.
  useEffect(() => {
    const el = rootRef.current;
    if (!el || typeof IntersectionObserver === "undefined") return;
    let wasVisible = false;
    const io = new IntersectionObserver(
      ([entry]) => {
        const visible = entry.isIntersecting;
        if (visible && !wasVisible) void reload();
        wasVisible = visible;
      },
      { threshold: 0 },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [reload]);

  useEffect(() => {
    ipc
      .renderStyles()
      .then((s) => alive.current && setRenderStyles(Array.isArray(s) ? s : []))
      .catch(() => alive.current && setRenderStyles([]));
  }, []);

  const loadSettings = useCallback(() => {
    ipc
      .renderAiSettingsGet()
      .then((s) => {
        if (!alive.current) return;
        setSettings(s);
        setSettingsError(null);
      })
      .catch((e) => {
        if (!alive.current) return;
        const err = toIpcError(e);
        setSettings(null);
        setSettingsError(
          err.code === "unknown_command"
            ? "This build's backend has no AI rendering yet. Captures and comparing still work."
            : err.message,
        );
      });
  }, []);

  useEffect(() => {
    if (mockReady) loadSettings();
  }, [loadSettings, mockReady]);

  // Thumbnails load one by one. A failed image does not break the list.
  useEffect(() => {
    for (const r of records) {
      if (requested.current.has(r.id)) continue;
      requested.current.add(r.id);
      ipc
        .renderData(r.id)
        .then((url) => alive.current && setThumbs((t) => ({ ...t, [r.id]: url })))
        .catch(() => alive.current && setThumbs((t) => ({ ...t, [r.id]: "" })));
    }
  }, [records]);

  const capture = useCallback(async () => {
    const app = useApp.getState();
    if (!app.captureView || capturing) return;
    setCapturing(true);
    // The 3D view flashes white the moment the frame is grabbed.
    useViewer.getState().flashCapture();
    try {
      const { png, camera } = await app.captureView();
      const record = await ipc.renderCapture(camera, png);
      if (!alive.current) return;
      requested.current.add(record.id);
      setThumbs((t) => ({ ...t, [record.id]: png }));
      setRecords((list) => [record, ...list.filter((r) => r.id !== record.id)]);
      useRenderUi.getState().setSourceId(record.id);
      setCompareId(record.id);
      setError(null);
      app.toast("success", "View captured");
      void reload();
    } catch (e) {
      app.reportError(e);
    } finally {
      if (alive.current) setCapturing(false);
    }
  }, [capturing, reload, setCompareId]);

  const goToCamera = useCallback((r: RenderRecord) => {
    const app = useApp.getState();
    if (app.viewMode === "2d") app.setViewMode("3d");
    if (app.activeCameraId) app.setActiveCamera(null);
    useRenderUi.getState().setFullOpen(false);
    useRenderUi.getState().setLightboxId(null);
    requestAnimationFrame(() => bus.emit("apply_camera", r.camera));
  }, []);

  const download = useCallback(
    async (r: RenderRecord) => {
      const app = useApp.getState();
      try {
        const png = thumbs[r.id] || (await ipc.renderData(r.id));
        let path: string | null = null;
        if (isTauri) {
          const { save } = await import("@tauri-apps/plugin-dialog");
          path = await save({
            defaultPath: `${fileName(r)}.png`,
            filters: [{ name: "PNG image", extensions: ["png"] }],
          });
          if (!path) return; // cancelled
        }
        const result = await ipc.exportImage(png, fileName(r), path);
        app.toast("success", `Image saved to ${result.path}`);
      } catch (e) {
        app.reportError(e);
      }
    },
    [thumbs],
  );

  const remove = useCallback(
    async (r: RenderRecord) => {
      try {
        await ipc.renderDelete(r.id);
        if (!alive.current) return;
        // The card collapses before it leaves the list, so the strip closes
        // the gap instead of snapping shut.
        const card = document.querySelector<HTMLElement>(`[data-card-id="${r.id}"]`);
        if (card) await collapseOut(card);
        if (!alive.current) return;
        setRecords((list) => list.filter((x) => x.id !== r.id));
        const ui = useRenderUi.getState();
        if (ui.compareId === r.id) ui.setCompareId(null);
        if (ui.sourceId === r.id) ui.setSourceId(null);
        if (ui.lightboxId === r.id) ui.setLightboxId(null);
        void reload();
      } catch (e) {
        useApp.getState().reportError(e);
      }
    },
    [reload],
  );

  const onGenerated = useCallback(
    (record: RenderRecord) => {
      requested.current.add(record.id);
      ipc
        .renderData(record.id)
        .then((url) => alive.current && setThumbs((t) => ({ ...t, [record.id]: url })))
        .catch(() => alive.current && setThumbs((t) => ({ ...t, [record.id]: "" })));
      setRecords((list) => [record, ...list.filter((r) => r.id !== record.id)]);
      useApp.getState().toast("success", "AI visualization ready");
      void reload();
    },
    [reload],
  );

  // Escape closes the full size studio, unless the lightbox is on top of it.
  useEffect(() => {
    if (!fullOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || useRenderUi.getState().lightboxId !== null) return;
      e.stopPropagation();
      setFullOpen(false);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [fullOpen, setFullOpen]);

  const addKey = useCallback(() => {
    focusSettings();
    useShell.getState().open("settings");
  }, [focusSettings]);

  // What the lightbox shows: an AI image against its source, or a capture alone.
  const lightboxRecord = records.find((r) => r.id === lightboxId) ?? null;
  const lightboxBefore = lightboxRecord
    ? lightboxRecord.source === "ai_visualization"
      ? (thumbs[lightboxRecord.source_render_id ?? ""] ?? "")
      : (thumbs[lightboxRecord.id] ?? "")
    : "";
  const lightboxAfter = lightboxRecord?.source === "ai_visualization" ? (thumbs[lightboxRecord.id] ?? "") : "";

  useEffect(() => {
    const r = records.find((x) => x.id === (compareId ?? sourceId));
    if (!r) return;
    const src = thumbs[r.id];
    if (!src) return;
    const img = new Image();
    img.onload = () => alive.current && setResolution(formatResolution(img.naturalWidth, img.naturalHeight));
    img.src = src;
  }, [compareId, sourceId, records, thumbs]);

  const studioProps = {
    records,
    thumbs,
    renderStyles,
    settings,
    settingsError,
    revision,
    canCapture,
    capturing,
    onCapture: () => void capture(),
    onGenerated,
    onGoToCamera: goToCamera,
    onDownload: (r: RenderRecord) => void download(r),
    onDelete: (r: RenderRecord) => void remove(r),
    onEnlarge: (id: string) => setLightboxId(id),
    onAddKey: addKey,
  };

  return (
    <div className={styles.root} data-testid="render-panel" ref={rootRef}>
      <header className={styles.header}>
        <div>
          <h2 className={styles.title}>Visuals</h2>
          <p className={styles.sub}>Capture the model, then render it with AI and compare the two.</p>
        </div>
        <button
          type="button"
          className={styles.primary}
          data-testid="capture-view"
          disabled={!hasDoc || !canCapture || capturing}
          title={canCapture ? "Save the current 3D view at 1920 x 1080" : "Open the 3D view to capture it"}
          onClick={() => void capture()}
        >
          {capturing ? "Capturing..." : "Capture view"}
        </button>
      </header>

      {!canCapture && hasDoc && <p className={styles.note}>Open the 3D view to capture it.</p>}

      {error && (
        <div className={styles.error} role="alert" data-testid="render-error">
          <span>Visuals could not be loaded: {error}</span>
          <button type="button" className={styles.link} onClick={() => void reload()} disabled={loading}>
            Try again
          </button>
        </div>
      )}

      {hasDoc ? (
        <RenderStudio variant="dock" {...studioProps} onOpenFull={() => setFullOpen(true)} />
      ) : (
        <p className={styles.empty}>Open a project to see its visuals.</p>
      )}

      {/* The panel is a size container, which would trap a fixed overlay
          inside the dock, so the studio and the lightbox mount on the body. */}
      {overlay.mounted &&
        createPortal(
          <div
          className={rs.overlay}
          data-stage={overlay.stage}
          data-testid="render-studio-overlay"
          role="dialog"
          aria-label="Render studio"
          onPointerDown={(e) => e.target === e.currentTarget && setFullOpen(false)}
        >
          <div className={rs.overlayPanel}>
            <div className={rs.overlayHead}>
              <div>
                <h3 className={rs.overlayTitle}>Render studio</h3>
                <p className={rs.overlaySub}>
                  The model view on the left, the AI visualization on the right. Drag the divider to compare.
                </p>
              </div>
            </div>
            <RenderStudio variant="full" {...studioProps} onCloseFull={() => setFullOpen(false)} />
          </div>
          </div>,
          document.body,
        )}

      <RenderLightbox
        open={lightboxId !== null}
        record={lightboxRecord}
        beforeSrc={lightboxBefore}
        afterSrc={lightboxAfter}
        resolution={resolution}
        onClose={() => setLightboxId(null)}
        onDownload={(r) => void download(r)}
        onGoToCamera={goToCamera}
      />
    </div>
  );
}
