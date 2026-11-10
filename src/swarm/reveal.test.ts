// The paced reveal (reveal.ts) and what it paces (items.ts).
import { describe, expect, it } from "vitest";
import type { DocState } from "../contract/bindings";
import { buildIndex } from "../editor2d/model";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import { stepItems } from "./items";
import { BEHIND_MS, CATCH_UP, DRAW_MAX_MS, DRAW_MIN_MS, GAP_MS, GLIDE_BASE_MS, GLIDE_MAX_MS, Reveal, drawMs, glideMs, hostsFirst } from "./reveal";
import { revealedIds } from "./sample";
import type { RevealItem, SwarmStep } from "./types";

const doc = fixture as unknown as DocState;

function line(id: string, part: string, x0: number, x1: number, y = 0, hosts: string[] = []): RevealItem {
  return { id, part, kind: "line", from: { x: x0, y }, to: { x: x1, y }, lengthMm: Math.abs(x1 - x0), hosts, label: `Drawing ${id}`, levelId: "L1", removed: false };
}

function spot(id: string, part: string, x: number, y: number, hosts: string[] = [], kind: RevealItem["kind"] = "settle"): RevealItem {
  return { id, part, kind, from: { x, y }, to: { x, y }, lengthMm: 0, hosts, label: `Placing ${id}`, levelId: "L1", removed: false };
}

/** The first moment an element shows (progress above 0), searched in 1 ms steps. */
function startOf(r: Reveal, id: string, from = 0, to = 60_000): number {
  for (let t = from; t <= to; t++) if (r.progress(id, t) > 0) return t;
  return Infinity;
}

describe("reveal timings", () => {
  it("glides 160 ms plus 0.083 ms per mm, 160 to 700 ms, divided by the speed", () => {
    expect(glideMs(0, 1)).toBe(GLIDE_BASE_MS);
    expect(glideMs(1000, 1)).toBeCloseTo(160 + 83, 6);
    expect(glideMs(100_000, 1)).toBe(GLIDE_MAX_MS);
    expect(glideMs(1000, 2)).toBeCloseTo((160 + 83) / 2, 6);
  });

  it("draws 0.114 ms per mm, 180 to 900 ms, divided by the speed", () => {
    expect(drawMs(100, 1)).toBe(DRAW_MIN_MS);
    expect(drawMs(4000, 1)).toBeCloseTo(456, 6);
    expect(drawMs(20_000, 1)).toBe(DRAW_MAX_MS);
    expect(drawMs(4000, 0.5)).toBeCloseTo(912, 6);
  });
});

describe("reveal order", () => {
  it("draws a part's elements one after another: the first in place, then glide, draw and a gap", () => {
    const r = new Reveal();
    r.setSpeed(1, 0);
    r.arrive("shell", [line("w1", "shell", 0, 4000), line("w2", "shell", 4000, 8000)], 0);
    // The first element: no glide (the cursor appears on it), a 456 ms draw.
    expect(r.progress("w1", 0)).toBe(0);
    expect(r.progress("w1", 228)).toBeCloseTo(0.5, 6);
    expect(r.progress("w1", 456)).toBe(1);
    // Then the gap and a glide of 0 mm (w2 starts where w1 ended).
    const w2 = 456 + GAP_MS + GLIDE_BASE_MS;
    expect(r.progress("w2", w2 - 1)).toBe(0);
    expect(r.progress("w2", w2 + 228)).toBeCloseTo(0.5, 6);
    expect(r.busy(w2 + 10)).toBe(true);
    expect(r.busy(w2 - 50)).toBe(false);
  });

  it("aims the cursor along the work: a glide eases, a draw is linear, a rest stays", () => {
    const r = new Reveal();
    r.setSpeed(1, 0);
    r.arrive("rooms", [spot("a", "rooms", 0, 0), spot("b", "rooms", 6000, 0)], 0);
    const glideStart = DRAW_MIN_MS + GAP_MS;
    const glide = r.target("rooms", glideStart + 10);
    expect(glide?.easing).toBe("glide");
    expect(glide?.at).toEqual({ x: 6000, y: 0 });
    expect(glide?.ms).toBeCloseTo(glideMs(6000, 1) - 10, 6);
    expect(glide?.label).toBe("Placing b");
    const drawing = r.target("rooms", glideStart + glideMs(6000, 1) + 5);
    expect(drawing?.easing).toBe("linear");
    const rest = r.target("rooms", 60_000);
    expect(rest).toMatchObject({ at: { x: 6000, y: 0 }, ms: 0 });
  });

  it("keeps parts on their own cursors, drawing at the same time", () => {
    const r = new Reveal();
    r.setSpeed(1, 0);
    r.arrive("shell", [line("w1", "shell", 0, 4000)], 0);
    r.arrive("roof", [spot("r1", "roof", 9000, 9000)], 0);
    expect(r.progress("w1", 100)).toBeGreaterThan(0);
    expect(r.progress("r1", 100)).toBeGreaterThan(0);
    expect(r.parts()).toEqual(["shell", "roof"]);
  });

  it("counts a step as revealed once all its elements are drawn", () => {
    const r = new Reveal();
    r.setSpeed(1, 0);
    r.known("shell", 2, ["old"]);
    r.arrive("shell", [line("w1", "shell", 0, 4000)], 0);
    expect(r.revealedSteps("shell", 0)).toBe(2);
    expect(r.revealedSteps("shell", 456)).toBe(3);
    // What was already in the draft shows as it is.
    expect(r.progress("old", 0)).toBe(1);
  });
});

describe("reveal honesty", () => {
  it("never draws an opening before its wall, even from another part", () => {
    const r = new Reveal();
    r.setSpeed(1, 0);
    // Shell is busy with a long queue; Rooms' door waits for the last wall.
    r.arrive("shell", [line("w1", "shell", 0, 8000), line("w2", "shell", 8000, 16000), line("w3", "shell", 16000, 24000)], 0);
    r.arrive("rooms", [spot("d1", "rooms", 20000, 0, ["w3"], "swing")], 0);
    const wallDone = startOf(r, "w3") + 0;
    let wallEnd = wallDone;
    while (r.progress("w3", wallEnd) < 1) wallEnd++;
    expect(startOf(r, "d1")).toBeGreaterThan(wallEnd);
    // While it waits, its cursor stays at the door.
    const waiting = r.segment("rooms", wallEnd - 5);
    expect(waiting?.kind).toBe("wait");
    expect(waiting?.to).toEqual({ x: 20000, y: 0 });
  });

  it("fills a room only after every bounding wall is drawn", () => {
    const r = new Reveal();
    r.setSpeed(1, 0);
    r.arrive("shell", [line("a", "shell", 0, 4000), line("b", "shell", 4000, 8000, 0)], 0);
    r.arrive("rooms", [spot("room", "rooms", 2000, 2000, ["a", "b"], "fill")], 0);
    let bEnd = 0;
    while (r.progress("b", bEnd) < 1) bEnd++;
    expect(startOf(r, "room")).toBeGreaterThan(bEnd);
  });

  it("puts a step's wall before the door it lists first", () => {
    const door = spot("d", "shell", 1000, 0, ["w"], "swing");
    const wall = line("w", "shell", 0, 4000);
    expect(hostsFirst([door, wall]).map((i) => i.id)).toEqual(["w", "d"]);
    const r = new Reveal();
    r.arrive("shell", [door, wall], 0);
    expect(startOf(r, "d")).toBeGreaterThan(startOf(r, "w"));
  });

  it("keeps an element shown when a later step changes it again", () => {
    const r = new Reveal();
    r.setSpeed(1, 0);
    r.arrive("shell", [line("w1", "shell", 0, 4000)], 0);
    r.arrive("shell", [line("w1", "shell", 0, 5000)], 1000);
    expect(r.progress("w1", 1001)).toBe(1);
  });
});

describe("reveal speed, catching up, skipping, reduced motion", () => {
  it("halves every time at speed 2", () => {
    const slow = new Reveal();
    slow.setSpeed(1, 0);
    slow.arrive("shell", [line("w1", "shell", 0, 4000), line("w2", "shell", 9000, 13000)], 0);
    const fast = new Reveal();
    fast.setSpeed(2, 0);
    fast.arrive("shell", [line("w1", "shell", 0, 4000), line("w2", "shell", 9000, 13000)], 0);
    expect(startOf(fast, "w2")).toBeCloseTo(startOf(slow, "w2") / 2, -1);
  });

  it("reschedules what waits when the speed changes, and leaves what draws alone", () => {
    const r = new Reveal();
    r.setSpeed(1, 0);
    r.arrive("shell", [line("w1", "shell", 0, 4000), line("w2", "shell", 9000, 13000)], 0);
    const before = startOf(r, "w2");
    r.setSpeed(3, 100);
    // w1 was drawing at 100 ms: its draw still ends at 456.
    expect(r.progress("w1", 455)).toBeLessThan(1);
    expect(r.progress("w1", 456)).toBe(1);
    // w2 now starts sooner.
    expect(startOf(r, "w2")).toBeLessThan(before);
    expect(startOf(r, "w2")).toBeGreaterThanOrEqual(456);
  });

  it("catches up at 3x when a part is more than 8 s behind", () => {
    const r = new Reveal();
    r.setSpeed(1, 0);
    // 20 long walls: each 900 ms draw plus glide and gap, far more than 8 s of work.
    const walls = Array.from({ length: 20 }, (_, i) => line(`w${i}`, "shell", i * 10_000, i * 10_000 + 9000, 0));
    r.arrive("shell", walls, 0);
    expect(r.behindMs(0)).toBeGreaterThan(BEHIND_MS);
    r.arrive("shell", [line("late", "shell", 300_000, 309_000)], 0);
    const start = startOf(r, "late", 0, 100_000);
    let end = start;
    while (r.progress("late", end) < 1) end++;
    expect(end - start).toBeCloseTo(DRAW_MAX_MS / CATCH_UP, -1);
  });

  it("skips to the end: everything shows and nothing waits", () => {
    const r = new Reveal();
    r.arrive("shell", [line("w1", "shell", 0, 4000), line("w2", "shell", 9000, 13000)], 0);
    r.arrive("rooms", [spot("d1", "rooms", 10000, 0, ["w2"], "swing")], 0);
    r.skip(50);
    for (const id of ["w1", "w2", "d1"]) expect(r.progress(id, 50)).toBe(1);
    expect(r.busy(50)).toBe(false);
    expect(r.pending(50)).toBe(false);
    expect(r.nextBoundary(50)).toBeNull();
  });

  it("shows elements as they arrive under reduced motion", () => {
    const r = new Reveal(() => true);
    r.arrive("shell", [line("w1", "shell", 0, 4000), line("w2", "shell", 9000, 13000)], 500);
    expect(r.progress("w1", 500)).toBe(1);
    expect(r.progress("w2", 500)).toBe(1);
    expect(r.busy(500)).toBe(false);
    expect(r.target("shell", 500)).toMatchObject({ ms: 0, at: { x: 13000, y: 0 } });
  });

  it("stays asleep at rest: no boundary left once every cursor rests", () => {
    const r = new Reveal();
    r.arrive("shell", [line("w1", "shell", 0, 4000)], 0);
    expect(r.nextBoundary(0)).not.toBeNull();
    expect(r.nextBoundary(10_000)).toBeNull();
    expect(r.busy(10_000)).toBe(false);
  });
});

describe("one picture for the plan and the 3D view", () => {
  it("samples the same moment the same way, whatever was sampled before", () => {
    const build = () => {
      const r = new Reveal();
      r.setSpeed(1.25, 0);
      r.arrive("shell", [line("w1", "shell", 0, 4000), line("w2", "shell", 4000, 9000), line("w3", "shell", 9000, 9000, 6000)], 0);
      r.arrive("rooms", [spot("d1", "rooms", 2000, 0, ["w1"], "swing"), spot("rm", "rooms", 2000, 2000, ["w1", "w2"], "fill")], 300);
      return r;
    };
    const ids = ["w1", "w2", "w3", "d1", "rm"];
    const times = Array.from({ length: 80 }, (_, i) => i * 53);
    const a = build();
    const b = build();
    // a samples forward, b backward: the reveal keeps no sampling state.
    const forward = times.map((t) => ids.map((id) => a.progress(id, t)));
    const backward = [...times].reverse().map((t) => ids.map((id) => b.progress(id, t))).reverse();
    expect(backward).toEqual(forward);
    // The 3D view's shown set is exactly what the plan draws (progress above 0).
    for (const t of times) {
      const plan = ids.filter((id) => a.progress(id, t) > 0);
      expect([...revealedIds(a, ids, t)].sort()).toEqual([...plan].sort());
    }
  });
});

describe("step items from the draft", () => {
  const index = buildIndex(doc, null);
  const walls = doc.project.elements.filter((e) => e.kind === "wall");
  const openings = doc.project.elements.filter((e) => e.kind === "opening");
  const rooms = doc.project.elements.filter((e) => e.kind === "room");

  it("draws walls start to end, swings doors on their wall, fills rooms after their walls", () => {
    const step: SwarmStep = {
      seq: 1,
      part: "shell",
      agent_id: "a1",
      label: "Add walls",
      status: "Laying out the outer walls",
      added: [walls[0].id, ...openings.map((o) => o.id), rooms[0].id],
      modified: [],
      removed: [],
      at: "",
    };
    const items = stepItems(step, index, index);
    const wall = items.find((i) => i.id === walls[0].id);
    expect(wall?.kind).toBe("line");
    if (walls[0].kind === "wall") {
      expect(wall?.from).toEqual(walls[0].start);
      expect(wall?.to).toEqual(walls[0].end);
    }
    expect(wall?.label).toBe("Laying out the outer walls");
    for (const o of openings) {
      const it = items.find((i) => i.id === o.id);
      if (o.kind !== "opening") continue;
      expect(it?.hosts).toEqual([o.wall_id]);
      expect(it?.kind).toBe(o.opening_type === "door" && o.style !== "sliding" ? "swing" : "line");
    }
    const room = items.find((i) => i.id === rooms[0].id);
    const geo = doc.derived.rooms.find((g) => g.room_id === rooms[0].id);
    expect(room?.kind).toBe("fill");
    expect(room?.hosts).toEqual(geo?.wall_ids);
  });

  it("labels the cursor with the step's status, else the part's own line, else the undo label", () => {
    const step: SwarmStep = { seq: 3, part: "shell", agent_id: "a1", label: "MCP: add walls", status: null, added: [walls[0].id], modified: [], removed: [], at: "" };
    expect(stepItems(step, index, index)[0].label).toBe("MCP: add walls");
    expect(stepItems(step, index, index, "Laying out the outer walls")[0].label).toBe("Laying out the outer walls");
    expect(stepItems({ ...step, status: "Closing the corner" }, index, index, "Laying out the outer walls")[0].label).toBe("Closing the corner");
  });

  it("takes removed elements from main, as ghosts that settle in place", () => {
    const step: SwarmStep = { seq: 2, part: "shell", agent_id: "a1", label: "Remove a wall", status: null, added: [], modified: [], removed: [walls[1].id], at: "" };
    const items = stepItems(step, buildIndex({ ...doc, project: { ...doc.project, elements: [] } }, null), index);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ id: walls[1].id, removed: true, kind: "settle", label: "Remove a wall" });
  });
});
