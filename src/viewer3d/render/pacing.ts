// How a render job shares the GPU with the live view (DECISIONS D23). The job
// never calls requestAnimationFrame (AGENTS.md, the 3D frame loop invariant):
// it hands the GPU one slice of path tracing, waits on a fence until that
// slice is done, and only then hands it the next one.
//
// A sample is traced band by band from the top, a band being some rows of
// the image, and a band in one or more slices from the left. The slice size
// follows the user:
// - active (pointer, wheel or key input in the last IDLE_AFTER_MS): slices of
//   about ACTIVE_SLICE_MS, each followed by as long again with the GPU left
//   alone, so the live view's frames slot in between them. Measured on an
//   Apple M5 orbiting while a render runs: 8 ms slices back to back cost the
//   live view about 1 ms of p95 frame time, 12 to 16 ms slices back to back 5
//   to 6 ms at 120 Hz; 12 ms slices with an equal rest stay within 0.5 ms of
//   the p95 with no render, indoors and out;
// - idle (no input for IDLE_AFTER_MS, or the window hidden or out of focus):
//   whole-width bands of about a tenth of a second, a whole sample in one
//   when it fits. Big draws pay: measured with GPU timers on an Apple M5 (a
//   night interior at HD), 540 rows took 583 ms in one draw, 836 ms in 16
//   and 1408 ms in 64.
// Input switches back to short slices at once: the very next slice is cut
// short, even in the middle of a band.
//
// Rows differ in cost (sky is cheap, a furnished room is not), so the job
// keeps a cost per row from the last time the row was traced and cuts each
// slice to the time it wants. Rows not measured yet count as the dearest row
// measured so far, which keeps a first sample from overrunning.
//
// Waiting. A fence is polled between message tasks (MessageChannel), which
// run as soon as the page is free, without the 4 ms clamp that nested
// setTimeout calls get. Part of a long slice is slept through on a timer
// first, so polling costs little CPU. The main thread is never blocked:
// every wait yields to input, the live view's frames and React.

export type PaceMode = "active" | "idle";

/** No input for this long and the job counts the user as away. */
export const IDLE_AFTER_MS = 1500;
/** GPU time per slice while the user works, ms. */
export const ACTIVE_SLICE_MS = 12;
/** While the user works, the GPU is left alone this many times as long as each slice took. */
export const ACTIVE_REST = 1;
/** GPU time per slice while the user is away, ms. */
export const IDLE_SLICE_MS = 100;
/** A whole sample goes in one slice while the user is away when it costs no more than this, ms. */
export const IDLE_SLICE_MAX_MS = 110;
/** Band heights are multiples of this many rows (fragment quads, GPU tiles). */
export const ROW_STEP = 8;
/** Slice widths are multiples of this many columns, and at least this wide. */
export const COL_STEP = 64;
/** Band of a first, unmeasured sample: a probe, rows. */
export const PROBE_ROWS = 32;

/** Active or idle, from the time of the last input and the window's state. */
export function paceMode(now: number, lastInputAt: number, visible: boolean, focused: boolean): PaceMode {
  if (!visible || !focused) return "idle";
  return now - lastInputAt >= IDLE_AFTER_MS ? "idle" : "active";
}

/** GPU time a slice aims at in a mode, ms. */
export function sliceTarget(mode: PaceMode): number {
  return mode === "idle" ? IDLE_SLICE_MS : ACTIVE_SLICE_MS;
}

/** How long to leave the GPU alone after a slice that took `ms`, ms: only while the user works. */
export function restAfter(mode: PaceMode, ms: number, rest = ACTIVE_REST): number {
  return mode === "active" ? Math.max(0, ms * rest) : 0;
}

/** Time between preview updates, ms: less often while the user works. */
export function previewEvery(mode: PaceMode): number {
  return mode === "active" ? 1500 : 450;
}

export interface SamplingState {
  /** Stop and save was asked for. */
  saving: boolean;
  /** Whole samples so far. */
  samples: number;
  targetSamples: number;
  elapsedMs: number;
  budgetMs: number;
}

/**
 * True when a render stops sampling: Stop and save, the sample target, or
 * the time budget. The time budget never ends a render before its first
 * whole sample, which would save nothing: a heavy view traced while the user
 * keeps working, or on a slow GPU, can need longer than the budget for one.
 */
export function samplingDone(s: SamplingState): boolean {
  if (s.saving || s.samples >= s.targetSamples) return true;
  return s.elapsedMs >= s.budgetMs && s.samples >= 1;
}

/**
 * Cost of each row of one sample at full width, ms, learned from the slices
 * traced so far. NaN until a row has been measured.
 */
export class RowCosts {
  readonly ms: Float64Array;
  /** The dearest row, found again only after a record: bands ask for it once per step. */
  private top = Number.NaN;
  private topStale = false;

  constructor(readonly rows: number) {
    this.ms = new Float64Array(rows).fill(Number.NaN);
  }

  /** The dearest row measured so far, or NaN before any. */
  get dearest(): number {
    if (this.topStale) {
      let top = Number.NaN;
      for (const v of this.ms) if (!Number.isNaN(v) && (Number.isNaN(top) || v > top)) top = v;
      this.top = top;
      this.topStale = false;
    }
    return this.top;
  }

  /** True when every row in the range has been measured. */
  known(from: number, rows: number): boolean {
    for (let y = from; y < Math.min(from + rows, this.rows); y++) if (Number.isNaN(this.ms[y])) return false;
    return true;
  }

  /**
   * Records a slice: `ms` for `share` of the width of `rows` rows from `from`,
   * spread evenly, blended with what was known.
   */
  record(from: number, rows: number, ms: number, share = 1): void {
    if (!(rows > 0) || !(ms >= 0) || !(share > 0)) return;
    const per = ms / share / rows;
    for (let y = from; y < Math.min(from + rows, this.rows); y++) {
      const old = this.ms[y];
      // A row's cost barely changes between samples: lean on the new reading,
      // keep a little of the old one to ride out a slice slowed by the live view.
      this.ms[y] = Number.isNaN(old) ? per : old * 0.3 + per * 0.7;
    }
    this.topStale = true;
  }

  /** Predicted cost of `rows` full-width rows from `from`, ms. Unmeasured rows count as the dearest one. */
  predict(from: number, rows: number): number {
    const fallback = this.dearest;
    let sum = 0;
    for (let y = from; y < Math.min(from + rows, this.rows); y++) {
      const v = this.ms[y];
      sum += Number.isNaN(v) ? fallback : v;
    }
    return sum;
  }

  /** Predicted cost of a whole sample, ms, or NaN before any row was measured. */
  get sample(): number {
    return Number.isNaN(this.dearest) ? Number.NaN : this.predict(0, this.rows);
  }
}

/** Where the next slice starts: the band's first row, its height once started, the next column. */
export interface SliceAt {
  row: number;
  /** Height of the band in progress; 0 before a band starts. */
  bandRows: number;
  col: number;
}

export interface Slice {
  rows: number;
  cols: number;
  /** Predicted GPU time, ms, or NaN when nothing is known yet. */
  predictedMs: number;
}

/**
 * Rows in the next band, starting at row `from`: as many as fit `targetMs`
 * by the row costs, in steps of ROW_STEP, at least one step, never past the
 * last row. Before any row is measured the band is a small probe. Idle, a
 * whole remaining sample that fits IDLE_SLICE_MAX_MS goes in one band.
 */
export function nextBand(costs: RowCosts, from: number, mode: PaceMode, targetMs = sliceTarget(mode)): number {
  const left = costs.rows - from;
  if (left <= 0) return 0;
  const dearest = costs.dearest;
  if (Number.isNaN(dearest)) return Math.min(left, PROBE_ROWS);
  if (mode === "idle" && costs.predict(from, left) <= IDLE_SLICE_MAX_MS) return left;
  let rows = Math.min(ROW_STEP, left);
  let sum = costs.predict(from, rows);
  while (rows < left) {
    const step = Math.min(ROW_STEP, left - rows);
    const more = costs.predict(from + rows, step);
    if (sum + more > targetMs) break;
    sum += more;
    rows += step;
  }
  // Do not leave a sliver for the next band: take it now.
  if (left - rows < ROW_STEP) rows = left;
  return rows;
}

/**
 * The next slice. A new band gets its height from `nextBand`; a band whose
 * full width would take longer than the target is cut into slices across,
 * in steps of COL_STEP columns, so a slow GPU or a busy user still gets
 * short slices. A band keeps its height until it is done.
 */
export function nextSlice(costs: RowCosts, at: SliceAt, width: number, mode: PaceMode, targetMs = sliceTarget(mode)): Slice {
  const rows = at.col > 0 && at.bandRows > 0 ? at.bandRows : nextBand(costs, at.row, mode, targetMs);
  const full = costs.predict(at.row, rows);
  const left = width - at.col;
  if (Number.isNaN(full) || full <= 0) return { rows, cols: left, predictedMs: Number.NaN };
  const perCol = full / width;
  const cap = mode === "idle" ? Math.max(targetMs, IDLE_SLICE_MAX_MS) : targetMs;
  if (perCol * left <= cap) return { rows, cols: left, predictedMs: perCol * left };
  let cols = Math.max(COL_STEP, Math.floor(targetMs / perCol / COL_STEP) * COL_STEP);
  // Do not leave a sliver: take it now.
  if (left - cols < COL_STEP) cols = left;
  cols = Math.min(cols, left);
  return { rows, cols, predictedMs: perCol * cols };
}

/**
 * How long to sleep on a timer before polling a fence for a slice predicted
 * to take `predictedMs`, ms. Half the slice at most: waking early costs a few
 * polls, waking late leaves the GPU idle.
 */
export function sleepBeforePoll(predictedMs: number, hidden: boolean): number {
  // Hidden pages get their timers slowed to about once a second: poll only.
  if (hidden || !(predictedMs > 8)) return 0;
  return Math.floor(predictedMs * 0.5 - 2);
}

/**
 * The cost to learn from a wait. A fence that was already done when the
 * timer woke says only that the slice took less than the nap; learning the
 * whole wait would make the next nap as long, and the estimate would feed
 * itself. Half the nap is learned instead: too low and the next slice gets
 * no nap and is timed exactly.
 */
export function learnedCost(waitedMs: number, napMs: number, doneOnWake: boolean): number {
  return doneOnWake && napMs > 0 ? Math.min(waitedMs, napMs) * 0.5 : waitedMs;
}

// ------------------------------------------------------------------ waiting

let channel: MessageChannel | null = null;
const queue: (() => void)[] = [];

/** Resolves on the next message task: yields to the page without a timer clamp. */
export function nextTask(): Promise<void> {
  if (typeof MessageChannel === "undefined") return new Promise((r) => setTimeout(r, 0));
  if (!channel) {
    channel = new MessageChannel();
    channel.port1.onmessage = () => queue.shift()?.();
  }
  return new Promise((r) => {
    queue.push(r);
    channel!.port2.postMessage(0);
  });
}

export const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/**
 * Waits until the GPU has done everything submitted so far, without blocking
 * the page. `ms` is the time from the fence to the wait ending: the slice's
 * cost on the GPU, plus the live view's frames that shared it. `cost` is what
 * to learn from it (`learnedCost`). No nap when `predictedMs` is not known.
 */
export async function gpuFence(
  gl: WebGL2RenderingContext,
  predictedMs: number,
  hidden: boolean,
  lost: () => boolean,
): Promise<{ ms: number; cost: number }> {
  const t0 = performance.now();
  const sync = gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0);
  if (!sync) return { ms: 0, cost: 0 };
  gl.flush();
  const nap = sleepBeforePoll(predictedMs, hidden);
  if (nap > 0) await sleep(nap);
  let polls = 0;
  for (;;) {
    const status = gl.clientWaitSync(sync, 0, 0);
    if (status === gl.ALREADY_SIGNALED || status === gl.CONDITION_SATISFIED || status === gl.WAIT_FAILED || lost()) break;
    polls++;
    await nextTask();
  }
  gl.deleteSync(sync);
  const ms = performance.now() - t0;
  return { ms, cost: learnedCost(ms, nap, polls === 0) };
}

// -------------------------------------------------------------- the user

const INPUT_EVENTS = ["pointerdown", "pointermove", "pointerup", "wheel", "keydown", "keyup", "touchstart", "touchmove"] as const;

/** Watches for input anywhere in the app, and the window's visibility and focus. */
export class InputWatch {
  /** performance.now() of the last input. Starts at creation: the click that started the render. */
  lastInputAt = performance.now();

  constructor() {
    if (typeof window === "undefined") return;
    for (const e of INPUT_EVENTS) window.addEventListener(e, this.onInput, { capture: true, passive: true });
  }

  private onInput = (): void => {
    this.lastInputAt = performance.now();
  };

  get hidden(): boolean {
    return typeof document !== "undefined" && document.visibilityState === "hidden";
  }

  mode(now = performance.now()): PaceMode {
    const focused = typeof document === "undefined" || document.hasFocus();
    return paceMode(now, this.lastInputAt, !this.hidden, focused);
  }

  dispose(): void {
    if (typeof window === "undefined") return;
    for (const e of INPUT_EVENTS) window.removeEventListener(e, this.onInput, { capture: true });
  }
}
