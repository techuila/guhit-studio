import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import type { Command, DocState } from "../contract/bindings";
import { ipc } from "../contract/ipc";
import { useApp } from "../state/store";
import { commitLatest, editLayer, editSettings, rebase, updateElement, updateElements } from "./commitLatest";
import { heightEdits, sharedNumber, thicknessEdits, unpinEdits, wallHeight, type WallEl } from "./multiWalls";

const base = () => structuredClone(fixture) as unknown as DocState;

/** A slow engine: applies each command after a delay, in arrival order, and counts them. */
function fakeEngine(start: DocState, delayMs = 15) {
  let doc = start;
  const sent: Command[] = [];
  const apply = (c: Command) => {
    const p = doc.project;
    switch (c.type) {
      case "set_project_settings":
        p.settings = c.settings;
        break;
      case "set_layer":
        p.layers = p.layers.map((l) => (l.key === c.layer.key ? c.layer : l));
        break;
      case "update_element":
        p.elements = p.elements.map((e) => (e.id === c.element.id ? c.element : e));
        break;
      case "batch":
        c.commands.forEach(apply);
        break;
      default:
        break;
    }
  };
  vi.spyOn(ipc, "docApply").mockImplementation(async (command: Command) => {
    sent.push(command);
    await new Promise((r) => setTimeout(r, delayMs));
    doc = structuredClone(doc);
    apply(command);
    doc.revision += 1;
    return { state: doc, diff: { added: [], removed: [], modified: [] } } as never;
  });
  return { sent, now: () => doc };
}

beforeEach(() => {
  useApp.setState({ doc: base(), saving: 0 });
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe("rebase", () => {
  it("lays only what changed over the latest object", () => {
    const drawn = { a: 1, b: "x", c: { d: 1, e: 2 } };
    const latest = { a: 5, b: "y", c: { d: 7, e: 2 } };
    expect(rebase(latest, drawn, { ...drawn, b: "z" })).toEqual({ a: 5, b: "z", c: { d: 7, e: 2 } });
    // Nested objects merge key by key: the site, a light, a point.
    expect(rebase(latest, drawn, { ...drawn, c: { d: 1, e: 9 } })).toEqual({ a: 5, b: "y", c: { d: 7, e: 9 } });
    // Arrays and null replace whole.
    expect(rebase({ p: [1, 2], q: 1 }, { p: [1], q: 1 }, { p: [3], q: 1 })).toEqual({ p: [3], q: 1 });
    expect(rebase({ h: 2700 as number | null }, { h: 2700 as number | null }, { h: null })).toEqual({ h: null });
  });
});

describe("fast inspector edits", () => {
  it("keeps Client, Location and Designer typed with Tab between, without a pause", async () => {
    const engine = fakeEngine(base());
    // All three fields were drawn from the same render, as when tabbing quickly.
    const st = useApp.getState().doc!.project.settings;
    const a = editSettings(st, { ...st, client_name: "Reyes family" });
    const b = editSettings(st, { ...st, location: "Antipolo, Rizal" });
    const c = editSettings(st, { ...st, designer: "Studio Guhit" });
    await Promise.all([a, b, c]);
    expect(engine.sent).toHaveLength(3);
    const saved = engine.now().project.settings;
    expect(saved.client_name).toBe("Reyes family");
    expect(saved.location).toBe("Antipolo, Rizal");
    expect(saved.designer).toBe("Studio Guhit");
    expect(useApp.getState().doc!.project.settings).toEqual(saved);
  });

  it("sends commits one after another, each built from the reply before it", async () => {
    const engine = fakeEngine(base());
    const seen: number[] = [];
    const one = commitLatest((d) => (seen.push(d.revision), { type: "set_project_settings", settings: d.project.settings }));
    const two = commitLatest((d) => (seen.push(d.revision), { type: "set_project_settings", settings: d.project.settings }));
    await Promise.all([one, two]);
    expect(seen[1]).toBe(seen[0] + 1);
    expect(engine.sent).toHaveLength(2);
  });

  it("keeps going after a refused commit", async () => {
    fakeEngine(base());
    vi.mocked(ipc.docApply).mockRejectedValueOnce({ code: "invalid", message: "No", element_ids: [] });
    useApp.setState({ reportError: (e) => e as never });
    const st = useApp.getState().doc!.project.settings;
    const a = editSettings(st, { ...st, client_name: "A" });
    const b = editSettings(st, { ...st, designer: "B" });
    expect(await a).toBeNull();
    await b;
    expect(useApp.getState().doc!.project.settings.designer).toBe("B");
  });

  it("keeps both of two quick edits to one element", async () => {
    const doc = useApp.getState().doc!;
    const wall = doc.project.elements.find((e): e is WallEl => e.kind === "wall")!;
    const engine = fakeEngine(base());
    await Promise.all([updateElement(wall, { ...wall, thickness_mm: 150 }), updateElement(wall, { ...wall, height_mm: 2400 })]);
    const now = engine.now().project.elements.find((e) => e.id === wall.id) as WallEl;
    expect(now.thickness_mm).toBe(150);
    expect(now.height_mm).toBe(2400);
  });

  it("sends nothing for an edit that changes nothing", async () => {
    const engine = fakeEngine(base());
    const st = useApp.getState().doc!.project.settings;
    expect(await editSettings(st, { ...st })).toBeNull();
    expect(engine.sent).toHaveLength(0);
  });

  it("toggles a layer from its latest state", async () => {
    const engine = fakeEngine(base());
    const layer = useApp.getState().doc!.project.layers.find((l) => l.key === "walls")!;
    await Promise.all([editLayer(layer, { ...layer, locked: !layer.locked }), editLayer(layer, { ...layer, visible: !layer.visible })]);
    const now = engine.now().project.layers.find((l) => l.key === "walls")!;
    expect(now.locked).toBe(!layer.locked);
    expect(now.visible).toBe(!layer.visible);
  });
});

describe("several walls at once", () => {
  const wallsOf = (doc: DocState) => doc.project.elements.filter((e): e is WallEl => e.kind === "wall");

  it("shows a shared value or Mixed", () => {
    expect(sharedNumber([100, 100, 100])).toEqual({ value: 100, mixed: false });
    expect(sharedNumber([100, 150])).toEqual({ value: 100, mixed: true });
  });

  it("sets the thickness of every wall in one batch, one undo step", async () => {
    const walls = wallsOf(useApp.getState().doc!).slice(0, 5);
    const engine = fakeEngine(base());
    await updateElements("Set thickness of 5 walls", thicknessEdits(walls, 125));
    expect(engine.sent).toHaveLength(1);
    const sent = engine.sent[0];
    expect(sent.type).toBe("batch");
    if (sent.type !== "batch") return;
    expect(sent.label).toBe("Set thickness of 5 walls");
    expect(sent.commands.every((c) => c.type === "update_element")).toBe(true);
    for (const w of wallsOf(engine.now()).filter((w) => walls.some((x) => x.id === w.id))) expect(w.thickness_mm).toBe(125);
  });

  it("sets a height, then follows the level, then pins it again", async () => {
    const doc = useApp.getState().doc!;
    const walls = wallsOf(doc).slice(0, 3);
    const engine = fakeEngine(base());
    await updateElements("h", heightEdits(walls, 2600));
    expect(wallsOf(engine.now()).slice(0, 3).map((w) => w.height_mm)).toEqual([2600, 2600, 2600]);
    const now = wallsOf(useApp.getState().doc!).slice(0, 3);
    await updateElements("h", heightEdits(now, null));
    const following = wallsOf(useApp.getState().doc!).slice(0, 3);
    expect(following.map((w) => w.height_mm)).toEqual([null, null, null]);
    const pinned = unpinEdits(useApp.getState().doc!, following).map((e) => (e.next as WallEl).height_mm);
    expect(pinned).toEqual(following.map((w) => wallHeight(useApp.getState().doc!, w)));
  });

  it("skips walls that already have the value", () => {
    const walls = wallsOf(base()).slice(0, 2);
    const edits = thicknessEdits([{ ...walls[0], thickness_mm: 125 }, { ...walls[1], thickness_mm: 100 }], 125);
    expect(edits.map((e) => e.base.id)).toEqual([walls[1].id]);
  });
});
