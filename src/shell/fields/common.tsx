// Field helpers shared by the element fields (ElementFields.tsx and
// fields/*.tsx): exact points, rotation, the tucked away position section,
// railings, the level picker. Every edit is one typed command.
import type { DocState, Element, Point, Railing, RailingStyle, Vec3 } from "../../contract/bindings";
import { Field, NumberField, ReadOnly, Section, Segmented, Select, Switch } from "../../ui/controls";
import { updateElement } from "../commitLatest";
import { useSection } from "../shellStore";
import s from "../Inspector.module.css";

export type Of<K extends Element["kind"]> = Extract<Element, { kind: K }>;

/** Lays the fields this edit changed over the latest element (`commitLatest.ts`). */
export const update = (base: Element, element: Element) => void updateElement(base, element);

export function PointFields({ label, point, unit, onCommit }: { label: string; point: Point; unit: "mm" | "m"; onCommit: (p: Point) => void }) {
  return (
    <Field label={label}>
      <NumberField label={`${label} X`} kind="length" unit={unit} value={point.x} onCommit={(x) => onCommit({ ...point, x })} />
      <NumberField label={`${label} Y`} kind="length" unit={unit} value={point.y} onCommit={(y) => onCommit({ ...point, y })} />
    </Field>
  );
}

export function Vec3Fields({ label, value, unit, onCommit }: { label: string; value: Vec3; unit: "mm" | "m"; onCommit: (v: Vec3) => void }) {
  return (
    <>
      <PointFields label={label} point={{ x: value.x, y: value.y }} unit={unit} onCommit={(p) => onCommit({ ...value, ...p })} />
      <Field label={`${label} height`}>
        <NumberField label={`${label} height`} kind="length" unit={unit} value={value.z} onCommit={(z) => onCommit({ ...value, z })} />
      </Field>
    </>
  );
}

export function Rotation({ value, onCommit }: { value: number; onCommit: (deg: number) => void }) {
  return (
    <Field label="Rotation" hint="Degrees, counter-clockwise">
      <NumberField label="Rotation" suffix="deg" step={15} min={-360} max={360} value={value} onCommit={onCommit} />
    </Field>
  );
}

/** Less-used exact coordinates, tucked away but always available. */
export function Position({ children }: { children: React.ReactNode }) {
  const [open, toggle] = useSection("position", false);
  return (
    <Section title="Exact position" open={open} onToggle={toggle}>
      {children}
    </Section>
  );
}

const RAILING_STYLES: Array<{ value: RailingStyle; label: string }> = [
  { value: "bars", label: "Bars" },
  { value: "glass", label: "Glass" },
  { value: "solid", label: "Solid" },
];

/** A railing switch, then its height and style while it is on. Each change is one update_element. */
export function RailingFields({
  railing,
  unit,
  defaultHeight,
  what,
  onChange,
}: {
  railing: Railing | null;
  unit: "mm" | "m";
  defaultHeight: number;
  what: string;
  onChange: (railing: Railing | null) => void;
}) {
  return (
    <>
      <Field label={what} hint="Along the open edges">
        <label className={s.inlineSwitch}>
          <Switch label={what} checked={!!railing} onChange={(on) => onChange(on ? { height_mm: defaultHeight, style: "bars" } : null)} />
          <span>{railing ? "Along the open edges" : "None"}</span>
        </label>
      </Field>
      {railing ? (
        <>
          <Field label={`${what} height`} hint="Top of the rail above the floor">
            <NumberField label={`${what} height`} kind="length" unit={unit} min={300} max={2000} step={50} value={railing.height_mm} onCommit={(height_mm) => onChange({ ...railing, height_mm })} />
          </Field>
          <Field label={`${what} style`}>
            <Segmented label={`${what} style`} stretch value={railing.style} options={RAILING_STYLES} onChange={(style) => onChange({ ...railing, style })} />
          </Field>
        </>
      ) : null}
    </>
  );
}

export function LevelField({ levelId, doc, onChange }: { levelId: string; doc: DocState; onChange: (levelId: string) => void }) {
  if (doc.project.levels.length <= 1) return null;
  return (
    <Field label="Level">
      <Select label="Level" value={levelId} options={doc.project.levels.map((l) => ({ value: l.id, label: l.name }))} onChange={onChange} />
    </Field>
  );
}

/** What a kind without its own fields yet shows: its name. Wave 2 lanes replace these stubs. */
export function NameOnly({ name }: { name: string }) {
  return (
    <div className={s.group}>
      <Field label="Name">
        <ReadOnly>{name}</ReadOnly>
      </Field>
    </div>
  );
}
