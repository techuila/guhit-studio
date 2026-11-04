import { describe, expect, it } from "vitest";
import type { LiveCamera, PlanView, Presence } from "../contract/bindings";
import { PresenceThrottle, SELECTION_MAX, buildPresence, samePresence, type PresenceInput } from "./presenceSync";

const base: PresenceInput = {
  selection: [],
  levelId: "L1",
  aiScope: false,
  cursor: null,
  typing: null,
  live: true,
  following: null,
  viewMode: "split",
  followed: false,
  planView: null,
  camera: null,
  pointer3d: null,
};

const area: PlanView = { center: { x: 4200.4, y: -1800.6 }, width_mm: 9000.2, height_mm: 6000.7 };
const cam: LiveCamera = { position: { x: 100.4, y: 200.6, z: 9000.5 }, target: { x: 4500, y: 3500, z: 1200 }, fov_deg: 45.04, nav: "orbit" };

const off = { following: null, view_mode: null, plan_view: null, camera: null, pointer_3d: null, status: null };

/** A throttle on a hand-driven clock and timer queue. */
function harness(intervalMs = 50) {
  let now = 0;
  const sent: Presence[] = [];
  const sentAt: number[] = [];
  const timers: Array<{ at: number; fn: () => void; id: number }> = [];
  let seq = 0;
  const t = new PresenceThrottle({
    intervalMs,
    send: (p) => {
      sent.push(p);
      sentAt.push(now);
    },
    now: () => now,
    setTimer: (fn, ms) => {
      const id = ++seq;
      timers.push({ at: now + ms, fn, id });
      return id;
    },
    clearTimer: (id) => {
      const i = timers.findIndex((x) => x.id === id);
      if (i >= 0) timers.splice(i, 1);
    },
  });
  const advance = (ms: number) => {
    const end = now + ms;
    for (;;) {
      timers.sort((a, b) => a.at - b.at);
      const next = timers[0];
      if (!next || next.at > end) break;
      timers.shift();
      now = next.at;
      next.fn();
    }
    now = end;
  };
  const p = (over: Partial<PresenceInput>) => buildPresence({ ...base, ...over });
  return { t, sent, sentAt, advance, p, pending: () => timers.length };
}

describe("buildPresence", () => {
  it("sends the pointer and typing only in a live session", () => {
    const input: PresenceInput = { ...base, cursor: { x: 1200.04, y: -35.26 }, typing: "hi" };
    expect(buildPresence(input)).toEqual({ cursor: { x: 1200, y: -35.3 }, level_id: "L1", selection: [], typing: "hi", ai_scope: false, ...off, view_mode: "split" });
    expect(buildPresence({ ...input, live: false })).toEqual({ cursor: null, level_id: "L1", selection: [], typing: null, ai_scope: false, ...off });
  });

  it("always carries selection, level and the AI scope", () => {
    const pr = buildPresence({ ...base, live: false, selection: ["a", "b"], aiScope: true, levelId: null });
    expect(pr.selection).toEqual(["a", "b"]);
    expect(pr.ai_scope).toBe(true);
    expect(pr.level_id).toBeNull();
  });

  it("cuts typing to 160 characters and the selection to the engine's limit", () => {
    const pr = buildPresence({ ...base, typing: "x".repeat(300), selection: Array.from({ length: SELECTION_MAX + 5 }, (_, i) => `e${i}`) });
    expect(pr.typing).toHaveLength(160);
    expect(pr.selection).toHaveLength(SELECTION_MAX);
  });

  it("keeps an open, empty cursor chat as an empty string", () => {
    expect(buildPresence({ ...base, typing: "" }).typing).toBe("");
  });

  it("drops a pointer that is not a number", () => {
    expect(buildPresence({ ...base, cursor: { x: Number.NaN, y: 3 } }).cursor).toBeNull();
  });
});

describe("buildPresence for follow mode", () => {
  const all: Partial<PresenceInput> = { following: "ana", planView: area, camera: cam, pointer3d: { x: 10.6, y: 20.2, z: 900.5 } };

  it("always sends who this window follows, its view mode and the 3D pointer while live", () => {
    const pr = buildPresence({ ...base, ...all });
    expect(pr.following).toBe("ana");
    expect(pr.view_mode).toBe("split");
    expect(pr.pointer_3d).toEqual({ x: 11, y: 20, z: 901 });
    expect(buildPresence({ ...base, ...all, live: false, followed: true })).toMatchObject(off);
  });

  it("sends the plan area and camera only while someone follows this window", () => {
    const alone = buildPresence({ ...base, ...all });
    expect(alone.plan_view).toBeNull();
    expect(alone.camera).toBeNull();
    const watched = buildPresence({ ...base, ...all, followed: true });
    expect(watched.plan_view).toEqual({ center: { x: 4200, y: -1801 }, width_mm: 9000, height_mm: 6001 });
    expect(watched.camera).toEqual({ position: { x: 100, y: 201, z: 9001 }, target: { x: 4500, y: 3500, z: 1200 }, fov_deg: 45, nav: "orbit" });
  });

  it("sends only the views on screen", () => {
    const plan = buildPresence({ ...base, ...all, followed: true, viewMode: "2d" });
    expect(plan.plan_view).not.toBeNull();
    expect(plan.camera).toBeNull();
    expect(plan.pointer_3d).toBeNull();
    const model = buildPresence({ ...base, ...all, followed: true, viewMode: "3d" });
    expect(model.plan_view).toBeNull();
    expect(model.camera).not.toBeNull();
  });

  it("drops an area or camera that is not finite or has no size, and keeps the field of view in 5 to 110", () => {
    const bad = buildPresence({ ...base, followed: true, planView: { ...area, width_mm: 0 }, camera: { ...cam, target: { x: Number.NaN, y: 0, z: 0 } } });
    expect(bad.plan_view).toBeNull();
    expect(bad.camera).toBeNull();
    expect(buildPresence({ ...base, followed: true, camera: { ...cam, fov_deg: 170 } }).camera?.fov_deg).toBe(110);
    expect(buildPresence({ ...base, followed: true, pointer3d: { x: Number.POSITIVE_INFINITY, y: 0, z: 0 } }).pointer_3d).toBeNull();
  });
});

describe("samePresence", () => {
  const a = buildPresence({ ...base, cursor: { x: 1, y: 2 }, selection: ["a"] });
  it("compares every field", () => {
    expect(samePresence(a, buildPresence({ ...base, cursor: { x: 1, y: 2 }, selection: ["a"] }))).toBe(true);
    expect(samePresence(a, buildPresence({ ...base, cursor: { x: 1, y: 2.5 }, selection: ["a"] }))).toBe(false);
    expect(samePresence(a, buildPresence({ ...base, cursor: null, selection: ["a"] }))).toBe(false);
    expect(samePresence(a, buildPresence({ ...base, cursor: { x: 1, y: 2 }, selection: ["b"] }))).toBe(false);
    expect(samePresence(a, buildPresence({ ...base, cursor: { x: 1, y: 2 }, selection: ["a"], typing: "" }))).toBe(false);
    expect(samePresence(a, buildPresence({ ...base, cursor: { x: 1, y: 2 }, selection: ["a"], aiScope: true }))).toBe(false);
    expect(samePresence(a, buildPresence({ ...base, cursor: { x: 1, y: 2 }, selection: ["a"], levelId: "L2" }))).toBe(false);
  });

  it("sees a change in any follow field, a camera-only move too", () => {
    const f = buildPresence({ ...base, followed: true, following: "ana", planView: area, camera: cam });
    const same = (over: Partial<PresenceInput>) => samePresence(f, buildPresence({ ...base, followed: true, following: "ana", planView: area, camera: cam, ...over }));
    expect(same({})).toBe(true);
    expect(same({ camera: { ...cam, position: { ...cam.position, x: 140 } } })).toBe(false);
    expect(same({ camera: { ...cam, nav: "walk" } })).toBe(false);
    expect(same({ camera: { ...cam, fov_deg: 60 } })).toBe(false);
    expect(same({ planView: { ...area, width_mm: 12000 } })).toBe(false);
    expect(same({ planView: null })).toBe(false);
    expect(same({ following: "ben" })).toBe(false);
    expect(same({ viewMode: "3d" })).toBe(false);
    expect(same({ pointer3d: { x: 1, y: 2, z: 3 } })).toBe(false);
    // Under a millimeter is the same view.
    expect(same({ camera: { ...cam, position: { ...cam.position, x: 100.3 } } })).toBe(true);
  });

  it("reads a field an older version leaves out as null", () => {
    const old = { cursor: null, level_id: "L1", selection: [], typing: null, ai_scope: false } as unknown as Presence;
    expect(samePresence(old, { ...old, ...off })).toBe(true);
  });
});

describe("PresenceThrottle", () => {
  it("sends the first change right away", () => {
    const h = harness();
    h.t.update(h.p({ cursor: { x: 1, y: 1 } }));
    expect(h.sent).toHaveLength(1);
    expect(h.pending()).toBe(0);
  });

  it("sends nothing when nothing changed", () => {
    const h = harness();
    h.t.update(h.p({ selection: ["a"] }));
    h.advance(200);
    h.t.update(h.p({ selection: ["a"] }));
    h.t.update(h.p({ selection: ["a"] }));
    h.advance(200);
    expect(h.sent).toHaveLength(1);
  });

  it("sends at most 20 a second, and the last state always arrives", () => {
    const h = harness(50);
    // A pointer moving every 5 ms for one second: 200 updates.
    for (let i = 0; i < 200; i++) {
      h.t.update(h.p({ cursor: { x: i, y: 0 } }));
      h.advance(5);
    }
    h.advance(100);
    expect(h.sent.length).toBeLessThanOrEqual(21);
    expect(h.sent.length).toBeGreaterThanOrEqual(19);
    expect(h.sent[h.sent.length - 1].cursor).toEqual({ x: 199, y: 0 });
  });

  it("keeps sends at least one interval apart", () => {
    const h = harness(50);
    for (let i = 0; i < 80; i++) {
      h.t.update(h.p({ cursor: { x: i, y: 0 } }));
      h.advance(7);
    }
    h.advance(100);
    expect(h.sentAt.length).toBeGreaterThan(5);
    for (let i = 1; i < h.sentAt.length; i++) expect(h.sentAt[i] - h.sentAt[i - 1]).toBeGreaterThanOrEqual(50);
  });

  it("drops a trailing send that came back to what was already sent", () => {
    const h = harness();
    h.t.update(h.p({ selection: ["a"] }));
    h.t.update(h.p({ selection: ["b"] }));
    h.t.update(h.p({ selection: ["a"] }));
    h.advance(60);
    expect(h.sent).toHaveLength(1);
  });

  it("re-sends an equal state after a reset", () => {
    const h = harness();
    h.t.update(h.p({ selection: ["a"] }));
    h.advance(60);
    h.t.reset();
    h.t.update(h.p({ selection: ["a"] }));
    expect(h.sent).toHaveLength(2);
  });

  it("sends a camera-only change", () => {
    const h = harness();
    h.t.update(h.p({ followed: true, camera: cam }));
    h.advance(60);
    h.t.update(h.p({ followed: true, camera: { ...cam, position: { x: 900, y: 200, z: 9000 } } }));
    expect(h.sent).toHaveLength(2);
    expect(h.sent[1].camera?.position.x).toBe(900);
  });

  it("clears its timer on dispose", () => {
    const h = harness();
    h.t.update(h.p({ selection: ["a"] }));
    h.t.update(h.p({ selection: ["b"] }));
    expect(h.pending()).toBe(1);
    h.t.dispose();
    expect(h.pending()).toBe(0);
    h.advance(100);
    expect(h.sent).toHaveLength(1);
  });
});
