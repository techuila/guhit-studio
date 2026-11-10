// Agent cursors of swarm parts follow the reveal (cursors.ts).
import { afterEach, describe, expect, it } from "vitest";
import type { DocState } from "../contract/bindings";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import { cursorIdsWithSwarm, followSwarmCursor, partOfAgent, pointIn3d, swarmCursorAgents, transitionOf, WORK_HEIGHT_MM } from "./cursors";
import { mockStatus } from "./mock";
import { kickReveal, reveal, useSwarm } from "./swarmStore";
import type { CursorTarget, RevealItem } from "./types";

const doc = fixture as unknown as DocState;
const level = doc.project.levels[0];

function wall(id: string, x0: number, x1: number): RevealItem {
  return { id, part: "shell", kind: "line", from: { x: x0, y: 0 }, to: { x: x1, y: 0 }, lengthMm: Math.abs(x1 - x0), hosts: [], label: `Drawing ${id}`, levelId: level.id, removed: false };
}

afterEach(() => {
  reveal.reset();
  useSwarm.setState({ status: null, draft: null });
});

describe("swarm part cursors", () => {
  it("finds the part an agent draws for", () => {
    const st = mockStatus();
    expect(partOfAgent(st, "agent-shell")).toBe("shell");
    expect(partOfAgent(st, "someone")).toBeNull();
    expect(partOfAgent(null, "agent-shell")).toBeNull();
  });

  it("shows a part's cursor where its reveal is, in place of its presence", () => {
    const st = mockStatus();
    reveal.arrive("shell", [wall("w1", 0, 4000)], performance.now());
    expect(swarmCursorAgents(st, level.id, performance.now())).toEqual(["agent-shell"]);
    expect(swarmCursorAgents(st, "another-level", performance.now())).toEqual([]);
    // A bound agent's presence point does not show it; its reveal does.
    expect(cursorIdsWithSwarm(["agent-rooms", "outsider"], st, level.id, performance.now())).toEqual(["outsider", "agent-shell"]);
  });

  it("hands the cursor each segment: a jump for the first, a glide, then a linear draw, and lets go at the end", () => {
    useSwarm.setState({ status: mockStatus() });
    const seen: Array<CursorTarget | null> = [];
    const stop = followSwarmCursor("agent-shell", (t) => seen.push(t));
    expect(seen).toEqual([null]);
    const t0 = performance.now();
    reveal.setSpeed(1, t0);
    reveal.arrive("shell", [wall("w1", 0, 4000), wall("w2", 9000, 13000)], t0);
    kickReveal();
    // The first element: drawn in place, linear.
    expect(seen[1]).toMatchObject({ at: { x: 4000, y: 0 }, easing: "linear", label: "Drawing w1" });
    // A tick inside the same segment changes nothing.
    kickReveal();
    expect(seen).toHaveLength(2);
    stop();
    // After unsubscribing, ticks reach nobody.
    kickReveal();
    expect(seen).toHaveLength(2);
  });

  it("lets go when the swarm ends, so the agent's own presence takes over", () => {
    useSwarm.setState({ status: mockStatus() });
    reveal.arrive("shell", [wall("w1", 0, 4000)], performance.now());
    const seen: Array<CursorTarget | null> = [];
    const stop = followSwarmCursor("agent-shell", (t) => seen.push(t));
    expect(seen[0]).not.toBeNull();
    useSwarm.setState({ status: null });
    expect(seen[seen.length - 1]).toBeNull();
    stop();
  });

  it("times the CSS glide by the segment: eased for a glide, linear for a draw", () => {
    expect(transitionOf({ part: "p", at: { x: 0, y: 0 }, ms: 243.4, easing: "glide", label: "", levelId: null })).toEqual({ duration: "243ms", easing: "var(--ease-in-out)" });
    expect(transitionOf({ part: "p", at: { x: 0, y: 0 }, ms: 456, easing: "linear", label: "", levelId: null })).toEqual({ duration: "456ms", easing: "linear" });
  });

  it("puts the 3D marker at work height over the part's level", () => {
    expect(pointIn3d({ x: 1000.4, y: 2000 }, level.id, doc)).toEqual({ x: 1000, y: 2000, z: level.elevation_mm + WORK_HEIGHT_MM });
  });
});
