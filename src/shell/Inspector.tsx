// Contextual inspector. Shows only what is relevant to the selection:
// nothing selected -> project, review, schedules, site, level, roof, layers;
// one element -> its fields; several -> shared actions.
import { useEffect, useMemo, useRef } from "react";
import type { Command, DocState, Element, Layer, LayerKey, PaperSize, ProjectSettings, Roof, Site } from "../contract/bindings";
import { bus } from "../state/bus";
import { useApp, useVisibleDoc } from "../state/store";
import { MaterialPicker } from "../ui/MaterialPicker";
import { Button, Field, IconButton, NumberField, Section, Segmented, Select, TextField, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import { formatArea } from "../ui/units";
import { DevicesMultiFields } from "./DeviceFields";
import { catalogItem, isDevice, type AssetEl } from "./devices";
import { ElementFields, elementTitle } from "./ElementFields";
import { LevelsSection } from "./LevelsSection";
import { PipesMultiFields, PlumbingSection, useReveal } from "./PipeSections";
import { ReviewSection } from "./ReviewSection";
import { SchedulesSection } from "./SchedulesSection";
import { ROOF_KINDS, deleteSelection } from "./actions";
import { EMPTY_NETWORK, SERVICE_LAYER_COLOR, SERVICE_LAYER_ORDER, isServiceLayer, lengthByLayer } from "./pipes";
import { hasScheduledDevices } from "./schedules";
import { CUSTOM_SITE, SITE_PRESETS, editSite, presetByKey, siteFromPreset, siteLabel, siteOf, utcLabel } from "./site";
import { isReviewShown, useProjectName, useSection, useShell } from "./shellStore";
import s from "./Inspector.module.css";

const dispatch = (command: Command) => void useApp.getState().dispatch(command);

// ---------------------------------------------------------------- nothing selected

const SCALES = [10, 20, 25, 50, 75, 100, 125, 150, 200, 250, 500];
const PAPERS: Array<{ value: PaperSize; label: string }> = [
  { value: "a4", label: "A4 (210 x 297)" },
  { value: "a3", label: "A3 (297 x 420)" },
  { value: "a2", label: "A2 (420 x 594)" },
  { value: "a1", label: "A1 (594 x 841)" },
];

const LAYER_LABEL: Record<LayerKey, string> = {
  walls: "Walls",
  openings: "Doors and windows",
  rooms: "Rooms and labels",
  columns: "Columns",
  stairs: "Stairs",
  assets: "Objects",
  annotations: "Text",
  dimensions: "Dimensions",
  underlays: "Traced image",
  cold_water: "Cold water",
  hot_water: "Hot water",
  drainage: "Drainage",
  vent: "Vent",
  storm: "Storm drainage",
  electrical: "Electrical",
  aircon: "Aircon",
};

function ProjectSection({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("project", true);
  const name = useProjectName();
  const st = doc.project.settings;
  const set = (patch: Partial<ProjectSettings>) => dispatch({ type: "set_project_settings", settings: { ...st, ...patch } });
  return (
    <Section title="Project" icon="folder" open={open} onToggle={toggle}>
      <Field label="Name">
        <TextField label="Project name" value={name} required onCommit={(v) => void useApp.getState().renameProject(v)} />
      </Field>
      <Field label="Client">
        <TextField label="Client" value={st.client_name} placeholder="Who is it for" onCommit={(v) => set({ client_name: v })} />
      </Field>
      <Field label="Location">
        <TextField label="Location" value={st.location} placeholder="City or barangay" onCommit={(v) => set({ location: v })} />
      </Field>
      <Field label="Designer">
        <TextField label="Designer" value={st.designer} placeholder="Your name or firm" onCommit={(v) => set({ designer: v })} />
      </Field>
    </Section>
  );
}

function DrawingSection({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("drawing", false);
  const st = doc.project.settings;
  const unit = st.display_unit;
  const set = (patch: Partial<ProjectSettings>) => dispatch({ type: "set_project_settings", settings: { ...st, ...patch } });
  const scales = SCALES.includes(st.scale_denominator) ? SCALES : [...SCALES, st.scale_denominator].sort((a, b) => a - b);
  return (
    <Section title="Drawing setup" icon="dimension" open={open} onToggle={toggle}>
      <Field label="Units">
        <Segmented
          label="Display unit"
          stretch
          value={unit}
          onChange={(v) => set({ display_unit: v })}
          options={[
            { value: "mm", label: "Millimeters" },
            { value: "m", label: "Meters" },
          ]}
        />
      </Field>
      <Field label="Scale">
        <Select
          label="Drawing scale"
          value={String(st.scale_denominator)}
          onChange={(v) => set({ scale_denominator: Number(v) })}
          options={scales.map((n) => ({ value: String(n), label: `1:${n}` }))}
        />
      </Field>
      <Field label="Paper">
        <Select label="Paper size" value={st.paper} onChange={(v) => set({ paper: v })} options={PAPERS} />
      </Field>
      <Field label="Wall thickness" hint="Used by new walls unless the wall tool says otherwise">
        <NumberField label="Default wall thickness" kind="length" unit={unit} min={50} max={1000} value={st.default_wall_thickness_mm} onCommit={(v) => set({ default_wall_thickness_mm: v })} />
      </Field>
      <Field label="Grid spacing">
        <NumberField label="Grid spacing" kind="length" unit={unit} min={10} max={5000} value={st.grid_mm} onCommit={(v) => set({ grid_mm: v })} />
      </Field>
    </Section>
  );
}

const CITY_OPTIONS = [...SITE_PRESETS.map((p) => ({ value: p.key, label: p.label })), { value: CUSTOM_SITE, label: "Custom" }];

/** Where the house stands and which way is north, for the sun (docs/CONTRACT.md, "Sun and light"). */
function SiteSection({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("site", false);
  const ref = useRef<HTMLDivElement>(null);
  const flash = useReveal("site", ref);
  const st = doc.project.settings;
  const site = siteOf(st);
  const set = (patch: Partial<ProjectSettings>) => dispatch({ type: "set_project_settings", settings: { ...st, ...patch } });
  const setSite = (next: Site) => set({ site: next });
  return (
    <div ref={ref} className={cx(s.reveal, flash && s.revealFlash)}>
      <Section title="Site" icon="pin" open={open} onToggle={toggle} aside={<span className={s.sectionAside}>{siteLabel(site)}</span>}>
        <Field label="City" hint="Sets the latitude, the longitude and Philippine time">
          <Select
            label="City"
            value={presetByKey(site.city) ? site.city : CUSTOM_SITE}
            options={CITY_OPTIONS}
            onChange={(key) => setSite(siteFromPreset(key) ?? { ...site, city: CUSTOM_SITE })}
          />
        </Field>
        <Field label="Latitude" hint="Degrees, north positive">
          <NumberField label="Latitude" suffix="°N" min={-90} max={90} step={0.1} decimals={4} value={site.latitude_deg} onCommit={(v) => setSite(editSite(site, { latitude_deg: v }))} />
        </Field>
        <Field label="Longitude" hint="Degrees, east positive">
          <NumberField label="Longitude" suffix="°E" min={-180} max={180} step={0.1} decimals={4} value={site.longitude_deg} onCommit={(v) => setSite(editSite(site, { longitude_deg: v }))} />
        </Field>
        <Field label="UTC offset" hint="Hours from UTC. Philippine time is +8 all year, with no daylight saving">
          <NumberField label="UTC offset in hours" suffix="h" min={-12} max={14} step={0.5} decimals={2} value={site.utc_offset_min / 60} onCommit={(h) => setSite(editSite(site, { utc_offset_min: Math.round(h * 60) }))} />
          <span className={s.fieldAfter}>{utcLabel(site.utc_offset_min)}</span>
        </Field>
        <Field label="North angle" hint="Rotation of true north from the top of the sheet, counter-clockwise">
          <NumberField label="North angle" suffix="deg" min={-360} max={360} step={5} value={st.north_angle_deg} onCommit={(v) => set({ north_angle_deg: v })} />
          <Icon name="north" size={16} className={s.north} style={{ transform: `rotate(${-st.north_angle_deg}deg)` }} />
        </Field>
        <p className={s.note}>Places the sun in the 3D view, renders and shadow studies. City presets are built in; nothing is looked up online.</p>
      </Section>
    </div>
  );
}

function RoofSection({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("roof", true);
  const roof = doc.project.roof;
  const unit = doc.project.settings.display_unit;
  const set = (patch: Partial<Roof>) => dispatch({ type: "set_roof", roof: { ...roof, ...patch } });
  const sloped = roof.kind === "shed" || roof.kind === "gable";
  return (
    <Section title="Roof" icon="roof" open={open} onToggle={toggle}>
      <Segmented label="Roof type" stretch value={roof.kind} onChange={(kind) => set({ kind })} options={ROOF_KINDS} />
      {roof.kind === "none" ? (
        <p className={s.note}>No roof is drawn. Pick a type to cover the building outline.</p>
      ) : (
        <>
          {sloped ? (
            <Field label="Pitch">
              <NumberField label="Roof pitch" suffix="deg" min={1} max={60} value={roof.pitch_deg} onCommit={(v) => set({ pitch_deg: v })} />
            </Field>
          ) : null}
          <Field label="Overhang" hint="Eave distance past the outer wall face">
            <NumberField label="Roof overhang" kind="length" unit={unit} min={0} max={3000} step={50} value={roof.overhang_mm} onCommit={(v) => set({ overhang_mm: v })} />
          </Field>
          <Field label="Thickness">
            <NumberField label="Roof thickness" kind="length" unit={unit} min={10} max={1000} value={roof.thickness_mm} onCommit={(v) => set({ thickness_mm: v })} />
          </Field>
          {sloped ? (
            <Field label={roof.kind === "gable" ? "Ridge runs" : "Slopes along"}>
              <Segmented
                label="Ridge axis"
                stretch
                value={roof.ridge_axis}
                onChange={(ridge_axis) => set({ ridge_axis })}
                options={[
                  { value: "x", label: "East-west" },
                  { value: "y", label: "North-south" },
                ]}
              />
            </Field>
          ) : null}
          <Field label="Material">
            <MaterialPicker label="Roof material" materials={doc.project.materials} prefer={["roof", "metal"]} value={roof.material_id} noneLabel="Default roofing" onChange={(material_id) => set({ material_id })} />
          </Field>
        </>
      )}
    </Section>
  );
}

function LayerRow({ layer, swatch, meta }: { layer: Layer; swatch?: string; meta?: string | null }) {
  const label = LAYER_LABEL[layer.key] ?? layer.key;
  return (
    <li className={cx(s.layer, !layer.visible && s.layerHidden)}>
      {swatch ? <span className={s.layerSwatch} style={{ background: swatch }} aria-hidden /> : null}
      <span className={s.layerName}>{label}</span>
      {meta ? <span className={s.layerMeta}>{meta}</span> : null}
      <IconButton
        icon={layer.locked ? "lock" : "unlock"}
        label={layer.locked ? `Unlock ${label}` : `Lock ${label}`}
        tip={layer.locked ? "Locked. Click to allow edits" : "Lock against edits"}
        tipSide="top-end"
        size={15}
        active={layer.locked}
        className={s.layerButton}
        onClick={() => dispatch({ type: "set_layer", layer: { ...layer, locked: !layer.locked } })}
      />
      <IconButton
        icon={layer.visible ? "eye" : "eyeOff"}
        label={layer.visible ? `Hide ${label}` : `Show ${label}`}
        tip={layer.visible ? "Hide" : "Show"}
        tipSide="top-end"
        size={15}
        className={s.layerButton}
        onClick={() => dispatch({ type: "set_layer", layer: { ...layer, visible: !layer.visible } })}
      />
    </li>
  );
}

/** Objects drawn on a service layer: lights and electrical devices on electrical, units on aircon. */
function objectsOnLayer(doc: DocState, key: LayerKey): number {
  if (key !== "electrical" && key !== "aircon") return 0;
  return doc.project.elements.filter(
    (e) => e.kind === "asset" && (key === "aircon" ? e.category === "aircon" : e.category === "electrical" || e.category === "lighting"),
  ).length;
}

function LayersSection({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("layers", false);
  const hidden = doc.project.layers.filter((l) => !l.visible).length;
  const building = doc.project.layers.filter((l) => !isServiceLayer(l.key));
  // Services in display order: plumbing, storm, electrical, aircon.
  const services = SERVICE_LAYER_ORDER.map((key) => doc.project.layers.find((l) => l.key === key)).filter((l): l is Layer => l !== undefined);
  const lengths = lengthByLayer((doc.derived.pipes ?? EMPTY_NETWORK).takeoff);
  const meta = (key: LayerKey) => {
    const m = lengths[key] ?? 0;
    const n = objectsOnLayer(doc, key);
    const parts = [m > 0 ? `${m.toFixed(1)} m` : null, n > 0 ? `${n} obj` : null].filter(Boolean);
    return parts.length > 0 ? parts.join(", ") : null;
  };
  return (
    <Section title="Layers" icon="layers" open={open} onToggle={toggle} aside={hidden > 0 ? <span className={s.sectionAside}>{hidden} hidden</span> : null}>
      <ul className={s.layers}>
        {building.map((layer) => (
          <LayerRow key={layer.key} layer={layer} />
        ))}
      </ul>
      {services.length > 0 ? (
        <>
          <div className={s.layerSubhead}>Services</div>
          <ul className={s.layers}>
            {services.map((layer) => (
              <LayerRow key={layer.key} layer={layer} swatch={SERVICE_LAYER_COLOR[layer.key]} meta={meta(layer.key)} />
            ))}
          </ul>
        </>
      ) : null}
    </Section>
  );
}

/** Nothing selected, or the review's shown objects: `shown` counts those. */
function ProjectInspector({ doc, shown = 0 }: { doc: DocState; shown?: number }) {
  const t = doc.derived.totals;
  const catalog = useApp((st) => st.catalog);
  const scheduled = useMemo(() => hasScheduledDevices(doc, catalog), [doc, catalog]);
  return (
    <>
      <header className={s.head}>
        <div className={s.headText}>
          <strong>Project</strong>
          <span>{shown > 0 ? `Showing ${shown === 1 ? "1 object" : `${shown} objects`} from the review` : "Nothing selected"}</span>
        </div>
        {shown > 0 ? (
          <Button size="sm" className={s.headButton} onClick={() => useShell.getState().setReviewShown(null)}>
            Edit {shown === 1 ? "it" : "them"}
          </Button>
        ) : null}
      </header>
      <div className={s.scroll}>
        <dl className={s.totals}>
          <div>
            <dt>Floor area</dt>
            <dd>{formatArea(t.floor_area_m2)}</dd>
          </div>
          <div>
            <dt>Rooms</dt>
            <dd>{t.room_count}</dd>
          </div>
          <div>
            <dt>Doors</dt>
            <dd>{t.door_count}</dd>
          </div>
          <div>
            <dt>Windows</dt>
            <dd>{t.window_count}</dd>
          </div>
        </dl>
        <ReviewSection doc={doc} />
        {scheduled ? <SchedulesSection doc={doc} /> : null}
        {doc.project.elements.some((e) => e.kind === "pipe") ? <PlumbingSection doc={doc} /> : null}
        <ProjectSection doc={doc} />
        <DrawingSection doc={doc} />
        <SiteSection doc={doc} />
        <LevelsSection doc={doc} />
        <RoofSection doc={doc} />
        <LayersSection doc={doc} />
      </div>
    </>
  );
}

// ---------------------------------------------------------------- several selected

const KIND_PLURAL: Record<Element["kind"], [string, string]> = {
  wall: ["wall", "walls"],
  opening: ["door or window", "doors and windows"],
  room: ["room", "rooms"],
  column: ["column", "columns"],
  stair: ["stair", "stairs"],
  asset: ["object", "objects"],
  annotation: ["text note", "text notes"],
  dimension: ["dimension", "dimensions"],
  camera: ["camera", "cameras"],
  underlay: ["traced image", "traced images"],
  linework: ["linework", "linework"],
  reference_model: ["reference model", "reference models"],
  pipe: ["pipe", "pipes"],
};

function MultiInspector({ doc, picked }: { doc: DocState; picked: Element[] }) {
  const catalog = useApp((st) => st.catalog);
  const devices = picked.filter((e): e is AssetEl => e.kind === "asset" && isDevice(e, catalogItem(catalog, e.catalog_key)));
  const counts = useMemo(() => {
    const map = new Map<Element["kind"], number>();
    for (const e of picked) map.set(e.kind, (map.get(e.kind) ?? 0) + 1);
    return [...map.entries()];
  }, [picked]);

  const withMaterial = picked.filter((e): e is Extract<Element, { material_id: string | null }> => "material_id" in e);
  const rooms = picked.filter((e): e is Extract<Element, { kind: "room" }> => e.kind === "room");
  const pipes = picked.filter((e): e is Extract<Element, { kind: "pipe" }> => e.kind === "pipe");
  const sharedOf = (values: Array<string | null>) => (values.every((v) => v === values[0]) ? { value: values[0], mixed: false } : { value: null, mixed: true });
  const mat = sharedOf(withMaterial.map((e) => e.material_id));
  const floor = sharedOf(rooms.map((e) => e.floor_material_id));

  return (
    <>
      <header className={s.head}>
        <Icon name="select" size={18} className={s.headIcon} />
        <div className={s.headText}>
          <strong>{picked.length} selected</strong>
          <span>{counts.map(([kind, n]) => `${n} ${KIND_PLURAL[kind][n === 1 ? 0 : 1]}`).join(", ")}</span>
        </div>
        <IconButton icon="fit" label="Zoom to the selection" tipSide="bottom-end" onClick={() => bus.emit("focus_elements", picked.map((e) => e.id))} />
      </header>
      <div className={s.scroll}>
        <div className={s.group}>
          {withMaterial.length > 0 ? (
            <Field label="Material">
              <MaterialPicker
                label="Material for the selection"
                materials={doc.project.materials}
                value={mat.value}
                mixed={mat.mixed}
                onChange={(id) => {
                  if (id) dispatch({ type: "set_material", ids: withMaterial.map((e) => e.id), material_id: id });
                }}
              />
            </Field>
          ) : null}
          {rooms.length > 0 ? (
            <Field label="Floor finish">
              <MaterialPicker
                label="Floor material for the selected rooms"
                materials={doc.project.materials}
                prefer={["floor", "wood"]}
                value={floor.value}
                mixed={floor.mixed}
                onChange={(id) =>
                  dispatch({
                    type: "batch",
                    label: "Set floor finish",
                    commands: rooms.map((r) => ({ type: "update_element", element: { ...r, floor_material_id: id } })),
                  })
                }
              />
            </Field>
          ) : null}
          {pipes.length > 0 ? <PipesMultiFields pipes={pipes} /> : null}
          {devices.length > 0 ? <DevicesMultiFields devices={devices} /> : null}
          {withMaterial.length === 0 && rooms.length === 0 && pipes.length === 0 && devices.length === 0 ? <p className={s.note}>These items have no shared settings. Select one to edit it.</p> : null}
        </div>
        <div className={s.group}>
          <Button variant="danger" icon="trash" onClick={deleteSelection}>
            Delete {picked.length} items
          </Button>
        </div>
      </div>
    </>
  );
}

// ---------------------------------------------------------------- root

export function Inspector() {
  // The visible doc so the inspector matches what the canvas draws: the AI
  // ghost while a preview is pending, the committed document otherwise.
  const doc = useVisibleDoc();
  const previewing = useApp((st) => st.preview !== null);
  const selection = useApp((st) => st.selection);
  const picked = useMemo(() => (doc ? doc.project.elements.filter((e) => selection.includes(e.id)) : []), [doc, selection]);
  const reviewShown = useShell((st) => st.reviewShown);
  const fromReview = isReviewShown(reviewShown, selection);
  // A selection made anywhere else ends the review's hold on the inspector.
  useEffect(() => {
    if (reviewShown && !fromReview) useShell.getState().setReviewShown(null);
  }, [reviewShown, fromReview]);

  if (!doc) return null;
  const head = picked.length === 1 ? elementTitle(picked[0], doc) : null;
  return (
    <div className={s.inspector}>
      {previewing ? (
        <div className={s.previewBanner}>
          <Icon name="copilot" size={14} />
          Previewing AI proposal
        </div>
      ) : null}
      {/* Read-only while a preview is pending: fields would edit a ghost the
          user has not accepted yet. `inert` also pulls focus and tabbing out. */}
      <div className={s.inspectorBody} inert={previewing}>
        {picked.length === 0 || fromReview ? (
          <ProjectInspector doc={doc} shown={fromReview ? picked.length : 0} />
        ) : head ? (
          <>
            <header className={s.head}>
              <Icon name={head.icon} size={18} className={s.headIcon} style={head.tint ? { color: head.tint } : undefined} />
              <div className={s.headText}>
                <strong>{head.title}</strong>
                <span>{head.subtitle}</span>
              </div>
              <IconButton icon="fit" label="Zoom to this" tipSide="bottom-end" onClick={() => bus.emit("focus_elements", [picked[0].id])} />
              <IconButton icon="trash" label="Delete" tip="Delete (Del)" tipSide="bottom-end" onClick={deleteSelection} />
            </header>
            <div className={s.scroll}>
              {/* Cross-fades on selection change (MOTION.md, Inspector row): a fresh
                  wrapper per element id, so the animation plays without remounting
                  fields the user might be mid-edit on within the same element. */}
              <div key={picked[0].id} className={s.fieldsEnter}>
                <ElementFields element={picked[0]} doc={doc} />
              </div>
            </div>
          </>
        ) : (
          <MultiInspector doc={doc} picked={picked} />
        )}
      </div>
    </div>
  );
}
