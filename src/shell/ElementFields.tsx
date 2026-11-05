// Editable fields for one selected element, by kind. Every edit is one typed
// command: update_element, or the specific semantic command where one exists.
import { useState } from "react";
import type { CameraPreset, Command, DocState, Element, OpeningStyle, Point, RoofShape, SlabKind, WallAnchor } from "../contract/bindings";
import { SLAB_RAILING_HEIGHT_MM, SLAB_THICKNESS_MM, slabAreaMm2 } from "../editor2d/slab";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { MaterialPicker } from "../ui/MaterialPicker";
import { Button, Field, NumberField, ReadOnly, Section, Segmented, Select, Switch, TextField } from "../ui/controls";
import type { IconName } from "../ui/icons";
import { formatAreaMm2, formatLength } from "../ui/units";
import { DOOR_STYLES, WINDOW_STYLES } from "./actions";
import { commitLatest, rebase } from "./commitLatest";
import { DeviceBasics, DeviceSections, deviceIcon } from "./DeviceFields";
import { DEVICE_LABEL, catalogItem, isDevice } from "./devices";
import { LevelField, PointFields, Position, RailingFields, Rotation, Vec3Fields, update, type Of } from "./fields/common";
import { BuiltInFields } from "./fields/BuiltInFields";
import { DimensionFields } from "./fields/DimensionFields";
import { GridFields } from "./fields/GridFields";
import { LotFields } from "./fields/LotFields";
import { OpeningTypeFields } from "./fields/OpeningTypeFields";
import { RoofFields } from "./fields/RoofFields";
import { RoomFields } from "./fields/RoomFields";
import { StairFields } from "./fields/StairFields";
import { ViewMarkFields } from "./fields/ViewMarkFields";
import { PipeFields } from "./PipeFields";
import { PIPE_COLOR, pipeLength, runLabel, sizeLabel } from "./pipes";
import { useSection } from "./shellStore";
import { TourFields } from "./tours/TourFields";
import s from "./Inspector.module.css";

/** Queued after any edit still on its way, so edits land in the order they were made. */
const dispatch = (command: Command) => void commitLatest(() => command);
/** Moves one end of a wall, keeping the other end where it is now. */
const moveWallEnd = (wall: Of<"wall">, which: "start" | "end", point: Point) =>
  void commitLatest((doc) => {
    const now = doc.project.elements.find((e): e is Of<"wall"> => e.id === wall.id && e.kind === "wall");
    if (!now) return null;
    const moved = rebase(now[which], wall[which], point);
    return { type: "set_wall_endpoints", wall_id: wall.id, start: which === "start" ? moved : now.start, end: which === "end" ? moved : now.end };
  });

const CAMERA_PRESETS: Array<{ value: CameraPreset; label: string }> = [
  { value: "eye_level", label: "Eye level" },
  { value: "exterior_corner", label: "Exterior corner" },
  { value: "top", label: "Top view" },
  { value: "axonometric", label: "Axonometric" },
  { value: "room_interior", label: "Room interior" },
  { value: "custom", label: "Custom" },
];

const SLAB_KINDS: Array<{ value: SlabKind; label: string }> = [
  { value: "floor", label: "Floor" },
  { value: "balcony", label: "Balcony" },
  { value: "deck", label: "Deck" },
  { value: "void", label: "Void" },
];

const ROOF_SHAPE_LABEL: Record<RoofShape, string> = {
  flat: "Flat",
  shed: "Shed",
  gable: "Gable",
  hip: "Hip",
};

const SLAB_TITLE: Record<SlabKind, string> = {
  floor: "Floor slab",
  balcony: "Balcony",
  deck: "Deck",
  void: "Void",
};

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
    case "slab":
      return { title: SLAB_TITLE[el.slab_kind], subtitle: formatAreaMm2(slabAreaMm2(el.polygon)), icon: "slab" };
    case "roof":
      return { title: "Roof", subtitle: el.outline.type === "top_footprint" ? "Over the top floor" : ROOF_SHAPE_LABEL[el.shape], icon: "roof" };
    case "grid":
      return { title: "Grid line", subtitle: el.label, icon: "grid" };
    case "view_mark":
      return { title: el.mark_kind === "section" ? "Section mark" : "Elevation mark", subtitle: el.label, icon: "dimension" };
    case "lot":
      return { title: "Lot", subtitle: `${el.corners.length} corners`, icon: "pin" };
    case "tour":
      return { title: "Tour", subtitle: el.name, icon: "camera" };
  }
}

// ---------------------------------------------------------------- helpers

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
          <NumberField label="Wall thickness" kind="length" unit={unit} min={20} max={1000} value={el.thickness_mm} onCommit={(thickness_mm) => update(el, { ...el, thickness_mm })} />
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
            onCommit={(height_mm) => update(el, { ...el, height_mm })}
          />
        </Field>
        <Field label="">
          <label className={s.inlineSwitch}>
            <Switch label="Use the level height" checked={el.height_mm === null} onChange={(match) => update(el, { ...el, height_mm: match ? null : levelHeight })} />
            <span>Same as level</span>
          </label>
        </Field>
        <Field label="Material">
          <MaterialPicker label="Wall material" materials={doc.project.materials} prefer={["wall"]} value={el.material_id} noneLabel="Default wall finish" onChange={(material_id) => update(el, { ...el, material_id })} />
        </Field>
      </div>
      <Position>
        <PointFields label="Start" point={el.start} unit={unit} onCommit={(start) => moveWallEnd(el, "start", start)} />
        <PointFields label="End" point={el.end} unit={unit} onCommit={(end) => moveWallEnd(el, "end", end)} />
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
      <OpeningTypeFields el={el} doc={doc} />
      <Field label="Type">
        <Segmented
          label="Opening type"
          stretch
          value={el.opening_type}
          onChange={(opening_type) => {
            if (opening_type === el.opening_type) return;
            update(
              el,
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
          onChange={(style) => update(el, { ...el, style })}
          options={styles.some((o) => o.value === el.style) ? styles : [...styles, { value: el.style, label: STYLE_LABEL[el.style] }]}
        />
      </Field>
      <Field label="Width">
        <NumberField label="Width" kind="length" unit={unit} min={200} max={10000} step={50} value={el.width_mm} onCommit={(width_mm) => update(el, { ...el, width_mm })} />
      </Field>
      <Field label="Height">
        <NumberField label="Height" kind="length" unit={unit} min={200} max={6000} step={50} value={el.height_mm} onCommit={(height_mm) => update(el, { ...el, height_mm })} />
      </Field>
      {isDoor ? null : (
        <Field label="Sill height" hint="Bottom of the window above the floor">
          <NumberField label="Sill height" kind="length" unit={unit} min={0} max={5000} step={50} value={el.sill_mm} onCommit={(sill_mm) => update(el, { ...el, sill_mm })} />
        </Field>
      )}
      <Field label="From wall start" hint="Distance from the start of the wall to the center of the opening">
        <NumberField label="Offset along the wall" kind="length" unit={unit} min={0} max={host?.length_mm} step={50} value={el.offset_mm} onCommit={(offset_mm) => update(el, { ...el, offset_mm })} />
      </Field>
      {swings ? (
        <Field label="Swing">
          <Button size="sm" onClick={() => update(el, { ...el, flip_side: !el.flip_side })}>
            Flip side
          </Button>
          <Button size="sm" onClick={() => update(el, { ...el, flip_hinge: !el.flip_hinge })}>
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
          onChange={(material_id) => update(el, { ...el, material_id })}
        />
      </Field>
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
            onChange={(shape) => update(el, { ...el, shape })}
            options={[
              { value: "rect", label: "Rectangular" },
              { value: "round", label: "Round" },
            ]}
          />
        </Field>
        <Field label={round ? "Diameter" : "Width"}>
          <NumberField label={round ? "Diameter" : "Width"} kind="length" unit={unit} min={50} max={3000} value={el.width_mm} onCommit={(width_mm) => update(el, { ...el, width_mm })} />
        </Field>
        {round ? null : (
          <>
            <Field label="Depth">
              <NumberField label="Depth" kind="length" unit={unit} min={50} max={3000} value={el.depth_mm} onCommit={(depth_mm) => update(el, { ...el, depth_mm })} />
            </Field>
            <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update(el, { ...el, rotation_deg })} />
          </>
        )}
        <Field label="Material">
          <MaterialPicker label="Column material" materials={doc.project.materials} prefer={["wall", "generic"]} value={el.material_id} noneLabel="Default concrete" onChange={(material_id) => update(el, { ...el, material_id })} />
        </Field>
      </div>
      <Position>
        <PointFields label="Center" point={el.center} unit={unit} onCommit={(center) => update(el, { ...el, center })} />
      </Position>
    </>
  );
}

function SlabFields({ el, doc }: { el: Of<"slab">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const isVoid = el.slab_kind === "void";
  return (
    <div className={s.group}>
      {/* Full width: four kinds do not fit beside a field label. */}
      <Segmented
        label="Slab kind"
        stretch
        value={el.slab_kind}
        options={SLAB_KINDS}
        // A void has no thickness: turning one back into a floor gives it the default.
        onChange={(slab_kind) => update(el, { ...el, slab_kind, thickness_mm: isVoid && slab_kind !== "void" && el.thickness_mm <= 0 ? SLAB_THICKNESS_MM : el.thickness_mm })}
      />
      {isVoid ? null : (
        <Field label="Thickness">
          <NumberField label="Slab thickness" kind="length" unit={unit} min={50} max={1000} step={10} value={el.thickness_mm} onCommit={(thickness_mm) => update(el, { ...el, thickness_mm })} />
        </Field>
      )}
      <Field label="Top offset" hint="Top of the slab above the level floor. Negative sinks it">
        <NumberField label="Top offset" kind="length" unit={unit} min={-3000} max={3000} step={10} value={el.top_offset_mm} onCommit={(top_offset_mm) => update(el, { ...el, top_offset_mm })} />
      </Field>
      <RailingFields railing={el.railing} unit={unit} defaultHeight={SLAB_RAILING_HEIGHT_MM} what="Railing" onChange={(railing) => update(el, { ...el, railing })} />
      <Field label="Area" hint="Plan area inside the outline">
        <ReadOnly>{formatAreaMm2(slabAreaMm2(el.polygon))}</ReadOnly>
      </Field>
      {isVoid ? <p className={s.note}>The floor inside this outline is cut away, so the space below is double height.</p> : null}
    </div>
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
            <TextField label="Object name" value={el.name} required onCommit={(name) => update(el, { ...el, name })} />
          </Field>
          <DeviceBasics el={el} item={item} doc={doc} />
        </div>
        <DeviceSections el={el} item={item} doc={doc} />
        <AssetSize>
          <Field label="Width">
            <NumberField label="Width" kind="length" unit={unit} min={10} max={20000} value={el.width_mm} onCommit={(width_mm) => update(el, { ...el, width_mm })} />
          </Field>
          <Field label="Depth">
            <NumberField label="Depth" kind="length" unit={unit} min={10} max={20000} value={el.depth_mm} onCommit={(depth_mm) => update(el, { ...el, depth_mm })} />
          </Field>
          <Field label="Height">
            <NumberField label="Height" kind="length" unit={unit} min={10} max={20000} value={el.height_mm} onCommit={(height_mm) => update(el, { ...el, height_mm })} />
          </Field>
          <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update(el, { ...el, rotation_deg })} />
        </AssetSize>
        <Position>
          <PointFields label="Center" point={el.position} unit={unit} onCommit={(position) => update(el, { ...el, position })} />
        </Position>
      </>
    );
  }
  return (
    <>
      <div className={s.group}>
        <Field label="Name">
          <TextField label="Object name" value={el.name} required onCommit={(name) => update(el, { ...el, name })} />
        </Field>
        <Field label="Width">
          <NumberField label="Width" kind="length" unit={unit} min={10} max={20000} value={el.width_mm} onCommit={(width_mm) => update(el, { ...el, width_mm })} />
        </Field>
        <Field label="Depth">
          <NumberField label="Depth" kind="length" unit={unit} min={10} max={20000} value={el.depth_mm} onCommit={(depth_mm) => update(el, { ...el, depth_mm })} />
        </Field>
        <Field label="Height">
          <NumberField label="Height" kind="length" unit={unit} min={10} max={20000} value={el.height_mm} onCommit={(height_mm) => update(el, { ...el, height_mm })} />
        </Field>
        <Field label="Above floor" hint="Height of the underside above the floor">
          <NumberField label="Height above the floor" kind="length" unit={unit} min={0} max={20000} step={50} value={el.elevation_mm} onCommit={(elevation_mm) => update(el, { ...el, elevation_mm })} />
        </Field>
        <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update(el, { ...el, rotation_deg })} />
      </div>
      {item?.parametric ? <BuiltInFields el={el} item={item} doc={doc} /> : null}
      <Position>
        <PointFields label="Center" point={el.position} unit={unit} onCommit={(position) => update(el, { ...el, position })} />
      </Position>
    </>
  );
}

function AnnotationFields({ el, doc }: { el: Of<"annotation">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  return (
    <>
      <div className={s.group}>
        <TextField label="Text" multiline value={el.text} placeholder="Write a note" onCommit={(text) => update(el, { ...el, text })} />
        <p className={s.note}>Press Ctrl or Cmd with Enter to apply.</p>
        <Field label="Text height" hint="In model units at the project scale">
          <NumberField label="Text height" kind="length" unit={unit} min={10} max={5000} value={el.size_mm} onCommit={(size_mm) => update(el, { ...el, size_mm })} />
        </Field>
        <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update(el, { ...el, rotation_deg })} />
      </div>
      <Position>
        <PointFields label="Anchor" point={el.position} unit={unit} onCommit={(position) => update(el, { ...el, position })} />
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
          <TextField label="Camera name" value={el.name} required onCommit={(name) => update(el, { ...el, name })} />
        </Field>
        <Field label="Preset">
          <Select label="Camera preset" value={el.preset} options={CAMERA_PRESETS} onChange={(preset) => update(el, { ...el, preset })} />
        </Field>
        <Field label="Field of view">
          <NumberField label="Field of view" suffix="deg" min={10} max={120} step={5} value={el.fov_deg} onCommit={(fov_deg) => update(el, { ...el, fov_deg })} />
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
        <Vec3Fields label="Camera" value={el.position} unit={unit} onCommit={(position) => update(el, { ...el, position, preset: "custom" })} />
        <Vec3Fields label="Looks at" value={el.target} unit={unit} onCommit={(target) => update(el, { ...el, target, preset: "custom" })} />
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
          <NumberField label="Millimeters per pixel" suffix="mm/px" min={0.01} max={1000} decimals={3} step={0.5} value={el.mm_per_px} onCommit={(mm_per_px) => update(el, { ...el, mm_per_px })} />
        </Field>
        <Field label="">
          <label className={s.inlineSwitch}>
            <Switch label="Scale checked" checked={el.scale_confirmed} onChange={(scale_confirmed) => update(el, { ...el, scale_confirmed })} />
            <span>I checked this scale</span>
          </label>
        </Field>
        {el.scale_confirmed ? null : <p className={s.note}>Measure one known length on the image before tracing. The scale is never guessed.</p>}
        <Field label="Opacity">
          <NumberField label="Opacity" suffix="%" min={5} max={100} step={5} decimals={0} value={Math.round(el.opacity * 100)} onCommit={(v) => update(el, { ...el, opacity: v / 100 })} />
        </Field>
        <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update(el, { ...el, rotation_deg })} />
        <Field label="">
          <label className={s.inlineSwitch}>
            <Switch label="Lock the image" checked={el.locked} onChange={(locked) => update(el, { ...el, locked })} />
            <span>Locked in place</span>
          </label>
        </Field>
      </div>
      <Position>
        <PointFields label="Bottom left" point={el.position} unit={unit} onCommit={(position) => update(el, { ...el, position })} />
      </Position>
    </>
  );
}

function LineworkFields({ el, doc }: { el: Of<"linework">; doc: DocState }) {
  return (
    <div className={s.group}>
      <Field label="Name">
        <TextField label="Linework name" value={el.name} required onCommit={(name) => update(el, { ...el, name })} />
      </Field>
      <LevelField levelId={el.level_id} doc={doc} onChange={(level_id) => update(el, { ...el, level_id })} />
      <Field label="Polylines" hint="Traced lines brought in from the source file">
        <ReadOnly>{el.polylines.length}</ReadOnly>
      </Field>
      <Field label="">
        <label className={s.inlineSwitch}>
          <Switch label="Lock the linework" checked={el.locked} onChange={(locked) => update(el, { ...el, locked })} />
          <span>Locked in place</span>
        </label>
      </Field>
      <p className={s.note}>Imported tracing reference, on the Underlays layer. Move, rotate or delete it; the lines themselves are not editable.</p>
    </div>
  );
}

// ---------------------------------------------------------------- pipes: PipeFields.tsx

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
          <TextField label="Reference model name" value={el.name} required onCommit={(name) => update(el, { ...el, name })} />
        </Field>
        <LevelField levelId={el.level_id} doc={doc} onChange={(level_id) => update(el, { ...el, level_id })} />
        <Field label="Model unit" hint="What one unit in the file means, so it lands at the right size">
          <Segmented label="Model unit" stretch value={selectedUnit} onChange={(v) => update(el, { ...el, scale_to_mm: MODEL_UNITS.find((u) => u.value === v)!.mm })} options={[...MODEL_UNITS]} />
        </Field>
        <Field label="Scale to mm" hint="Multiply model units by this to get millimeters">
          <NumberField label="Scale to millimeters" min={0.0001} decimals={4} value={el.scale_to_mm} onCommit={(scale_to_mm) => update(el, { ...el, scale_to_mm })} />
        </Field>
        <Field label="Elevation" hint="Height of the model origin above the level floor">
          <NumberField label="Elevation" kind="length" unit={unit} step={50} value={el.elevation_mm} onCommit={(elevation_mm) => update(el, { ...el, elevation_mm })} />
        </Field>
        <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => update(el, { ...el, rotation_deg })} />
        <Field label="">
          <label className={s.inlineSwitch}>
            <Switch label="Lock the model" checked={el.locked} onChange={(locked) => update(el, { ...el, locked })} />
            <span>Locked in place</span>
          </label>
        </Field>
        <p className={s.note}>Shown in 2D as a marker. Its real shape only shows in the 3D view.</p>
      </div>
      <Position>
        <PointFields label="Position" point={el.position} unit={unit} onCommit={(position) => update(el, { ...el, position })} />
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
    case "slab":
      return <SlabFields el={element} doc={doc} />;
    case "roof":
      return <RoofFields el={element} doc={doc} />;
    case "grid":
      return <GridFields el={element} doc={doc} />;
    case "view_mark":
      return <ViewMarkFields el={element} doc={doc} />;
    case "lot":
      return <LotFields el={element} doc={doc} />;
    case "tour":
      return <TourFields el={element} doc={doc} />;
  }
}
