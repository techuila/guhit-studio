// The tour timeline (lane L9): shots as blocks as long as they last, the
// transition into each on its left edge, a scrub bar and the playhead.
// Dragging a block's right edge makes the shot longer or shorter; the edge
// tracks the pointer 1:1 (docs/MOTION.md rule 2) at the scale the drag
// started with, and the drop is one `update_element`. Escape puts it back.
import { useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { cx } from "../../ui/controls";
import { seekTour } from "../../viewer3d/tour/player";
import { useTours } from "../../viewer3d/tour/tourStore";
import { TRANSITIONS, clock, cleanSeconds, draggedSeconds, saveTour, withShot, type TourEl } from "./tourActions";
import s from "./Tours.module.css";

/** At least this many pixels a second, so short shots stay grabbable. */
const MIN_PPS = 14;

interface Drag {
  index: number;
  startX: number;
  startSeconds: number;
  pps: number;
  seconds: number;
}

export function Timeline({ tour, selected, onSelect }: { tour: TourEl; selected: number | null; onSelect: (i: number) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(600);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [settled, setSettled] = useState<number | null>(null);
  const playhead = useTours((t) => (t.tourId === tour.id ? t.playhead : 0));

  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    setWidth(el.clientWidth);
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const lengths = tour.shots.map((sh, i) => (drag && drag.index === i ? drag.seconds : sh.seconds));
  const total = lengths.reduce((a, b) => a + b, 0);
  const fitPps = Math.max(MIN_PPS, (width - 4) / Math.max(total, 0.1));
  const pps = drag ? drag.pps : fitPps;
  const starts: number[] = [];
  lengths.reduce((at, len) => (starts.push(at), at + len), 0);

  // A drop settles with a short glow, once.
  useEffect(() => {
    if (settled === null) return;
    const id = window.setTimeout(() => setSettled(null), 260);
    return () => window.clearTimeout(id);
  }, [settled]);

  const onHandleDown = (i: number) => (e: ReactPointerEvent<HTMLSpanElement>) => {
    e.stopPropagation();
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    onSelect(i);
    setDrag({ index: i, startX: e.clientX, startSeconds: tour.shots[i].seconds, pps: fitPps, seconds: tour.shots[i].seconds });
  };
  const onHandleMove = (e: ReactPointerEvent<HTMLSpanElement>) => {
    if (!drag) return;
    setDrag({ ...drag, seconds: draggedSeconds(drag.startSeconds, e.clientX - drag.startX, drag.pps) });
  };
  const onHandleUp = () => {
    if (!drag) return;
    const next = cleanSeconds(drag.seconds);
    setDrag(null);
    setSettled(drag.index);
    if (next !== tour.shots[drag.index].seconds) saveTour(tour, withShot(tour, drag.index, { seconds: next }));
  };

  // Escape mid drag puts the edge back where it was.
  useEffect(() => {
    if (!drag) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setDrag(null);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [drag]);

  const scrubAt = (e: ReactPointerEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const t = Math.min(Math.max((e.clientX - rect.left) / pps, 0), total);
    useTours.getState().setPlayhead(t);
    void seekTour(tour.id, t);
  };
  const scrubbing = useRef(false);

  const step = pps >= 40 ? 1 : pps >= 12 ? 5 : 10;
  const ticks: number[] = [];
  for (let t = 0; t <= total + 1e-6; t += step) ticks.push(t);
  const inner = Math.max(total * pps, 1);

  return (
    <div ref={box} className={s.timeline} data-testid="tour-timeline">
      <div className={cx(s.timelineInner, drag && s.dragging)} style={{ width: inner }}>
        <div
          className={s.ruler}
          role="slider"
          aria-label="Scrub the tour"
          aria-valuemin={0}
          aria-valuemax={Math.round(total * 10) / 10}
          aria-valuenow={Math.round(playhead * 10) / 10}
          aria-valuetext={clock(playhead, true)}
          tabIndex={0}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            scrubbing.current = true;
            scrubAt(e);
          }}
          onPointerMove={(e) => {
            if (scrubbing.current) scrubAt(e);
          }}
          onPointerUp={() => {
            scrubbing.current = false;
          }}
          onKeyDown={(e) => {
            const d = e.key === "ArrowRight" ? 1 / 3 : e.key === "ArrowLeft" ? -1 / 3 : 0;
            if (!d) return;
            e.preventDefault();
            const t = Math.min(Math.max(playhead + d * (e.shiftKey ? 15 : 1), 0), total);
            useTours.getState().setPlayhead(t);
            void seekTour(tour.id, t);
          }}
        >
          {ticks.map((t) => (
            <span key={t}>
              <span className={s.tick} style={{ left: t * pps }} />
              <span className={s.tickLabel} style={{ left: t * pps }}>
                {clock(t)}
              </span>
            </span>
          ))}
        </div>
        <div className={s.track}>
          {tour.shots.map((sh, i) => {
            const len = lengths[i];
            const tr = sh.transition;
            const trSeconds = Math.min(tr.seconds, len);
            return (
              <button
                key={sh.id}
                type="button"
                className={cx(s.block, i === selected && s.blockOn, drag?.index === i && s.blockDragging, settled === i && s.settled)}
                style={{ width: Math.max(len * pps - (i > 0 ? 2 : 0), 6) }}
                onClick={() => onSelect(i)}
                aria-pressed={i === selected}
                data-testid="tour-shot"
              >
                {tr.kind !== "cut" && trSeconds > 0 ? <span className={s.shade} style={{ width: trSeconds * pps }} aria-hidden /> : null}
                <span className={s.blockName}>{sh.name}</span>
                <span className={s.blockMeta}>{len.toFixed(1)} s</span>
                <span
                  className={s.handle}
                  role="separator"
                  aria-label={`Length of ${sh.name}`}
                  onPointerDown={onHandleDown(i)}
                  onPointerMove={onHandleMove}
                  onPointerUp={onHandleUp}
                  onPointerCancel={() => setDrag(null)}
                  onClick={(e) => e.stopPropagation()}
                  data-testid="tour-shot-handle"
                />
              </button>
            );
          })}
          {tour.shots.map((sh, i) => {
            const kind = TRANSITIONS.find((k) => k.value === sh.transition.kind);
            return (
              <button
                key={`t-${sh.id}`}
                type="button"
                className={cx(s.chip, sh.transition.kind === "cut" && s.chipCut)}
                style={{ left: Math.max(starts[i] * pps, 14) }}
                onClick={() => onSelect(i)}
                data-tip={`Into "${sh.name}": ${kind?.label ?? sh.transition.kind}${sh.transition.seconds > 0 ? `, ${sh.transition.seconds} s` : ""}`}
                aria-label={`Transition into ${sh.name}: ${kind?.label}`}
              >
                {kind?.short ?? sh.transition.kind}
              </button>
            );
          })}
          <span className={s.playhead} style={{ transform: `translateX(${playhead * pps}px)` }} aria-hidden />
        </div>
      </div>
    </div>
  );
}
