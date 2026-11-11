// The AI preview's breath in 3D (docs/MOTION.md rule 11, M3): two breaths
// when a proposal arrives, then a hold at 78%, and the frame loop goes idle.
import { describe, expect, it } from "vitest";
import { Animator } from "./animator";
import { BREATH_HIGH, BREATH_KEY, BREATH_LOW, BREATH_MS, BREATHS, PREVIEW_OPACITY, PreviewBreath, breathCurve, startBreath } from "./breath";

const full = () => new Animator(() => true);
const reduced = () => new Animator(() => false);
const at = (a: Animator, t: number) => {
  a.sample(t);
  return a.value(BREATH_KEY);
};

/**
 * The engine's frame loop, reduced to what decides whether it goes on
 * (ViewerEngine `schedule` and `frame`): at most one pending frame, each
 * frame samples the Animator, and another frame is asked for only while
 * something still animates. Returns how many frames ran and when the last one did.
 */
function runFrameLoop(anim: Animator, start: number, frameMs = 16): { frames: number; endedAt: number; maxPending: number } {
  const queue: Array<(now: number) => void> = [];
  let pending = 0;
  let maxPending = 0;
  const schedule = (cb: (now: number) => void) => {
    if (pending > 0) return; // one pending frame at a time
    pending++;
    maxPending = Math.max(maxPending, pending);
    queue.push(cb);
  };
  const frame = (now: number) => {
    anim.sample(now);
    if (anim.animating()) schedule(frame);
  };
  schedule(frame);
  let now = start;
  let frames = 0;
  while (queue.length > 0 && frames < 100_000) {
    const cb = queue.shift()!;
    pending--;
    now += frameMs;
    frames++;
    cb(now);
  }
  return { frames, endedAt: now, maxPending };
}

describe("the AI preview breath in 3D", () => {
  it("breathes twice, 0.55 to 0.85 and 1.6 s each, then holds at 0.78", () => {
    const a = full();
    startBreath(a, 0);
    expect(a.value(BREATH_KEY)).toBe(BREATH_LOW);
    expect(at(a, BREATH_MS / 2)).toBeCloseTo(BREATH_HIGH, 6);
    expect(at(a, BREATH_MS)).toBeCloseTo(BREATH_LOW, 6);
    expect(at(a, BREATH_MS * 1.5)).toBeCloseTo(BREATH_HIGH, 6);
    expect(a.active()).toBe(1);
    expect(at(a, BREATHS * BREATH_MS)).toBe(PREVIEW_OPACITY);
    expect(a.active()).toBe(0);
    expect(a.animating()).toBe(false);
  });

  it("goes idle after the breath: the frame loop stops and nothing redraws at rest", () => {
    const a = full();
    startBreath(a, 0);
    const run = runFrameLoop(a, 0);
    // One frame per 16 ms for the 3.2 s breath, then no more.
    expect(run.endedAt).toBeGreaterThanOrEqual(BREATHS * BREATH_MS);
    expect(run.endedAt).toBeLessThan(BREATHS * BREATH_MS + 32);
    expect(run.frames).toBeLessThanOrEqual(Math.ceil((BREATHS * BREATH_MS) / 16) + 1);
    expect(run.maxPending).toBe(1);
    expect(a.active()).toBe(0);
    // Long after, sampling moves nothing, so a frame would draw nothing new.
    expect(a.sample(60_000)).toBe(false);
    expect(a.value(BREATH_KEY)).toBe(PREVIEW_OPACITY);
  });

  it("lands on the hold without a jump", () => {
    let prev = BREATH_LOW + (PREVIEW_OPACITY - BREATH_LOW) * breathCurve(0);
    let worst = 0;
    for (let i = 1; i <= 3200; i++) {
      const v = BREATH_LOW + (PREVIEW_OPACITY - BREATH_LOW) * breathCurve(i / 3200);
      worst = Math.max(worst, Math.abs(v - prev));
      prev = v;
    }
    expect(prev).toBeCloseTo(PREVIEW_OPACITY, 9);
    // At 1 ms steps the opacity never moves more than a fraction of a percent.
    expect(worst).toBeLessThan(0.002);
    expect(breathCurve(-1)).toBe(0);
    expect(breathCurve(2)).toBe(1);
  });

  it("holds the steady tint at once under reduced motion: no frames at all", () => {
    const a = reduced();
    startBreath(a, 0);
    expect(a.value(BREATH_KEY)).toBe(PREVIEW_OPACITY);
    expect(a.active()).toBe(0);
    expect(runFrameLoop(a, 0).frames).toBe(1);
  });

  it("breathes once per proposal: a hover or a selection on it never starts it over", () => {
    const a = full();
    const b = new PreviewBreath();
    b.sync(a, "w1,w2", 0);
    expect(a.active()).toBe(1);
    runFrameLoop(a, 0);
    expect(a.active()).toBe(0);
    // The same proposal syncs again (a hover, a selection, a rebuild).
    b.sync(a, "w1,w2", 4000);
    expect(a.active()).toBe(0);
    expect(a.value(BREATH_KEY)).toBe(PREVIEW_OPACITY);
    // A new proposal breathes again, and lands again.
    b.sync(a, "w3", 5000);
    expect(a.active()).toBe(1);
    expect(runFrameLoop(a, 5000).endedAt).toBeLessThan(5000 + BREATHS * BREATH_MS + 32);
    expect(a.active()).toBe(0);
  });

  it("holds when the preview clears, and the next one breathes from the start", () => {
    const a = full();
    const b = new PreviewBreath();
    b.sync(a, "w1", 0);
    a.sample(500);
    b.sync(a, null, 500); // Apply or Discard: the preview is gone
    expect(a.active()).toBe(0);
    expect(a.value(BREATH_KEY)).toBe(PREVIEW_OPACITY);
    b.sync(a, "w1", 900); // the same ids proposed again
    expect(a.value(BREATH_KEY)).toBe(BREATH_LOW);
    expect(a.active()).toBe(1);
    b.reset(a);
    expect(a.active()).toBe(0);
  });
});

describe("the 3D frame loop invariant", () => {
  // Every source file of the 3D view (src/viewer3d), as text.
  const sources = import.meta.glob(["../**/*.ts", "../**/*.tsx", "!../**/*.test.ts", "!../**/*.test.tsx"], {
    query: "?raw",
    import: "default",
    eager: true,
  }) as Record<string, string>;
  const code = (src: string) => src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, "");

  it("asks for frames in one place only: ViewerEngine.schedule()", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(50);
    const calls = Object.entries(sources).flatMap(([path, src]) => {
      const n = (code(src).match(/requestAnimationFrame\s*\(/g) ?? []).length;
      return n > 0 ? [{ path, n }] : [];
    });
    expect(calls).toEqual([{ path: "./ViewerEngine.ts", n: 1 }]);
    expect(code(sources["./ViewerEngine.ts"])).toMatch(
      /private schedule\(\): void \{\s*if \(this\.raf \|\| this\.disposed \|\| this\.paused\(\)\) return;\s*this\.raf = requestAnimationFrame\(this\.frame\);/,
    );
  });
});
