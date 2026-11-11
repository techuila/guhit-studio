// The top bar's fit rule: it takes its fold steps in order until nothing
// overlaps (the gate's 1440 px live session with an agent overlapped at the
// old last step). The real layout is measured by the Playwright probe; this
// pins the rule it measures with.
import { describe, expect, it } from "vitest";
import { FOLD_STEPS, barFits, type BarEdges } from "./TopBar";

// A 1440 px bar: left group to 330, middle 560 to 880, right group's column from 896.
const roomy: BarEdges = { leftEnd: 330, centerStart: 560, centerEnd: 880, rightStart: 960, rightBox: 896, rightEnd: 1430, barEnd: 1430 };

describe("top bar fit", () => {
  it("fits when every group keeps to its place", () => {
    expect(barFits(roomy)).toBe(true);
  });

  it("does not fit when the right group runs past its column into the middle", () => {
    // The gate's case: the palette button over Zoom to fit.
    expect(barFits({ ...roomy, rightStart: 860 })).toBe(false);
  });

  it("does not fit when the left group's words reach the middle group", () => {
    expect(barFits({ ...roomy, leftEnd: 556 })).toBe(false);
  });

  it("does not fit when the right group spills out of the bar", () => {
    expect(barFits({ ...roomy, rightEnd: 1440 })).toBe(false);
  });

  it("folds words first, then the bar itself, one step at a time", () => {
    expect(FOLD_STEPS[0]).toEqual({ fold: 0, squeeze: 0 });
    for (let i = 1; i < FOLD_STEPS.length; i++) {
      const [a, b] = [FOLD_STEPS[i - 1], FOLD_STEPS[i]];
      expect(b.fold + b.squeeze).toBe(a.fold + a.squeeze + 1);
      expect(b.fold).toBeGreaterThanOrEqual(a.fold);
    }
    // The bar gives way only once the right group's words are all folded.
    expect(FOLD_STEPS.filter((s) => s.squeeze > 0).every((s) => s.fold === 3)).toBe(true);
  });
});
