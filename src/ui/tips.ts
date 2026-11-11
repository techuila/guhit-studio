// Tooltip warm-up (MOTION.md, tooltips). A tooltip waits --tip-delay before
// it shows. Once one has shown, the next shows at once while the pointer
// slides along a row of buttons (the rail, the top bar), and the row cools
// down WARM_MS after the pointer leaves every tooltip. global.css reads the
// `data-tips-warm` attribute this sets on <html>.
import { TIP_DELAY_MS } from "./motion";

/** How long the row stays warm after the pointer leaves the last tooltip. */
export const WARM_MS = 600;

interface Root {
  setAttribute(name: string, value: string): void;
  removeAttribute(name: string): void;
}

export interface TipWarmup {
  /** The pointer entered an element with a tooltip. */
  enter(el: unknown): void;
  /** The pointer left that element for one without a tooltip. */
  leave(): void;
  isWarm(): boolean;
  dispose(): void;
}

export function createTipWarmup(root: Root): TipWarmup {
  let warm = false;
  let current: unknown = null;
  let showTimer: ReturnType<typeof setTimeout> | undefined;
  let coolTimer: ReturnType<typeof setTimeout> | undefined;
  const setWarm = (on: boolean) => {
    warm = on;
    if (on) root.setAttribute("data-tips-warm", "");
    else root.removeAttribute("data-tips-warm");
  };
  return {
    enter(el) {
      clearTimeout(coolTimer);
      if (el === current) return;
      current = el;
      if (warm) return;
      clearTimeout(showTimer);
      // The tooltip has shown once the delay passes with the pointer still on it.
      showTimer = setTimeout(() => setWarm(true), TIP_DELAY_MS);
    },
    leave() {
      current = null;
      clearTimeout(showTimer);
      clearTimeout(coolTimer);
      if (warm) coolTimer = setTimeout(() => setWarm(false), WARM_MS);
    },
    isWarm: () => warm,
    dispose() {
      clearTimeout(showTimer);
      clearTimeout(coolTimer);
      if (warm) setWarm(false);
    },
  };
}

function tipOf(target: EventTarget | null): Element | null {
  if (!(target instanceof Element)) return null;
  const el = target.closest("[data-tip]");
  return el && el.getAttribute("data-tip") && el.getAttribute("data-tip-off") !== "true" ? el : null;
}

/** Wires the warm-up to the document. Call once at startup; returns an uninstall. */
export function installTipWarmup(doc: Document = document): () => void {
  const warmup = createTipWarmup(doc.documentElement);
  const over = (e: PointerEvent) => {
    const el = tipOf(e.target);
    if (el) warmup.enter(el);
  };
  const out = (e: PointerEvent) => {
    if (!tipOf(e.target)) return;
    if (!tipOf(e.relatedTarget)) warmup.leave();
  };
  doc.addEventListener("pointerover", over, true);
  doc.addEventListener("pointerout", out, true);
  return () => {
    doc.removeEventListener("pointerover", over, true);
    doc.removeEventListener("pointerout", out, true);
    warmup.dispose();
  };
}
