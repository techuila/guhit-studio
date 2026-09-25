// The Render entry point in the 3D toolbar: renders the current view with
// the Visuals panel's size and quality (render/renderQueue.ts) and brings the
// Visuals panel up, where the image refines. While a render runs the button
// shows how far along it is, and a click just brings the panel up again.

import { useShell } from "../../shell/shellStore";
import { bus } from "../../state/bus";
import { useRenderQueue } from "./renderQueue";
import vs from "../Viewer3D.module.css";

const stroke = { fill: "none", stroke: "currentColor", strokeWidth: 1.4, strokeLinecap: "round", strokeLinejoin: "round" } as const;

export function RenderButton({ disabled }: { disabled?: boolean }) {
  const running = useRenderQueue((q) => q.running);
  const pct = useRenderQueue((q) => {
    const p = q.progress;
    if (!p || p.phase !== "rendering" && p.phase !== "enhanced") return null;
    const t = p.budgetMs > 0 ? p.elapsedMs / p.budgetMs : 0;
    const n = p.targetSamples > 0 ? p.samples / p.targetSamples : 0;
    return Math.round(Math.min(1, Math.max(t, n)) * 100);
  });
  return (
    <button
      type="button"
      className={vs.btn}
      data-active={running}
      data-testid="render-button"
      disabled={disabled && !running}
      title={running ? "Rendering: see the Visuals panel (Esc cancels)" : "Render this view with the path tracer, saved to Visuals"}
      onClick={() => {
        useShell.getState().setDockTab("visuals");
        if (!running) bus.emit("render", { views: "current" });
      }}
    >
      <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
        <path {...stroke} d="M2.2 4.6h2.4l1.1-1.6h4.6l1.1 1.6h2.4v8.2H2.2z" />
        <path {...stroke} d="M8 6.2l.7 1.5 1.6.2-1.2 1.1.3 1.6L8 9.8l-1.4.8.3-1.6-1.2-1.1 1.6-.2z" />
      </svg>
      <span className={vs.label}>{running ? (pct === null ? "Rendering" : `Rendering ${pct}%`) : "Render"}</span>
    </button>
  );
}
