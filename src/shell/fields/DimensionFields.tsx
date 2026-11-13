// Dimension fields (lane L1): the measured length (and each segment of a
// dimension string), what an automatic dimension measures, a replacement
// text, the line offset and the measured points. Editing the points of an
// automatic dimension makes it a plain one (the engine clears `auto`).
import type { DimRef, DocState, Element } from "../../contract/bindings";
import { stringGeometry } from "../../editor2d/permit/shapes";
import { Field, NumberField, ReadOnly, TextField } from "../../ui/controls";
import { Icon } from "../../ui/icons";
import { formatLength } from "../../ui/units";
import { PointFields, Position, update, type Of } from "./common";
import css from "./permit.module.css";
import s from "../Inspector.module.css";

const SIDE: Record<string, string> = { north: "North", south: "South", east: "East", west: "West" };
const TIER: Record<string, string> = { overall: "overall", walls: "to the walls", openings: "to the openings" };
const FACE: Record<string, string> = { centerline: "wall centerlines", exterior: "outer faces", interior: "inner faces" };

/** What an automatic dimension measures, in words. */
export function autoText(ref: DimRef, elements: readonly Element[]): string {
  switch (ref.type) {
    case "exterior":
      return `${SIDE[ref.side] ?? ref.side} side, ${TIER[ref.tier] ?? ref.tier}`;
    case "openings": {
      const n = ref.wall_ids.length;
      return `Openings along ${n === 1 ? "1 wall" : `${n} walls`}, to the ${FACE[ref.face] ?? ref.face}`;
    }
    case "grids": {
      const labels = ref.grid_ids.map((id) => elements.find((e) => e.id === id)).flatMap((e) => (e && e.kind === "grid" ? [e.label] : []));
      return labels.length > 0 ? `Grid lines ${labels.join(", ")}` : "Grid lines";
    }
  }
}

export function DimensionFields({ el, doc }: { el: Of<"dimension">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const through = el.through ?? [];
  const isString = through.length > 0;
  const segments = isString ? stringGeometry(el).segments : [];
  const setThrough = (i: number, p: { x: number; y: number }) => update(el, { ...el, through: through.map((q, j) => (j === i ? p : q)) });
  return (
    <>
      <div className={s.group}>
        {el.auto ? (
          <>
            <Field label="Follows">
              <span className={css.autoPill}>
                <Icon name="link" size={12} />
                Automatic
              </span>
            </Field>
            <p className={css.typeLine}>
              <span className={css.autoWhat}>{autoText(el.auto, doc.project.elements)}</span>
            </p>
            <p className={s.note}>It follows the model after every change. Editing its points makes it a plain dimension.</p>
          </>
        ) : null}
        <Field label={isString ? "Overall" : "Measured"}>
          <ReadOnly>{formatLength(Math.hypot(el.b.x - el.a.x, el.b.y - el.a.y), unit)}</ReadOnly>
        </Field>
        {isString ? (
          <Field label="Segments" hint="Each length along the line, in order">
            <ul className={css.segments}>
              {segments.map((sg, i) => (
                <li key={i} className={css.segment} style={{ animationDelay: `${Math.min(i, 7) * 30}ms` }}>
                  {formatLength(sg.length, unit)}
                </li>
              ))}
            </ul>
          </Field>
        ) : null}
        <Field label="Show text" hint={isString ? "Shown under the line; the segments keep their lengths" : "Leave empty to show the measured length"}>
          <TextField label="Replacement text" value={el.text_override ?? ""} placeholder={isString ? "None" : "Measured length"} onCommit={(t) => update(el, { ...el, text_override: t === "" ? null : t })} />
        </Field>
        <Field label="Line offset" hint="Distance of the dimension line from the measured points">
          <NumberField label="Dimension line offset" kind="length" unit={unit} step={50} value={el.offset_mm} onCommit={(offset_mm) => update(el, { ...el, offset_mm })} />
        </Field>
      </div>
      <Position>
        <PointFields label="From" point={el.a} unit={unit} onCommit={(a) => update(el, { ...el, a })} />
        {through.map((p, i) => (
          <PointFields key={i} label={`Point ${i + 2}`} point={p} unit={unit} onCommit={(q) => setThrough(i, q)} />
        ))}
        <PointFields label="To" point={el.b} unit={unit} onCommit={(b) => update(el, { ...el, b })} />
      </Position>
    </>
  );
}
