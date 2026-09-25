// Inspector parts for electrical, lighting and aircon objects: the circuit tag
// and mounting height, the light a fixture gives, an aircon unit's line set,
// and links (what a switch controls, what switches a light). Several devices
// selected share the circuit tag and on or off.
//
// Guhit coordinates: the circuit tag is a free label and nothing here checks
// circuits, loads, ratings or aircon sizing (DECISIONS D21).
import type { ReactNode } from "react";
import type { AirconSpec, AssetLight, CatalogItem, Command, DocState, Element } from "../contract/bindings";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { Button, Field, IconButton, NumberField, ReadOnly, Segmented, Switch, TextField, cx } from "../ui/controls";
import { Icon, type IconName } from "../ui/icons";
import { useListPresence } from "../ui/useListPresence";
import { activateTool, linkFrom } from "./actions";
import {
  AIRCON_ROLE_LABEL,
  DEVICE_LABEL,
  LIGHT_COLORS,
  LUMEN_PRESETS,
  approxLedWatts,
  canLink,
  colorOf,
  elevationFor,
  hpLabel,
  isThreeWay,
  kelvinOf,
  lightsOn,
  lineLimitsLabel,
  lineSetLabel,
  linkedFrom,
  linkedTo,
  mountRef,
  mountingHeight,
  sharedCircuit,
  withoutLink,
  type AssetEl,
  type LightColor,
} from "./devices";
import s from "./Inspector.module.css";

const dispatch = (command: Command) => void useApp.getState().dispatch(command);
const update = (element: Element) => dispatch({ type: "update_element", element });

/** The icon for a device in lists and the inspector head. */
export function deviceIcon(item: CatalogItem | undefined, asset?: AssetEl): IconName {
  if (item?.aircon) return "aircon";
  if (item?.light || asset?.light) return "bulb";
  if (item?.device === "switch") return "link";
  if (item?.device) return "plug";
  return "asset";
}

// ---------------------------------------------------------------- one device

/** Circuit tag and mounting height, in the object's first group. */
export function DeviceBasics({ el, item, doc }: { el: AssetEl; item: CatalogItem | undefined; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const ref = mountRef(item);
  return (
    <>
      {item?.device ? (
        <Field label="Counts as" hint="The row of the PH electrical inspection form it counts under">
          <ReadOnly>{DEVICE_LABEL[item.device][0]}</ReadOnly>
        </Field>
      ) : null}
      <Field label="Circuit" hint="A free tag for the schedules, for example L1 or C3">
        <TextField label="Circuit tag" value={el.circuit} placeholder="For example L1" onCommit={(circuit) => update({ ...el, circuit })} />
      </Field>
      <Field label="Mounting" hint={ref === "center" ? "Floor to the center of the box" : "Floor to the underside"}>
        <NumberField
          label={ref === "center" ? "Mounting height, floor to the center" : "Mounting height, floor to the underside"}
          kind="length"
          unit={unit}
          min={-5000}
          max={20000}
          step={50}
          value={mountingHeight(el, ref)}
          onCommit={(h) => update({ ...el, elevation_mm: elevationFor(h, el, ref) })}
        />
        <span className={s.fieldAfter}>{ref === "center" ? "to center" : "to underside"}</span>
      </Field>
    </>
  );
}

/** Light, aircon and links groups, after the object's first group. */
export function DeviceSections({ el, item, doc }: { el: AssetEl; item: CatalogItem | undefined; doc: DocState }) {
  return (
    <>
      {el.light ? <LightGroup el={el} light={el.light} /> : null}
      {item?.aircon ? <AirconGroup spec={item.aircon} /> : null}
      <LinksGroup el={el} item={item} doc={doc} />
      <p className={cx(s.note, s.deviceNote)}>The circuit tag is a label for the schedules. Circuits, loads and ratings are for the licensed engineer.</p>
    </>
  );
}

function GroupHead({ icon, children, aside }: { icon: IconName; children: ReactNode; aside?: ReactNode }) {
  return (
    <div className={s.groupHead}>
      <Icon name={icon} size={14} />
      <span>{children}</span>
      {aside}
    </div>
  );
}

function LightGroup({ el, light }: { el: AssetEl; light: AssetLight }) {
  const set = (patch: Partial<AssetLight>) => update({ ...el, light: { ...light, ...patch } });
  const color = colorOf(light.kelvin);
  return (
    <div className={s.group}>
      <GroupHead icon="bulb">Light</GroupHead>
      <Field label="On" hint="Night views light the fixtures that are on">
        <label className={s.inlineSwitch}>
          <Switch label="Light on" checked={light.on} onChange={(on) => set({ on })} />
          <span>{light.on ? "On in the model" : "Off in the model"}</span>
        </label>
      </Field>
      <Field label="Output" hint="Luminous flux. 900 lm is about a 9 W LED bulb">
        <NumberField label="Light output in lumens" suffix="lm" min={10} max={50000} step={100} decimals={0} value={light.lumens} onCommit={(lumens) => set({ lumens })} />
      </Field>
      <div className={s.indented}>
        <div className={s.chips} role="radiogroup" aria-label="Lumen presets">
          {LUMEN_PRESETS.map((v) => {
            const on = Math.abs(light.lumens - v) < 0.5;
            return (
              <button key={v} type="button" role="radio" aria-checked={on} className={cx(s.chip, on && s.chipOn)} onClick={() => set({ lumens: v })}>
                {v}
              </button>
            );
          })}
        </div>
        <span className={s.fieldHint}>About {approxLedWatts(light.lumens)} W of LED</span>
      </div>
      <div className={s.labelRow}>
        <span>Color</span>
        <span className={s.mono}>{Math.round(light.kelvin)} K</span>
      </div>
      <Segmented<LightColor>
        label="Light color"
        stretch
        value={color}
        options={LIGHT_COLORS.map((c) => ({ value: c.value, label: c.label, tip: `${c.kelvin} K` }))}
        onChange={(c) => set({ kelvin: kelvinOf(c) })}
      />
    </div>
  );
}

function AirconGroup({ spec }: { spec: AirconSpec }) {
  const hasLines = lineSetLabel(spec) !== null;
  const m = (v: number) => `${Math.round(v * 100) / 100} m`;
  const mm = (v: number) => `${Math.round(v * 100) / 100} mm`;
  return (
    <div className={s.group}>
      <GroupHead icon="aircon">Aircon, {AIRCON_ROLE_LABEL[spec.role].toLowerCase()} unit</GroupHead>
      <Field label="Capacity" hint="Nominal, as sold in PH">
        <ReadOnly>{hpLabel(spec.hp)}</ReadOnly>
      </Field>
      {hasLines ? (
        <>
          <Field label="Line set" hint="Outside diameters of the copper lines: liquid, then gas">
            <ReadOnly>
              {Math.round(spec.liquid_mm * 100) / 100} / {mm(spec.gas_mm)}
            </ReadOnly>
            <span className={s.fieldAfter}>liquid / gas</span>
          </Field>
          {lineLimitsLabel(spec) ? (
            <>
              <Field label="Length" hint="Line set length the manual allows">
                <ReadOnly>
                  {m(spec.min_line_m)} to {m(spec.max_line_m)}
                </ReadOnly>
              </Field>
              <Field label="Rise" hint="Largest height difference between the units">
                <ReadOnly>up to {m(spec.max_rise_m)}</ReadOnly>
              </Field>
            </>
          ) : null}
          {spec.included_line_m > 0 ? (
            <Field label="Included" hint="Line set in a standard installation. Installers usually bill each meter beyond it">
              <ReadOnly>{m(spec.included_line_m)}</ReadOnly>
              <span className={s.fieldAfter}>standard install</span>
            </Field>
          ) : null}
        </>
      ) : (
        <Field label="Line set">
          <ReadOnly>None, a window unit</ReadOnly>
        </Field>
      )}
      <p className={s.note}>From PH manufacturer manuals, for review. The unit's own manual and the PME decide.</p>
    </div>
  );
}

type LinkMode = "controls" | "feeds" | "switched_by" | "outlet" | "linked_from";

function linkMode(el: AssetEl, item: CatalogItem | undefined): LinkMode {
  if (item?.device === "switch") return "controls";
  if (canLink(el, item)) return "feeds";
  if (item?.aircon) return "outlet";
  if (el.light || item?.device === "lighting_outlet") return "switched_by";
  return "linked_from";
}

const LINK_TITLE: Record<LinkMode, string> = {
  controls: "Controls",
  feeds: "Feeds",
  switched_by: "Switched by",
  outlet: "Outlet",
  linked_from: "Linked from",
};

const LINK_EMPTY: Record<LinkMode, string> = {
  controls: "Controls nothing yet. Link it to the lights it switches.",
  feeds: "Feeds nothing yet.",
  switched_by: "No switch yet.",
  outlet: "No outlet yet. Link an aircon outlet to it.",
  linked_from: "Nothing links to it.",
};

const linkKey = (a: AssetEl) => a.id;

function LinksGroup({ el, item, doc }: { el: AssetEl; item: CatalogItem | undefined; doc: DocState }) {
  const catalog = useApp((st) => st.catalog);
  const mode = linkMode(el, item);
  const outgoing = mode === "controls" || mode === "feeds";
  const list = outgoing ? linkedTo(el, doc.project.elements) : linkedFrom(el.id, doc.project.elements);
  const rows = useListPresence(list, linkKey);
  // Aircon units show the outlet only for the units that plug in: indoor and window.
  if (mode === "outlet" && item?.aircon?.role === "outdoor" && list.length === 0) return null;
  if (mode === "linked_from" && list.length === 0) return null;
  const threeWay = mode === "controls" && isThreeWay(el, doc.project.elements);

  const remove = (other: AssetEl) => (outgoing ? update(withoutLink(el, other.id)) : update(withoutLink(other, el.id)));
  const go = (other: AssetEl) => {
    useApp.getState().select([other.id]);
    bus.emit("focus_elements", [other.id]);
  };

  return (
    <div className={s.group}>
      <GroupHead icon="link" aside={<span className={s.groupCount}>{list.length}</span>}>
        {LINK_TITLE[mode]}
      </GroupHead>
      {list.length === 0 ? <p className={s.note}>{LINK_EMPTY[mode]}</p> : null}
      {rows.length > 0 ? (
        <ul className={s.linkList}>
          {rows.map(({ key, item: other, entering, leaving }) => {
            const otherItem = catalog.find((c) => c.key === other.catalog_key);
            return (
              <li key={key} className={cx(s.linkRow, entering && s.linkRowIn, leaving && s.linkRowOut)} inert={leaving}>
                <div className={s.linkRowInner}>
                  <button type="button" className={s.linkName} onClick={() => go(other)} data-tip="Select it" data-tip-side="top-start">
                    <Icon name={deviceIcon(otherItem, other)} size={14} />
                    <span>{other.name}</span>
                  </button>
                  {other.circuit ? <span className={s.linkTag}>{other.circuit}</span> : null}
                  <IconButton icon="close" label={`Unlink ${other.name}`} tip="Unlink" tipSide="top-end" size={13} className={s.linkRemove} onClick={() => remove(other)} />
                </div>
              </li>
            );
          })}
        </ul>
      ) : null}
      {threeWay ? <p className={s.note}>Shares a light with another switch: a 3-way, drawn S3 on the plan.</p> : null}
      <div className={s.actionsRow}>
        {outgoing ? (
          <Button size="sm" icon="link" onClick={() => linkFrom(el.id)} data-tip="Link tool (L): click the lights it controls" data-tip-side="top-start">
            Link more
          </Button>
        ) : (
          <Button
            size="sm"
            icon="link"
            onClick={() => {
              useApp.getState().select([]);
              activateTool("link");
            }}
            data-tip={mode === "outlet" ? "Link tool (L): click the outlet, then this unit" : "Link tool (L): click a switch, then this"}
            data-tip-side="top-start"
          >
            {mode === "outlet" ? "Link an outlet" : "Link a switch"}
          </Button>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- several devices

/** Circuit tag and on or off for every selected device, each change one undo step. */
export function DevicesMultiFields({ devices }: { devices: AssetEl[] }) {
  const n = devices.length;
  const circuit = sharedCircuit(devices);
  const lights = lightsOn(devices);
  const setCircuit = (tag: string) =>
    dispatch({ type: "batch", label: n === 1 ? "Set circuit tag" : `Set circuit tag of ${n} devices`, commands: devices.map((d) => ({ type: "update_element", element: { ...d, circuit: tag } })) });
  const setOn = (on: boolean) => {
    const fixtures = devices.filter((d) => d.light && d.light.on !== on);
    if (fixtures.length === 0) return;
    dispatch({
      type: "batch",
      label: on ? `Turn on ${fixtures.length === 1 ? "a light" : `${fixtures.length} lights`}` : `Turn off ${fixtures.length === 1 ? "a light" : `${fixtures.length} lights`}`,
      commands: fixtures.map((d) => ({ type: "update_element", element: { ...d, light: { ...d.light!, on } } })),
    });
  };
  return (
    <>
      <Field label="Circuit" hint={`A free tag for the ${n} selected devices`}>
        <TextField label={`Circuit tag for the ${n} selected devices`} value={circuit.value} placeholder={circuit.mixed ? "Mixed tags" : "For example L1"} onCommit={setCircuit} />
      </Field>
      {lights.total > 0 ? (
        <Field label="Lights" hint="On or off in the model, for night views">
          <label className={s.inlineSwitch}>
            <Switch label={`Turn the ${lights.total} selected lights on`} checked={lights.on === lights.total} onChange={setOn} />
            <span>
              {lights.on === lights.total ? `All ${lights.total} on` : lights.on === 0 ? `All ${lights.total} off` : `${lights.on} of ${lights.total} on`}
            </span>
          </label>
        </Field>
      ) : null}
    </>
  );
}
