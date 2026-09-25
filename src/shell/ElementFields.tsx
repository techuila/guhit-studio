// Editable fields for one selected element, by kind. Every edit is one typed
// command: update_element, or the specific semantic command where one exists.
import { Fragment, useState } from "react";
import type {
  CameraPreset,
  Command,
  DocState,
  Element,
  OpeningStyle,
  Point,
  RoomUsage,
  Vec3,
  WallAnchor,
} from "../contract/bindings";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { MaterialPicker } from "../ui/MaterialPicker";
import { Button, Field, NumberField, ReadOnly, Section, Segmented, Select, Switch, TextField, cx } from "../ui/controls";
import { Icon, type IconName } from "../ui/icons";
import { formatAreaMm2, formatLength } from "../ui/units";
import { DOOR_STYLES, WINDOW_STYLES, walkTo } from "./actions";
import { DeviceBasics, DeviceSections, deviceIcon } from "./DeviceFields";
import { DEVICE_LABEL, catalogItem, isDevice } from "./devices";
import {
  PIPE_COLOR,
  PIPE_MATERIAL_LABEL,
  PIPE_SYSTEMS,
  drainMinSlopePct,
  formatDiameter,
  inMenu,
  materialsFor,
  midPoint,
  pipeFalls,
  pipeLength,
  reversed,
  runLabel,
  segmentFalls,
  sizeLabel,
  sizesFor,
  tradeOf,
  withMaterial,
  withSystem,
  type SegmentFall,
} from "./pipes";
import { useSection } from "./shellStore";
import s from "./Inspector.module.css";

type Of<K extends Element["kind"]> = Extract<Element, { kind: K }>;

const dispatch = (command: Command) => void useApp.getState().dispatch(command);
const update = (element: Element) => dispatch({ type: "update_element", element });

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

const CAMERA_PRESETS: Array<{ value: CameraPreset; label: string }> = [
  { value: "eye_level", label: "Eye level" },
  { value: "exterior_corner", label: "Exterior corner" },
  { value: "top", label: "Top view" },
  { value: "axonometric", label: "Axonometric" },
  { value: "room_interior", label: "Room interior" },
  { value: "custom", label: "Custom" },
];

const STYLE_LABEL: Record<OpeningStyle, string> = {
  swing_single: "Single swing",
  swing_double: "Double swing",
  sliding: "Sliding",
  fixed: "Fixed glass",
  casement: "Casement",
  jalousie: "Jalousie",
};

export function elementTitle(el: Element, doc: DocState): { title: string; subtitle: string; icon: IconName; tint?: string } {
  const unit = doc.project.settings.display_unit;
  switch (el.kind) {
    case "wall": {
      const g = doc.derived.walls.find((w) => w.wall_id === el.id);
      return { title: "Wall", subtitle: g ? `${formatLength(g.length_mm, unit)} long${g.exterior ? ", exterior" : ""}` : "", icon: "wall" };
    }
    case "opening":
      return { title: el.opening_type === "door" ? "Door" : "Window", subtitle: STYLE_LABEL[el.style], icon: el.opening_type === "door" ? "door" : "window" };
    case "room":
      return { title: "Room", subtitle: el.name, icon: "room" };
    case "column":
      return { title: "Column", subtitle: el.shape === "round" ? "Round" : "Rectangular", icon: "column" };
    case "stair":
      return { title: "Stair", subtitle: `${el.riser_count} risers`, icon: "stair" };
    case "asset": {
      // Devices say what they are: "Switch", "Lighting outlet", "Aircon indoor unit".
      const item = catalogItem(useApp.getState().catalog, el.catalog_key);
      const title = item?.device ? DEVICE_LABEL[item.device][0] : el.light ? "Light" : "Object";
      return { title, subtitle: el.name, icon: item?.device || el.light ? deviceIcon(item, el) : "asset" };
    }
    case "annotation":
      return { title: "Text", subtitle: el.text.split("\n")[0] ?? "", icon: "text" };
    case "dimension":
      return { title: "Dimension", subtitle: formatLength(Math.hypot(el.b.x - el.a.x, el.b.y - el.a.y), unit), icon: "dimension" };
    case "camera":
      return { title: "Camera", subtitle: el.name, icon: "camera" };
    case "underlay":
      return { title: "Traced image", subtitle: el.file_name, icon: "visuals" };
    case "linework":
      return { title: "Linework", subtitle: el.name, icon: "linework" };
    case "reference_model":
      return { title: "Reference model", subtitle: el.name, icon: "model" };
    case "pipe":
      return {
        title: runLabel(el.system),
        subtitle: el.name || `${sizeLabel(el.material, el.diameter_mm)}, ${formatLength(pipeLength(el.points), unit)}`,
        icon: "pipe",
        tint: PIPE_COLOR[el.system],
      };
  }
}

// ---------------------------------------------------------------- helpers

function PointFields({ label, point, unit, onCommit }: { label: string; point: Point; unit: "mm" | "m"; onCommit: (p: Point) => void }) {
  return (
    <Field label={label}>
      <NumberField label={`${label} X`} kind="length" unit={unit} value={point.x} onCommit={(x) => onCommit({ ...point, x })} />
      <NumberField label={`${label} Y`} kind="length" unit={unit} value={point.y} onCommit={(y) => onCommit({ ...point, y })} />
    </Field>
  );
}

function Vec3Fields({ label, value, unit, onCommit }: { label: string; value: Vec3; unit: "mm" | "m"; onCommit: (v: Vec3) => void }) {
  return (
    <>
      <PointFields label={label} point={{ x: value.x, y: value.y }} unit={unit} onCommit={(p) => onCommit({ ...value, ...p })} />
      <Field label={`${label} height`}>
        <NumberField label={`${label} height`} kind="length" unit={unit} value={value.z} onCommit={(z) => onCommit({ ...value, z })} />
      </Field>
    </>
  );
}

function Rotation({ value, onCommit }: { value: number; onCommit: (deg: number) => void }) {
  return (
    <Field label="Rotation" hint="Degrees, counter-clockwise">
      <NumberField label="Rotation" suffix="deg" step={15} min={-360} max={360} value={value} onCommit={onCommit} />
    </Field>
  );
}

/** Less-used exact coordinates, tucked away but always available. */
function Position({ children }: { children: React.ReactNode }) {
  const [open, toggle] = useSection("position", false);
  return (
    <Section title="Exact position" open={open} onToggle={toggle}>
      {children}
    </Section>
  );
}

// ---------------------------------------------------------------- per kind

let lastAnchor: WallAnchor = "start";

function WallFields({ el, doc }: { el: Of<"wall">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const [anchor, setAnchor] = useState<WallAnchor>(lastAnchor);
  const geometry = doc.derived.walls.find((w) => w.wall_id === el.id);
  const length = geometry?.length_mm ?? Math.hypot(el.end.x - el.start.x, el.end.y - el.start.y);
  const level = doc.project.levels.find((l) => l.id === el.level_id);
  const levelHeight = level?.height_mm ?? 3000;
  return (
    <>
      <div className={s.group}>
        <Field label="Length">
          <NumberField label="Wall length" kind="length" unit={unit} min={1} value={length} onCommit={(length_mm) => dispatch({ type: "set_wall_length", wall_id: el.id, length_mm, anchor })} />
        </Field>
        <Field label="Keep fixed" hint="Which part of the wall stays in place when the length changes">
          <Segmented
            label="Fixed point when the length changes"
            stretch
            value={anchor}
            onChange={(a) => {
              lastAnchor = a;
              setAnchor(a);
            }}
            options={[
              { value: "start", label: "Start" },
              { value: "center", label: "Middle" },
              { value: "end", label: "End" },
            ]}
          />
        </Field>
        <Field label="Thickness">
          <NumberField label="Wall thickness" kind="length" unit={unit} min={20} max={1000} value={el.thickness_mm} onCommit={(thickness_mm) => update({ ...el, thickness_mm })} />
        </Field>
        <Field label="Height">
          <NumberField
            label="Wall height"
            kind="length"
            unit={unit}
            min={100}
            max={20000}
            step={50}
            disabled={el.height_mm === null}
            value={el.height_mm ?? levelHeight}
            onCommit={(height_mm) => update({ ...el, height_mm })}
          />
        </Field>
        <Field label="">
          <label className={s.inlineSwitch}>
            <Switch label="Use the level height" checked={el.height_mm === null} onChange={(match) => update({ ...el, height_mm: match ? null : levelHeight })} />
            <span>Same as level</span>
          </label>
        </Field>
        <Field label="Material">
          <MaterialPicker label="Wall material" materials={doc.project.materials} prefer={["wall"]} value={el.material_id} noneLabel="Default wall finish" onChange={(material_id) => update({ ...el, material_id })} />
        </Field>
      </div>
      <Position>
        <PointFields label="Start" point={el.start} unit={unit} onCommit={(start) => dispatch({ type: "set_wall_endpoints", wall_id: el.id, start, end: el.end })} />
        <PointFields label="End" point={el.end} unit={unit} onCommit={(end) => dispatch({ type: "set_wall_endpoints", wall_id: el.id, start: el.start, end })} />
      </Position>
    </>
  );
}

function OpeningFields({ el, doc }: { el: Of<"opening">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const isDoor = el.opening_type === "door";
  const styles = isDoor ? DOOR_STYLES : WINDOW_STYLES;
  const host = doc.derived.walls.find((w) => w.wall_id === el.wall_id);
  const swings = el.style === "swing_single" || el.style === "swing_double" || el.style === "casement";
  return (
    <div className={s.group}>
      <Field label="Type">
        <Segmented
          label="Opening type"
          stretch
          value={el.opening_type}
          onChange={(opening_type) => {
            if (opening_type === el.opening_type) return;
            update(
              opening_type === "door"
                ? { ...el, opening_type, style: "swing_single", sill_mm: 0, height_mm: Math.max(el.height_mm, 2100) }
                : { ...el, opening_type, style: "sliding", sill_mm: 900, height_mm: Math.min(el.height_mm, 1200) },
            );
          }}
          options={[
            { value: "door", label: "Door", icon: "door" },
            { value: "window", label: "Window", icon: "window" },
          ]}
        />
      </Field>
      <Field label="Style">
        <Select
          label="Style"
          value={el.style}
          onChange={(style) => update({ ...el, style })}
          options={styles.some((o) => o.value === el.style) ? styles : [...styles, { value: el.style, label: STYLE_LABEL[el.style] }]}
        />
      </Field>
      <Field label="Width">
        <NumberField label="Width" kind="length" unit={unit} min={200} max={10000} step={50} value={el.width_mm} onCommit={(width_mm) => update({ ...el, width_mm })} />
      </Field>
      <Field label="Height">
        <NumberField label="Height" kind="length" unit={unit} min={200} max={6000} step={50} value={el.height_mm} onCommit={(height_mm) => update({ ...el, height_mm })} />
      </Field>
      {isDoor ? null : (
        <Field label="Sill height" hint="Bottom of the window above the floor">
          <NumberField label="Sill height" kind="length" unit={unit} min={0} max={5000} step={50} value={el.sill_mm} onCommit={(sill_mm) => update({ ...el, sill_mm })} />
        </Field>
      )}
      <Field label="From wall start" hint="Distance from the start of the wall to the center of the opening">
        <NumberField label="Offset along the wall" kind="length" unit={unit} min={0} max={host?.length_mm} step={50} value={el.offset_mm} onCommit={(offset_mm) => update({ ...el, offset_mm })} />
      </Field>
      {swings ? (
        <Field label="Swing">
          <Button size="sm" onClick={() => update({ ...el, flip_side: !el.flip_side })}>
            Flip side
          </Button>
          <Button size="sm" onClick={() => update({ ...el, flip_hinge: !el.flip_hinge })}>
            Flip hinge
          </Button>
        </Field>
      ) : null}
      <Field label="Material">
        <MaterialPicker
          label="Opening material"
          materials={doc.project.materials}
          prefer={isDoor ? ["wood", "metal", "glass"] : ["glass", "metal", "wood"]}
          value={el.material_id}
          noneLabel={isDoor ? "Default door finish" : "Default window finish"}
          onChange={(material_id) => update({ ...el, material_id })}
        />
      </Field>
    </div>
  );
}

function RoomFields({ el, doc }: { el: Of<"room">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const g = doc.derived.rooms.find((r) => r.room_id === el.id);
  return (
    <div className={s.group}>
      <Field label="Name">
        <TextField label="Room name" value={el.name} required onCommit={(name) => update({ ...el, name, auto_named: false })} />
      </Field>
      <Field label="Use">
        <Select label="Room use" value={el.usage} options={USAGE} onChange={(usage) => update({ ...el, usage })} />
      </Field>
      <Field label="Floor finish">
        <MaterialPicker label="Floor material" materials={doc.project.materials} prefer={["floor", "wood"]} value={el.floor_material_id} noneLabel="Default floor" onChange={(floor_material_id) => update({ ...el, floor_material_id })} />
      </Field>
      <Field label="Net area" hint="Measured on the inner wall faces">
        <ReadOnly>{g ? formatAreaMm2(g.area_mm2) : "Not enclosed"}</ReadOnly>
      </Field>
      <Field label="Perimeter">
        <ReadOnly>{g ? formatLength(g.perimeter_mm, unit) : "-"}</ReadOnly>
      </Field>
      {g ? null : <p className={s.note}>The walls around this room do not close. Join them to get an area.</p>}
    </div>
  );
}

function ColumnFields({ el, doc }: { el: Of<"column">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const round = el.shape === "round";
  return (
    <>
      <div className={s.group}>
        <Field label="Shape">
          <Segmented
            label="Column shape"
            stretch
            value={el.shape}
            onChange={(shape) => update({ ...el, shape })}
            options={[
              { value: "rect", label: "Rectangular" },
              { value: "round", label: "Round" },
            ]}
          />
        </Field>
        <Field label={round ? "Diameter" : "Width"}>
          <NumberField label={round ? "Diameter" : "Width"} kind="length" unit={unit} min={50} max={3000} value={el.width_mm} onCommit={(width_mm) => update({ ...el, width_mm })} />
        </Field>
        {round ? null : (
          <>
            <Field label="Depth">
              <NumberField label="Depth" kind="length" unit={unit} min={50} max={3000} value={el.depth_mm} onCommit={(depth_mm) => update({ ...el, depth_mm })} />
            </Field>
            <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update({ ...el, rotation_deg })} />
          </>
        )}
        <Field label="Material">
          <MaterialPicker label="Column material" materials={doc.project.materials} prefer={["wall", "generic"]} value={el.material_id} noneLabel="Default concrete" onChange={(material_id) => update({ ...el, material_id })} />
        </Field>
      </div>
      <Position>
        <PointFields label="Center" point={el.center} unit={unit} onCommit={(center) => update({ ...el, center })} />
      </Position>
    </>
  );
}

function StairFields({ el, doc }: { el: Of<"stair">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const level = doc.project.levels.find((l) => l.id === el.level_id);
  const riser = level ? level.height_mm / Math.max(1, el.riser_count) : null;
  const tread = el.run_mm / Math.max(1, el.riser_count - 1);
  return (
    <>
      <div className={s.group}>
        <Field label="Width">
          <NumberField label="Stair width" kind="length" unit={unit} min={500} max={5000} step={50} value={el.width_mm} onCommit={(width_mm) => update({ ...el, width_mm })} />
        </Field>
        <Field label="Run" hint="Plan length of the flight">
          <NumberField label="Stair run" kind="length" unit={unit} min={500} max={20000} step={50} value={el.run_mm} onCommit={(run_mm) => update({ ...el, run_mm })} />
        </Field>
        <Field label="Risers">
          <NumberField label="Number of risers" integer min={2} max={40} value={el.riser_count} onCommit={(riser_count) => update({ ...el, riser_count })} />
        </Field>
        <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update({ ...el, rotation_deg })} />
        <Field label="Each riser" hint="Level height divided by the number of risers">
          <ReadOnly>{riser ? formatLength(riser, unit) : "-"}</ReadOnly>
        </Field>
        <Field label="Each tread">
          <ReadOnly>{formatLength(tread, unit)}</ReadOnly>
        </Field>
      </div>
      <Position>
        <PointFields label="First riser" point={el.origin} unit={unit} onCommit={(origin) => update({ ...el, origin })} />
      </Position>
    </>
  );
}

/** Width, depth and height: rarely changed for a device, so tucked away there. */
function AssetSize({ children }: { children: React.ReactNode }) {
  const [open, toggle] = useSection("device-size", false);
  return (
    <Section title="Size and rotation" open={open} onToggle={toggle}>
      {children}
    </Section>
  );
}

function AssetFields({ el, doc }: { el: Of<"asset">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const catalog = useApp((st) => st.catalog);
  const item = catalogItem(catalog, el.catalog_key);
  if (isDevice(el, item)) {
    return (
      <>
        <div className={s.group}>
          <Field label="Name">
            <TextField label="Object name" value={el.name} required onCommit={(name) => update({ ...el, name })} />
          </Field>
          <DeviceBasics el={el} item={item} doc={doc} />
        </div>
        <DeviceSections el={el} item={item} doc={doc} />
        <AssetSize>
          <Field label="Width">
            <NumberField label="Width" kind="length" unit={unit} min={10} max={20000} value={el.width_mm} onCommit={(width_mm) => update({ ...el, width_mm })} />
          </Field>
          <Field label="Depth">
            <NumberField label="Depth" kind="length" unit={unit} min={10} max={20000} value={el.depth_mm} onCommit={(depth_mm) => update({ ...el, depth_mm })} />
          </Field>
          <Field label="Height">
            <NumberField label="Height" kind="length" unit={unit} min={10} max={20000} value={el.height_mm} onCommit={(height_mm) => update({ ...el, height_mm })} />
          </Field>
          <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update({ ...el, rotation_deg })} />
        </AssetSize>
        <Position>
          <PointFields label="Center" point={el.position} unit={unit} onCommit={(position) => update({ ...el, position })} />
        </Position>
      </>
    );
  }
  return (
    <>
      <div className={s.group}>
        <Field label="Name">
          <TextField label="Object name" value={el.name} required onCommit={(name) => update({ ...el, name })} />
        </Field>
        <Field label="Width">
          <NumberField label="Width" kind="length" unit={unit} min={10} max={20000} value={el.width_mm} onCommit={(width_mm) => update({ ...el, width_mm })} />
        </Field>
        <Field label="Depth">
          <NumberField label="Depth" kind="length" unit={unit} min={10} max={20000} value={el.depth_mm} onCommit={(depth_mm) => update({ ...el, depth_mm })} />
        </Field>
        <Field label="Height">
          <NumberField label="Height" kind="length" unit={unit} min={10} max={20000} value={el.height_mm} onCommit={(height_mm) => update({ ...el, height_mm })} />
        </Field>
        <Field label="Above floor" hint="Height of the underside above the floor">
          <NumberField label="Height above the floor" kind="length" unit={unit} min={0} max={20000} step={50} value={el.elevation_mm} onCommit={(elevation_mm) => update({ ...el, elevation_mm })} />
        </Field>
        <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update({ ...el, rotation_deg })} />
      </div>
      <Position>
        <PointFields label="Center" point={el.position} unit={unit} onCommit={(position) => update({ ...el, position })} />
      </Position>
    </>
  );
}

function AnnotationFields({ el, doc }: { el: Of<"annotation">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  return (
    <>
      <div className={s.group}>
        <TextField label="Text" multiline value={el.text} placeholder="Write a note" onCommit={(text) => update({ ...el, text })} />
        <p className={s.note}>Press Ctrl or Cmd with Enter to apply.</p>
        <Field label="Text height" hint="In model units at the project scale">
          <NumberField label="Text height" kind="length" unit={unit} min={10} max={5000} value={el.size_mm} onCommit={(size_mm) => update({ ...el, size_mm })} />
        </Field>
        <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update({ ...el, rotation_deg })} />
      </div>
      <Position>
        <PointFields label="Anchor" point={el.position} unit={unit} onCommit={(position) => update({ ...el, position })} />
      </Position>
    </>
  );
}

function DimensionFields({ el, doc }: { el: Of<"dimension">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  return (
    <>
      <div className={s.group}>
        <Field label="Measured">
          <ReadOnly>{formatLength(Math.hypot(el.b.x - el.a.x, el.b.y - el.a.y), unit)}</ReadOnly>
        </Field>
        <Field label="Show text" hint="Leave empty to show the measured length">
          <TextField label="Replacement text" value={el.text_override ?? ""} placeholder="Measured length" onCommit={(t) => update({ ...el, text_override: t === "" ? null : t })} />
        </Field>
        <Field label="Line offset" hint="Distance of the dimension line from the measured points">
          <NumberField label="Dimension line offset" kind="length" unit={unit} step={50} value={el.offset_mm} onCommit={(offset_mm) => update({ ...el, offset_mm })} />
        </Field>
      </div>
      <Position>
        <PointFields label="From" point={el.a} unit={unit} onCommit={(a) => update({ ...el, a })} />
        <PointFields label="To" point={el.b} unit={unit} onCommit={(b) => update({ ...el, b })} />
      </Position>
    </>
  );
}

function CameraFields({ el, doc }: { el: Of<"camera">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const activeCameraId = useApp((st) => st.activeCameraId);
  const lookThrough = () => {
    const app = useApp.getState();
    if (app.viewMode === "2d") app.setViewMode("split");
    app.setActiveCamera(el.id);
    bus.emit("apply_camera", el);
  };
  return (
    <>
      <div className={s.group}>
        <Field label="Name">
          <TextField label="Camera name" value={el.name} required onCommit={(name) => update({ ...el, name })} />
        </Field>
        <Field label="Preset">
          <Select label="Camera preset" value={el.preset} options={CAMERA_PRESETS} onChange={(preset) => update({ ...el, preset })} />
        </Field>
        <Field label="Field of view">
          <NumberField label="Field of view" suffix="deg" min={10} max={120} step={5} value={el.fov_deg} onCommit={(fov_deg) => update({ ...el, fov_deg })} />
        </Field>
        <div className={s.actionsRow}>
          <Button icon="camera" onClick={lookThrough}>
            Look through this camera
          </Button>
          {activeCameraId === el.id ? (
            <Button variant="ghost" onClick={() => useApp.getState().setActiveCamera(null)}>
              Back to free orbit
            </Button>
          ) : null}
        </div>
      </div>
      <Position>
        <Vec3Fields label="Camera" value={el.position} unit={unit} onCommit={(position) => update({ ...el, position, preset: "custom" })} />
        <Vec3Fields label="Looks at" value={el.target} unit={unit} onCommit={(target) => update({ ...el, target, preset: "custom" })} />
      </Position>
    </>
  );
}

function UnderlayFields({ el, doc }: { el: Of<"underlay">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  return (
    <>
      <div className={s.group}>
        <Field label="Image">
          <ReadOnly>
            {el.width_px} x {el.height_px} px
          </ReadOnly>
        </Field>
        <Field label="Scale" hint="Millimeters in the real building for each image pixel">
          <NumberField label="Millimeters per pixel" suffix="mm/px" min={0.01} max={1000} decimals={3} step={0.5} value={el.mm_per_px} onCommit={(mm_per_px) => update({ ...el, mm_per_px })} />
        </Field>
        <Field label="">
          <label className={s.inlineSwitch}>
            <Switch label="Scale checked" checked={el.scale_confirmed} onChange={(scale_confirmed) => update({ ...el, scale_confirmed })} />
            <span>I checked this scale</span>
          </label>
        </Field>
        {el.scale_confirmed ? null : <p className={s.note}>Measure one known length on the image before tracing. The scale is never guessed.</p>}
        <Field label="Opacity">
          <NumberField label="Opacity" suffix="%" min={5} max={100} step={5} decimals={0} value={Math.round(el.opacity * 100)} onCommit={(v) => update({ ...el, opacity: v / 100 })} />
        </Field>
        <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update({ ...el, rotation_deg })} />
        <Field label="">
          <label className={s.inlineSwitch}>
            <Switch label="Lock the image" checked={el.locked} onChange={(locked) => update({ ...el, locked })} />
            <span>Locked in place</span>
          </label>
        </Field>
      </div>
      <Position>
        <PointFields label="Bottom left" point={el.position} unit={unit} onCommit={(position) => update({ ...el, position })} />
      </Position>
    </>
  );
}

function LevelField({ levelId, doc, onChange }: { levelId: string; doc: DocState; onChange: (levelId: string) => void }) {
  if (doc.project.levels.length <= 1) return null;
  return (
    <Field label="Level">
      <Select label="Level" value={levelId} options={doc.project.levels.map((l) => ({ value: l.id, label: l.name }))} onChange={onChange} />
    </Field>
  );
}

function LineworkFields({ el, doc }: { el: Of<"linework">; doc: DocState }) {
  return (
    <div className={s.group}>
      <Field label="Name">
        <TextField label="Linework name" value={el.name} required onCommit={(name) => update({ ...el, name })} />
      </Field>
      <LevelField levelId={el.level_id} doc={doc} onChange={(level_id) => update({ ...el, level_id })} />
      <Field label="Polylines" hint="Traced lines brought in from the source file">
        <ReadOnly>{el.polylines.length}</ReadOnly>
      </Field>
      <Field label="">
        <label className={s.inlineSwitch}>
          <Switch label="Lock the linework" checked={el.locked} onChange={(locked) => update({ ...el, locked })} />
          <span>Locked in place</span>
        </label>
      </Field>
      <p className={s.note}>Imported tracing reference, on the Underlays layer. Move, rotate or delete it; the lines themselves are not editable.</p>
    </div>
  );
}

// ---------------------------------------------------------------- pipes

function coord(mm: number, unit: "mm" | "m"): string {
  return unit === "m" ? (mm / 1000).toFixed(2) : String(Math.round(mm));
}

function pct(value: number, digits: number): string {
  return `${value.toFixed(digits)}%`;
}

/** How one drainage segment falls, in words. */
function fallText(f: SegmentFall, min: number, unit: "mm" | "m"): string {
  const over = `over ${formatLength(f.horizontalMm, unit)}`;
  if (f.pct === null) return `Vertical, ${f.dropMm >= 0 ? "drops" : "rises"} ${formatLength(Math.abs(f.dropMm), unit)}`;
  if (f.steep) return `${f.dropMm >= 0 ? "Drops" : "Rises"} ${formatLength(Math.abs(f.dropMm), unit)} ${over}`;
  // A low fall never reads as the default itself: 1.996 shows 1.996, not 2.00.
  const digits = f.low && Math.abs(Number(f.pct.toFixed(2))) >= min ? 3 : 2;
  if (Math.abs(f.dropMm) < 0.05) return `Level ${over}`;
  if (f.dropMm < 0) return `Rises ${pct(-f.pct, digits)} ${over}`;
  return `Falls ${pct(f.pct, digits)} ${over}`;
}

function PipeFields({ el, doc }: { el: Of<"pipe">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  // Drainage, storm and condensate flow from the first point to the last, and fall.
  const drainage = pipeFalls(el.system);
  const falls = drainage ? segmentFalls(el) : [];
  const min = drainMinSlopePct(el.diameter_mm);
  const lowCount = falls.filter((f) => f.low).length;

  const materials = materialsFor(el.system);
  const materialOptions = (materials.includes(el.material) ? materials : [...materials, el.material]).map((m) => ({ value: m, label: PIPE_MATERIAL_LABEL[m] }));
  const sizes = sizesFor(el.system, el.material);
  const sizeOptions = (inMenu(el.system, el.material, el.diameter_mm) ? sizes : [...sizes, el.diameter_mm].sort((a, b) => a - b)).map((d) => ({
    value: String(d),
    label: `${formatDiameter(d)} mm`,
  }));
  const setHeight = (i: number, z: number) => update({ ...el, points: el.points.map((p, j) => (j === i ? { ...p, z } : p)) });

  return (
    <>
      <div className={s.group}>
        <Field label="Name">
          <TextField label="Run name" value={el.name} placeholder={tradeOf(el.system).group === "plumbing" ? "For example, sink waste" : tradeOf(el.system).group === "electrical" ? "For example, kitchen outlets" : "For example, bedroom line set"} onCommit={(name) => update({ ...el, name })} />
        </Field>
        <Field label="System" hint="Each system has its own layer">
          <span className={s.pipeSwatch} style={{ background: PIPE_COLOR[el.system] }} aria-hidden />
          <Select label="Pipe system" value={el.system} options={PIPE_SYSTEMS.map((p) => ({ value: p.value, label: p.label }))} onChange={(system) => update(withSystem(el, system))} />
        </Field>
        <Field label="Material">
          <Select label="Pipe material" value={el.material} options={materialOptions} onChange={(material) => update(withMaterial(el, material))} />
        </Field>
        <Field label={el.system === "refrigerant" ? "Gas line" : "Size"} hint={`Nominal size${el.system === "refrigerant" ? " of the gas line; the liquid line is 6.35 mm" : ""}. Sizing is for ${tradeOf(el.system).pro}.`}>
          <Select label="Pipe size" value={String(el.diameter_mm)} options={sizeOptions} onChange={(v) => update({ ...el, diameter_mm: Number(v) })} />
        </Field>
        <Field label="Length" hint="Centerline, along every segment">
          <ReadOnly>{formatLength(pipeLength(el.points), unit)}</ReadOnly>
        </Field>
      </div>
      <div className={s.group}>
        <div className={s.pointsHead}>
          <span>{drainage ? "Points, in flow order" : "Points"}</span>
          <span>Height above floor</span>
        </div>
        <div className={s.points}>
          {el.points.map((p, i) => (
            <Fragment key={i}>
              <div className={s.point}>
                <span className={s.pointIndex}>{i + 1}</span>
                <span className={s.pointXY} title="Plan position. Drag the point in the plan to move it.">
                  <i>x</i>
                  {coord(p.x, unit)} <i>y</i>
                  {coord(p.y, unit)}
                </span>
                <NumberField label={`Height of point ${i + 1}`} kind="length" unit={unit} min={-5000} max={20000} step={10} value={p.z} onCommit={(z) => setHeight(i, z)} />
              </div>
              {drainage && i < falls.length ? (
                <div className={cx(s.fall, falls[i].low && s.fallLow)} title={falls[i].low ? (falls[i].dropMm < 0 ? "Runs uphill" : `Less than the ${min}% default fall for this size`) : undefined}>
                  <Icon name={falls[i].low ? "warning" : "chevronDown"} size={12} />
                  <span>{fallText(falls[i], min, unit)}</span>
                </div>
              ) : null}
            </Fragment>
          ))}
        </div>
        {drainage ? (
          <p className={s.note}>
            Flows from point 1 to point {el.points.length}. {lowCount > 0 ? `${lowCount === 1 ? "One segment falls" : `${lowCount} segments fall`} less than the ${min}% default.` : `Every segment falls at least the ${min}% default.`}
          </p>
        ) : null}
        <div className={s.actionsRow}>
          {drainage ? (
            <Button size="sm" icon="flip" onClick={() => update(reversed(el))}>
              Reverse flow
            </Button>
          ) : null}
          <Button size="sm" icon="walk" onClick={() => void walkTo([el.id], midPoint(el.points))}>
            Walk here
          </Button>
        </div>
      </div>
    </>
  );
}

export const MODEL_UNITS = [
  { value: "mm", label: "mm", mm: 1 },
  { value: "cm", label: "cm", mm: 10 },
  { value: "m", label: "m", mm: 1000 },
  { value: "inch", label: "in", mm: 25.4 },
  { value: "feet", label: "ft", mm: 304.8 },
] as const;

function ReferenceModelFields({ el, doc }: { el: Of<"reference_model">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const selectedUnit = MODEL_UNITS.find((u) => Math.abs(u.mm - el.scale_to_mm) < 1e-6)?.value ?? "mm";
  return (
    <>
      <div className={s.group}>
        <Field label="Name">
          <TextField label="Reference model name" value={el.name} required onCommit={(name) => update({ ...el, name })} />
        </Field>
        <LevelField levelId={el.level_id} doc={doc} onChange={(level_id) => update({ ...el, level_id })} />
        <Field label="Model unit" hint="What one unit in the file means, so it lands at the right size">
          <Segmented label="Model unit" stretch value={selectedUnit} onChange={(v) => update({ ...el, scale_to_mm: MODEL_UNITS.find((u) => u.value === v)!.mm })} options={[...MODEL_UNITS]} />
        </Field>
        <Field label="Scale to mm" hint="Multiply model units by this to get millimeters">
          <NumberField label="Scale to millimeters" min={0.0001} decimals={4} value={el.scale_to_mm} onCommit={(scale_to_mm) => update({ ...el, scale_to_mm })} />
        </Field>
        <Field label="Elevation" hint="Height of the model origin above the level floor">
          <NumberField label="Elevation" kind="length" unit={unit} step={50} value={el.elevation_mm} onCommit={(elevation_mm) => update({ ...el, elevation_mm })} />
        </Field>
        <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update({ ...el, rotation_deg })} />
        <Field label="">
          <label className={s.inlineSwitch}>
            <Switch label="Lock the model" checked={el.locked} onChange={(locked) => update({ ...el, locked })} />
            <span>Locked in place</span>
          </label>
        </Field>
        <p className={s.note}>Shown in 2D as a marker. Its real shape only shows in the 3D view.</p>
      </div>
      <Position>
        <PointFields label="Position" point={el.position} unit={unit} onCommit={(position) => update({ ...el, position })} />
      </Position>
    </>
  );
}

export function ElementFields({ element, doc }: { element: Element; doc: DocState }) {
  switch (element.kind) {
    case "wall":
      return <WallFields el={element} doc={doc} />;
    case "opening":
      return <OpeningFields el={element} doc={doc} />;
    case "room":
      return <RoomFields el={element} doc={doc} />;
    case "column":
      return <ColumnFields el={element} doc={doc} />;
    case "stair":
      return <StairFields el={element} doc={doc} />;
    case "asset":
      return <AssetFields el={element} doc={doc} />;
    case "annotation":
      return <AnnotationFields el={element} doc={doc} />;
    case "dimension":
      return <DimensionFields el={element} doc={doc} />;
    case "camera":
      return <CameraFields el={element} doc={doc} />;
    case "underlay":
      return <UnderlayFields el={element} doc={doc} />;
    case "linework":
      return <LineworkFields el={element} doc={doc} />;
    case "reference_model":
      return <ReferenceModelFields el={element} doc={doc} />;
    case "pipe":
      return <PipeFields el={element} doc={doc} />;
  }
}
