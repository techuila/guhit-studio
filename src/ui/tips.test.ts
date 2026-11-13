// Tooltip warm-up (lane L0): the first tooltip waits, the next ones along a
// row show at once, and the row cools down after the pointer leaves.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TIP_DELAY_MS } from "./motion";
import { WARM_MS, createTipWarmup } from "./tips";

function fakeRoot() {
  const attrs = new Map<string, string>();
  return {
    attrs,
    setAttribute: (k: string, v: string) => void attrs.set(k, v),
    removeAttribute: (k: string) => void attrs.delete(k),
  };
}

describe("tooltip warm-up", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("warms up only after the first tooltip has waited its delay", () => {
    const root = fakeRoot();
    const w = createTipWarmup(root);
    w.enter("undo");
    vi.advanceTimersByTime(TIP_DELAY_MS - 1);
    expect(w.isWarm()).toBe(false);
    vi.advanceTimersByTime(1);
    expect(w.isWarm()).toBe(true);
    expect(root.attrs.has("data-tips-warm")).toBe(true);
  });

  it("does not warm up when the pointer only passes over", () => {
    const w = createTipWarmup(fakeRoot());
    w.enter("undo");
    vi.advanceTimersByTime(TIP_DELAY_MS / 2);
    w.leave();
    vi.advanceTimersByTime(TIP_DELAY_MS * 2);
    expect(w.isWarm()).toBe(false);
  });

  it("stays warm while the pointer slides along the row, then cools down", () => {
    const root = fakeRoot();
    const w = createTipWarmup(root);
    w.enter("wall");
    vi.advanceTimersByTime(TIP_DELAY_MS);
    w.leave();
    vi.advanceTimersByTime(WARM_MS - 1);
    w.enter("door");
    expect(w.isWarm()).toBe(true);
    vi.advanceTimersByTime(WARM_MS * 3);
    expect(w.isWarm()).toBe(true);
    w.leave();
    vi.advanceTimersByTime(WARM_MS);
    expect(w.isWarm()).toBe(false);
    expect(root.attrs.has("data-tips-warm")).toBe(false);
  });
});
