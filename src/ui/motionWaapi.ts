// Web Animations API helpers for DOM surfaces that need measured motion:
// grow in, collapse out, FLIP and origin transforms. Durations and curves come
// from the tokens in src/styles/tokens.css through ./motion, never from raw
// numbers: `cssEase` reads the --ease-* custom properties off the document so
// a WAAPI animation uses exactly the curve the CSS uses.

import { dur, motionOK, type DurKey } from "./motion";

export type EaseName = "out" | "in" | "inOut" | "spring";

const VAR: Record<EaseName, string> = {
  out: "--ease-out",
  in: "--ease-in",
  inOut: "--ease-in-out",
  spring: "--ease-spring",
};

const cache = new Map<EaseName, string>();

/** The CSS easing token as a string WAAPI accepts. */
export function cssEase(name: EaseName): string {
  const hit = cache.get(name);
  if (hit) return hit;
  let value = "ease-out";
  if (typeof document !== "undefined") {
    const read = getComputedStyle(document.documentElement).getPropertyValue(VAR[name]).trim();
    if (read) value = read;
  }
  cache.set(name, value);
  return value;
}

/**
 * Runs a keyframe animation with a duration token. Returns null and leaves
 * the element alone when motion is reduced or the element is gone.
 */
export function play(
  el: Element | null | undefined,
  keyframes: Keyframe[],
  durKey: DurKey,
  easing: EaseName = "out",
  opts: { scale?: number; delay?: number; fill?: FillMode } = {},
): Animation | null {
  if (!el || !motionOK() || typeof el.animate !== "function") return null;
  const ms = dur(durKey) * (opts.scale ?? 1);
  if (ms <= 0) return null;
  return el.animate(keyframes, {
    duration: ms,
    easing: cssEase(easing),
    delay: opts.delay ?? 0,
    fill: opts.fill ?? "none",
  });
}

/** Resolves when the animation ends, right away when there was none. */
export async function settled(anim: Animation | null): Promise<void> {
  if (!anim) return;
  try {
    await anim.finished;
  } catch {
    // Cancelled because the element went away. Nothing to do.
  }
}

/** Grows an element from zero height into its natural height. */
export function growIn(el: HTMLElement | null, durKey: DurKey = "panel"): Animation | null {
  if (!el) return null;
  const height = el.getBoundingClientRect().height;
  if (height <= 0) return null;
  const previous = el.style.overflow;
  const anim = play(
    el,
    [
      { height: "0px", opacity: 0, transform: "scale(0.96)", overflow: "hidden" },
      { height: `${height}px`, opacity: 1, transform: "none", overflow: "hidden" },
    ],
    durKey,
  );
  if (anim) void settled(anim).then(() => (el.style.overflow = previous));
  return anim;
}

/** Collapses an element to nothing. Await it, then unmount. */
export async function collapseOut(el: HTMLElement | null, durKey: DurKey = "base"): Promise<void> {
  if (!el) return;
  const height = el.getBoundingClientRect().height;
  await settled(
    play(
      el,
      [
        { height: `${height}px`, opacity: 1, overflow: "hidden" },
        { height: "0px", opacity: 0, overflow: "hidden" },
      ],
      durKey,
      "in",
      { fill: "forwards" },
    ),
  );
}

/**
 * FLIP: pass the rectangles the tracked children had before the change and
 * every one that moved slides from where it was. Returns the new rectangles
 * to keep for the next change.
 */
export function flip(
  items: Iterable<HTMLElement>,
  before: Map<string, DOMRect>,
  key: (el: HTMLElement) => string | undefined,
  durKey: DurKey = "base",
): Map<string, DOMRect> {
  const after = new Map<string, DOMRect>();
  for (const el of items) {
    const id = key(el);
    if (!id) continue;
    const rect = el.getBoundingClientRect();
    after.set(id, rect);
    const was = before.get(id);
    if (!was) continue;
    const dx = was.left - rect.left;
    const dy = was.top - rect.top;
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) continue;
    play(el, [{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], durKey, "inOut");
  }
  return after;
}

/**
 * Opens a panel from the rectangle it came from (a thumbnail, a button) and
 * closes back into it. `from` is null when there is no origin to grow from.
 */
export function originTransform(from: DOMRect | null, to: DOMRect): string {
  if (!from || to.width < 1 || to.height < 1) return "scale(0.96)";
  const scale = Math.max(Math.min(from.width / to.width, 1), 0.05);
  const dx = from.left + from.width / 2 - (to.left + to.width / 2);
  const dy = from.top + from.height / 2 - (to.top + to.height / 2);
  return `translate(${dx}px, ${dy}px) scale(${scale})`;
}
