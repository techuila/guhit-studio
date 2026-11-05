import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import type { Command, DocState, Element } from "../contract/bindings";
import { ipc } from "../contract/ipc";
import { useApp } from "../state/store";
import { pt } from "../editor2d/geom";
import { commitLatest, editLayer, editSettings, editSheet, editSignatory, editTitleBlock, rebase, updateElement, updateElements } from "./commitLatest";
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
      case "set_title_block":
        // The engine keeps the stored signatory (sheets.rs).
        p.title_block = { ...c.title_block, signatory: p.title_block.signatory };
        break;
      case "set_signatory":
        p.title_block = { ...p.title_block, signatory: c.signatory };
        break;
      case "upsert_sheet": {
        // New viewports get ids, as the engine gives them.
        const sheet = { ...c.sheet, viewports: c.sheet.viewports.map((v, i) => (v.id ? v : { ...v, id: `vp-new-${doc.revision}-${i}` })) };
        const rest = p.sheet_set.sheets.filter((sh) => sh.id !== sheet.id);
        const at = c.index ?? p.sheet_set.sheets.findIndex((sh) => sh.id === sheet.id);
        rest.splice(at < 0 ? rest.length : at, 0, sheet);
        p.sheet_set = { ...p.sheet_set, sheets: rest };
        break;
      }
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
    // An array that changed length, and null, replace whole.
    expect(rebase({ p: [1, 2], q: 1 }, { p: [1], q: 1 }, { p: [3], q: 1 })).toEqual({ p: [3], q: 1 });
    expect(rebase({ h: 2700 as number | null }, { h: 2700 as number | null }, { h: null })).toEqual({ h: null });
  });

  it("merges arrays item by item: by id when the items have one, else by place", () => {
    const v = (id: string, title: string | null, x = 0) => ({ id, title, x });
    // Two viewports, each edited by a different quick edit.
    const drawn = [v("a", null), v("b", null)];
    const latest = [v("a", "PLAN"), v("b", null)];
    expect(rebase(latest, drawn, [v("a", null), v("b", null, 50)])).toEqual([v("a", "PLAN"), v("b", null, 50)]);
    // Added since, added by the edit (after the item it follows there), removed by the edit.
    expect(rebase([v("a", "PLAN"), v("b", null), v("c", null)], drawn, [v("b", null), v("", "NEW")])).toEqual([v("b", null), v("", "NEW"), v("c", null)]);
    // A tour shot put in after the first one stays there, next to an edit of the first.
    const shots = [v("s1", "Fly in"), v("s2", "Orbit"), v("s3", "Walk")];
    expect(rebase([v("s1", "Fly in", 5), v("s2", "Orbit"), v("s3", "Walk")], shots, [v("s1", "Fly in"), v("", "Shot 4"), v("s2", "Orbit"), v("s3", "Walk")])).toEqual([
      v("s1", "Fly in", 5),
      v("", "Shot 4"),
      v("s2", "Orbit"),
      v("s3", "Walk"),
    ]);
    expect(rebase(shots, shots, [v("", "First"), ...shots])[0]).toEqual(v("", "First"));
    // An item deleted since stays deleted.
    expect(rebase([v("b", null)], drawn, [v("a", "X"), v("b", null)])).toEqual([v("b", null)]);
    // An edit that reorders wins whole.
    expect(rebase(latest, drawn, [v("b", null), v("a", null)])).toEqual([v("b", null), v("a", null)]);
    // Without ids and the same length: by place (a lot's sides).
    const side = (role: string, setback_mm: number) => ({ role, setback_mm });
    const sides = [side("side", 0), side("side", 0), side("side", 0)];
    expect(rebase([side("road", 0), side("side", 0), side("side", 0)], sides, [side("side", 0), side("side", 0), side("rear", 0)])).toEqual([
      side("road", 0),
      side("side", 0),
      side("rear", 0),
    ]);
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

describe("permit set edits with a slow engine (GR3 P1-2)", () => {
  // A busy engine, or a live guest over the relay: each doc_apply answers late.
  const SLOW_MS = 300;

  it("keeps the PRC and the PTR typed with Tab between", async () => {
    const engine = fakeEngine(base(), SLOW_MS);
    // Both fields were drawn from the same render.
    const sig = useApp.getState().doc!.project.title_block.signatory;
    await Promise.all([editSignatory(sig, { ...sig, prc_no: "0123456" }), editSignatory(sig, { ...sig, ptr_no: "7654321" })]);
    expect(engine.sent).toHaveLength(2);
    expect(engine.now().project.title_block.signatory).toMatchObject({ prc_no: "0123456", ptr_no: "7654321" });
    expect(useApp.getState().doc!.project.title_block.signatory).toMatchObject({ prc_no: "0123456", ptr_no: "7654321" });
  });

  it("keeps the lot, block and TCT numbers typed one after another", async () => {
    const engine = fakeEngine(base(), SLOW_MS);
    const tb = useApp.getState().doc!.project.title_block;
    await Promise.all([editTitleBlock(tb, { ...tb, lot_no: "12" }), editTitleBlock(tb, { ...tb, block_no: "3" }), editTitleBlock(tb, { ...tb, tct_no: "T-0042" })]);
    expect(engine.now().project.title_block).toMatchObject({ lot_no: "12", block_no: "3", tct_no: "T-0042" });
  });

  it("keeps quick edits to two viewports of a sheet, its title, a move and a new view", async () => {
    const d = base();
    const vp = (id: string) => ({ id, view: { type: "notes" as const, text: "" }, scale_denominator: null, position: null, title: null });
    const sheet = { id: "s1", number: "A-1", title: "Plans", paper: "a3" as const, orientation: "landscape" as const, viewports: [vp("v1"), vp("v2")], revision_ids: [] };
    d.project.sheet_set = { sheets: [sheet, { ...sheet, id: "s2", number: "A-2", viewports: [] }], revisions: [] };
    useApp.setState({ doc: structuredClone(d) });
    const engine = fakeEngine(d, SLOW_MS);
    const drawn = useApp.getState().doc!.project.sheet_set.sheets[0];
    const setVp = (id: string, patch: object) => ({ ...drawn, viewports: drawn.viewports.map((v) => (v.id === id ? { ...v, ...patch } : v)) });
    await Promise.all([
      editSheet(drawn, setVp("v1", { title: "GROUND FLOOR PLAN" })),
      editSheet(drawn, setVp("v2", { scale_denominator: 100 })),
      editSheet(drawn, { ...drawn, title: "Floor plans" }),
      editSheet(drawn, setVp("v1", { position: { x: 200, y: 150 } })),
      editSheet(drawn, { ...drawn, viewports: [...drawn.viewports, vp("")] }),
    ]);
    const now = engine.now().project.sheet_set.sheets[0];
    expect(now.title).toBe("Floor plans");
    expect(now.viewports).toHaveLength(3);
    expect(now.viewports[0]).toMatchObject({ id: "v1", title: "GROUND FLOOR PLAN", position: { x: 200, y: 150 } });
    expect(now.viewports[1]).toMatchObject({ id: "v2", scale_denominator: 100 });
    // Moving a sheet in the set sends the latest sheet, with its edits.
    await editSheet(drawn, drawn, 1);
    const moved = engine.now().project.sheet_set.sheets;
    expect(moved.map((sh) => sh.id)).toEqual(["s2", "s1"]);
    expect(moved[1].title).toBe("Floor plans");
  });

  it("keeps quick edits to two sides of the lot", async () => {
    const d = base();
    const side = { role: "side" as const, setback_mm: 0, road: "", road_width_mm: null };
    const lot = { kind: "lot" as const, id: "lot1", corners: [pt(0, 0), pt(10000, 0), pt(10000, 20000), pt(0, 20000)], sides: [side, side, side, side], grade_mm: 0 };
    d.project.elements.push(lot as Element);
    useApp.setState({ doc: structuredClone(d) });
    const engine = fakeEngine(d, SLOW_MS);
    const drawn = useApp.getState().doc!.project.elements.find((e) => e.id === "lot1") as Extract<Element, { kind: "lot" }>;
    const setSide = (i: number, patch: object) => ({ ...drawn, sides: drawn.sides.map((sd, j) => (j === i ? { ...sd, ...patch } : sd)) });
    await Promise.all([updateElement(drawn, setSide(0, { role: "road", road: "Mabini St." })), updateElement(drawn, setSide(1, { setback_mm: 2000 }))]);
    const now = engine.now().project.elements.find((e) => e.id === "lot1") as typeof lot;
    expect(now.sides[0]).toMatchObject({ role: "road", road: "Mabini St." });
    expect(now.sides[1]).toMatchObject({ setback_mm: 2000 });
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
