// Dev harness for the copilot dock. Mounts AiDock in a 340 px column next to
// a plain readout of the app store, against the real Rust engine over the dev
// bridge. Open /src/ai/dev/index.html on the Vite dev server.
//   ?template=blank   start from an empty project (default: sample-bungalow)
//   ?h=420            dock height in px (default: full height)
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import "../../styles/tokens.css";
import { useApp } from "../../state/store";
import { useCopilot } from "../copilotStore";
import { AiDock } from "../AiDock";

declare global {
  interface Window {
    __app: typeof useApp;
    __copilot: typeof useCopilot;
  }
}
window.__app = useApp;
window.__copilot = useCopilot;

const params = new URLSearchParams(location.search);

function Readout() {
  const doc = useApp((s) => s.doc);
  const preview = useApp((s) => s.preview);
  const selection = useApp((s) => s.selection);
  const hoverId = useApp((s) => s.hoverId);
  const toasts = useApp((s) => s.toasts);
  const rooms = doc?.project.elements.filter((e) => e.kind === "room") ?? [];
  const walls = doc?.project.elements.filter((e) => e.kind === "wall") ?? [];

  const addUserWall = () =>
    void useApp.getState().dispatch({
      type: "add_wall",
      start: { x: -3000, y: -3000 },
      end: { x: -1000, y: -3000 },
      thickness_mm: null,
      height_mm: null,
      material_id: null,
      level_id: null,
    });

  const btn: React.CSSProperties = { marginRight: 6, marginBottom: 6, padding: "3px 8px", cursor: "pointer" };
  return (
    <div style={{ padding: 16, overflow: "auto", fontFamily: "var(--font-mono)", fontSize: 12, userSelect: "text" }}>
      <h3 style={{ margin: "0 0 8px", fontFamily: "var(--font-ui)" }}>AiDock harness</h3>
      <div>
        <button style={btn} id="sel-room" onClick={() => useApp.getState().select(rooms[0] ? [rooms[0].id] : [])}>select first room</button>
        <button style={btn} id="sel-wall" onClick={() => useApp.getState().select(walls[0] ? [walls[0].id] : [])}>select first wall</button>
        <button style={btn} id="sel-none" onClick={() => useApp.getState().select([])}>select none</button>
        <button style={btn} id="user-wall" onClick={addUserWall}>user adds a wall</button>
        <button style={btn} id="undo" onClick={() => void useApp.getState().undo()}>undo</button>
        <button style={btn} id="close" onClick={() => void useApp.getState().closeProject()}>close project</button>
      </div>
      <pre id="readout" style={{ whiteSpace: "pre-wrap" }}>
        {JSON.stringify(
          {
            project: doc ? { id: doc.project.id, name: doc.project.name } : null,
            revision: doc?.revision ?? null,
            elements: doc?.project.elements.length ?? 0,
            totals: doc?.derived.totals ?? null,
            can_undo: doc?.can_undo ?? false,
            undo_label: doc?.undo_label ?? null,
            selection,
            hoverId,
            preview: preview ? { revision: preview.state.revision, elements: preview.state.project.elements.length, diff: preview.diff } : null,
            toasts: toasts.map((t) => `${t.kind}: ${t.message}`),
          },
          null,
          2,
        )}
      </pre>
    </div>
  );
}

function Harness() {
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const template = params.get("template") ?? "sample-bungalow";
    useApp
      .getState()
      .createProject("AI harness", template)
      .then(() => {
        if (!useApp.getState().doc) setError("Could not create a project. Is the dev bridge running?");
      });
  }, []);
  const h = params.get("h");
  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 340px", height: "100vh", background: "var(--surface-2)" }}>
      <div style={{ minWidth: 0, overflow: "auto" }}>
        {error ? <p style={{ padding: 16, color: "var(--danger)" }}>{error}</p> : null}
        <Readout />
      </div>
      <div style={{ height: h ? Number(h) : "100vh", alignSelf: "end", borderLeft: "1px solid var(--line-strong)", borderTop: "1px solid var(--line-strong)" }}>
        <AiDock />
      </div>
    </div>
  );
}

// No StrictMode here: the harness creates a project on mount and a double
// mount would create two.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<Harness />);
