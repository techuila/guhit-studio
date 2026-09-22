// The render studio: pick a capture, set the look, generate an AI
// visualization, compare it with the model view side by side.
//
// Geometry is the authority and the image is derived from it (AGENTS.md):
// nothing here writes back into the model, and every AI image keeps its
// "AI visualization" badge.

import { useCallback, useEffect, useRef, useState } from "react";
import type { RenderAiRequest, RenderAiSettings, RenderRecord, RenderStyle } from "../../contract/bindings";
import { ipc, toIpcError } from "../../contract/ipc";
import { Button, CheckRow, IconButton, Segmented, Select, cx } from "../../ui/controls";
import type { IconName } from "../../ui/icons";
import { CompareSlider } from "./CompareSlider";
import { GalleryStrip } from "./GalleryStrip";
import {
  BUILDING_TYPES,
  QUALITIES,
  buildPrompt,
  costForQuality,
  elapsedLabel,
  formatResolution,
  providerLabel,
  useRenderUi,
} from "./renderStore";
import s from "./render.module.css";

const ACTIONS: Array<{ id: "enlarge" | "download" | "goto-camera" | "delete"; label: string; icon: IconName }> = [
  { id: "enlarge", label: "Enlarge", icon: "fit" },
  { id: "download", label: "Download", icon: "export" },
  { id: "goto-camera", label: "Go to camera", icon: "camera" },
  { id: "delete", label: "Delete", icon: "trash" },
];

export interface RenderStudioProps {
  variant: "dock" | "full";
  records: RenderRecord[];
  thumbs: Record<string, string>;
  renderStyles: RenderStyle[];
  settings: RenderAiSettings | null;
  /** One sentence when the settings call itself failed. */
  settingsError: string | null;
  revision: number;
  canCapture: boolean;
  capturing: boolean;
  onCapture: () => void;
  /** A new record arrived from the provider. */
  onGenerated: (record: RenderRecord) => void;
  onGoToCamera: (record: RenderRecord) => void;
  onDownload: (record: RenderRecord) => void;
  onDelete: (record: RenderRecord) => void;
  onEnlarge: (id: string) => void;
  onAddKey: () => void;
  onOpenFull?: () => void;
  onCloseFull?: () => void;
}

export function RenderStudio({
  variant,
  records,
  thumbs,
  renderStyles,
  settings,
  settingsError,
  revision,
  canCapture,
  capturing,
  onCapture,
  onGenerated,
  onGoToCamera,
  onDownload,
  onDelete,
  onEnlarge,
  onAddKey,
  onOpenFull,
  onCloseFull,
}: RenderStudioProps) {
  const config = useRenderUi((st) => st.config);
  const patchConfig = useRenderUi((st) => st.patchConfig);
  const sourceId = useRenderUi((st) => st.sourceId);
  const setSourceId = useRenderUi((st) => st.setSourceId);
  const compareId = useRenderUi((st) => st.compareId);
  const setCompareId = useRenderUi((st) => st.setCompareId);
  const compareMode = useRenderUi((st) => st.compareMode);
  const setCompareMode = useRenderUi((st) => st.setCompareMode);
  const divider = useRenderUi((st) => st.divider);
  const setDivider = useRenderUi((st) => st.setDivider);

  const [busy, setBusy] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [genError, setGenError] = useState<string | null>(null);
  const [resolution, setResolution] = useState("");
  const abandoned = useRef(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const captures = records.filter((r) => r.source === "model_view");
  const source = captures.find((r) => r.id === sourceId) ?? captures[0] ?? null;

  // Keep the studio pointing at a capture that still exists.
  useEffect(() => {
    if (source && source.id !== sourceId) setSourceId(source.id);
    if (!source && sourceId) setSourceId(null);
  }, [source, sourceId, setSourceId]);

  const shown = records.find((r) => r.id === compareId) ?? null;
  const shownIsAi = shown?.source === "ai_visualization";
  const beforeRecord = shownIsAi
    ? (records.find((r) => r.id === shown?.source_render_id) ?? null)
    : (shown ?? source);
  const afterRecord = shownIsAi ? shown : null;
  const beforeSrc = beforeRecord ? (thumbs[beforeRecord.id] ?? "") : "";
  const afterSrc = afterRecord ? (thumbs[afterRecord.id] ?? "") : "";
  const currentRecord = afterRecord ?? beforeRecord;

  const styleOptions = renderStyles.map((st) => ({ value: st.key, label: st.name }));
  const activeStyle = renderStyles.find((st) => st.key === config.styleKey) ?? renderStyles[0] ?? null;

  const backendReady = settings !== null;
  const hasKey = settings?.has_api_key ?? false;
  const cost = costForQuality(settings, config.quality);

  // Elapsed seconds while the provider works.
  useEffect(() => {
    if (!busy) return;
    const started = performance.now();
    setElapsed(0);
    const id = window.setInterval(() => setElapsed(performance.now() - started), 250);
    return () => window.clearInterval(id);
  }, [busy]);

  const generate = useCallback(async () => {
    if (!source || busy) return;
    const request: RenderAiRequest = {
      source_render_id: source.id,
      style_key: activeStyle?.key ?? null,
      prompt: buildPrompt(config.buildingType, config.extras),
      quality: config.quality,
      keep_geometry: config.keepGeometry,
    };
    abandoned.current = false;
    setGenError(null);
    setBusy(true);
    try {
      const result = await ipc.renderAiGenerate(request);
      if (!alive.current || abandoned.current) return;
      onGenerated(result.record);
      setCompareId(result.record.id);
      setCompareMode("compare");
      setDivider(50);
    } catch (e) {
      if (!alive.current || abandoned.current) return;
      const err = toIpcError(e);
      setGenError(
        err.code === "unknown_command"
          ? "This build's backend has no AI rendering yet."
          : err.message || "The image provider did not return an image.",
      );
    } finally {
      if (alive.current && !abandoned.current) setBusy(false);
    }
  }, [source, busy, activeStyle, config, onGenerated, setCompareId, setCompareMode, setDivider]);

  const cancel = () => {
    abandoned.current = true;
    setBusy(false);
  };

  const pick = (r: RenderRecord) => {
    if (r.source === "ai_visualization") {
      setCompareId(r.id);
      setCompareMode("compare");
      if (r.source_render_id) setSourceId(r.source_render_id);
    } else {
      setSourceId(r.id);
      setCompareId(r.id);
    }
  };

  const disabledReason = !backendReady
    ? (settingsError ?? "AI rendering is not available in this build yet.")
    : !hasKey
      ? "Add a Google AI Studio key to generate visualizations."
      : !source
        ? "Capture a 3D view first, then generate from it."
        : null;

  return (
    <div className={cx(s.studio, variant === "full" && s.studioFull)} data-testid="render-studio" data-variant={variant}>
      <div className={s.studioGrid}>
        <div className={s.configColumn}>
          <section className={s.block}>
            <h4 className={s.blockTitle}>Source</h4>
            <div className={s.sourceRow}>
              <div className={s.sourceThumb}>
                {source && thumbs[source.id] ? (
                  <img src={thumbs[source.id]} alt={`Model view: ${source.camera?.name ?? "view"}`} />
                ) : (
                  <span className={s.cardWait}>{source ? "Loading" : "No capture yet"}</span>
                )}
              </div>
              <div className={s.sourceMeta}>
                {captures.length > 0 ? (
                  <Select
                    label="Source capture"
                    value={source?.id ?? ""}
                    options={captures.map((r) => ({
                      value: r.id,
                      label: `${r.camera?.name || "View"} - rev ${r.revision}`,
                    }))}
                    onChange={(id) => {
                      setSourceId(id);
                      setCompareId(id);
                    }}
                  />
                ) : (
                  <p className={s.hint}>No captures yet. Set up the 3D view, then capture it.</p>
                )}
                <Button size="sm" icon="camera" disabled={!canCapture || capturing} onClick={onCapture} data-testid="studio-capture">
                  {capturing ? "Capturing" : "Capture current view"}
                </Button>
                {source && source.revision < revision ? (
                  <span className={s.staleNote}>The model changed since this capture.</span>
                ) : null}
              </div>
            </div>
          </section>

          <section className={s.block}>
            <h4 className={s.blockTitle}>Configuration</h4>

            {!backendReady || !hasKey ? (
              <div className={s.noKey} data-testid="render-no-key">
                <p>
                  {backendReady
                    ? "AI visualizations come from Google's Gemini image models with your own Google AI Studio key. The key stays on this computer."
                    : (settingsError ?? "This build's backend has no AI rendering yet. Captures and comparing still work.")}
                </p>
                {backendReady ? (
                  <Button size="sm" variant="primary" onClick={onAddKey} data-testid="add-google-key">
                    Add Google AI key
                  </Button>
                ) : null}
              </div>
            ) : null}

            <label className={s.field}>
              <span className={s.fieldLabel}>Visual style</span>
              <Select
                label="Visual style"
                value={activeStyle?.key ?? ""}
                options={styleOptions.length > 0 ? styleOptions : [{ value: "", label: "No styles" }]}
                onChange={(key) => patchConfig({ styleKey: key })}
              />
            </label>
            {activeStyle ? <p className={s.styleDesc} data-testid="style-desc">{activeStyle.description}</p> : null}

            <label className={s.field}>
              <span className={s.fieldLabel}>Building type</span>
              <Select
                label="Building type"
                value={config.buildingType}
                options={BUILDING_TYPES}
                onChange={(value) => patchConfig({ buildingType: value })}
              />
            </label>

            <label className={s.field}>
              <span className={s.fieldLabel}>Extras</span>
              <textarea
                className={s.textarea}
                rows={variant === "full" ? 3 : 2}
                value={config.extras}
                data-testid="extras"
                placeholder="Materials, time of day, landscaping"
                onChange={(e) => patchConfig({ extras: e.target.value })}
                onKeyDown={(e) => e.stopPropagation()}
              />
            </label>

            <div className={s.field}>
              <span className={s.fieldLabel}>Quality</span>
              <Segmented
                label="Quality"
                stretch
                value={config.quality}
                options={QUALITIES.map((q) => ({ value: q.value, label: q.label, tip: q.note }))}
                onChange={(value) => patchConfig({ quality: value })}
              />
            </div>
            {cost ? (
              <p className={s.hint} data-testid="cost-hint">
                {QUALITIES.find((q) => q.value === config.quality)?.note}. Cost: {cost}.
              </p>
            ) : null}

            <CheckRow
              checked={config.keepGeometry}
              onChange={(next) => patchConfig({ keepGeometry: next })}
              hint="Holds the walls, openings and camera exactly as modelled."
            >
              Keep geometry
            </CheckRow>

            {busy ? (
              <div className={s.progress} data-testid="render-progress" role="status">
                <div className={s.progressBar} aria-hidden>
                  <span className={s.progressFill} />
                </div>
                <div className={s.progressRow}>
                  <span>Rendering, {elapsedLabel(elapsed)} elapsed</span>
                  <button
                    type="button"
                    className={s.linkBtn}
                    data-testid="render-cancel"
                    title="Stops the app waiting. The provider still finishes the image and may still charge for it."
                    onClick={cancel}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            ) : (
              <Button
                variant="primary"
                icon="visuals"
                className={s.generate}
                disabled={disabledReason !== null}
                title={disabledReason ?? undefined}
                data-testid="render-generate"
                onClick={() => void generate()}
              >
                {cost && hasKey ? `Render with AI (${cost})` : "Render with AI"}
              </Button>
            )}

            {genError ? (
              <p className={s.error} role="alert" data-testid="render-gen-error">
                {genError}
              </p>
            ) : null}
          </section>
        </div>

        <div className={s.previewColumn}>
          <div className={s.previewHead}>
            <Segmented
              label="Compare mode"
              value={compareMode}
              options={[
                { value: "compare" as const, label: "Compare" },
                { value: "result" as const, label: "Result only" },
              ]}
              onChange={(m) => setCompareMode(m)}
            />
            <div className={s.previewActions}>
              {variant === "dock" && onOpenFull ? (
                <Button size="sm" icon="fit" onClick={onOpenFull} data-testid="open-studio">
                  Open studio
                </Button>
              ) : null}
              {ACTIONS.map((a) => {
                const run = () => {
                  if (!currentRecord) return;
                  if (a.id === "enlarge") onEnlarge(currentRecord.id);
                  else if (a.id === "download") onDownload(currentRecord);
                  else if (a.id === "goto-camera") onGoToCamera(currentRecord);
                  else onDelete(currentRecord);
                };
                // The dock is narrow, so there the actions are icons with tooltips.
                return variant === "dock" ? (
                  <IconButton
                    key={a.id}
                    icon={a.icon}
                    label={a.label}
                    tipSide="top"
                    size={15}
                    disabled={!currentRecord}
                    data-testid={a.id}
                    onClick={run}
                  />
                ) : (
                  <Button
                    key={a.id}
                    size="sm"
                    icon={a.icon}
                    variant={a.id === "delete" ? "danger" : "default"}
                    disabled={!currentRecord}
                    data-testid={a.id}
                    onClick={run}
                  >
                    {a.label}
                  </Button>
                );
              })}
              {variant === "full" && onCloseFull ? (
                <Button size="sm" icon="close" onClick={onCloseFull} data-testid="close-studio">
                  Close
                </Button>
              ) : null}
            </div>
          </div>

          {beforeSrc ? (
            <CompareSlider
              beforeSrc={beforeSrc}
              afterSrc={afterSrc}
              value={divider}
              onChange={setDivider}
              mode={compareMode}
              size={variant}
              testId={variant === "full" ? "compare-slider-studio" : "compare-slider"}
              onResolution={(w, h) => setResolution(formatResolution(w, h))}
            />
          ) : (
            <div className={s.previewEmpty} data-testid="preview-empty">
              {captures.length === 0
                ? "Capture the 3D view to start. The capture is the left half of every comparison."
                : "Loading the capture"}
            </div>
          )}

          <div className={s.badges} data-testid="render-badges">
            {resolution ? <span className={s.badge}>Result {resolution}</span> : null}
            <span className={s.badge}>Geometry: as modelled</span>
            {afterRecord?.provider ? <span className={s.badge}>{providerLabel(afterRecord.provider)}</span> : null}
            {afterRecord ? <span className={cx(s.badge, s.badgeAi)}>AI visualization</span> : null}
          </div>
          <p className={s.synthid}>
            AI images carry an invisible SynthID watermark from Google. They are visualizations for discussion, not
            construction documents, and they never change the model.
          </p>
        </div>
      </div>

      <section className={s.block}>
        <div className={s.blockHead}>
          <h4 className={s.blockTitle}>History</h4>
          <span className={s.count}>{records.length}</span>
        </div>
        {records.length === 0 ? (
          <p className={s.hint}>Nothing saved yet. Every capture and every visualization lands here.</p>
        ) : (
          <GalleryStrip
            records={records}
            thumbs={thumbs}
            selectedId={compareId}
            sourceId={source?.id ?? null}
            revision={revision}
            onPick={pick}
            onDelete={onDelete}
            size={variant}
          />
        )}
      </section>
    </div>
  );
}
