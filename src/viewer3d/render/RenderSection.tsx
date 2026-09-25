// The Render section at the top of the Visuals panel: size and quality, the
// Render and Render all views buttons, and while a render runs the image as
// it refines, the time left and the GPU in use. A finished render is in the
// gallery below; "Visualize with AI" opens the studio on it (D17).

import { useLayoutEffect, useRef } from "react";
import { bus } from "../../state/bus";
import { useApp } from "../../state/store";
import { Segmented } from "../../ui/controls";
import { usePresence } from "../../ui/motion";
import { useLastTruthy } from "../../ui/motionDom";
import { QUALITIES, RENDER_SIZES, type TraceQuality, type RenderSizeKey } from "./renderJob";
import { timeLeftLabel, useRenderQueue } from "./renderQueue";
import { useRenderUi } from "./renderStore";
import s from "./RenderSection.module.css";

const PHASE_TEXT = {
  preparing: "Getting the scene ready",
  compiling: "Preparing the path tracer",
  rendering: "Rendering",
  finishing: "Removing noise",
  enhanced: "Enhanced capture",
} as const;

export function RenderSection({ canRender }: { canRender: boolean }) {
  const size = useRenderQueue((q) => q.size);
  const quality = useRenderQueue((q) => q.quality);
  const running = useRenderQueue((q) => q.running);
  const last = useRenderQueue((q) => q.last);
  const error = useRenderQueue((q) => q.error);
  const savedViews = useApp((a) => a.doc?.project.elements.filter((e) => e.kind === "camera").length ?? 0);
  const presence = usePresence(running, "panel");
  const sizeInfo = RENDER_SIZES.find((x) => x.key === size) ?? RENDER_SIZES[0];

  return (
    <section className={s.section} data-testid="render-section" aria-label="Render">
      <div className={s.head}>
        <div>
          <h3 className={s.title}>Render</h3>
          <p className={s.sub}>A path traced still of the 3D view: real bounce light, soft shadows, lit lamps. The view stays usable while it runs.</p>
        </div>
      </div>

      <div className={s.options}>
        <div className={s.field}>
          <span className={s.fieldLabel}>Size</span>
          <Segmented<RenderSizeKey>
            label="Render size"
            stretch
            value={size}
            options={RENDER_SIZES.map((x) => ({ value: x.key, label: x.label, tip: x.note }))}
            onChange={(v) => useRenderQueue.getState().setSize(v)}
          />
        </div>
        <div className={s.field}>
          <span className={s.fieldLabel}>Quality</span>
          <Segmented<TraceQuality>
            label="Render quality"
            stretch
            value={quality}
            options={(Object.keys(QUALITIES) as TraceQuality[]).map((k) => ({
              value: k,
              label: QUALITIES[k].label,
              tip: `Up to ${QUALITIES[k].seconds >= 120 ? `${QUALITIES[k].seconds / 60} minutes` : `${QUALITIES[k].seconds} seconds`}`,
            }))}
            onChange={(v) => useRenderQueue.getState().setQuality(v)}
          />
        </div>
      </div>
      <p className={s.hint}>
        {sizeInfo.note}. {QUALITIES[quality].label}: up to {QUALITIES[quality].seconds >= 120 ? `${QUALITIES[quality].seconds / 60} minutes` : `${QUALITIES[quality].seconds} seconds`}, then noise is removed. Stop early and it saves what is there.
      </p>

      <div className={s.actions}>
        <button
          type="button"
          className={s.primary}
          data-testid="render-view"
          disabled={running || !canRender}
          title={canRender ? "Render the current 3D view" : "Open the 3D view to render it"}
          onClick={() => bus.emit("render", { views: "current" })}
        >
          Render view
        </button>
        <button
          type="button"
          className={s.btn}
          data-testid="render-all"
          disabled={running || savedViews === 0}
          title={savedViews === 0 ? "Save a view in the 3D toolbar first" : "Render every saved view, one after the other"}
          onClick={() => bus.emit("render", { views: "all" })}
        >
          Render all views{savedViews > 0 ? ` (${savedViews})` : ""}
        </button>
      </div>

      {presence.mounted && <RenderProgressView stage={presence.stage} />}

      {!running && last && (
        <div className={s.result} data-kind={last.kind} data-testid="render-result" role="status">
          <span>
            {last.kind === "path_traced" ? "Render saved" : "Enhanced capture saved"}: {last.name}
            <small>
              {last.kind === "path_traced"
                ? `${last.samples} samples in ${Math.round(last.seconds)} s, denoised`
                : `${last.note} A refined raster image, labelled Enhanced capture.`}
            </small>
          </span>
          <span className={s.actions}>
            <button
              type="button"
              className={`${s.btn} ${s.ai}`}
              data-testid="render-visualize"
              title="Open the render studio on this render to make an AI visualization of it"
              onClick={() => {
                const ui = useRenderUi.getState();
                ui.setSourceId(last.recordId);
                ui.setCompareId(last.recordId);
                ui.setFullOpen(true);
              }}
            >
              Visualize with AI
            </button>
            <button type="button" className={s.btn} onClick={() => useRenderUi.getState().setLightboxId(last.recordId)}>
              Open
            </button>
          </span>
        </div>
      )}
      {!running && error && (
        <p className={s.error} role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

function RenderProgressView({ stage }: { stage: string }) {
  const progress = useRenderQueue((q) => q.progress);
  const current = useRenderQueue((q) => q.current);
  const preview = useRenderQueue((q) => q.preview);
  const version = useRenderQueue((q) => q.previewVersion);
  const hostRef = useRef<HTMLCanvasElement>(null);

  // The job draws into its own small canvas; this one mirrors it.
  useLayoutEffect(() => {
    const dst = hostRef.current;
    if (!dst || !preview) return;
    if (dst.width !== preview.width || dst.height !== preview.height) {
      dst.width = preview.width;
      dst.height = preview.height;
    }
    dst.getContext("2d")?.drawImage(preview, 0, 0);
  }, [preview, version]);

  // The last reading stays on screen while the section closes.
  const p = useLastTruthy(progress);
  const phase = p?.phase ?? "preparing";
  const determinate = p !== null && (phase === "rendering" || phase === "enhanced") && p.targetSamples > 0;
  const byTime = p && p.budgetMs > 0 ? p.elapsedMs / p.budgetMs : 0;
  const bySamples = p && p.targetSamples > 0 ? p.samples / p.targetSamples : 0;
  const done = Math.min(1, Math.max(byTime, bySamples));

  return (
    <div className={s.reveal} data-stage={stage} data-testid="render-progress-view">
      <div className={s.revealInner}>
        <div className={s.preview} data-ready={version > 0}>
          <canvas ref={hostRef} aria-label="The render as it refines" />
          {p?.note ? <span className={s.previewNote}>{p.note}</span> : null}
        </div>
        <div className={s.bar} role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(done * 100)}>
          <span className={s.fill} data-indeterminate={!determinate} style={{ width: `${Math.round(done * 100)}%` }} />
        </div>
        <div className={s.stats}>
          <span data-testid="render-phase">
            {PHASE_TEXT[phase]}
            {current && current.total > 1 ? `, view ${current.index} of ${current.total}` : ""}
            {current ? `: ${current.name}` : ""}
          </span>
          <span>
            {determinate && p ? `${p.samples} of ${p.targetSamples} samples, ${timeLeftLabel(p.etaMs)}` : ""}
          </span>
          <span data-testid="render-gpu">GPU: {p?.gpu ?? "starting"}</span>
        </div>
        <div className={s.actions}>
          <button type="button" className={s.primary} data-testid="render-stop" onClick={() => useRenderQueue.getState().stop(true)}>
            Stop and save
          </button>
          <button type="button" className={s.btn} data-testid="render-cancel" title="Esc" onClick={() => useRenderQueue.getState().stop(false)}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
