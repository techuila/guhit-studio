// Room fields (lane L1): name, use, floor finish and the derived area, then
// the wall, ceiling and base finishes, their remarks and the ceiling height
// for the room and finish schedules (`Room::finishes`,
// `ceiling_height_mm`). The net area is the schedules' number
// (`Derived::room_schedule`) once the engine has it.
import type { DocState, RoomFinishes, RoomUsage } from "../../contract/bindings";
import { MaterialPicker } from "../../ui/MaterialPicker";
import { Field, NumberField, ReadOnly, Select, Switch, TextField } from "../../ui/controls";
import { formatArea, formatLength } from "../../ui/units";
import { update, type Of } from "./common";
import { Bump } from "./permitCommon";
import s from "../Inspector.module.css";

const USAGE: Array<{ value: RoomUsage; label: string }> = [
  { value: "living", label: "Living room" },
  { value: "dining", label: "Dining" },
  { value: "kitchen", label: "Kitchen" },
  { value: "bedroom", label: "Bedroom" },
  { value: "master_bedroom", label: "Master bedroom" },
  { value: "bathroom", label: "Toilet and bath" },
  { value: "powder_room", label: "Powder room" },
  { value: "laundry", label: "Laundry and service" },
  { value: "garage", label: "Garage or carport" },
  { value: "porch", label: "Porch or lanai" },
  { value: "hallway", label: "Hallway" },
  { value: "storage", label: "Storage" },
  { value: "office", label: "Office or study" },
  { value: "other", label: "Other" },
];

export const NO_FINISHES: RoomFinishes = { wall_material_id: null, ceiling_material_id: null, base_material_id: null, remarks: "" };

export function RoomFields({ el, doc }: { el: Of<"room">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const g = doc.derived.rooms.find((r) => r.room_id === el.id);
  const row = doc.derived.room_schedule?.find((r) => r.room_id === el.id);
  const level = doc.project.levels.find((l) => l.id === el.level_id);
  // The level's ceiling: the schedule's figure for this room when the room has none of its own.
  const levelCeiling = (el.ceiling_height_mm == null ? row?.ceiling_height_mm : undefined) ?? level?.height_mm ?? 3000;
  const finishes = el.finishes ?? NO_FINISHES;
  const setFinishes = (patch: Partial<RoomFinishes>) => update(el, { ...el, finishes: { ...finishes, ...patch } });
  const own = el.ceiling_height_mm != null;
  const net = row ? formatArea(row.floor_area_m2) : null;
  return (
    <div className={s.group}>
      <Field label="Name">
        <TextField label="Room name" value={el.name} required onCommit={(name) => update(el, { ...el, name, auto_named: false })} />
      </Field>
      <Field label="Use">
        <Select label="Room use" value={el.usage} options={USAGE} onChange={(usage) => update(el, { ...el, usage })} />
      </Field>
      <Field label="Floor finish">
        <MaterialPicker label="Floor material" materials={doc.project.materials} prefer={["floor", "wood"]} value={el.floor_material_id} noneLabel="Default floor" onChange={(floor_material_id) => update(el, { ...el, floor_material_id })} />
      </Field>
      <Field label="Net area" hint="As the schedules count it: voids, balconies and decks come off">
        <ReadOnly>{net ? <Bump value={net} /> : "Not enclosed"}</ReadOnly>
      </Field>
      <Field label="Perimeter">
        <ReadOnly>{g ? formatLength(g.perimeter_mm, unit) : "-"}</ReadOnly>
      </Field>
      {g ? null : <p className={s.note}>The walls around this room do not close. Join them to get an area.</p>}
      <Field label="Wall finish">
        <MaterialPicker label="Wall finish" materials={doc.project.materials} prefer={["wall", "wood"]} value={finishes.wall_material_id} noneLabel="Not set" onChange={(wall_material_id) => setFinishes({ wall_material_id })} />
      </Field>
      <Field label="Ceiling finish">
        <MaterialPicker label="Ceiling finish" materials={doc.project.materials} prefer={["wall", "generic", "wood"]} value={finishes.ceiling_material_id} noneLabel="Not set" onChange={(ceiling_material_id) => setFinishes({ ceiling_material_id })} />
      </Field>
      <Field label="Base" hint="Skirting or base">
        <MaterialPicker label="Base finish" materials={doc.project.materials} prefer={["floor", "wood", "generic"]} value={finishes.base_material_id} noneLabel="Not set" onChange={(base_material_id) => setFinishes({ base_material_id })} />
      </Field>
      <Field label="Finish remarks">
        <TextField label="Finish remarks" multiline value={finishes.remarks} placeholder="For the finish schedule" onCommit={(remarks) => setFinishes({ remarks })} />
      </Field>
      <Field label="Ceiling height" hint="Above this room's floor">
        <NumberField
          label="Ceiling height"
          kind="length"
          unit={unit}
          min={1800}
          max={20000}
          step={50}
          disabled={!own}
          value={el.ceiling_height_mm ?? levelCeiling}
          onCommit={(ceiling_height_mm) => update(el, { ...el, ceiling_height_mm })}
        />
      </Field>
      <Field label="">
        <label className={s.inlineSwitch}>
          <Switch label="Use the level's ceiling" checked={!own} onChange={(match) => update(el, { ...el, ceiling_height_mm: match ? null : levelCeiling })} />
          <span>Same as level</span>
        </label>
      </Field>
    </div>
  );
}
