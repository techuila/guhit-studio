// The AI preview's breath (docs/MOTION.md rule 11). When a proposal's ghost
// arrives in 3D it breathes twice, opacity 0.55 to 0.85, 1.6 s each, then
// holds at 0.78. Nothing loops: once the breath lands the engine's frame loop
// stops, and the 3D view draws nothing while the architect reads the
// proposal (AGENTS.md, the 3D frame loop invariant). Pure: the engine runs it
// on its Animator under the "breath" key.

import type { Animator } from "./animator";

export const BREATH_KEY = "breath";
export const BREATH_LOW = 0.55;
export const BREATH_HIGH = 0.85;
/** One breath, low to high and back. */
export const BREATH_MS = 1600;
/** How many breaths a new proposal gets before it holds. */
export const BREATHS = 2;
/** Where the breath settles and holds, and the whole look under reduced motion. */
export const PREVIEW_OPACITY = 0.78;

/** The top of a breath on the curve's scale (0 is BREATH_LOW, 1 is PREVIEW_OPACITY). */
const PEAK = (BREATH_HIGH - BREATH_LOW) / (PREVIEW_OPACITY - BREATH_LOW);
/** Where the last half breath starts: it settles at the hold instead of falling back. */
const LAST = 1 - 1 / (2 * BREATHS);

/**
 * The breath over its whole run, `x` from 0 to 1: 0 is BREATH_LOW and 1 is
 * PREVIEW_OPACITY, so the track lands exactly on the hold and nothing jumps
 * when it ends. Cosine breaths, flat at every turn.
 */
export function breathCurve(x: number): number {
  if (x <= 0) return 0;
  if (x >= 1) return 1;
  if (x < LAST) return PEAK * (0.5 - 0.5 * Math.cos(x * 2 * BREATHS * Math.PI));
  const u = (x - LAST) / (1 - LAST);
  return 1 + (PEAK - 1) * (0.5 + 0.5 * Math.cos(u * Math.PI));
}

/**
 * Starts the breath. It is a finite track: the Animator reports it active for
 * BREATHS * BREATH_MS and then idle, holding PREVIEW_OPACITY. Reduced motion
 * lands on the hold at once.
 */
export function startBreath(anim: Animator, now: number): void {
  anim.set(BREATH_KEY, BREATH_LOW);
  anim.to(BREATH_KEY, PREVIEW_OPACITY, now, { duration: BREATHS * BREATH_MS, easing: breathCurve });
}

/** The preview's opacity at rest: no breath running. */
export function holdBreath(anim: Animator): void {
  anim.set(BREATH_KEY, PREVIEW_OPACITY);
}

/**
 * Which proposal has had its breath. A new proposal breathes from the start;
 * a hover, a selection or a rebuild on the same one never starts it over, so
 * the view stays at rest while the proposal waits.
 */
export class PreviewBreath {
  private breathed: string | null = null;

  /** `proposal`: what identifies the preview on screen (its ids), or null when no preview tint shows. */
  sync(anim: Animator, proposal: string | null, now: number): void {
    if (proposal === null) {
      this.breathed = null;
      holdBreath(anim);
      return;
    }
    if (this.breathed === proposal) return;
    this.breathed = proposal;
    startBreath(anim, now);
  }

  /** Every highlight was released: the next preview breathes again. */
  reset(anim: Animator): void {
    this.breathed = null;
    if (anim.has(BREATH_KEY)) holdBreath(anim);
  }
}
