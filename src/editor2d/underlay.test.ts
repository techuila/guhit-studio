import { afterEach, describe, expect, it, vi } from "vitest";
import type { DocState } from "../contract/bindings";
import { useApp } from "../state/store";
import { UNDERLAY_KEY } from "./slab";
import { useUnderlay } from "./underlay";

const open = (id: string | null) => useApp.setState({ doc: id ? ({ project: { id } } as unknown as DocState) : null });

describe("the underlay switch follows the open project", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    open(null);
  });

  it("saves to the project on screen, never to the one before", () => {
    const m = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => m.get(k) ?? null,
      setItem: (k: string, v: string) => void m.set(k, v),
      removeItem: (k: string) => void m.delete(k),
    });
    open("p1");
    expect(useUnderlay.getState().on).toBe(true);
    useUnderlay.getState().set(false);
    expect(m.get(`${UNDERLAY_KEY}p1`)).toBe("0");

    // Another project opens: its own switch, on by default.
    open("p2");
    expect(useUnderlay.getState().projectId).toBe("p2");
    expect(useUnderlay.getState().on).toBe(true);

    // A stale project id in the switch (a caller that synced the wrong
    // one) cannot make it save there: the open project wins.
    useUnderlay.setState({ projectId: "p1", on: false });
    useUnderlay.getState().toggle();
    expect(m.get(`${UNDERLAY_KEY}p2`)).toBe("0");
    expect(m.get(`${UNDERLAY_KEY}p1`)).toBe("0");
    useUnderlay.getState().set(true);
    expect(m.has(`${UNDERLAY_KEY}p2`)).toBe(false);
    expect(m.get(`${UNDERLAY_KEY}p1`)).toBe("0");

    // Back to the first project: it remembers off.
    open("p1");
    expect(useUnderlay.getState().on).toBe(false);
  });
});

describe("slab tool defaults", () => {
  it("draws a floor until another kind is picked", () => {
    // A void on the lowest level would open onto the ground; the engine
    // refuses it, and the tool never starts there.
    expect(useApp.getState().toolOptions.slabKind).toBe("floor");
  });
});
