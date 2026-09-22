// DOM-measurement motion helpers that src/ui/motion.ts (contract, read-only)
// does not provide: sliding indicators, presence-by-boolean and transient
// flash/shake state. Rules and inventory: docs/MOTION.md. Tokens: src/styles/tokens.css.
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode, type RefObject } from "react";
import { usePresence, type DurKey, type PresenceStage } from "./motion";

/**
 * Wraps `usePresence` so a call site can keep its existing
 * `{open ? <X/> : null}` shape while X's exit animates: replace it with
 * `<Presence open={open} exit="base">{(stage) => <X stage={stage} />}</X>`.
 * X should render nothing special for "enter"/"idle" (CSS transitions from
 * the stylesheet's default "closed" look) and apply `data-stage={stage}`
 * somewhere CSS keys off, typically its outermost element.
 */
export function Presence({
  open,
  exit = "base",
  children,
}: {
  open: boolean;
  exit?: DurKey;
  children: (stage: PresenceStage) => ReactNode;
}) {
  const p = usePresence(open, exit);
  if (!p.mounted) return null;
  return <>{children(p.stage)}</>;
}

/**
 * Remembers the most recent non-null/non-undefined value. Pair with
 * `Presence`: the boolean that controls `open` often clears (and the data it
 * needs, e.g. "which item") in the same state update, but the exiting UI
 * still needs that data while it animates out.
 */
export function useLastTruthy<T>(value: T | null | undefined): T | null {
  const ref = useRef<T | null>(value ?? null);
  if (value !== null && value !== undefined) ref.current = value;
  return ref.current;
}

interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

function measure(container: HTMLElement, active: HTMLElement): Rect {
  const cr = container.getBoundingClientRect();
  const ar = active.getBoundingClientRect();
  return { x: ar.left - cr.left, y: ar.top - cr.top, width: ar.width, height: ar.height };
}

/**
 * Inline style for a single sliding indicator (tool rail pill, segmented
 * thumb, tab underline, palette row highlight): one element positioned with
 * `transform: translate()` and sized to the active item, measured from the
 * DOM. It does not animate on first mount or when nothing is active yet, and
 * it repositions instantly (no slide) on container resize.
 *
 * Style the indicator element with:
 *   position: absolute; top: 0; left: 0;
 *   transition: transform var(--dur-base) var(--ease-in-out), width var(--dur-base) var(--ease-in-out), height var(--dur-base) var(--ease-in-out);
 * and spread the returned style. When `instant` is true, also apply
 * `transitionDuration: "0.01ms"` (or gate on `motionOK()`), which this hook
 * does not do for you so it stays a plain data hook.
 */
export function useSlidingIndicator(
  containerRef: RefObject<HTMLElement | null>,
  activeRef: RefObject<HTMLElement | null>,
  deps: unknown[],
): { style: CSSProperties; instant: boolean; visible: boolean } {
  const [rect, setRect] = useState<Rect | null>(null);
  const [instant, setInstant] = useState(true);
  const mounted = useRef(false);

  const remeasure = () => {
    const c = containerRef.current;
    const a = activeRef.current;
    if (!c || !a) return;
    setRect(measure(c, a));
  };

  // eslint-disable-next-line react-hooks/exhaustive-deps
  useLayoutEffect(remeasure, deps);

  useEffect(() => {
    if (!mounted.current) {
      mounted.current = true;
      // Skip the very first placement, then arm the transition next frame.
      const id = requestAnimationFrame(() => setInstant(false));
      return () => cancelAnimationFrame(id);
    }
  }, [rect]);

  useEffect(() => {
    const c = containerRef.current;
    if (!c) return;
    const onResize = () => {
      setInstant(true);
      remeasure();
      requestAnimationFrame(() => requestAnimationFrame(() => setInstant(false)));
    };
    const ro = new ResizeObserver(onResize);
    ro.observe(c);
    window.addEventListener("resize", onResize);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", onResize);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!rect) return { style: {}, instant: true, visible: false };
  return {
    style: { transform: `translate(${rect.x}px, ${rect.y}px)`, width: rect.width, height: rect.height },
    instant,
    visible: true,
  };
}

/**
 * A one-shot transient flag, true for `ms` after each call to `fire()`.
 * Used for the number field's "committed" teal flash and "invalid" shake.
 * Retriggerable: calling `fire()` again while already active restarts it.
 */
export function useFlash(ms: number): { active: boolean; fire: () => void } {
  const [active, setActive] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  const fire = () => {
    window.clearTimeout(timer.current);
    setActive(false);
    // Force a reflow so a retrigger restarts a CSS animation keyed by the class.
    requestAnimationFrame(() => {
      setActive(true);
      timer.current = window.setTimeout(() => setActive(false), ms);
    });
  };

  useEffect(() => () => window.clearTimeout(timer.current), []);

  return { active, fire };
}
