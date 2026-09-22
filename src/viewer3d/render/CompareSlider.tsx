// Before and after slider: the model view on the left of the divider, the AI
// visualization on the right. The drag tracks the pointer 1:1 (MOTION.md rule
// 2); only the handle's pick-up and settle animate.

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { clampPercent, percentFromKey, percentFromPointer } from "./renderStore";
import s from "./render.module.css";

export interface CompareSliderProps {
  /** The model capture. */
  beforeSrc: string;
  /** The AI visualization. Empty shows the capture alone. */
  afterSrc: string;
  beforeLabel?: string;
  afterLabel?: string;
  /** Divider position in percent. */
  value: number;
  onChange: (pct: number) => void;
  /** "result" hides the divider and shows the AI image whole. */
  mode?: "compare" | "result";
  /** Natural size of the AI image, for the resolution badge. */
  onResolution?: (width: number, height: number) => void;
  /** Permanent badge on every AI image (DECISIONS D8). */
  aiBadge?: boolean;
  /** How much height the box may take: dock, full size studio, whole screen. */
  size?: "dock" | "full" | "screen";
  testId?: string;
}

export function CompareSlider({
  beforeSrc,
  afterSrc,
  beforeLabel = "Model view",
  afterLabel = "AI visualization",
  value,
  onChange,
  mode = "compare",
  onResolution,
  aiBadge = true,
  size = "dock",
  testId = "compare-slider",
}: CompareSliderProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const [grabbed, setGrabbed] = useState(false);
  // The box takes the shape of the image, so nothing is letterboxed unless the
  // two images really differ.
  const [ratio, setRatio] = useState(16 / 9);
  const compare = mode === "compare" && afterSrc !== "";
  const pct = compare ? clampPercent(value) : 0;

  const moveTo = useCallback(
    (clientX: number) => {
      const el = rootRef.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      onChange(percentFromPointer(clientX, r.left, r.width));
    },
    [onChange],
  );

  const onPointerDown = (e: ReactPointerEvent<HTMLElement>) => {
    if (!compare || e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    setGrabbed(true);
    moveTo(e.clientX);
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLElement>) => {
    if (!grabbed) return;
    // 1:1, no smoothing: the divider is wherever the pointer is.
    moveTo(e.clientX);
  };

  const endDrag = (e: ReactPointerEvent<HTMLElement>) => {
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
    setGrabbed(false);
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLElement>) => {
    const next = percentFromKey(e.key, pct);
    if (next === null) return;
    e.preventDefault();
    e.stopPropagation();
    onChange(next);
  };

  // The handle stays inside the box while the container resizes.
  useEffect(() => {
    if (value !== clampPercent(value)) onChange(clampPercent(value));
  }, [value, onChange]);

  return (
    <div
      ref={rootRef}
      className={s.compare}
      data-testid={testId}
      data-mode={compare ? "compare" : "result"}
      data-dragging={grabbed || undefined}
      data-size={size}
      data-pct={compare ? pct.toFixed(2) : "0"}
      style={{ "--ar": String(ratio) } as CSSProperties}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
    >
      <img
        className={s.compareImg}
        src={beforeSrc}
        alt={beforeLabel}
        draggable={false}
        onLoad={(e) => {
          const { naturalWidth: w, naturalHeight: h } = e.currentTarget;
          if (w > 0 && h > 0) setRatio(w / h);
          if (!afterSrc) onResolution?.(w, h);
        }}
      />
      {afterSrc ? (
        <div className={s.compareAfter} style={{ clipPath: `inset(0 0 0 ${pct}%)` }} data-testid="compare-after">
          <img
            className={s.compareImg}
            src={afterSrc}
            alt={afterLabel}
            draggable={false}
            onLoad={(e) => onResolution?.(e.currentTarget.naturalWidth, e.currentTarget.naturalHeight)}
          />
        </div>
      ) : null}

      {aiBadge && afterSrc ? (
        <span className={s.aiBadge} data-testid="ai-badge">
          AI visualization
        </span>
      ) : null}

      {compare ? (
        <>
          <span className={s.compareLabel} data-side="left" style={{ opacity: pct < 16 ? 0 : 1 }}>
            {beforeLabel}
          </span>
          <span className={s.compareLabel} data-side="right" style={{ opacity: pct > 84 ? 0 : 1 }}>
            {afterLabel}
          </span>
          <div className={s.compareLine} style={{ left: `${pct}%` }} aria-hidden />
          <button
            type="button"
            className={s.compareHandle}
            style={{ left: `${pct}%` }}
            data-grabbed={grabbed || undefined}
            data-testid="compare-handle"
            role="slider"
            aria-label="Compare the model view and the AI visualization"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(pct)}
            aria-valuetext={`${Math.round(pct)} percent AI visualization`}
            aria-orientation="horizontal"
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={endDrag}
            onPointerCancel={endDrag}
            onKeyDown={onKeyDown}
          >
            <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden>
              <path d="M10 7.5L5.5 12 10 16.5M14 7.5l4.5 4.5L14 16.5" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </button>
        </>
      ) : null}
    </div>
  );
}
