// The render studio: pick a capture, set the look, generate an AI
// visualization, compare it with the model view side by side.
//
// Geometry is the authority and the image is derived from it (AGENTS.md):
// nothing here writes back into the model, and every AI image keeps its
// "AI visualization" badge.
//
// AI render v2 (lane L6, DECISIONS D53): the price of every call shows
// before it runs; the capture's maps (lines, depth) and the project look go
// with it; the drift check compares the image's edges with the model's
// lines, and an image it flags waits here, unsaved, until the architect
// keeps or discards it. The agents' daily cap shows and can be raised.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { MapKind, RenderAiRequest, RenderAiSettings, RenderRecord, RenderStyle } from "../../contract/bindings";
import { toIpcError } from "../../contract/ipc";
import { useApp } from "../../state/store";
import { Button, CheckRow, IconButton, Segmented, Select, cx } from "../../ui/controls";
import type { IconName } from "../../ui/icons";
import { renderWithAi, type FlowPhase } from "./aiFlow";
import { aiRender, driftState, estimateLine, type AgentUsage, type Draft, type Estimate } from "./aiRender";
import { AgentCap, DriftCard, LookBlock, MAP_LABELS, MapsStrip, PriceLine, useMapImage } from "./AiRenderParts";
import { CompareSlider } from "./CompareSlider";
import { saveMapsFor } from "./conditioning";
import { GalleryStrip } from "./GalleryStrip";
import { NO_AI_RENDER, captureLabel, savedLabel } from "./renderMeta";
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

const PHASE_TEXT: Record<FlowPhase, string> = {
  pricing: "Pricing it",
  maps: "Drawing the model's lines and depth",
  rendering: "Rendering",
};

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
  /** Records changed in place (maps drawn, the look moved): load them again. */
  onReload?: () => void;
  /** Plays a tour video in the system player (desktop only). */
  onOpenVideo?: (record: RenderRecord) => void;
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
  onReload,
  onOpenVideo,
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

  const [phase, setPhase] = useState<FlowPhase | null>(null);
  const busy = phase !== null;
  const [elapsed, setElapsed] = useState(0);
  const [genError, setGenError] = useState<string | null>(null);
  const [resolution, setResolution] = useState("");
  const [estimate, setEstimate] = useState<Estimate | null>(null);
  const [estimating, setEstimating] = useState(false);
  const [useLook, setUseLook] = useState(true);
  const [held, setHeld] = useState<Draft | null>(null);
  const [holding, setHolding] = useState(false);
  const [showDrift, setShowDrift] = useState(false);
  const [keptOverlay, setKeptOverlay] = useState("");
  const [mapPreview, setMapPreview] = useState<MapKind | null>(null);
  const [drawingMaps, setDrawingMaps] = useState(false);
  const [usage, setUsage] = useState<AgentUsage | null>(null);
  const [capBusy, setCapBusy] = useState(false);
  const abandoned = useRef(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const captures = records.filter((r) => r.source === "model_view" && !r.video);
  const source = captures.find((r) => r.id === sourceId) ?? captures[0] ?? null;
  const look = records.find((r) => r.look) ?? null;

  // Keep the studio pointing at a capture that still exists.
  useEffect(() => {
    if (source && source.id !== sourceId) setSourceId(source.id);
    if (!source && sourceId) setSourceId(null);
  }, [source, sourceId, setSourceId]);

  const shown = records.find((r) => r.id === compareId) ?? null;
  const shownIsAi = shown?.source === "ai_visualization";
  const beforeRecord = held
    ? source
    : shownIsAi
      ? (records.find((r) => r.id === shown?.source_render_id) ?? null)
      : (shown ?? source);
  const afterRecord = held ? null : shownIsAi ? shown : null;
  const mapSrc = useMapImage(beforeRecord, mapPreview);
  const beforeSrc = mapPreview && mapSrc ? mapSrc : beforeRecord ? (thumbs[beforeRecord.id] ?? "") : "";
  const afterSrc = held ? held.image : afterRecord ? (thumbs[afterRecord.id] ?? "") : "";
  const currentRecord = afterRecord ?? beforeRecord;
  const afterDrift = afterRecord?.drift ?? null;

  // The overlay of a kept image's drift check, loaded when it is asked for.
  useEffect(() => {
    setKeptOverlay("");
    if (!afterRecord?.drift || held) return;
    let on = true;
    aiRender
      .driftOverlay(afterRecord.id)
      .then((url) => on && setKeptOverlay(url ?? ""))
      .catch(() => {});
    return () => {
      on = false;
    };
  }, [afterRecord?.id, afterRecord?.drift, held]);

  // A map preview belongs to the capture it came from.
  useEffect(() => setMapPreview(null), [beforeRecord?.id]);

  const styleOptions = renderStyles.map((st) => ({ value: st.key, label: st.name }));
  const activeStyle = renderStyles.find((st) => st.key === config.styleKey) ?? renderStyles[0] ?? null;

  const backendReady = settings !== null;
  const hasKey = settings?.has_api_key ?? false;
  const fallbackCost = costForQuality(settings, config.quality);

  const request: RenderAiRequest | null = useMemo(
    () =>
      source
        ? {
            source_render_id: source.id,
            style_key: activeStyle?.key ?? null,
            prompt: buildPrompt(config.buildingType, config.extras),
            quality: config.quality,
            keep_geometry: config.keepGeometry,
            // None is the project look; "none" leaves it out of this one render.
            ...(look && !useLook ? { style_reference_id: "none" } : {}),
          }
        : null,
    [source, activeStyle, config, look, useLook],
  );
  const requestKey = request ? JSON.stringify(request) : "";
  const mapsKey = `${source?.maps?.length ?? 0}|${look?.id ?? ""}`;

  // The price of the render as set up, before anything is spent. Follows
  // every change to the request, a moment after it settles.
  useEffect(() => {
    if (!request || !backendReady) {
      setEstimate(null);
      return;
    }
    let on = true;
    setEstimating(true);
    const t = window.setTimeout(() => {
      aiRender
        .estimate(request)
        .then((e) => {
          if (!on) return;
          setEstimate(e);
          setUsage(e.agents);
        })
        .catch(() => on && setEstimate(null))
        .finally(() => on && setEstimating(false));
    }, 160);
    return () => {
      on = false;
      window.clearTimeout(t);
    };
    // requestKey stands for request; mapsKey for what the estimate lists.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requestKey, mapsKey, backendReady]);

  useEffect(() => {
    if (!backendReady) return;
    aiRender
      .usage()
      .then((u) => alive.current && setUsage(u))
      .catch(() => {});
  }, [backendReady, revision]);

  // Elapsed seconds while the provider works.
  useEffect(() => {
    if (phase !== "rendering") return;
    const started = performance.now();
    setElapsed(0);
    const id = window.setInterval(() => setElapsed(performance.now() - started), 250);
    return () => window.clearInterval(id);
  }, [phase]);

  const drawMaps = useCallback(async () => {
    if (!source) return;
    setDrawingMaps(true);
    try {
      await saveMapsFor(source);
      onReload?.();
    } finally {
      if (alive.current) setDrawingMaps(false);
    }
  }, [source, onReload]);

  const showResult = useCallback(
    (record: RenderRecord) => {
      onGenerated(record);
      setCompareId(record.id);
      setCompareMode("compare");
      setDivider(50);
    },
    [onGenerated, setCompareId, setCompareMode, setDivider],
  );

  const generate = useCallback(async () => {
    if (!request || busy) return;
    abandoned.current = false;
    setGenError(null);
    setHeld(null);
    setMapPreview(null);
    try {
      const result = await renderWithAi(
        request,
        {
          estimate: aiRender.estimate,
          drawMaps,
          draft: aiRender.draft,
          keep: aiRender.keep,
        },
        {
          onPrice: (e) => alive.current && setEstimate(e),
          onPhase: (p) => alive.current && setPhase(p),
        },
      );
      if (!alive.current || abandoned.current) return;
      if (result.kind === "held") {
        setHeld(result.draft);
        setShowDrift(true);
        setCompareMode("compare");
        setDivider(50);
      } else {
        showResult(result.record);
      }
    } catch (e) {
      if (!alive.current || abandoned.current) return;
      const err = toIpcError(e);
      setGenError(err.code === "unknown_command" ? NO_AI_RENDER : err.message || "The image provider did not return an image.");
    } finally {
      if (alive.current && !abandoned.current) setPhase(null);
    }
  }, [request, busy, drawMaps, showResult, setCompareMode, setDivider]);

  const keepHeld = useCallback(async () => {
    if (!held) return;
    setHolding(true);
    try {
      const record = await aiRender.keep(held.pending_id);
      if (!alive.current) return;
      setHeld(null);
      showResult(record);
    } catch (e) {
      useApp.getState().reportError(e);
    } finally {
      if (alive.current) setHolding(false);
    }
  }, [held, showResult]);

  const discardHeld = useCallback(async () => {
    if (!held) return;
    setHolding(true);
    try {
      await aiRender.discard(held.pending_id);
    } catch {
      // Gone already: drafts expire. Nothing was saved either way.
    } finally {
      if (alive.current) {
        setHeld(null);
        setHolding(false);
        useApp.getState().toast("info", "Discarded. Nothing was saved to Visuals.");
      }
    }
  }, [held]);

  const setLook = useCallback(
    async (id: string | null) => {
      try {
        await aiRender.setLook(id);
        onReload?.();
        useApp.getState().toast("success", id ? "Set as the project look. Later renders follow it." : "The project look is cleared.");
      } catch (e) {
        useApp.getState().reportError(e);
      }
    },
    [onReload],
  );

  const setCap = useCallback(async (cap: number) => {
    setCapBusy(true);
    try {
      const u = await aiRender.setCap(cap);
      if (alive.current) setUsage(u);
    } catch (e) {
      useApp.getState().reportError(e);
    } finally {
      if (alive.current) setCapBusy(false);
    }
  }, []);

  const cancel = () => {
    abandoned.current = true;
    setPhase(null);
  };

  const pick = (r: RenderRecord) => {
    setHeld(null);
    if (r.video) {
      // A tour video shows its poster; it is never the source of an AI image.
      setCompareId(r.id);
    } else if (r.source === "ai_visualization") {
      setCompareId(r.id);
      setCompareMode("compare");
      if (r.source_render_id) setSourceId(r.source_render_id);
    } else {
      setSourceId(r.id);
      setCompareId(r.id);
    }
  };

  const disabledReason = !backendReady
    ? (settingsError ?? NO_AI_RENDER)
    : !hasKey
      ? "Add a Google AI Studio key to generate visualizations."
      : !source
        ? "Capture a 3D view first, then generate from it."
        : held
          ? "Keep or discard the image above first."
          : null;

  const price = estimate ? estimateLine(estimate) : fallbackCost;
  const lookThumb = look ? (thumbs[look.id] ?? "") : "";
  const canBeLook = !!currentRecord && !currentRecord.video;
  const isLook = !!currentRecord?.look;

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
                  // A long view name ends in an ellipsis; the tip has it whole.
                  <span
                    className={s.sourceSelect}
                    data-tip={source ? `${source.camera?.name || "View"}, saved ${savedLabel(source.created_at)}` : ""}
                    data-tip-side="bottom-start"
                    data-testid="source-select"
                  >
                    <Select
                      label="Source capture"
                      value={source?.id ?? ""}
                      options={captures.map((r) => ({ value: r.id, label: captureLabel(r.camera?.name ?? "", r.created_at) }))}
                      onChange={(id) => {
                        setHeld(null);
                        setSourceId(id);
                        setCompareId(id);
                      }}
                    />
                  </span>
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
            <MapsStrip
              record={source}
              drawing={drawingMaps || phase === "maps"}
              preview={mapPreview}
              onPreview={setMapPreview}
              onDraw={() => void drawMaps().catch((e) => useApp.getState().reportError(e))}
            />
          </section>

          <LookBlock look={look} thumb={lookThumb} use={useLook} onUse={setUseLook} onClear={() => void setLook(null)} />

          <section className={s.block}>
            <h4 className={s.blockTitle}>Configuration</h4>

            {!backendReady || !hasKey ? (
              <div className={s.noKey} data-testid="render-no-key">
                <p>
                  {backendReady
                    ? "AI visualizations come from Google's Gemini image models with your own Google AI Studio key. The key stays on this computer."
                    : (settingsError ?? NO_AI_RENDER)}
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
            <PriceLine
              estimate={estimate}
              loading={estimating}
              fallback={fallbackCost ? `${QUALITIES.find((q) => q.value === config.quality)?.note}. Cost: ${fallbackCost}.` : ""}
            />

            <CheckRow
              checked={config.keepGeometry}
              onChange={(next) => patchConfig({ keepGeometry: next })}
              hint="Holds the walls, openings and camera exactly as modeled."
            >
              Keep geometry
            </CheckRow>

            {busy ? (
              <div className={s.progress} data-testid="render-progress" role="status">
                <div className={s.progressBar} aria-hidden>
                  <span className={s.progressFill} />
                </div>
                <div className={s.progressRow}>
                  <span>
                    {phase === "rendering" ? `Rendering, ${elapsedLabel(elapsed)} elapsed` : PHASE_TEXT[phase ?? "pricing"]}
                  </span>
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
                {price && hasKey ? `Render with AI (${estimate ? `about $${estimate.price_usd}` : price})` : "Render with AI"}
              </Button>
            )}

            {genError ? (
              <p className={s.error} role="alert" data-testid="render-gen-error">
                {genError}
              </p>
            ) : null}
          </section>

          <AgentCap usage={usage} onSet={(cap) => void setCap(cap)} busy={capBusy} />
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
              {currentRecord?.video && onOpenVideo ? (
                <Button size="sm" icon="render" onClick={() => onOpenVideo(currentRecord)} data-testid="open-video">
                  {currentRecord.video.codec === "png" ? "Show frames" : "Open video"}
                </Button>
              ) : null}
              {variant === "dock" ? (
                <IconButton
                  icon="pin"
                  label={isLook ? "Clear the project look" : "Set as project look"}
                  tipSide="top"
                  size={15}
                  active={isLook}
                  disabled={!canBeLook || !!held}
                  data-testid="set-look"
                  onClick={() => currentRecord && void setLook(isLook ? null : currentRecord.id)}
                />
              ) : (
                <Button
                  size="sm"
                  icon="pin"
                  disabled={!canBeLook || !!held}
                  data-testid="set-look"
                  onClick={() => currentRecord && void setLook(isLook ? null : currentRecord.id)}
                >
                  {isLook ? "Clear look" : "Set as project look"}
                </Button>
              )}
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
                    disabled={!currentRecord || !!held}
                    data-testid={a.id}
                    onClick={run}
                  />
                ) : (
                  <Button
                    key={a.id}
                    size="sm"
                    icon={a.icon}
                    variant={a.id === "delete" ? "danger" : "default"}
                    disabled={!currentRecord || !!held}
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

          <DriftCard
            draft={held}
            showOverlay={showDrift}
            onToggleOverlay={() => setShowDrift((v) => !v)}
            onKeep={() => void keepHeld()}
            onDiscard={() => void discardHeld()}
            busy={holding}
          />

          {beforeSrc ? (
            <CompareSlider
              beforeSrc={beforeSrc}
              afterSrc={afterSrc}
              beforeLabel={mapPreview ? `${MAP_LABELS[mapPreview]} map` : "Model view"}
              afterLabel={held ? "Not saved yet" : "AI visualization"}
              value={divider}
              onChange={setDivider}
              mode={compareMode}
              size={variant}
              testId={variant === "full" ? "compare-slider-studio" : "compare-slider"}
              onResolution={(w, h) => setResolution(formatResolution(w, h))}
              overlaySrc={held ? (held.overlay ?? "") : keptOverlay}
              overlayOn={showDrift}
            />
          ) : (
            <div className={s.previewEmpty} data-testid="preview-empty">
              {captures.length === 0
                ? "Capture the 3D view to start. The capture is the left half of every comparison."
                : "Loading the capture"}
            </div>
          )}

          {/* One quiet meta line. The image itself carries the one "AI
              visualization" badge (CompareSlider). */}
          <div className={s.badges} data-testid="render-badges">
            {resolution ? <span className={s.badge}>Result {resolution}</span> : null}
            <span className={s.badge}>Geometry: as modeled</span>
            {afterRecord?.provider ? <span className={s.badge}>{providerLabel(afterRecord.provider)}</span> : null}
            {currentRecord?.look ? <span className={cx(s.badge, s.badgeLook)}>Project look</span> : null}
            {afterDrift ? (
              driftState(afterRecord) === "drift" ? (
                <button
                  type="button"
                  className={cx(s.badge, s.badgeDrift)}
                  title={afterDrift.note}
                  data-testid="badge-drift"
                  onClick={() => setShowDrift((v) => !v)}
                >
                  May not match the model: {Math.round(afterDrift.missing * 100)}% of edges unmatched
                </button>
              ) : (
                <span className={cx(s.badge, s.badgeMatch)} title={afterDrift.note} data-testid="badge-match">
                  Lines match: {100 - Math.round(afterDrift.missing * 100)}%
                </span>
              )
            ) : null}
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
            onOpenVideo={onOpenVideo}
            size={variant}
          />
        )}
      </section>
    </div>
  );
}
