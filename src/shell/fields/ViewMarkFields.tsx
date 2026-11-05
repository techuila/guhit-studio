// Section and elevation mark fields (lane L1): its kind, label, which way it
// looks (left or right of start to end), how deep it draws, its level and
// its two ends. Its drawing is worked out from the mark, never stored.
import type { DocState, MarkKind } from "../../contract/bindings";
import { elevationName, lookDir } from "../../editor2d/permit/labels";
import { Field, NumberField, Segmented, Switch, TextField } from "../../ui/controls";
import { LevelField, PointFields, Position, update, type Of } from "./common";
import s from "../Inspector.module.css";

const KINDS: Array<{ value: MarkKind; label: string }> = [
  { value: "section", label: "Section" },
  { value: "elevation", label: "Elevation" },
];

/** A depth picked when "everything" is switched off. */
const DEFAULT_DEPTH_MM = 6000;

export function ViewMarkFields({ el, doc }: { el: Of<"view_mark">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const facing = elevationName(lookDir(el.start, el.end, el.flip), doc.project.settings.north_angle_deg);
  return (
    <>
      <div className={s.group}>
        <Field label="Kind">
          <Segmented label="Mark kind" stretch value={el.mark_kind} options={KINDS} onChange={(mark_kind) => update(el, { ...el, mark_kind })} />
        </Field>
        <Field label="Label" hint={el.mark_kind === "section" ? "A, B, C" : "The side it shows, like North or Front"}>
          <TextField label="Mark label" value={el.label} required onCommit={(label) => update(el, { ...el, label })} />
        </Field>
        <Field label="Looks" hint="Left or right of the line from its start to its end">
          <Segmented
            label="Which way it looks"
            stretch
            value={el.flip ? "right" : "left"}
            options={[
              { value: "left", label: "Left" },
              { value: "right", label: "Right" },
            ]}
            onChange={(v) => update(el, { ...el, flip: v === "right" })}
          />
        </Field>
        {el.mark_kind === "elevation" ? <p className={s.note}>It shows the {facing.toLowerCase()} side of the building.</p> : null}
        <Field label="Depth" hint="How far past the line it draws">
          <NumberField
            label="View depth"
            kind="length"
            unit={unit}
            min={100}
            max={1_000_000}
            step={500}
            disabled={el.depth_mm === null}
            value={el.depth_mm ?? DEFAULT_DEPTH_MM}
            onCommit={(depth_mm) => update(el, { ...el, depth_mm })}
          />
        </Field>
        <Field label="">
          <label className={s.inlineSwitch}>
            <Switch label="Draw everything past the line" checked={el.depth_mm === null} onChange={(all) => update(el, { ...el, depth_mm: all ? null : DEFAULT_DEPTH_MM })} />
            <span>Everything</span>
          </label>
        </Field>
        <LevelField levelId={el.level_id} doc={doc} onChange={(level_id) => update(el, { ...el, level_id })} />
      </div>
      <Position>
        <PointFields label="Start" point={el.start} unit={unit} onCommit={(start) => update(el, { ...el, start })} />
        <PointFields label="End" point={el.end} unit={unit} onCommit={(end) => update(el, { ...el, end })} />
      </Position>
    </>
  );
}
