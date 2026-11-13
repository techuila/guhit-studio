// Grid line fields (lane L1): its label, its length (the end moves, the
// start stays) and its two ends. Labels are unique: the engine refuses a
// taken one and says which line has it.
import type { DocState } from "../../contract/bindings";
import { Field, NumberField, TextField } from "../../ui/controls";
import { PointFields, Position, update, type Of } from "./common";
import s from "../Inspector.module.css";

/** The engine's shortest grid line (`grids::validate`). */
const MIN_MM = 100;

export function GridFields({ el, doc }: { el: Of<"grid">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const dx = el.end.x - el.start.x;
  const dy = el.end.y - el.start.y;
  const length = Math.hypot(dx, dy);
  const setLength = (len: number) => {
    const ux = length > 1e-9 ? dx / length : 1;
    const uy = length > 1e-9 ? dy / length : 0;
    update(el, { ...el, end: { x: el.start.x + ux * len, y: el.start.y + uy * len } });
  };
  return (
    <>
      <div className={s.group}>
        <Field label="Label" hint="1, 2, 3 run along x; A, B, C along y">
          <TextField label="Grid label" value={el.label} required onCommit={(label) => update(el, { ...el, label })} />
        </Field>
        <Field label="Length" hint="The end moves; the start stays">
          <NumberField label="Grid line length" kind="length" unit={unit} min={MIN_MM} max={1_000_000} step={500} value={length} onCommit={setLength} />
        </Field>
      </div>
      <Position>
        <PointFields label="Start" point={el.start} unit={unit} onCommit={(start) => update(el, { ...el, start })} />
        <PointFields label="End" point={el.end} unit={unit} onCommit={(end) => update(el, { ...el, end })} />
      </Position>
    </>
  );
}
