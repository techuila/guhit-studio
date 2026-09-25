// Dev harness for the 3D viewer. Not part of the app bundle.
// http://localhost:1522/src/viewer3d/dev/index.html
//   ?fixture=1  skip the bridge and load the fixture
//   ?fresh=1    always create a new bridge project
//   ?pack=0     start with the CC0 GLB furniture and PBR maps off
//   ?hdri=0     start with the HDRI sky off
//   ?template=plumbing-demo   open (or create) a project from another template

import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import "../../styles/tokens.css";
import type { Command, DocState, Element, Roof } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";
import { bus } from "../../state/bus";
import { useApp } from "../../state/store";
import { useViewer } from "../viewerStore";
import { Viewer3D } from "../Viewer3D";
import { RenderPanel } from "../RenderPanel";
import { liveEngineCount, type ViewerEngine } from "../engine/ViewerEngine";
import { KNOWN_ASSET_KEYS } from "../scene/assets";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";

const params = new URLSearchParams(location.search);
type Source = "loading" | "bridge" | "fixture";

declare global {
  interface Window {
    __app: typeof useApp;
    __ipc: typeof ipc;
    __viewer3d?: ViewerEngine;
    __source: Source;
    __viewer: typeof useViewer;
    __bus: typeof bus;
  }
}
window.__app = useApp;
window.__ipc = ipc;
window.__source = "loading";
window.__viewer = useViewer;
window.__bus = bus;
const template = params.get("template") ?? "sample-bungalow";
const projectName = template === "sample-bungalow" ? "3D dev" : `3D dev ${template}`;

async function boot(): Promise<Source> {
  if (!params.has("fixture")) {
    try {
      let doc: DocState | null = null;
      if (!params.has("fresh")) {
        const existing = (await ipc.hubList()).find((p) => p.name === projectName);
        if (existing) doc = await ipc.hubOpen(existing.id);
      }
      doc ??= await ipc.hubCreate(projectName, undefined, template);
      useApp.getState().setDoc(doc);
      return "bridge";
    } catch (e) {
      console.info("harness: bridge not available, using the fixture", e);
    }
  }
  useApp.getState().setDoc(fixture as unknown as DocState);
  return "fixture";
}

/** Applies a command through the bridge, or patches the fixture locally. */
async function apply(source: Source, command: Command, local: (doc: DocState) => DocState) {
  const app = useApp.getState();
  if (source === "bridge") {
    await app.dispatch(command);
  } else if (app.doc) {
    const next = local(structuredClone(app.doc));
    next.revision = app.doc.revision + 1;
    app.setDoc(next);
  }
}

function testContent(levelId: string): Element[] {
  const out: Element[] = [];
  const catalog = useApp.getState().catalog;
  KNOWN_ASSET_KEYS.forEach((key, i) => {
    const item = catalog.find((c) => c.key === key);
    const col = i % 8;
    const row = Math.floor(i / 8);
    out.push({
      kind: "asset",
      id: "",
      level_id: levelId,
      catalog_key: key,
      name: item?.name ?? key,
      category: item?.category ?? "furniture",
      position: { x: -9000 + col * 3400, y: -4000 - row * 3800 },
      rotation_deg: 0,
      width_mm: item?.width_mm ?? 1000,
      depth_mm: item?.depth_mm ?? 1000,
      height_mm: item?.height_mm ?? 1000,
      elevation_mm: item?.elevation_mm ?? 0,
      light: item?.light ?? null,
      links: [],
      circuit: "",
    });
  });
  out.push({ kind: "asset", id: "", level_id: levelId, catalog_key: "unknown-thing", name: "Unknown", category: "furniture", position: { x: 12000, y: 1000 }, rotation_deg: 30, width_mm: 900, depth_mm: 600, height_mm: 700, elevation_mm: 0, light: null, links: [], circuit: "" });
  out.push({ kind: "asset", id: "", level_id: levelId, catalog_key: "sofa-3", name: "Sofa", category: "furniture", position: { x: 2400, y: 5300 }, rotation_deg: 0, width_mm: 2100, depth_mm: 900, height_mm: 800, elevation_mm: 0, light: null, links: [], circuit: "" });
  out.push({ kind: "asset", id: "", level_id: levelId, catalog_key: "coffee-table", name: "Coffee table", category: "furniture", position: { x: 2400, y: 4000 }, rotation_deg: 0, width_mm: 1100, depth_mm: 600, height_mm: 420, elevation_mm: 0, light: null, links: [], circuit: "" });
  out.push({ kind: "asset", id: "", level_id: levelId, catalog_key: "dining-4", name: "Dining", category: "furniture", position: { x: 3300, y: 1500 }, rotation_deg: 90, width_mm: 1200, depth_mm: 800, height_mm: 750, elevation_mm: 0, light: null, links: [], circuit: "" });
  out.push({ kind: "column", id: "", level_id: levelId, center: { x: 11000, y: 5000 }, shape: "rect", width_mm: 300, depth_mm: 400, rotation_deg: 0, material_id: null });
  out.push({ kind: "column", id: "", level_id: levelId, center: { x: 12500, y: 5000 }, shape: "round", width_mm: 350, depth_mm: 350, rotation_deg: 0, material_id: null });
  out.push({ kind: "stair", id: "", level_id: levelId, origin: { x: 11000, y: -2500 }, rotation_deg: 0, width_mm: 1000, run_mm: 3900, riser_count: 16 });
  out.push({ kind: "stair", id: "", level_id: levelId, origin: { x: 14000, y: -2500 }, rotation_deg: -90, width_mm: 900, run_mm: 3000, riser_count: 12 });
  return out;
}

function Harness() {
  const [source, setSource] = useState<Source>("loading");
  const [mounted, setMounted] = useState(true);
  const [stats, setStats] = useState("");
  const [pack, setPack] = useState("");
  // The CC0 pack, split the way the two looks are compared: GLB furniture plus
  // PBR maps on one switch, the HDRI sky and its light on the other.
  const [packAssets, setPackAssets] = useState(params.get("pack") !== "0");
  const [hdri, setHdri] = useState(params.get("hdri") !== "0");
  const doc = useApp((s) => s.doc);
  const toasts = useApp((s) => s.toasts);

  useEffect(() => {
    let live = true;
    void boot().then((s) => {
      if (!live) return;
      window.__source = s;
      setSource(s);
      if (s === "bridge") void useApp.getState().loadCatalog();
    });
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    const t = setInterval(() => {
      const e = window.__viewer3d;
      const s = mounted && e ? e.stats() : null;
      setStats(JSON.stringify({ engines: liveEngineCount(), ...(s ?? {}) }));
      setPack(
        s
          ? `models ${s.pack.models.loaded}/${s.pack.models.available} (${s.pack.models.pending} loading)  textures ${s.pack.textures.loaded} (${s.pack.textures.pending} loading)  hdri ${s.pack.hdri ? "on" : "off"}${s.pack.textures.flat.length > 0 ? `  blank: ${s.pack.textures.flat.join(", ")}` : ""}`
          : "",
      );
    }, 500);
    return () => clearInterval(t);
  }, [mounted]);

  // A remount makes a new engine, which starts with both looks on.
  useEffect(() => {
    if (!mounted) return;
    window.__viewer3d?.setPackOptions({ assets: packAssets, hdri });
  }, [mounted, packAssets, hdri]);

  const first = (kind: Element["kind"], n = 0) => doc?.project.elements.filter((e) => e.kind === kind)[n]?.id;
  const select = (id: string | undefined) => useApp.getState().select(id ? [id] : []);

  const setRoof = (patch: Partial<Roof>) => {
    if (!doc) return;
    const roof = { ...doc.project.roof, ...patch };
    void apply(source, { type: "set_roof", roof }, (d) => {
      d.project.roof = roof;
      return d;
    });
  };

  const addContent = () => {
    if (!doc) return;
    const elements = testContent(doc.project.levels[0].id);
    void apply(
      source,
      { type: "batch", label: "Add 3D test content", commands: elements.map((element) => ({ type: "add_element", element })) },
      (d) => {
        elements.forEach((e, i) => d.project.elements.push({ ...e, id: `test-${i}` }));
        return d;
      },
    );
  };

  const ghost = async () => {
    const app = useApp.getState();
    if (!app.doc) return;
    if (app.preview) return app.setPreview(null);
    if (source === "bridge") {
      try {
        const levelId = app.doc.project.levels[0].id;
        const preview = await ipc.docPreview({
          type: "batch",
          label: "Ghost test",
          commands: [
            { type: "add_wall", start: { x: 2500, y: 0 }, end: { x: 2500, y: 3000 }, thickness_mm: 100, height_mm: null, material_id: null, level_id: levelId },
            { type: "add_element", element: { kind: "asset", id: "", level_id: levelId, catalog_key: "wardrobe", name: "Wardrobe", category: "furniture", position: { x: 5700, y: 5500 }, rotation_deg: 0, width_mm: 1200, depth_mm: 600, height_mm: 2100, elevation_mm: 0, light: null, links: [], circuit: "" } },
          ],
        });
        app.setPreview(preview);
        return;
      } catch (e) {
        app.reportError(e);
      }
    }
    const ids = [first("wall", 4), first("asset")].filter((x): x is string => !!x);
    app.setPreview({ state: app.doc, diff: { added: [], modified: ids, removed: [], summary: "Fake preview" } });
  };

  const btn = (label: string, fn: () => void, id?: string) => (
    <button key={label} type="button" data-testid={id ?? `h-${label.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`} onClick={fn} style={{ fontSize: 11, padding: "3px 7px", border: "1px solid var(--line)", borderRadius: 4, background: "var(--surface)", cursor: "pointer" }}>
      {label}
    </button>
  );

  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 400px", gridTemplateRows: "auto 1fr", height: "100%" }}>
      <div style={{ gridColumn: "1 / 3", display: "flex", flexWrap: "wrap", gap: 4, alignItems: "center", padding: "6px 8px", background: "var(--surface-2)", borderBottom: "1px solid var(--line)" }}>
        <strong data-testid="h-source" style={{ fontSize: 11, marginRight: 6 }}>
          source: {source}
        </strong>
        {btn("Select wall", () => select(first("wall")))}
        {btn("Select wall 5", () => select(first("wall", 4)))}
        {btn("Select room 1", () => select(first("room")))}
        {btn("Select room 2", () => select(first("room", 1)))}
        {btn("Select door", () => select(first("opening")))}
        {btn("Clear selection", () => select(undefined))}
        {btn("Roof none", () => setRoof({ kind: "none" }))}
        {btn("Roof flat", () => setRoof({ kind: "flat" }))}
        {btn("Roof shed", () => setRoof({ kind: "shed" }))}
        {btn("Roof gable", () => setRoof({ kind: "gable" }))}
        {btn("Ridge x", () => setRoof({ ridge_axis: "x" }))}
        {btn("Ridge y", () => setRoof({ ridge_axis: "y" }))}
        {btn("Clay tile", () => setRoof({ material_id: "mat-roof-clay-tile" }))}
        {btn("Add test content", addContent)}
        {btn("Ghost preview", () => void ghost())}
        {btn("Active camera", () => useApp.getState().setActiveCamera(first("camera") ?? null))}
        {btn("Lose context", () => window.__viewer3d?.renderer.forceContextLoss())}
        {btn("Restore context", () => window.__viewer3d?.renderer.forceContextRestore())}
        {btn(mounted ? "Unmount" : "Mount", () => setMounted((m) => !m), "h-mount")}
        {btn("Undo", () => void useApp.getState().undo())}
        {btn("Walk to first pipe finding", () => {
          const issue = useApp.getState().doc?.derived.issues.find((i) => i.location && i.element_ids.length > 0);
          if (issue) bus.emit("walk_to", { ids: issue.element_ids, location: issue.location });
        })}
        {btn("Focus first pipe", () => {
          const pipe = doc?.project.elements.find((e) => e.kind === "pipe");
          if (pipe) bus.emit("focus_elements", [pipe.id]);
        })}
        {btn("Cold water layer", () => {
          const d = useApp.getState().doc;
          const layer = d?.project.layers.find((l) => l.key === "cold_water");
          if (!d || !layer) return;
          void apply(source, { type: "set_layer", layer: { ...layer, visible: !layer.visible } }, (x) => {
            x.project.layers = x.project.layers.map((l) => (l.key === "cold_water" ? { ...l, visible: !l.visible } : l));
            return x;
          });
        })}
        {btn(`Pack assets: ${packAssets ? "on" : "off"}`, () => setPackAssets((v) => !v), "h-pack-assets")}
        {btn(`HDRI: ${hdri ? "on" : "off"}`, () => setHdri((v) => !v), "h-pack-hdri")}
        <code data-testid="h-pack" style={{ fontSize: 10, color: "var(--ink-2)" }}>
          {pack}
        </code>
        <code data-testid="h-stats" style={{ fontSize: 10, color: "var(--ink-3)" }}>
          {stats}
        </code>
      </div>
      <div style={{ position: "relative", minWidth: 0, minHeight: 0 }}>{mounted && <Viewer3D />}</div>
      <div style={{ borderLeft: "1px solid var(--line)", minHeight: 0 }}>
        <RenderPanel />
      </div>
      <div style={{ position: "fixed", right: 410, bottom: 10, display: "flex", flexDirection: "column", gap: 4 }}>
        {toasts.slice(-3).map((t) => (
          <div key={t.id} data-testid="h-toast" style={{ padding: "6px 10px", borderRadius: 6, fontSize: 12, color: "#fff", background: t.kind === "error" ? "var(--danger)" : "var(--chrome)" }}>
            {t.message}
          </div>
        ))}
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Harness />
  </React.StrictMode>,
);
