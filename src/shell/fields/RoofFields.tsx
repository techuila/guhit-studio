// Roof fields (lane L2): the roof over the top floor in the project
// inspector, and one roof element's fields (the house, a lanai, a carport):
// its shape, pitch and ridge, eaves, fascia and gutter, material, the areas
// the engine worked out (`Derived::roofs`) and posts under its open eaves.
import { useState } from "react";
import type { DocState, Roof, RoofKind, RoofShape, Trim } from "../../contract/bindings";
import { topRoof } from "../../contract/roof";
import { useApp } from "../../state/store";
import { MaterialPicker } from "../../ui/MaterialPicker";
import { Button, Field, NumberField, ReadOnly, Section, Segmented, Switch } from "../../ui/controls";
import { formatArea } from "../../ui/units";
import { editRoof } from "../commitLatest";
import { useSection } from "../shellStore";
import { update, type Of } from "./common";
import { Bump } from "./permitCommon";
import { Reveal } from "./StairFields";
import css from "./roofStair.module.css";
import s from "../Inspector.module.css";

/** Every roof shape, hip included (actions.ts `ROOF_KINDS` predates hip). */
export const ROOF_SHAPES: Array<{ value: RoofShape; label: string }> = [
  { value: "flat", label: "Flat" },
  { value: "shed", label: "Shed" },
  { value: "gable", label: "Gable" },
  { value: "hip", label: "Hip" },
];

/** The roof over the top floor's choices: none, then every shape. */
export const ROOF_KINDS: Array<{ value: RoofKind; label: string }> = [{ value: "none", label: "None" }, ...ROOF_SHAPES];

const AXES = [
  { value: "x" as const, label: "East-west" },
  { value: "y" as const, label: "North-south" },
];

/** Which way a shed rises: its slope axis, against it with `flip` (a lanai on any side slopes away from the house). */
type Rise = "north" | "east" | "south" | "west";
const RISES: Array<{ value: Rise; label: string }> = [
  { value: "north", label: "North" },
  { value: "east", label: "East" },
  { value: "south", label: "South" },
  { value: "west", label: "West" },
];
const SHED_RISE: Record<Rise, { ridge_axis: "x" | "y"; flip: boolean }> = {
  north: { ridge_axis: "y", flip: false },
  east: { ridge_axis: "x", flip: false },
  south: { ridge_axis: "y", flip: true },
  west: { ridge_axis: "x", flip: true },
};

/** The way a shed element rises. */
export function shedRise(el: { ridge_axis: "x" | "y"; flip?: boolean }): Rise {
  const flip = el.flip === true;
  return el.ridge_axis === "x" ? (flip ? "west" : "east") : flip ? "south" : "north";
}

/** Fascia and gutter sizes a switch turns on with. */
const FASCIA_MM = 150;
const GUTTER_MM = 125;
/** Posts under open eaves: 150 mm square, at most 3 m apart. */
const POST_MM = 150;
const POST_SPACING_MM = 3000;

export function RoofSection({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("roof", true);
  // The roof over the top floor, never `project.roof` (DECISIONS D54).
  const roof = topRoof(doc.project);
  const unit = doc.project.settings.display_unit;
  const set = (patch: Partial<Roof>) => void editRoof(roof, { ...roof, ...patch });
  const sloped = roof.kind === "shed" || roof.kind === "gable" || roof.kind === "hip";
  return (
    <Section title="Roof" icon="roof" open={open} onToggle={toggle}>
      <Segmented label="Roof type" stretch value={roof.kind} onChange={(kind) => set({ kind })} options={ROOF_KINDS} />
      {roof.kind === "none" ? (
        <p className={s.note}>No roof is drawn. Pick a type to cover the building outline.</p>
      ) : (
        <>
          <Reveal open={sloped}>
            <Field label="Pitch">
              <NumberField label="Roof pitch" suffix="deg" min={1} max={60} value={roof.pitch_deg} onCommit={(v) => set({ pitch_deg: v })} />
            </Field>
          </Reveal>
          <Field label="Overhang" hint="Eave distance past the outer wall face">
            <NumberField label="Roof overhang" kind="length" unit={unit} min={0} max={3000} step={50} value={roof.overhang_mm} onCommit={(v) => set({ overhang_mm: v })} />
          </Field>
          <Field label="Thickness">
            <NumberField label="Roof thickness" kind="length" unit={unit} min={10} max={1000} value={roof.thickness_mm} onCommit={(v) => set({ thickness_mm: v })} />
          </Field>
          <Reveal open={roof.kind === "gable" || roof.kind === "shed"}>
            <Field label={roof.kind === "shed" ? "Slopes along" : "Ridge runs"}>
              <Segmented label="Ridge axis" stretch value={roof.ridge_axis} onChange={(ridge_axis) => set({ ridge_axis })} options={AXES} />
            </Field>
          </Reveal>
          <Field label="Material">
            <MaterialPicker label="Roof material" materials={doc.project.materials} prefer={["roof", "metal"]} value={roof.material_id} noneLabel="Default roofing" onChange={(material_id) => set({ material_id })} />
          </Field>
        </>
      )}
    </Section>
  );
}

/** A fascia board or gutter: a switch, and its size while it is on. */
function TrimField({ what, trim, size, unit, onChange }: { what: string; trim: Trim | null | undefined; size: number; unit: "mm" | "m"; onChange: (trim: Trim | null) => void }) {
  return (
    <Field label={what}>
      <NumberField label={`${what} size`} kind="length" unit={unit} min={10} max={1000} step={25} value={trim?.size_mm ?? size} disabled={!trim} onCommit={(size_mm) => onChange({ size_mm, material_id: trim?.material_id ?? null })} />
      <label className={css.auto}>
        <Switch label={`${what} along the eaves`} checked={!!trim} onChange={(on) => onChange(on ? { size_mm: size, material_id: null } : null)} />
        <span>{trim ? "On" : "Off"}</span>
      </label>
    </Field>
  );
}

/** One roof element: the roof over the top floor, a lanai, a carport. */
export function RoofFields({ el, doc }: { el: Of<"roof">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const g = (doc.derived.roofs ?? []).find((r) => r.roof_id === el.id) ?? null;
  const top = el.outline.type === "top_footprint";
  const level = doc.project.levels.find((l) => l.id === (el.level_id ?? g?.level_id));
  const [posting, setPosting] = useState(false);
  const set = (patch: Partial<Of<"roof">>) => update(el, { ...el, ...patch });
  const pitched = el.shape !== "flat";
  const setShape = (shape: RoofShape) => {
    if (shape === el.shape) return;
    // Gable sides belong to a hip only.
    set(shape === "hip" ? { shape } : { shape, gable_sides: [] });
  };
  const addPosts = async () => {
    setPosting(true);
    try {
      await useApp.getState().dispatch({ type: "add_roof_posts", roof_id: el.id, size_mm: POST_MM, max_spacing_mm: POST_SPACING_MM });
    } finally {
      setPosting(false);
    }
  };
  return (
    <>
      <div className={s.group}>
        {top ? <p className={s.note}>The roof over the top floor follows the building outline.</p> : null}
        {/* Full width: four shapes do not fit beside a label. */}
        <Segmented label="Roof shape" stretch value={el.shape} options={ROOF_SHAPES} onChange={setShape} />
        <Reveal open={pitched}>
          <Field label="Pitch" hint="Degrees from level">
            <NumberField label="Roof pitch" suffix="deg" min={1} max={60} value={el.pitch_deg} onCommit={(pitch_deg) => set({ pitch_deg })} />
          </Field>
        </Reveal>
        <Reveal open={el.shape === "gable"}>
          <Field label="Ridge runs">
            <Segmented label="Ridge axis" stretch value={el.ridge_axis} options={AXES} onChange={(ridge_axis) => set({ ridge_axis })} />
          </Field>
        </Reveal>
        <Reveal open={el.shape === "shed"}>
          <Field label="Rises toward" hint="The high side; the eave and gutter are on the other">
            <Segmented label="Shed rises toward" stretch value={shedRise(el)} options={RISES} onChange={(rise) => set(SHED_RISE[rise])} />
          </Field>
        </Reveal>
        <Field label="Overhang" hint="How far the eaves reach past the outline">
          <NumberField label="Roof overhang" kind="length" unit={unit} min={0} max={5000} step={50} value={el.overhang_mm} onCommit={(overhang_mm) => set({ overhang_mm })} />
        </Field>
        <Field label="Thickness">
          <NumberField label="Roof thickness" kind="length" unit={unit} min={10} max={1000} step={10} value={el.thickness_mm} onCommit={(thickness_mm) => set({ thickness_mm })} />
        </Field>
        <Field label="Eave height" hint="Underside at the wall line above the floor; Level uses the level height">
          <NumberField
            label="Eave height"
            kind="length"
            unit={unit}
            min={500}
            max={20000}
            step={50}
            value={el.eave_mm ?? level?.height_mm ?? 3000}
            onCommit={(eave_mm) => set({ eave_mm })}
          />
          <label className={css.auto}>
            <Switch label="Eaves at the level height" checked={el.eave_mm == null} onChange={(on) => set({ eave_mm: on ? null : (level?.height_mm ?? 3000) })} />
            <span>Level</span>
          </label>
        </Field>
        <TrimField what="Fascia" trim={el.fascia} size={FASCIA_MM} unit={unit} onChange={(fascia) => set({ fascia })} />
        <TrimField what="Gutter" trim={el.gutter} size={GUTTER_MM} unit={unit} onChange={(gutter) => set({ gutter })} />
        <Field label="Material">
          <MaterialPicker label="Roof material" materials={doc.project.materials} prefer={["roof", "metal"]} value={el.material_id} noneLabel="Default roofing" onChange={(material_id) => set({ material_id })} />
        </Field>
      </div>
      <div className={s.group}>
        <Field label="Plan area" hint="Covered in plan, eaves included">
          <ReadOnly>
            <Bump value={g ? formatArea(g.plan_area_m2) : "-"} />
          </ReadOnly>
        </Field>
        <Field label="Surface" hint="The sloped area to roof">
          <ReadOnly>
            <Bump value={g ? formatArea(g.surface_area_m2) : "-"} />
          </ReadOnly>
        </Field>
        {top ? null : (
          <div className={css.posts}>
            <div className={s.actionsRow}>
              <Button size="sm" icon="column" disabled={posting} onClick={() => void addPosts()}>
                Add posts under open eaves
              </Button>
            </div>
            <p className={s.note}>Square posts, 150 mm, at most 3 m apart along the edges no wall carries.</p>
          </div>
        )}
      </div>
    </>
  );
}
