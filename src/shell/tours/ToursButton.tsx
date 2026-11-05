// The top bar's Tours button (lane L9): opens and closes the Tours panel.
// While a video exports it shows how far along it is.
import { IconButton } from "../../ui/controls";
import { useTours } from "../../viewer3d/tour/tourStore";
import s from "./Tours.module.css";

export function ToursButton() {
  const open = useTours((t) => t.open);
  const exporting = useTours((t) => t.exporting);
  const pct = exporting && exporting.total > 0 ? Math.round((exporting.done / exporting.total) * 100) : null;
  const label = pct !== null ? `Tours, exporting a video: ${pct}%` : "Tours";
  return (
    <span className={s.buttonWrap}>
      <IconButton icon="camera" tone="chrome" label={label} tip={label} active={open} onClick={() => useTours.getState().setOpen(!open)} data-testid="tours-button" />
      {pct !== null ? (
        <span className={s.buttonPct} aria-hidden>
          {pct}%
        </span>
      ) : null}
    </span>
  );
}
