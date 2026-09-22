// Shared motion helpers. CONTRACT FILE - owned by the orchestrator.
// Rules and the full inventory: docs/MOTION.md. Tokens: src/styles/tokens.css.
// No animation library: CSS, the Web Animations API and requestAnimationFrame.

import { useEffect, useRef, useState } from "react";

/** Durations in ms. Keep in sync with the --dur-* tokens. */
export const DUR = { press: 80, hover: 120, base: 180, panel: 240, scene: 360 } as const;
export type DurKey = keyof typeof DUR;

const reduced =
  typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-reduced-motion: reduce)")
    : null;

/** False when the user asked for reduced motion. Canvas and three.js code must check this. */
export function motionOK(): boolean {
  return !reduced?.matches;
}

/** Duration to use from JS: 0 when motion is reduced. */
export function dur(key: DurKey): number {
  return motionOK() ? DUR[key] : 0;
}

function bezier(x1: number, y1: number, x2: number, y2: number) {
  const cx = 3 * x1, bx = 3 * (x2 - x1) - cx, ax = 1 - cx - bx;
  const cy = 3 * y1, by = 3 * (y2 - y1) - cy, ay = 1 - cy - by;
  const sx = (t: number) => ((ax * t + bx) * t + cx) * t;
  const sy = (t: number) => ((ay * t + by) * t + cy) * t;
  const dx = (t: number) => (3 * ax * t + 2 * bx) * t + cx;
  return (x: number) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    let t = x;
    for (let i = 0; i < 6; i++) {
      const d = dx(t);
      if (Math.abs(d) < 1e-6) break;
      t -= (sx(t) - x) / d;
    }
    return sy(Math.min(1, Math.max(0, t)));
  };
}

/** JS twins of the CSS easing tokens, for canvas and three.js animation. */
export const ease = {
  out: bezier(0.22, 1, 0.36, 1),
  in: bezier(0.55, 0, 0.8, 0.3),
  inOut: bezier(0.65, 0, 0.35, 1),
  /** Small settle overshoot, for things that land. */
  spring: (x: number) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    return 1 - Math.exp(-6.5 * x) * Math.cos(9 * x);
  },
} as const;

export interface Tween {
  /** Stops the tween where it is. `onDone` does not fire. */
  cancel: () => void;
  /** Jumps to the end state and fires `onDone`. */
  finish: () => void;
}

/**
 * Drives `onFrame(progress 0..1, already eased)` with requestAnimationFrame.
 * With reduced motion or a zero duration it jumps straight to 1.
 * Interruptible: cancel it and start a new one from the current value.
 */
export function tween(
  ms: number,
  onFrame: (p: number) => void,
  opts: { easing?: (x: number) => number; onDone?: () => void } = {},
): Tween {
  const easing = opts.easing ?? ease.out;
  let raf = 0;
  let done = false;
  const end = () => {
    if (done) return;
    done = true;
    cancelAnimationFrame(raf);
    onFrame(1);
    opts.onDone?.();
  };
  if (ms <= 0 || !motionOK()) {
    end();
    return { cancel: () => {}, finish: () => {} };
  }
  const t0 = performance.now();
  const step = (now: number) => {
    if (done) return;
    const x = Math.min(1, (now - t0) / ms);
    if (x >= 1) return end();
    onFrame(easing(x));
    raf = requestAnimationFrame(step);
  };
  raf = requestAnimationFrame(step);
  return {
    cancel: () => {
      done = true;
      cancelAnimationFrame(raf);
    },
    finish: end,
  };
}

export type PresenceStage = "enter" | "idle" | "exit";

/**
 * Keeps a component mounted while its exit animation plays.
 *   const p = usePresence(open, "base");
 *   if (!p.mounted) return null;
 *   <div data-stage={p.stage} ...>   // style enter/exit in CSS off [data-stage]
 */
export function usePresence(open: boolean, exit: DurKey = "base") {
  const [mounted, setMounted] = useState(open);
  const [stage, setStage] = useState<PresenceStage>(open ? "idle" : "exit");
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    window.clearTimeout(timer.current);
    if (open) {
      setMounted(true);
      setStage("enter");
      // Two frames so the enter styles are committed before switching to idle.
      const id = requestAnimationFrame(() => requestAnimationFrame(() => setStage("idle")));
      return () => cancelAnimationFrame(id);
    }
    setStage("exit");
    timer.current = window.setTimeout(() => setMounted(false), Math.round(dur(exit) * 0.7));
    return () => window.clearTimeout(timer.current);
  }, [open, exit]);

  return { mounted, stage };
}
