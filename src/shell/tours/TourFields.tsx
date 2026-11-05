// Tour fields (lane L9), when a tour is selected: its name and frame rate,
// its shots with their moves and lengths, and play, edit and export. The
// timeline itself is in the Tours panel.
import type { DocState } from "../../contract/bindings";
import { Button, Field, ReadOnly, Segmented, TextField } from "../../ui/controls";
import { useTours } from "../../viewer3d/tour/tourStore";
import { type Of } from "../fields/common";
import s from "../Inspector.module.css";
import t from "./Tours.module.css";
import { MOVES, TRANSITIONS, clock, exportFromPanel, playTour, saveTour, tourSeconds } from "./tourActions";

const FPS: Array<{ value: "24" | "30" | "60"; label: string }> = [
  { value: "24", label: "24" },
  { value: "30", label: "30" },
  { value: "60", label: "60" },
];

export function TourFields({ el }: { el: Of<"tour">; doc: DocState }) {
  const exporting = useTours((x) => x.exporting);
  const open = () => {
    useTours.getState().select(el.id);
    useTours.getState().setOpen(true);
  };
  return (
    <div className={s.group}>
      <Field label="Name">
        <TextField label="Tour name" value={el.name} required onCommit={(name) => saveTour(el, { ...el, name })} />
      </Field>
      <Field label="Frames a second">
        <Segmented label="Frames a second" stretch value={String(el.fps) as "24" | "30" | "60"} options={FPS} onChange={(v) => saveTour(el, { ...el, fps: Number(v) })} />
      </Field>
      <Field label="Length">
        <ReadOnly>
          {clock(tourSeconds(el), true)}, {el.shots.length} shots
        </ReadOnly>
      </Field>
      <ul className={t.shots} aria-label="Shots">
        {el.shots.map((sh) => (
          <li key={sh.id}>
            <span>
              {sh.name}
              {sh.transition.kind !== "cut" ? `, ${TRANSITIONS.find((k) => k.value === sh.transition.kind)?.label.toLowerCase()} in` : ""}
            </span>
            <span>
              {MOVES.find((m) => m.value === sh.kind)?.label}, {sh.seconds.toFixed(1)} s
            </span>
          </li>
        ))}
      </ul>
      <div className={t.actions}>
        <Button size="sm" variant="primary" icon="walk" onClick={() => playTour(el.id)}>
          Play
        </Button>
        <Button size="sm" icon="pencil" onClick={open}>
          Edit shots
        </Button>
        <Button size="sm" icon="export" disabled={!!exporting} onClick={() => void exportFromPanel(el.id, "hd")}>
          Export HD video
        </Button>
      </div>
    </div>
  );
}
