// "Visuals" panel. Tier 1 captures of the live 3D view, tied to the model
// revision and camera they came from, plus the (not yet wired) AI
// visualization styles. Geometry is the authority, images are derived from it.

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { RenderRecord, RenderStyle } from "../contract/bindings";
import { ipc, isTauri, toIpcError } from "../contract/ipc";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { usePresence } from "../ui/motion";
import { collapseOut, flip, growIn, originTransform, play, settled } from "../ui/motionWaapi";
import { useViewer } from "./viewerStore";
import styles from "./RenderPanel.module.css";

function formatTime(rfc3339: string): string {
  const d = new Date(rfc3339);
  if (Number.isNaN(d.getTime())) return rfc3339;
  return d.toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

function sourceLabel(r: RenderRecord): string {
  return r.source === "model_view" ? "Model view" : "AI visualization";
}

function fileName(r: RenderRecord): string {
  const base = (r.camera?.name || "view").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  return `${base || "view"}-rev${r.revision}`;
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
  const [styleKey, setStyleKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const [openId, setOpenId] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const alive = useRef(true);
  const requested = useRef(new Set<string>());
  const rootRef = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLUListElement>(null);
  /** Card rectangles before the last change, for the FLIP when one is added. */
  const cardRects = useRef(new Map<string, DOMRect>());
  /** Thumbnail the large preview grew from, so it can shrink back into it. */
  const openOrigin = useRef<DOMRect | null>(null);
  const figureRef = useRef<HTMLElement>(null);
  const lightbox = usePresence(openId !== null, "base");

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
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
    void reload();
  }, [projectId, rendersVersion, reload]);

  // The panel stays mounted but hidden (`hidden` attribute) while another
  // dock tab is active, so a capture made while this tab was out of view
  // (a hub thumbnail capture, a future AI-triggered render, and so on) would
  // otherwise show a stale list. Reload whenever it becomes visible again.
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

  // Cards slide to their new place when one is added or removed. Measured
  // before React paints, animated after.
  useLayoutEffect(() => {
    const grid = gridRef.current;
    if (!grid) return;
    const items = [...grid.querySelectorAll<HTMLElement>("[data-card-id]")];
    cardRects.current = flip(items, cardRects.current, (el) => el.dataset.cardId);
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
      // The new card grows into the gallery once it is in the DOM.
      requestAnimationFrame(() => growIn(gridRef.current?.querySelector<HTMLElement>(`[data-card-id="${record.id}"]`) ?? null));
      setError(null);
      app.toast("success", "View captured");
      // Re-sync with the backend list rather than trusting only the local
      // splice above, so the gallery reflects this capture for certain.
      void reload();
    } catch (e) {
      app.reportError(e);
    } finally {
      if (alive.current) setCapturing(false);
    }
  }, [capturing, reload]);

  const goToCamera = useCallback((r: RenderRecord) => {
    const app = useApp.getState();
    if (app.viewMode === "2d") app.setViewMode("3d");
    if (app.activeCameraId) app.setActiveCamera(null);
    // Let a freshly shown viewer mount before it receives the request.
    requestAnimationFrame(() => bus.emit("apply_camera", r.camera));
  }, []);

  const saveImage = useCallback(
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

  const remove = useCallback(async (r: RenderRecord) => {
    try {
      await ipc.renderDelete(r.id);
      if (!alive.current) return;
      // The card collapses before it leaves the list, so the gallery closes
      // the gap instead of snapping shut.
      const card = gridRef.current?.querySelector<HTMLElement>(`[data-card-id="${r.id}"]`) ?? null;
      if (card) {
        cardRects.current.delete(r.id);
        await collapseOut(card);
      }
      if (!alive.current) return;
      setRecords((list) => list.filter((x) => x.id !== r.id));
      setConfirmDelete(null);
      setOpenId((id) => (id === r.id ? null : id));
    } catch (e) {
      useApp.getState().reportError(e);
    }
  }, []);

  const openCard = useCallback((r: RenderRecord, e: { currentTarget: HTMLElement }) => {
    openOrigin.current = e.currentTarget.getBoundingClientRect();
    setOpenId(r.id);
  }, []);

  const live = records.find((r) => r.id === openId) ?? null;
  const lastOpened = useRef<RenderRecord | null>(null);
  if (live) lastOpened.current = live;
  // Kept while the closing animation plays.
  const opened = live ?? lastOpened.current;

  // The large preview grows out of its thumbnail and shrinks back into it.
  useLayoutEffect(() => {
    const figure = figureRef.current;
    if (!figure || !lightbox.mounted) return;
    const to = figure.getBoundingClientRect();
    const from = originTransform(openOrigin.current, to);
    if (lightbox.stage === "exit") {
      void settled(play(figure, [{ transform: "none", opacity: 1 }, { transform: from, opacity: 0 }], "base", "in", { scale: 0.7, fill: "forwards" }));
      return;
    }
    if (lightbox.stage === "enter") {
      play(figure, [{ transform: from, opacity: 0 }, { transform: "none", opacity: 1 }], "base", "out");
    }
  }, [lightbox.mounted, lightbox.stage]);

  useEffect(() => {
    if (!live) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpenId(null);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [live]);

  return (
    <div className={styles.root} data-testid="render-panel" ref={rootRef}>
      <header className={styles.header}>
        <div>
          <h2 className={styles.title}>Visuals</h2>
          <p className={styles.sub}>Captures of the live model. Each one keeps its revision and camera.</p>
        </div>
        <button
          type="button"
          className={styles.primary}
          data-testid="capture-view"
          disabled={!hasDoc || !canCapture || capturing}
          title={canCapture ? "Save the current 3D view at 1920 x 1080" : "Open the 3D view to capture it"}
          onClick={capture}
        >
          {capturing ? "Capturing..." : "Capture view"}
        </button>
      </header>

      {!canCapture && hasDoc && <p className={styles.note}>Open the 3D view to capture it.</p>}

      {error && (
        <div className={styles.error} role="alert" data-testid="render-error">
          <span>Visuals could not be loaded: {error}</span>
          <button type="button" className={styles.link} onClick={() => void reload()}>
            Try again
          </button>
        </div>
      )}

      <section className={styles.section}>
        <div className={styles.sectionHead}>
          <h3>Model views</h3>
          <span className={styles.count}>{records.length}</span>
          <button type="button" className={styles.link} onClick={() => void reload()} disabled={loading}>
            Refresh
          </button>
        </div>

        {records.length === 0 && !error && (
          <p className={styles.empty}>
            {hasDoc ? "No captures yet. Set up a view in 3D, then press Capture view." : "Open a project to see its visuals."}
          </p>
        )}

        <ul className={styles.grid} ref={gridRef}>
          {records.map((r) => {
            const stale = r.revision < revision;
            const thumb = thumbs[r.id];
            return (
              <li key={r.id} className={styles.card} data-testid="render-card" data-card-id={r.id}>
                <button type="button" className={styles.thumb} onClick={(e) => openCard(r, e)} title="Open large">
                  {thumb ? <img src={thumb} alt={`${sourceLabel(r)}: ${r.camera?.name ?? "view"}`} /> : <span className={styles.thumbWait}>{thumb === "" ? "Image missing" : "Loading"}</span>}
                  <span className={styles.badge} data-ai={r.source !== "model_view"}>
                    {sourceLabel(r)}
                  </span>
                </button>
                <div className={styles.meta}>
                  <div className={styles.metaTop}>
                    <span className={styles.camName} title={r.camera?.name}>
                      {r.camera?.name || "View"}
                    </span>
                    <span className={styles.rev}>rev {r.revision}</span>
                  </div>
                  <div className={styles.time}>{formatTime(r.created_at)}</div>
                  {stale && (
                    <div className={styles.stale} data-testid="render-stale">
                      Model changed since this capture
                    </div>
                  )}
                </div>
                <div className={styles.actions}>
                  <button type="button" className={styles.link} onClick={(e) => openCard(r, e)}>
                    Open
                  </button>
                  <button type="button" className={styles.link} onClick={() => goToCamera(r)} data-testid="render-goto">
                    Go to camera
                  </button>
                  <button type="button" className={styles.link} onClick={() => void saveImage(r)} data-testid="render-save">
                    Save image
                  </button>
                  {confirmDelete === r.id ? (
                    <span className={styles.confirm}>
                      Delete?
                      <button type="button" className={styles.linkDanger} onClick={() => void remove(r)} data-testid="render-delete-yes">
                        Yes
                      </button>
                      <button type="button" className={styles.link} onClick={() => setConfirmDelete(null)}>
                        No
                      </button>
                    </span>
                  ) : (
                    <button type="button" className={styles.linkDanger} onClick={() => setConfirmDelete(r.id)} data-testid="render-delete">
                      Delete
                    </button>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      </section>

      <section className={styles.section} aria-disabled="true">
        <div className={styles.sectionHead}>
          <h3>AI visualization</h3>
          <span className={styles.off}>Not available</span>
        </div>
        <p className={styles.notice} data-testid="ai-disabled">
          AI rendering needs an image provider, and none is configured in this build. AI images are visualizations, not
          construction documents. They never change the model.
        </p>
        <div className={styles.styleGrid}>
          {renderStyles.map((s) => (
            <button
              key={s.key}
              type="button"
              className={styles.style}
              data-selected={styleKey === s.key}
              onClick={() => setStyleKey(s.key)}
              disabled
              title="Needs an image provider"
            >
              <span className={styles.styleName}>{s.name}</span>
              <span className={styles.styleDesc}>{s.description}</span>
            </button>
          ))}
        </div>
        <textarea
          className={styles.prompt}
          rows={3}
          disabled
          placeholder="Describe the mood, materials or time of day. Available once an image provider is configured."
        />
        <button type="button" className={styles.primary} disabled>
          Generate AI visualization
        </button>
      </section>

      {lightbox.mounted && opened && (
        <div className={styles.lightbox} data-stage={lightbox.stage} role="dialog" aria-label="Capture" onClick={() => setOpenId(null)}>
          <figure className={styles.figure} ref={figureRef} onClick={(e) => e.stopPropagation()}>
            {thumbs[opened.id] ? <img src={thumbs[opened.id]} alt={opened.camera?.name ?? "Capture"} /> : <div className={styles.thumbWait}>Loading</div>}
            <figcaption>
              <span>
                <strong>{sourceLabel(opened)}</strong> - {opened.camera?.name || "View"} - rev {opened.revision} - {formatTime(opened.created_at)}
                {opened.revision < revision ? " - model changed since this capture" : ""}
              </span>
              <span className={styles.figActions}>
                <button type="button" className={styles.linkLight} onClick={() => goToCamera(opened)}>
                  Go to camera
                </button>
                <button type="button" className={styles.linkLight} onClick={() => void saveImage(opened)}>
                  Save image
                </button>
                <button type="button" className={styles.linkLight} onClick={() => setOpenId(null)}>
                  Close
                </button>
              </span>
            </figcaption>
          </figure>
        </div>
      )}
    </div>
  );
}
