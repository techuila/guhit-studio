// Full screen view of one visual, with the same compare slider. Escape closes.
// The backdrop fades in and out through usePresence so nothing pops away.

import { useEffect } from "react";
import { createPortal } from "react-dom";
import type { RenderRecord } from "../../contract/bindings";
import { usePresence } from "../../ui/motion";
import { CompareSlider } from "./CompareSlider";
import { savedLabel } from "./renderMeta";
import { providerLabel, useRenderUi } from "./renderStore";
import s from "./render.module.css";

export interface RenderLightboxProps {
  open: boolean;
  /** The image on the right of the divider, or the capture when it stands alone. */
  record: RenderRecord | null;
  beforeSrc: string;
  afterSrc: string;
  resolution: string;
  onClose: () => void;
  onDownload: (record: RenderRecord) => void;
  onGoToCamera: (record: RenderRecord) => void;
}

export function RenderLightbox({
  open,
  record,
  beforeSrc,
  afterSrc,
  resolution,
  onClose,
  onDownload,
  onGoToCamera,
}: RenderLightboxProps) {
  const presence = usePresence(open, "base");
  const divider = useRenderUi((st) => st.divider);
  const setDivider = useRenderUi((st) => st.setDivider);
  const compareMode = useRenderUi((st) => st.compareMode);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, onClose]);

  if (!presence.mounted || !record || !beforeSrc) return null;
  const ai = record.source === "ai_visualization";

  return createPortal(
    <div className={s.lightbox} data-stage={presence.stage} data-testid="render-lightbox" role="dialog" aria-label="Visual, full screen">
      <div className={s.lightboxStage}>
        <CompareSlider
          beforeSrc={beforeSrc}
          afterSrc={afterSrc}
          value={divider}
          onChange={setDivider}
          mode={compareMode}
          size="screen"
          testId="compare-slider-full"
        />
      </div>
      <div className={s.lightboxBar}>
        <span className={s.lightboxMeta}>
          <strong>{ai ? "AI visualization" : "Model view"}</strong>
          <span>{record.camera?.name || "View"}</span>
          <span>{savedLabel(record.created_at)}</span>
          {resolution ? <span>{resolution}</span> : null}
          {ai && record.provider ? <span>{providerLabel(record.provider)}</span> : null}
          <span>Geometry: as modeled</span>
          {ai ? <span>Carries a SynthID watermark</span> : null}
        </span>
        <span className={s.lightboxActions}>
          <button type="button" className={s.lightBtn} onClick={() => onGoToCamera(record)}>
            Go to camera
          </button>
          <button type="button" className={s.lightBtn} onClick={() => onDownload(record)} data-testid="lightbox-download">
            Download
          </button>
          <button type="button" className={s.lightBtn} onClick={onClose} data-testid="lightbox-close">
            Close
          </button>
        </span>
      </div>
    </div>,
    document.body,
  );
}
