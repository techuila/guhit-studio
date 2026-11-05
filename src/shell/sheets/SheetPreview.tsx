// The live preview of one sheet (lane L4): the engine's sheet SVG as an
// image, refetched on every revision, with a box over each viewport. A box
// selects its viewport; dragging it moves the viewport 1:1 with the
// pointer, and on drop the engine places it (clamped into the drawing area)
// and the box settles there (FLIP, `--dur-base`).
import { useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import type { Sheet } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";
import { useApp } from "../../state/store";
import { Spinner, cx } from "../../ui/controls";
import { play } from "../../ui/motionWaapi";
import { droppedPosition, parseSheetSvg, viewTitle, type SheetLayout, type ViewportBox } from "./sheetModel";
import s from "./sheets.module.css";

interface Drag {
  id: string;
  startX: number;
  startY: number;
  dx: number;
  dy: number;
  moved: boolean;
}

export function SheetPreview({
  sheet,
  selectedViewport,
  onSelectViewport,
  onMoveViewport,
}: {
  sheet: Sheet;
  selectedViewport: string | null;
  onSelectViewport: (id: string | null) => void;
  /** `shownScale`: the scale the box shows now, null for a table or an image. */
  onMoveViewport: (id: string, position: { x: number; y: number }, shownScale: number | null) => void;
}) {
  const doc = useApp((st) => st.doc);
  const revision = useApp((st) => st.doc?.revision ?? 0);
  const [url, setUrlState] = useState<string | null>(null);
  const urlRef = useRef<string | null>(null);
  const setUrl = (next: string | null) => {
    if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    urlRef.current = next;
    setUrlState(next);
  };
  const [layout, setLayout] = useState<SheetLayout | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [drag, setDragState] = useState<Drag | null>(null);
  // The live drag, read by the pointer handlers: a fast drag can move and
  // drop before React renders the state.
  const dragRef = useRef<Drag | null>(null);
  const setDrag = (d: Drag | null) => {
    dragRef.current = d;
    setDragState(d);
  };
  const [fit, setFit] = useState<{ w: number; h: number } | null>(null);
  const stage = useRef<HTMLDivElement>(null);
  const boxes = useRef(new Map<string, HTMLDivElement>());
  // Where a dropped box was let go, in px, until the new layout lands.
  const pending = useRef<{ id: string; left: number; top: number } | null>(null);

  // Fetch the sheet on every revision. An older answer never replaces a newer one.
  useEffect(() => {
    let live = true;
    setLoading(true);
    ipc
      .sheetPreview(sheet.id)
      .then((d) => {
        if (!live) return;
        setUrl(URL.createObjectURL(new Blob([d.svg], { type: "image/svg+xml" })));
        setLayout(parseSheetSvg(d.svg));
        setError(null);
      })
      .catch((e: unknown) => {
        if (live) setError(typeof e === "object" && e && "message" in e ? String((e as { message: unknown }).message) : String(e));
      })
      .finally(() => {
        if (live) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [sheet.id, revision]);

  useEffect(
    () => () => {
      if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    },
    [],
  );

  // The paper keeps its aspect inside the stage.
  useLayoutEffect(() => {
    const el = stage.current;
    if (!el || !layout) return;
    const measure = () => {
      const r = el.getBoundingClientRect();
      const pad = 36;
      const k = Math.min((r.width - pad) / layout.page.w, (r.height - pad) / layout.page.h);
      if (k > 0) setFit({ w: layout.page.w * k, h: layout.page.h * k });
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [layout]);

  // A dropped box slides from where it was let go to where the engine put it.
  useLayoutEffect(() => {
    const p = pending.current;
    if (!p || !layout) return;
    pending.current = null;
    const el = boxes.current.get(p.id);
    if (!el) return;
    const dx = p.left - el.offsetLeft;
    const dy = p.top - el.offsetTop;
    if (Math.abs(dx) > 0.5 || Math.abs(dy) > 0.5) {
      play(el, [{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], "base", "out");
    }
  }, [layout]);

  const pxPerMm = fit && layout ? fit.w / layout.page.w : 1;
  const pct = (v: number, of: number) => `${(v / of) * 100}%`;

  const down = (e: ReactPointerEvent<HTMLDivElement>, box: ViewportBox) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.currentTarget.setPointerCapture(e.pointerId);
    onSelectViewport(box.id);
    setDrag({ id: box.id, startX: e.clientX, startY: e.clientY, dx: 0, dy: 0, moved: false });
  };
  const move = (e: ReactPointerEvent<HTMLDivElement>) => {
    const cur = dragRef.current;
    if (!cur) return;
    const dx = e.clientX - cur.startX;
    const dy = e.clientY - cur.startY;
    setDrag({ ...cur, dx, dy, moved: cur.moved || Math.hypot(dx, dy) > 3 });
  };
  const up = (e: ReactPointerEvent<HTMLDivElement>, box: ViewportBox) => {
    const cur = dragRef.current;
    if (!cur || !layout) return;
    const d = { ...cur, dx: e.clientX - cur.startX, dy: e.clientY - cur.startY };
    setDrag(null);
    if (!cur.moved && Math.hypot(d.dx, d.dy) <= 3) return;
    const el = boxes.current.get(box.id);
    if (el) pending.current = { id: box.id, left: el.offsetLeft + d.dx, top: el.offsetTop + d.dy };
    onMoveViewport(box.id, droppedPosition(box, d.dx / pxPerMm, d.dy / pxPerMm, layout.page.h), box.scale);
  };
  const nudge = (box: ViewportBox, key: string, big: boolean) => {
    if (!layout) return false;
    const step = big ? 10 : 1;
    const delta = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] }[key];
    if (!delta) return false;
    onMoveViewport(box.id, droppedPosition(box, delta[0], delta[1], layout.page.h), box.scale);
    return true;
  };

  const titleOf = (id: string) => {
    const vp = sheet.viewports.find((v) => v.id === id);
    if (!vp || !doc) return "";
    return vp.title?.trim() || viewTitle(doc, vp.view);
  };

  return (
    <div ref={stage} className={s.stage} onPointerDown={() => onSelectViewport(null)}>
      {error && !url ? (
        <p className={s.stageEmpty}>{error}</p>
      ) : layout && fit && url ? (
        <div className={s.paper} style={{ width: fit.w, height: fit.h }}>
          <img className={cx(s.paperImg, loading && s.paperImgLoading)} src={url} alt={`Sheet ${sheet.number}: ${sheet.title}`} draggable={false} />
          <div className={s.overlay}>
            {layout.viewports.map((box) => {
              const dragging = drag?.moved && drag.id === box.id;
              const f = box.frame;
              return (
                <div
                  key={box.id}
                  ref={(el) => {
                    if (el) boxes.current.set(box.id, el);
                    else boxes.current.delete(box.id);
                  }}
                  role="button"
                  tabIndex={0}
                  aria-label={`Viewport ${titleOf(box.id)}. Drag or use the arrow keys to move it.`}
                  aria-pressed={box.id === selectedViewport}
                  className={cx(s.vpBox, box.id === selectedViewport && s.vpOn, dragging && s.vpDragging)}
                  style={{
                    left: pct(f.x, layout.page.w),
                    top: pct(f.y, layout.page.h),
                    width: pct(f.w, layout.page.w),
                    height: pct(f.h, layout.page.h),
                    transform: dragging ? `translate(${drag.dx}px, ${drag.dy}px)` : undefined,
                  }}
                  onPointerDown={(e) => down(e, box)}
                  onPointerMove={move}
                  onPointerUp={(e) => up(e, box)}
                  onPointerCancel={() => setDrag(null)}
                  onKeyDown={(e) => {
                    e.stopPropagation();
                    if (nudge(box, e.key, e.shiftKey)) e.preventDefault();
                    else if (e.key === "Enter" || e.key === " ") onSelectViewport(box.id);
                  }}
                >
                  <span className={s.vpTag}>
                    {titleOf(box.id)}
                    {box.scale ? `  1:${box.scale}` : ""}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      ) : (
        <Spinner className={s.spinner} />
      )}
      {loading && url ? <Spinner size={16} className={s.spinner} /> : null}
      {sheet.viewports.length === 0 && !loading ? <span className={s.stageNote}>No views on this sheet yet. Add one in the Sheet tab.</span> : null}
    </div>
  );
}
