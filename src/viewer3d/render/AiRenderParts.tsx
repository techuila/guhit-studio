// The AI render v2 pieces of the render studio (lane L6, DECISIONS D53):
// the price line, the conditioning maps, the project look, the drift
// warning and the agents' daily cap. Each one animates with the motion
// tokens (docs/MOTION.md): the price bumps when it changes, the maps rise in
// together as one block, the warning rises from below and leaves faster, the
// cap meter fills along its track.

import { useEffect, useState, type CSSProperties } from "react";
import type { MapKind, RenderRecord } from "../../contract/bindings";
import { Button, Select, Switch, cx } from "../../ui/controls";
import { Icon } from "../../ui/icons";
import { usePresence } from "../../ui/motion";
import { aiRender, CAP_STEPS, capLine, capShare, priceLabel, type AgentUsage, type Draft, type Estimate } from "./aiRender";
import s from "./render.module.css";

// ------------------------------------------------------------------ price

export function PriceLine({ estimate, loading, fallback }: { estimate: Estimate | null; loading: boolean; fallback: string }) {
  if (!estimate) {
    return fallback ? (
      <p className={s.hint} data-testid="cost-hint">
        {fallback}
      </p>
    ) : null;
  }
  return (
    <div className={s.price} data-pro={estimate.pro || undefined} data-loading={loading || undefined} data-testid="ai-price">
      {/* The key remounts the amount, so a new price bumps in. */}
      <span key={`${estimate.price_usd}-${estimate.model}`} className={s.priceAmount}>
        about {priceLabel(estimate)}
      </span>
      <span className={s.priceModel}>
        {estimate.model_label}, {estimate.image_size}
      </span>
      {estimate.pro ? (
        <span className={s.proPill} title={`Gemini Pro because ${estimate.why}`}>
          Pro: {estimate.why}
        </span>
      ) : null}
      <span className={s.priceNote}>Google list price per image, charged to your key. Shown before every call.</span>
    </div>
  );
}

// ------------------------------------------------------------------- maps

export const MAP_LABELS: Record<MapKind, string> = {
  lines: "Lines",
  depth: "Depth",
  normal: "Normals",
  ids: "Parts",
  clay: "Clay",
};

const MAP_ORDER: MapKind[] = ["lines", "depth", "normal", "ids", "clay"];
const mapCache = new Map<string, string>();

/** A map's image, loaded once. */
export function useMapImage(record: RenderRecord | null, kind: MapKind | null): string {
  const key = record && kind ? `${record.id}:${kind}` : "";
  const has = !!record && !!kind && (record.maps ?? []).some((m) => m.kind === kind);
  const [src, setSrc] = useState(() => mapCache.get(key) ?? "");
  useEffect(() => {
    if (!record || !kind || !has) {
      setSrc("");
      return;
    }
    const cached = mapCache.get(key);
    if (cached) {
      setSrc(cached);
      return;
    }
    let alive = true;
    aiRender
      .mapData(record.id, kind)
      .then((url) => {
        mapCache.set(key, url);
        if (alive) setSrc(url);
      })
      .catch(() => alive && setSrc(""));
    return () => {
      alive = false;
    };
  }, [key, has, record, kind]);
  return src;
}

function MapChip({ record, kind, active, onPick }: { record: RenderRecord; kind: MapKind; active: boolean; onPick: () => void }) {
  const src = useMapImage(record, kind);
  return (
    <button
      type="button"
      className={s.mapChip}
      aria-pressed={active}
      data-active={active || undefined}
      data-testid={`map-${kind}`}
      title={active ? "Back to the model view" : `Compare the ${MAP_LABELS[kind].toLowerCase()} map with the AI image`}
      onClick={onPick}
    >
      <span className={s.mapThumb}>{src ? <img src={src} alt="" /> : <span className={s.mapWait} />}</span>
      <span className={s.mapLabel}>{MAP_LABELS[kind]}</span>
    </button>
  );
}

/**
 * The source's conditioning maps: what goes to the image model with the
 * capture. A chip swaps the map in for the model view in the comparison.
 */
export function MapsStrip({
  record,
  drawing,
  preview,
  onPreview,
  onDraw,
}: {
  record: RenderRecord | null;
  drawing: boolean;
  preview: MapKind | null;
  onPreview: (kind: MapKind | null) => void;
  onDraw: () => void;
}) {
  if (!record) return null;
  const have = MAP_ORDER.filter((k) => (record.maps ?? []).some((m) => m.kind === k));
  return (
    <div className={s.maps} data-testid="maps-strip" data-drawing={drawing || undefined}>
      <div className={s.mapsHead}>
        <span className={s.fieldLabel}>Model maps</span>
        <span className={s.mapsNote}>
          {drawing
            ? "Drawing the model's lines and depth"
            : have.length > 0
              ? "Lines and depth go with the capture; the AI must follow them."
              : "None yet. They go with the capture so the AI follows the model."}
        </span>
      </div>
      {have.length > 0 ? (
        <div className={s.mapRow} key={record.id}>
          {have.map((k) => (
            <MapChip key={k} record={record} kind={k} active={preview === k} onPick={() => onPreview(preview === k ? null : k)} />
          ))}
        </div>
      ) : drawing ? (
        <div className={s.mapRow} aria-busy>
          {MAP_ORDER.map((k) => (
            <span key={k} className={cx(s.mapChip, s.mapGhost)}>
              <span className={s.mapThumb}>
                <span className={s.mapWait} />
              </span>
              <span className={s.mapLabel}>{MAP_LABELS[k]}</span>
            </span>
          ))}
        </div>
      ) : (
        <Button size="sm" icon="linework" onClick={onDraw} data-testid="draw-maps">
          Draw maps
        </Button>
      )}
    </div>
  );
}

// ------------------------------------------------------------------- look

/** The project look: one approved image every later AI render follows. */
export function LookBlock({
  look,
  thumb,
  use,
  onUse,
  onClear,
}: {
  look: RenderRecord | null;
  thumb: string;
  use: boolean;
  onUse: (next: boolean) => void;
  onClear: () => void;
}) {
  return (
    <section className={s.block} data-testid="look-block">
      <div className={s.blockHead}>
        <h4 className={s.blockTitle}>Project look</h4>
        {look ? <span className={s.lookPill}>Set</span> : null}
      </div>
      {look ? (
        <div className={s.lookRow}>
          <span className={s.lookThumb}>{thumb ? <img src={thumb} alt="The project look" /> : null}</span>
          <div className={s.lookMeta}>
            <label className={s.lookSwitch}>
              <Switch checked={use} onChange={onUse} label="Use the project look" />
              <span>{use ? "Renders follow it" : "Not for this render"}</span>
            </label>
            <span className={s.hint}>Its materials, light and mood, never its shapes. Renders with it use Gemini Pro.</span>
            <button type="button" className={s.linkBtn} onClick={onClear} data-testid="look-clear">
              Clear the look
            </button>
          </div>
        </div>
      ) : (
        <p className={s.hint}>Pick an AI image you like and set it as the project look: later renders match its materials and light.</p>
      )}
    </section>
  );
}

// ------------------------------------------------------------------ drift

/** The warning before a flagged image is saved: keep it anyway, or discard it. */
export function DriftCard({
  draft,
  showOverlay,
  onToggleOverlay,
  onKeep,
  onDiscard,
  busy,
}: {
  draft: Draft | null;
  showOverlay: boolean;
  onToggleOverlay: () => void;
  onKeep: () => void;
  onDiscard: () => void;
  busy: boolean;
}) {
  const p = usePresence(!!draft, "panel");
  const [last, setLast] = useState<Draft | null>(draft);
  useEffect(() => {
    if (draft) setLast(draft);
  }, [draft]);
  const shown = draft ?? last;
  if (!p.mounted || !shown?.drift) return null;
  const missing = Math.round(shown.drift.missing * 100);
  return (
    <div className={s.drift} data-stage={p.stage} role="alert" data-testid="drift-warning">
      <div className={s.driftHead}>
        <Icon name="warning" size={16} />
        <strong>This image may not show the design</strong>
      </div>
      <p className={s.driftNote}>{shown.drift.note}</p>
      <div className={s.driftMeter} aria-label={`${missing}% of the model's edges have no match`}>
        <span style={{ "--share": shown.drift.missing } as CSSProperties} />
      </div>
      <div className={s.driftActions}>
        <Button size="sm" icon={showOverlay ? "eyeOff" : "eye"} onClick={onToggleOverlay} data-testid="drift-show">
          {showOverlay ? "Hide what moved" : "Show what moved"}
        </Button>
        <span className={s.driftSpacer} />
        <Button size="sm" variant="ghost" onClick={onDiscard} disabled={busy} data-testid="drift-discard">
          Discard
        </Button>
        <Button size="sm" variant="primary" onClick={onKeep} disabled={busy} data-testid="drift-keep">
          Keep anyway
        </Button>
      </div>
    </div>
  );
}

// ------------------------------------------------------------- agents' cap

/** How many AI images agents may make in this project today, and the raise. */
export function AgentCap({ usage, onSet, busy }: { usage: AgentUsage | null; onSet: (cap: number) => void; busy: boolean }) {
  if (!usage) return null;
  const steps = [...new Set<number>([...CAP_STEPS, usage.cap])].sort((a, b) => a - b);
  const share = capShare(usage);
  const full = usage.used >= usage.cap;
  return (
    <section className={s.block} data-testid="agent-cap" data-full={full || undefined}>
      <div className={s.blockHead}>
        <h4 className={s.blockTitle}>AI images per day for agents</h4>
        <span className={s.count}>
          {usage.used} / {usage.cap}
        </span>
      </div>
      <div className={s.capMeter} aria-hidden>
        <span style={{ "--share": share } as CSSProperties} />
      </div>
      <div className={s.capRow}>
        <span className={cx(s.capLine, full && s.capFull)} key={`${usage.used}-${usage.cap}`}>
          {capLine(usage)}
        </span>
        <Select
          label="AI images per day for agents"
          value={String(usage.cap)}
          disabled={busy}
          options={steps.map((n) => ({
            value: String(n),
            label: n === usage.default_cap ? `${n} a day (default)` : n === 0 ? "None" : `${n} a day`,
          }))}
          onChange={(v) => onSet(Number(v))}
        />
      </div>
      <p className={s.hint}>
        Each one costs you money. Agents are refused past the limit and told to ask you; your own renders are never limited.
      </p>
    </section>
  );
}
