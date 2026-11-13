// The swarm draft in 3D (draft3d.ts, view3d.ts): its own group, rebuilt only
// where the draft changed, shown as the reveal reaches each element, at most
// 4 rebuilds a second.
import * as THREE from "three";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DocState } from "../contract/bindings";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import { DRAFT_MIN_MS, DRAFT_OPACITY, DraftGroup, rateLimit, type DraftOverlay } from "./draft3d";
import { mockStatus } from "./mock";
import { kickReveal, reveal, useSwarm } from "./swarmStore";
import type { SwarmDraftState } from "./types";
import { connectDraft3D, overlayOf } from "./view3d";

const doc = fixture as unknown as DocState;
const walls = doc.project.elements.filter((e) => e.kind === "wall");
const openings = doc.project.elements.filter((e) => e.kind === "opening");
const asset = doc.project.elements.find((e) => e.kind === "asset")!;

function overlay(ids: string[], state: DocState = doc, key = "1"): DraftOverlay {
  return { key, state, parts: [{ color: "#c92a2a", ids }], context: [], hide: [] };
}

function meshes(root: THREE.Object3D): THREE.Mesh[] {
  const out: THREE.Mesh[] = [];
  root.traverse((o) => (o as THREE.Mesh).isMesh && out.push(o as THREE.Mesh));
  return out;
}

afterEach(() => {
  reveal.reset();
  useSwarm.setState({ status: null, draft: null, showDraft: true });
  vi.useRealTimers();
});

describe("the draft group", () => {
  it("builds one hidden form per draft element, in the part's color at 55%, never picked", () => {
    const g = new DraftGroup();
    g.sync(overlay([walls[0].id, asset.id]));
    expect(g.ids().sort()).toEqual([walls[0].id, asset.id].sort());
    expect(g.shownIds()).toEqual([]);
    for (const m of meshes(g.root)) {
      const mat = m.material as THREE.MeshStandardMaterial;
      expect(mat.opacity).toBe(DRAFT_OPACITY);
      expect(`#${mat.color.getHexString()}`).toBe("#c92a2a");
      expect(m.castShadow).toBe(false);
      const hits: THREE.Intersection[] = [];
      m.raycast(new THREE.Raycaster(), hits);
      expect(hits).toHaveLength(0);
    }
    g.dispose();
  });

  it("shows what the reveal reached, handing back only the meshes that just came in", () => {
    const g = new DraftGroup();
    g.sync(overlay([walls[0].id, walls[1].id]));
    const first = g.show((id) => id === walls[0].id);
    expect(first.length).toBeGreaterThan(0);
    expect(g.shownIds()).toEqual([walls[0].id]);
    expect(g.show((id) => id === walls[0].id)).toEqual([]);
    const second = g.show(() => true);
    expect(second.length).toBeGreaterThan(0);
    expect(second.some((m) => first.includes(m))).toBe(false);
    g.dispose();
  });

  it("rebuilds only the elements whose form changed, and keeps what was shown", () => {
    const g = new DraftGroup();
    g.sync(overlay([walls[0].id, walls[1].id]));
    g.show(() => true);
    const before = new Map(g.root.children.map((c) => [c.userData.draftId as string, c]));
    // Same draft again: nothing rebuilt, and it says so (the view then stays asleep).
    expect(g.sync(overlay([walls[0].id, walls[1].id], doc, "2"))).toBe(false);
    for (const c of g.root.children) expect(c).toBe(before.get(c.userData.draftId as string));
    // One wall moved: only it is rebuilt, still shown.
    const moved = { ...doc, project: { ...doc.project, elements: doc.project.elements.map((e) => (e.id === walls[1].id && e.kind === "wall" ? { ...e, end: { x: e.end.x + 500, y: e.end.y } } : e)) } };
    expect(g.sync(overlay([walls[0].id, walls[1].id], moved, "3"))).toBe(true);
    const after = new Map(g.root.children.map((c) => [c.userData.draftId as string, c]));
    expect(after.get(walls[0].id)).toBe(before.get(walls[0].id));
    expect(after.get(walls[1].id)).not.toBe(before.get(walls[1].id));
    expect(g.shownIds().sort()).toEqual([walls[0].id, walls[1].id].sort());
    // Gone from the draft: gone from the group.
    expect(g.sync(overlay([walls[0].id], moved, "4"))).toBe(true);
    expect(g.ids()).toEqual([walls[0].id]);
    g.dispose();
    expect(g.root.children).toHaveLength(0);
  });

  it("cuts the draft's openings into a draft wall, so a draft door shows", () => {
    const host = openings[0].kind === "opening" ? openings[0].wall_id : "";
    const without = { ...doc, project: { ...doc.project, elements: doc.project.elements.filter((e) => e.kind !== "opening") } };
    const plain = new DraftGroup();
    plain.sync(overlay([host], without));
    const cut = new DraftGroup();
    cut.sync(overlay([host]));
    const count = (g: DraftGroup) => meshes(g.root).reduce((n, m) => n + (m.geometry.getAttribute("position")?.count ?? 0), 0);
    expect(count(cut)).toBeGreaterThan(count(plain));
    plain.dispose();
    cut.dispose();
  });
});

describe("handing the draft to a 3D view", () => {
  function draft(ids: string[], revision = 1): SwarmDraftState {
    return { swarm_id: "sw-1", draft_revision: revision, state: doc, parts: [{ part: "shell", color: 1, ids }], removed: [], steps: [], display_modified: [walls[2].id] };
  }

  it("hides the model's version of what the draft changes, and the walls whose look changes", () => {
    const o = overlayOf(draft([walls[0].id, "new-wall"]), doc);
    expect(o.hide.sort()).toEqual([walls[0].id, walls[2].id].sort());
    expect(o.context).toEqual([walls[2].id]);
    expect(o.key).toBe("sw-1:1");
  });

  it("sends the overlay when the draft or its toggle changes, what shows at each boundary, and takes it away at the end", () => {
    const overlays: Array<DraftOverlay | null> = [];
    let shownCalls = 0;
    let lastShown: ((id: string) => boolean) | null = null;
    const stop = connectDraft3D(
      {
        setDraftOverlay: (o) => overlays.push(o),
        setDraftShown: (fn) => {
          shownCalls++;
          lastShown = fn;
        },
      },
      () => doc,
    );
    expect(overlays).toEqual([null]);
    useSwarm.setState({ status: mockStatus(), draft: draft([walls[0].id]) });
    expect(overlays.length).toBe(2);
    expect(overlays[1]?.key).toBe("sw-1:1");
    // The reveal holds the wall back: the view hides it until the reveal reaches it.
    reveal.arrive("shell", [{ id: "x", part: "shell", kind: "line", from: { x: 0, y: 0 }, to: { x: 9000, y: 0 }, lengthMm: 9000, hosts: [], label: "", levelId: null, removed: false }], performance.now());
    reveal.arrive("shell", [{ id: walls[0].id, part: "shell", kind: "line", from: { x: 0, y: 0 }, to: { x: 4000, y: 0 }, lengthMm: 4000, hosts: [], label: "", levelId: null, removed: false }], performance.now());
    const calls = shownCalls;
    kickReveal();
    expect(shownCalls).toBe(calls + 1);
    expect(lastShown!(walls[0].id)).toBe(false);
    reveal.skip(performance.now());
    expect(lastShown!(walls[0].id)).toBe(true);
    useSwarm.setState({ showDraft: false });
    expect(overlays[overlays.length - 1]).toBeNull();
    stop();
    expect(overlays[overlays.length - 1]).toBeNull();
  });
});

describe("at most 4 rebuilds a second", () => {
  it("runs at once, then holds requests inside the window to one trailing run", () => {
    vi.useFakeTimers();
    let t = 0;
    let runs = 0;
    const limit = rateLimit(DRAFT_MIN_MS, () => runs++, () => t);
    limit.request();
    expect(runs).toBe(1);
    for (let i = 0; i < 10; i++) {
      t += 10;
      limit.request();
    }
    expect(runs).toBe(1);
    t = DRAFT_MIN_MS;
    vi.advanceTimersByTime(DRAFT_MIN_MS);
    expect(runs).toBe(2);
    // 2 s of requests every 16 ms: at most 4 a second.
    for (let i = 0; i < 125; i++) {
      t += 16;
      vi.advanceTimersByTime(16);
      limit.request();
    }
    expect(runs).toBeLessThanOrEqual(2 + 9);
    limit.cancel();
  });
});
