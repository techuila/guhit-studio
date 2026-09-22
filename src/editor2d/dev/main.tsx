// Dev harness: mounts only PlanCanvas full window with a tiny tool switcher.
// Tries the dev bridge first, falls back to the fixture when it is not reachable.
// URL flags: ?fixture forces the fixture, ?blank opens an empty bridge project.

import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "../../styles/tokens.css";
import type { CatalogItem, DocState } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";
import { bus } from "../../state/bus";
import { useApp, type Tool } from "../../state/store";
import { PlanCanvas } from "../PlanCanvas";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";

const BTN = { border: "1px solid var(--chrome-2)", borderRadius: 4, padding: "3px 8px", background: "var(--chrome-2)", color: "#fff", cursor: "pointer" } as const;

const TOOLS: Tool[] = ["select", "pan", "wall", "rect_room", "door", "window", "column", "stair", "asset", "dimension", "text", "camera"];

const FALLBACK_CATALOG: CatalogItem[] = [
  { key: "bed-double", name: "Double bed", category: "furniture", width_mm: 1370, depth_mm: 1900, height_mm: 500, elevation_mm: 0 },
  { key: "sofa-3", name: "Sofa, 3 seater", category: "furniture", width_mm: 2100, depth_mm: 900, height_mm: 800, elevation_mm: 0 },
  { key: "wc", name: "Water closet", category: "sanitary", width_mm: 400, depth_mm: 700, height_mm: 780, elevation_mm: 0 },
];

function Harness() {
  const tool = useApp((s) => s.tool);
  const cursor = useApp((s) => s.cursor);
  const selection = useApp((s) => s.selection);
  const toasts = useApp((s) => s.toasts);
  const snapEnabled = useApp((s) => s.snapEnabled);
  const orthoEnabled = useApp((s) => s.orthoEnabled);
  const catalog = useApp((s) => s.catalog);
  const assetKey = useApp((s) => s.toolOptions.assetKey);
  const doc = useApp((s) => s.doc);
  const [source, setSource] = useState("loading");

  useEffect(() => {
    const q = new URLSearchParams(location.search);
    const st = useApp.getState();
    const useFixture = (): void => {
      st.setDoc(fixture as unknown as DocState);
      useApp.setState({ catalog: FALLBACK_CATALOG, screen: "editor" });
      setSource("fixture");
    };
    if (q.has("fixture")) {
      useFixture();
      return;
    }
    ipc
      .hubCreate("2D dev", undefined, q.has("blank") ? "blank" : "sample-bungalow")
      .then(async (d) => {
        st.setDoc(d);
        useApp.setState({ screen: "editor" });
        setSource("bridge");
        await st.loadCatalog();
      })
      .catch(useFixture);
  }, []);

  // Test hooks for scripted checks.
  useEffect(() => {
    (window as unknown as Record<string, unknown>).__app = useApp;
    (window as unknown as Record<string, unknown>).__bus = bus;
  }, []);

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
      <div
        style={{
          display: "flex",
          gap: 4,
          alignItems: "center",
          padding: "4px 8px",
          background: "var(--chrome)",
          color: "var(--chrome-text)",
          fontSize: 12,
          flex: "0 0 32px",
          height: 32,
          overflow: "hidden",
          whiteSpace: "nowrap",
        }}
      >
        {TOOLS.map((t) => (
          <button
            key={t}
            data-tool={t}
            onClick={() => useApp.getState().setTool(t)}
            style={{
              border: "1px solid var(--chrome-2)",
              borderRadius: 4,
              padding: "3px 8px",
              cursor: "pointer",
              background: tool === t ? "var(--accent)" : "var(--chrome-2)",
              color: "#fff",
            }}
          >
            {t}
          </button>
        ))}
        <select
          data-testid="asset-key"
          value={assetKey ?? ""}
          onChange={(e) => useApp.getState().setTool("asset", { assetKey: e.target.value || null })}
        >
          <option value="">asset...</option>
          {catalog.map((c) => (
            <option key={c.key} value={c.key}>
              {c.key}
            </option>
          ))}
        </select>
        <button data-testid="snap" style={BTN} onClick={() => useApp.getState().toggle("snapEnabled")}>snap {snapEnabled ? "on" : "off"}</button>
        <button data-testid="ortho" style={BTN} onClick={() => useApp.getState().toggle("orthoEnabled")}>ortho {orthoEnabled ? "on" : "off"}</button>
        <button data-testid="grid" style={BTN} onClick={() => useApp.getState().toggle("gridVisible")}>grid</button>
        <button data-testid="fit" style={BTN} onClick={() => bus.emit("zoom_to_fit")}>fit</button>
        <button data-testid="undo" style={BTN} onClick={() => void useApp.getState().undo()}>undo</button>
        <button data-testid="redo" style={BTN} onClick={() => void useApp.getState().redo()}>redo</button>
        <span data-testid="status" style={{ marginLeft: "auto", fontFamily: "var(--font-mono)", minWidth: 330, textAlign: "right" }}>
          {source} | rev {doc?.revision ?? "-"} | els {doc?.project.elements.length ?? 0} | sel {selection.length} |{" "}
          {cursor ? `${Math.round(cursor.x)}, ${Math.round(cursor.y)}` : "-"}
        </span>
      </div>
      <div style={{ flex: "1 1 auto", minHeight: 0, position: "relative" }}>
        <PlanCanvas />
        <div style={{ position: "absolute", right: 8, top: 8, display: "grid", gap: 4, pointerEvents: "none" }}>
          {toasts.slice(-3).map((t) => (
            <div
              key={t.id}
              data-testid="toast"
              style={{ background: t.kind === "error" ? "var(--danger)" : "var(--ink)", color: "#fff", padding: "4px 8px", borderRadius: 4, fontSize: 12 }}
            >
              {t.message}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

createRoot(document.getElementById("root") as HTMLElement).render(<Harness />);
