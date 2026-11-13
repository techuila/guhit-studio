// The views viewer (lane L4): one drawing at a time, full screen (an
// elevation, a section, a plan or ceiling plan, the roof or site plan, a
// schedule) from `view_svg`. Mounted once by EditorShell; it opens itself on
// the `open_view` bus event. Drag pans and the wheel zooms about the
// pointer, both 1:1; double-click or F fits; Esc closes.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ViewRef } from "../../contract/bindings";
import { ipc, toIpcError } from "../../contract/ipc";
import { bus } from "../../state/bus";
import { useApp } from "../../state/store";
import { IconButton, Select, Spinner, cx } from "../../ui/controls";
import { motionOK } from "../../ui/motion";
import { useLastTruthy, Presence } from "../../ui/motionDom";
import { SCALES, projectViews, viewKey, viewLabel } from "../sheets/sheetModel";
import s from "./views.module.css";

export function ViewsPanel() {
  const [view, setView] = useState<ViewRef | null>(null);
  useEffect(() => bus.on("open_view", ({ view }) => setView(view)), []);
  const shown = useLastTruthy(view);
  return (
    <Presence open={view !== null} exit="panel">
      {(stage) => (shown ? <Viewer view={shown} stage={stage} onPick={setView} onClose={() => setView(null)} /> : null)}
    </Presence>
  );
}

interface Pic {
  url: string;
  /** Paper size of the drawing, mm. */
  w: number;
  h: number;
  scale: number | null;
}

function Viewer({ view, stage, onPick, onClose }: { view: ViewRef; stage: "enter" | "idle" | "exit"; onPick: (v: ViewRef) => void; onClose: () => void }) {
  const doc = useApp((st) => st.doc);
  const revision = useApp((st) => st.doc?.revision ?? 0);
  const [scale, setScale] = useState<number | null>(null);
  const [pic, setPic] = useState<Pic | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  // Pan (px) and zoom (px per mm) of the drawing in the canvas.
  const [cam, setCam] = useState<{ x: number; y: number; k: number } | null>(null);
  const [ease, setEase] = useState(false);
  const [panning, setPanning] = useState(false);
  const canvas = useRef<HTMLDivElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const pan = useRef<{ x: number; y: number; cx: number; cy: number } | null>(null);
  const urlRef = useRef<string | null>(null);
  const key = viewKey(view);

  useEffect(() => setScale(null), [key]);

  useEffect(() => {
    let live = true;
    setLoading(true);
    ipc
      .viewSvg(view, scale ?? undefined)
      .then((d) => {
        if (!live) return;
        const m = d.svg.match(/viewBox="0 0 ([\d.]+) ([\d.]+)"/);
        const url = URL.createObjectURL(new Blob([d.svg], { type: "image/svg+xml" }));
        if (urlRef.current) URL.revokeObjectURL(urlRef.current);
        urlRef.current = url;
        setPic({ url, w: m ? Number(m[1]) : 420, h: m ? Number(m[2]) : 297, scale: d.scale_denominator });
        setError(null);
      })
      .catch((e: unknown) => live && setError(toIpcError(e).message))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
    // `view` is keyed by `key`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, scale, revision]);

  useEffect(
    () => () => {
      if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    },
    [],
  );

  const fit = useCallback(
    (animate: boolean) => {
      const el = canvas.current;
      if (!el || !pic) return;
      const r = el.getBoundingClientRect();
      const k = Math.min((r.width - 48) / pic.w, (r.height - 48) / pic.h);
      setEase(animate && motionOK());
      setCam({ x: (r.width - pic.w * k) / 2, y: (r.height - pic.h * k) / 2, k });
    },
    [pic],
  );

  // A new drawing fits the canvas.
  const fitted = useRef<string | null>(null);
  useLayoutEffect(() => {
    if (!pic) return;
    const sig = `${key}:${pic.w}:${pic.h}`;
    if (fitted.current !== sig) {
      fitted.current = sig;
      fit(false);
    }
  }, [pic, key, fit]);

  // Focus moves in, and back to where it was when the viewer closes: opened
  // from the Sheets panel, Esc then closes Sheets too, as in any dialog.
  useEffect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    root.current?.focus();
    return () => {
      if (before?.isConnected) before.focus();
    };
  }, []);

  const onWheel = (e: React.WheelEvent) => {
    if (!cam || !canvas.current) return;
    const r = canvas.current.getBoundingClientRect();
    const px = e.clientX - r.left;
    const py = e.clientY - r.top;
    const f = Math.exp(-e.deltaY * 0.0015);
    const k = Math.max(0.05, Math.min(80, cam.k * f));
    const g = k / cam.k;
    setEase(false);
    setCam({ x: px - (px - cam.x) * g, y: py - (py - cam.y) * g, k });
  };

  const title = viewLabel(doc ?? { project: { levels: [], elements: [] } as never }, view);
  const choices = doc ? projectViews(doc) : [];
  const groups = [...new Set(choices.map((c) => c.group))];
  const scaled = !["schedule", "notes", "vicinity_map", "perspective"].includes(view.type);

  return (
    <div
      ref={root}
      className={s.root}
      data-stage={stage}
      role="dialog"
      aria-modal="true"
      aria-label={`View: ${title}`}
      tabIndex={-1}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") onClose();
        else if (e.key === "f" || e.key === "F") fit(true);
      }}
    >
      <header className={s.head}>
        <h2>{title}</h2>
        <span className={s.scaleNote}>{pic?.scale && scaled ? `1:${pic.scale}` : scaled ? "" : "Not to scale"}</span>
        {loading ? <Spinner size={14} /> : null}
        <span className={s.spacer} />
        {scaled ? (
          <span className={s.scaleSelect}>
          <Select
            label="Scale"
            value={scale ? String(scale) : "fit"}
            onChange={(v) => setScale(v === "fit" ? null : Number(v))}
            options={[{ value: "fit", label: "Scale: best fit" }, ...SCALES.map((n) => ({ value: String(n), label: `1:${n}` }))]}
          />
          </span>
        ) : null}
        <IconButton icon="fit" label="Fit to the window" tip="Fit (F)" onClick={() => fit(true)} />
        <IconButton icon="close" label="Close" tip="Close (Esc)" tipSide="bottom-end" onClick={onClose} />
      </header>
      <nav className={s.list} aria-label="Views">
        {groups.map((g) => (
          <div key={g}>
            <div className={s.group}>{g}</div>
            {choices
              .filter((c) => c.group === g)
              .map((c) => (
                <button key={viewKey(c.view)} type="button" className={cx(s.item, viewKey(c.view) === key && s.itemOn)} onClick={() => onPick(c.view)}>
                  {c.label}
                </button>
              ))}
          </div>
        ))}
      </nav>
      <div
        ref={canvas}
        className={cx(s.canvas, panning && s.canvasPanning)}
        onWheel={onWheel}
        onDoubleClick={() => fit(true)}
        onPointerDown={(e) => {
          if (e.button !== 0 || !cam) return;
          e.currentTarget.setPointerCapture(e.pointerId);
          pan.current = { x: e.clientX, y: e.clientY, cx: cam.x, cy: cam.y };
          setEase(false);
          setPanning(true);
        }}
        onPointerMove={(e) => {
          const p = pan.current;
          if (!p || !cam) return;
          setCam({ ...cam, x: p.cx + e.clientX - p.x, y: p.cy + e.clientY - p.y });
        }}
        onPointerUp={() => {
          pan.current = null;
          setPanning(false);
        }}
        onPointerCancel={() => {
          pan.current = null;
          setPanning(false);
        }}
      >
        {error ? (
          <p className={s.status}>{error}</p>
        ) : pic && cam ? (
          <div
            className={cx(s.sheet, ease && s.sheetEase)}
            style={{ width: pic.w * cam.k, height: pic.h * cam.k, transform: `translate(${cam.x}px, ${cam.y}px)` }}
            onTransitionEnd={() => setEase(false)}
          >
            <img key={pic.url} src={pic.url} alt={title} draggable={false} />
          </div>
        ) : (
          <p className={s.status}>
            <Spinner />
          </p>
        )}
        <span className={s.hint}>Drag to pan, scroll to zoom, double-click to fit</span>
      </div>
    </div>
  );
}
