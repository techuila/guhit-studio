// Pure decision for what a container resize should do to the view. Kept
// separate from PlanController so the policy is testable without a canvas,
// a ResizeObserver or real timers.
//
// The bug this exists to prevent: the shell animates pane size with CSS
// (--dur-panel) on a view mode change, so the 2D canvas container passes
// through a burst of intermediate sizes, some very small. Refitting on every
// one of those intermediate sizes (the old behavior) leaves the plan fit to
// whatever the smallest or otherwise last-checked intermediate size was,
// because once the model "fits" the shrunk view it never refits again as the
// pane keeps growing.
//
// The fix: never let an intermediate tick change scale. Only the trailing
// edge of a burst (no resize call for about one --dur-panel) may refit, and
// only when the view is in "auto fit" state.

/** What a single resize callback should do. */
export type ResizeAction =
  /** Zero size viewport: nothing to compute. */
  | "ignore"
  /** Keep the model point at the pane center where it was; scale unchanged. */
  | "recenter"
  /** Refit the view to the model bounds: only on the settled trailing edge, only in auto fit. */
  | "fit";

export interface ResizeDecisionInput {
  /** True after zoom-to-fit / first-load fit / pressing F; false after any user wheel, pinch, pan or keyboard zoom. */
  autoFit: boolean;
  /** True only on the trailing edge of a resize burst (the debounced call), false on every live tick. */
  settled: boolean;
  width: number;
  height: number;
}

export function decideResize(input: ResizeDecisionInput): ResizeAction {
  if (!(input.width > 0) || !(input.height > 0)) return "ignore";
  if (input.settled && input.autoFit) return "fit";
  return "recenter";
}
