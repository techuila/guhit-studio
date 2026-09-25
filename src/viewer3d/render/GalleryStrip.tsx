// The history strip: every saved visual, grouped so an AI visualization sits
// next to the capture it was made from. Newest group first. Cards slide to
// their new place (FLIP) when one is added or removed.

import { useLayoutEffect, useRef, useState } from "react";
import type { RenderRecord } from "../../contract/bindings";
import { flip, play } from "../../ui/motionWaapi";
import { clockLabel, dateLabel } from "../light/sun";
import { kindLabel } from "./renderMeta";
import { formatResolution, groupRenders, type RenderGroup } from "./renderStore";
import s from "./render.module.css";

function timeLabel(rfc3339: string): string {
  const d = new Date(rfc3339);
  if (Number.isNaN(d.getTime())) return rfc3339;
  return d.toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

export interface GalleryStripProps {
  records: RenderRecord[];
  thumbs: Record<string, string>;
  /** Highlighted card. */
  selectedId: string | null;
  /** The capture the studio is working from. */
  sourceId: string | null;
  revision: number;
  onPick: (record: RenderRecord) => void;
  onDelete: (record: RenderRecord) => void;
  /** Set by the panel so the delete collapse can find the card. */
  listRef?: React.RefObject<HTMLUListElement | null>;
  size?: "dock" | "full";
}

export function GalleryStrip({
  records,
  thumbs,
  selectedId,
  sourceId,
  revision,
  onPick,
  onDelete,
  listRef,
  size = "dock",
}: GalleryStripProps) {
  const ownRef = useRef<HTMLUListElement>(null);
  const ref = listRef ?? ownRef;
  // Image sizes, read off the thumbnails as they load.
  const [sizes, setSizes] = useState<Record<string, string>>({});
  const rects = useRef(new Map<string, DOMRect>());
  const groups: RenderGroup[] = groupRenders(records);

  useLayoutEffect(() => {
    const list = ref.current;
    if (!list) return;
    const items = [...list.querySelectorAll<HTMLElement>("[data-card-id]")];
    const had = rects.current.size > 0;
    // A new card grows into the strip; the others slide to their new place.
    for (const el of items) {
      const id = el.dataset.cardId;
      if (!had || !id || rects.current.has(id)) continue;
      play(el, [{ transform: "scale(0.92)", opacity: 0 }, { transform: "none", opacity: 1 }], "panel");
    }
    rects.current = flip(items, rects.current, (el) => el.dataset.cardId);
  }, [records, ref]);

  return (
    <ul className={s.strip} data-size={size} ref={ref} data-testid="render-gallery">
      {groups.map((g) => {
        const key = g.source ? g.source.id : `orphan-${g.visualizations[0]?.id}`;
        const cards = g.source ? [g.source, ...g.visualizations] : g.visualizations;
        return (
          <li key={key} className={s.group} data-testid="render-group" data-group-id={key}>
            {cards.map((r) => {
              const ai = r.source === "ai_visualization";
              const thumb = thumbs[r.id];
              const meta = ai ? null : (r.info ?? null);
              const light = r.camera?.light ?? null;
              const size = meta?.width && meta?.height ? formatResolution(meta.width, meta.height) : sizes[r.id];
              // A render's record carries the revision it started from.
              const stale = r.revision < revision;
              return (
                <div
                  key={r.id}
                  className={s.card}
                  data-card-id={r.id}
                  data-testid="render-card"
                  data-ai={ai || undefined}
                  data-selected={r.id === selectedId || undefined}
                  data-source={r.id === sourceId || undefined}
                >
                  <button
                    type="button"
                    className={s.cardThumb}
                    onClick={() => onPick(r)}
                    title={ai ? "Open this visualization" : "Use this capture"}
                    data-testid={ai ? "card-ai" : "card-view"}
                  >
                    {thumb ? (
                      <img
                        src={thumb}
                        alt={`${ai ? "AI visualization" : "Model view"}: ${r.camera?.name ?? "view"}`}
                        onLoad={(e) => {
                          const img = e.currentTarget;
                          const label = formatResolution(img.naturalWidth, img.naturalHeight);
                          if (label && sizes[r.id] !== label) setSizes((m) => ({ ...m, [r.id]: label }));
                        }}
                      />
                    ) : (
                      <span className={s.cardWait}>{thumb === "" ? "Image missing" : "Loading"}</span>
                    )}
                    <span className={s.cardBadge} data-ai={ai || undefined} data-kind={meta?.kind}>
                      {ai ? "AI visualization" : kindLabel(meta)}
                    </span>
                  </button>
                  <div className={s.cardMeta}>
                    <span className={s.cardName} title={r.camera?.name ?? "View"}>
                      {r.camera?.name || "View"}
                    </span>
                    <span className={s.cardTime} title={`Saved ${timeLabel(r.created_at)}`} data-testid="card-light">
                      {light ? `${clockLabel(light.minutes)}, ${dateLabel(light.month, light.day)}` : timeLabel(r.created_at)}
                    </span>
                    {size ? <span className={s.cardTime}>{size}</span> : null}
                    {stale ? <span className={s.cardStale}>Model changed</span> : null}
                  </div>
                  <button
                    type="button"
                    className={s.cardDelete}
                    aria-label={`Delete ${ai ? "visualization" : "capture"}`}
                    title="Delete"
                    data-testid="card-delete"
                    onClick={() => onDelete(r)}
                  >
                    <svg viewBox="0 0 20 20" width="13" height="13" aria-hidden>
                      <path d="M5 5l10 10M15 5L5 15" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
                    </svg>
                  </button>
                </div>
              );
            })}
          </li>
        );
      })}
    </ul>
  );
}
