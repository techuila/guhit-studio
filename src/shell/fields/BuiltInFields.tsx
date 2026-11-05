// Inspector fields for a parametric built-in (lane L5): wall cabinets, the
// sink and cooktop positions, and doors. Hooked from AssetFields for catalog
// items with `parametric`. Positions are from the left end seen from the
// front. An object without options shows the defaults for its width
// (`builtInDefaults`); the first edit stores them. The engine checks every
// value (`built_in_options`) and the inspector shows its refusal.
import type { BuiltInParams, CatalogItem, DocState } from "../../contract/bindings";
import { Field, NumberField, Switch } from "../../ui/controls";
import { builtInDefaults } from "../../viewer3d/scene/builtins";
import { update, type Of } from "./common";
import css from "./builtIn.module.css";
import s from "../Inspector.module.css";

/** What each built-in has: a sink, a cooktop, wall cabinets (a mirror cabinet on a vanity), doors. */
const PARTS: Record<string, { sink: boolean; cooktop: boolean; upper: string | null; doors: boolean }> = {
  kitchen_run: { sink: true, cooktop: true, upper: "Wall cabinets", doors: true },
  counter: { sink: true, cooktop: true, upper: null, doors: true },
  vanity: { sink: true, cooktop: false, upper: "Mirror cabinet", doors: true },
  wardrobe: { sink: false, cooktop: false, upper: null, doors: true },
  shelf: { sink: false, cooktop: false, upper: null, doors: false },
};

/** A position on the run, or none: a switch and the distance from the left end. */
function Spot({ label, value, fallback, min, max, unit, onCommit }: { label: string; value: number | null; fallback: number; min: number; max: number; unit: DocState["project"]["settings"]["display_unit"]; onCommit: (v: number | null) => void }) {
  return (
    <Field label={label} hint="From the left end, seen from the front">
      <div className={css.row}>
        <NumberField label={`${label} from the left end`} kind="length" unit={unit} min={min} max={max} step={50} value={value ?? fallback} disabled={value === null} onCommit={(v) => onCommit(v)} />
        <label className={css.toggle}>
          <Switch label={`${label} on`} checked={value !== null} onChange={(on) => onCommit(on ? fallback : null)} />
          <span>{value !== null ? "On" : "None"}</span>
        </label>
      </div>
    </Field>
  );
}

export function BuiltInFields({ el, item, doc }: { el: Of<"asset">; item: CatalogItem; doc: DocState }) {
  const kind = item.parametric;
  if (!kind) return null;
  const parts = PARTS[kind];
  const unit = doc.project.settings.display_unit;
  const w = el.width_mm;
  const defaults = builtInDefaults(kind, w);
  const p: BuiltInParams = el.built_in ?? defaults;
  const set = (patch: Partial<BuiltInParams>) => update(el, { ...el, built_in: { ...p, ...patch } });
  const mostDoors = Math.max(1, Math.floor(w / 250));
  if (!parts.sink && !parts.cooktop && !parts.upper && !parts.doors) return null;
  return (
    <div className={s.group}>
      {parts.upper ? (
        <Field label={parts.upper}>
          <label className={css.toggle}>
            <Switch label={parts.upper} checked={p.upper} onChange={(upper) => set({ upper })} />
            <span>{p.upper ? "On" : "Off"}</span>
          </label>
        </Field>
      ) : null}
      {parts.sink ? (
        <Spot label="Sink" value={p.sink_at_mm} fallback={defaults.sink_at_mm ?? Math.round(w / 2 / 50) * 50} min={150} max={Math.max(150, w - 150)} unit={unit} onCommit={(sink_at_mm) => set({ sink_at_mm })} />
      ) : null}
      {parts.cooktop ? (
        <Spot label="Cooktop" value={p.cooktop_at_mm} fallback={defaults.cooktop_at_mm ?? Math.round((w * 0.75) / 50) * 50} min={300} max={Math.max(300, w - 300)} unit={unit} onCommit={(cooktop_at_mm) => set({ cooktop_at_mm })} />
      ) : null}
      {parts.doors ? (
        <Field label="Doors" hint={`Up to ${mostDoors} across ${Math.round(w)} mm`}>
          <NumberField label="Doors across the front" integer min={kind === "wardrobe" ? 1 : 0} max={mostDoors} value={p.doors} onCommit={(doors) => set({ doors: Math.round(doors) })} />
        </Field>
      ) : null}
    </div>
  );
}
