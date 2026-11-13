// Door and window types (lane L1, DECISIONS D53): the types panel, where
// D1, W1 and the rest are made and edited, and its entry in the project
// inspector. EditorShell mounts `TypesPanel` once; the inspector mounts
// `TypesSection`. The panel opens through `openTypesPanel` (a small store
// of its own: the bus is a frozen hub). Every edit is one command:
// `UpsertOpeningSpec` (the engine writes the type into every opening of
// it), `DeleteOpeningSpecs` (its openings become untyped at their size) and
// `AssignOpeningTypes`.
import { useState } from "react";
import { create } from "zustand";
import type { DocState, OpeningSpec, OpeningType } from "../contract/bindings";
import { useApp } from "../state/store";
import { Dialog } from "../ui/Dialog";
import { MaterialPicker } from "../ui/MaterialPicker";
import { Button, Field, NumberField, Section, Select, Switch, TextField, cx } from "../ui/controls";
import { Presence } from "../ui/motionDom";
import { DOOR_STYLES, WINDOW_STYLES } from "./actions";
import { commitLatest, rebase } from "./commitLatest";
import { STYLE_NAME, TYPE_NOUN, byMark, sizeText } from "./fields/permitCommon";
import { useSection } from "./shellStore";
import css from "./TypesPanel.module.css";
import s from "./Inspector.module.css";

interface PanelState {
  open: boolean;
  /** The type being edited. */
  typeId: string | null;
}

const usePanel = create<PanelState>(() => ({ open: false, typeId: null }));

/** Opens the types panel, on `typeId` when given. */
export function openTypesPanel(typeId: string | null = null): void {
  usePanel.setState({ open: true, typeId });
}

export function closeTypesPanel(): void {
  usePanel.setState({ open: false });
}

/** How many openings each type has, from `Derived::opening_schedule`. */
export function typeCounts(doc: DocState): Map<string, number> {
  const out = new Map<string, number>();
  for (const r of doc.derived.opening_schedule?.rows ?? []) if (r.type_id) out.set(r.type_id, (out.get(r.type_id) ?? 0) + r.total);
  return out;
}

/** Openings with no type: the schedule's rows with an empty mark. */
export function untypedCount(doc: DocState): number {
  return (doc.derived.opening_schedule?.rows ?? []).reduce((n, r) => (r.type_id ? n : n + r.total), 0);
}

const plural = (n: number, one: string): string => `${n} ${one}${n === 1 ? "" : "s"}`;

/** A new type of this kind with the tools' default size; the engine names it. */
function blankType(kind: OpeningType): OpeningSpec {
  const door = kind === "door";
  return {
    id: "",
    mark: "",
    opening_type: kind,
    style: door ? "swing_single" : "sliding",
    width_mm: door ? 900 : 1200,
    height_mm: door ? 2100 : 1200,
    sill_mm: door ? null : 900,
    material_id: null,
    description: door ? "Single swing door" : "Sliding window",
    remarks: "",
  };
}

/** `UpsertOpeningSpec` for an edit made on `base`, laid over the type as it is now. */
function editType(base: OpeningSpec, next: OpeningSpec): void {
  if (JSON.stringify(base) === JSON.stringify(next)) return;
  void commitLatest((doc) => {
    const now = doc.project.opening_types.find((t) => t.id === base.id);
    return now ? { type: "upsert_opening_spec", spec: rebase(now, base, next) } : null;
  });
}

/** One type. Rows rise in together as one block, never staggered (MOTION.md rule 8). */
function TypeRow({ t, count, on, onPick }: { t: OpeningSpec; count: number; on?: boolean; onPick?: () => void }) {
  const door = t.opening_type === "door";
  const body = (
    <>
      <span className={cx(css.mark, !door && css.markWindow)}>{t.mark}</span>
      <span className={css.what}>
        <span className={css.size}>{sizeText(t.width_mm, t.height_mm)}</span>
        <span className={css.style} title={t.description || undefined}>
          {t.description && t.description !== `${STYLE_NAME[t.style]} ${TYPE_NOUN[t.opening_type]}` ? `${STYLE_NAME[t.style]}, ${t.description}` : STYLE_NAME[t.style]}
        </span>
      </span>
      <span className={css.count}>{count}</span>
    </>
  );
  if (!onPick) {
    return <li className={cx(css.row, css.rowStatic)}>{body}</li>;
  }
  return (
    <li>
      <button type="button" className={cx(css.row, on && css.rowOn)} aria-pressed={on} onClick={onPick}>
        {body}
      </button>
    </li>
  );
}

function TypeEditor({ t, count, doc }: { t: OpeningSpec; count: number; doc: DocState }) {
  const [confirm, setConfirm] = useState(false);
  const unit = doc.project.settings.display_unit;
  const door = t.opening_type === "door";
  const styles = door ? DOOR_STYLES : WINDOW_STYLES;
  const noun = TYPE_NOUN[t.opening_type];
  const remove = async () => {
    setConfirm(false);
    const ok = await useApp.getState().dispatch({ type: "delete_opening_specs", ids: [t.id] });
    if (ok) usePanel.setState({ typeId: null });
  };
  return (
    <div className={css.editor}>
      <div className={css.editorHead}>
        <span className={cx(css.mark, !door && css.markWindow)}>{t.mark}</span>
        <span>{door ? "Door type" : "Window type"}</span>
      </div>
      <p className={css.used}>{count > 0 ? `${plural(count, noun)} of this type. Changes here go to every one.` : `No ${noun}s of this type yet.`}</p>
      <Field label="Mark" hint="Unique, like D1 or W3">
        <TextField label="Type mark" value={t.mark} required onCommit={(mark) => editType(t, { ...t, mark })} />
      </Field>
      <Field label="Style">
        <Select
          label="Style"
          value={t.style}
          options={styles.some((o) => o.value === t.style) ? styles : [...styles, { value: t.style, label: STYLE_NAME[t.style] }]}
          onChange={(style) => editType(t, { ...t, style })}
        />
      </Field>
      <Field label="Width">
        <NumberField label="Type width" kind="length" unit={unit} min={200} max={10000} step={50} value={t.width_mm} onCommit={(width_mm) => editType(t, { ...t, width_mm })} />
      </Field>
      <Field label="Height">
        <NumberField label="Type height" kind="length" unit={unit} min={200} max={6000} step={50} value={t.height_mm} onCommit={(height_mm) => editType(t, { ...t, height_mm })} />
      </Field>
      {door ? null : (
        <>
          <Field label="Sill height" hint="Bottom of the window above the floor">
            <NumberField
              label="Type sill height"
              kind="length"
              unit={unit}
              min={0}
              max={5000}
              step={50}
              disabled={t.sill_mm === null || t.sill_mm === undefined}
              value={t.sill_mm ?? 900}
              onCommit={(sill_mm) => editType(t, { ...t, sill_mm })}
            />
          </Field>
          <Field label="">
            <label className={s.inlineSwitch}>
              <Switch label="Each window keeps its own sill" checked={t.sill_mm === null || t.sill_mm === undefined} onChange={(own) => editType(t, { ...t, sill_mm: own ? null : 900 })} />
              <span>Each window keeps its own sill</span>
            </label>
          </Field>
        </>
      )}
      <Field label="Material">
        <MaterialPicker
          label="Type material"
          materials={doc.project.materials}
          prefer={door ? ["wood", "metal", "glass"] : ["glass", "metal", "wood"]}
          value={t.material_id ?? null}
          noneLabel={door ? "Default door finish" : "Default window finish"}
          onChange={(material_id) => editType(t, { ...t, material_id })}
        />
      </Field>
      <Field label="Description" hint="What the schedule says">
        <TextField label="Type description" value={t.description} placeholder={`${STYLE_NAME[t.style]} ${noun}`} onCommit={(description) => editType(t, { ...t, description })} />
      </Field>
      <Field label="Remarks">
        <TextField label="Type remarks" multiline value={t.remarks} placeholder="Hardware, finish, glazing" onCommit={(remarks) => editType(t, { ...t, remarks })} />
      </Field>
      <div className={css.actions}>
        <Button size="sm" variant="danger" icon="trash" onClick={() => setConfirm((c) => !c)} aria-expanded={confirm}>
          Delete {t.mark}
        </Button>
      </div>
      <div className={cx(css.confirmReveal, confirm && css.confirmRevealOpen)} inert={!confirm}>
        <div className={css.confirmClip}>
          <div className={css.confirm}>
            <p>
              {count === 0
                ? `No ${noun} uses ${t.mark}.`
                : count === 1
                  ? `1 ${noun} becomes untyped. It keeps its size.`
                  : `${plural(count, noun)} become untyped. They keep their size.`}
            </p>
            <div className={css.actions}>
              <Button size="sm" variant="danger" onClick={() => void remove()}>
                Delete {t.mark}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setConfirm(false)}>
                Keep it
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function PanelBody({ doc }: { doc: DocState }) {
  const typeId = usePanel((st) => st.typeId);
  const types = [...(doc.project.opening_types ?? [])].sort(byMark);
  const counts = typeCounts(doc);
  const picked = types.find((t) => t.id === typeId) ?? null;
  const add = async (kind: OpeningType) => {
    const before = new Set(types.map((t) => t.id));
    const result = await useApp.getState().dispatch({ type: "upsert_opening_spec", spec: blankType(kind) });
    const made = result?.state.project.opening_types.find((t) => !before.has(t.id));
    if (made) usePanel.setState({ typeId: made.id });
  };
  const group = (kind: OpeningType, title: string) => {
    const list = types.filter((t) => t.opening_type === kind);
    return (
      <div>
        <div className={css.groupHead}>
          <span>{title}</span>
          <Button size="sm" variant="ghost" icon="plus" onClick={() => void add(kind)}>
            New {TYPE_NOUN[kind]} type
          </Button>
        </div>
        {list.length === 0 ? (
          <p className={css.empty}>No {TYPE_NOUN[kind]} types yet.</p>
        ) : (
          <ul className={css.list}>
            {list.map((t) => (
              <TypeRow key={t.id} t={t} count={counts.get(t.id) ?? 0} on={t.id === typeId} onPick={() => usePanel.setState({ typeId: t.id === typeId ? null : t.id })} />
            ))}
          </ul>
        )}
      </div>
    );
  };
  return (
    <div className={css.body}>
      {group("door", "Doors")}
      {group("window", "Windows")}
      {picked ? <TypeEditor key={picked.id} t={picked} count={counts.get(picked.id) ?? 0} doc={doc} /> : null}
    </div>
  );
}

/** The types panel: a side sheet over the plan. */
export function TypesPanel() {
  const open = usePanel((st) => st.open);
  const doc = useApp((st) => st.doc);
  return (
    <Presence open={open && !!doc} exit="panel">
      {(stage) =>
        doc ? (
          <Dialog title="Door and window types" onClose={closeTypesPanel} placement="side" width={380} stage={stage}>
            <PanelBody doc={doc} />
          </Dialog>
        ) : null
      }
    </Presence>
  );
}

/** The project inspector's door and window types section. Nothing while there are no openings or types. */
export function TypesSection({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("types", true);
  const types = [...(doc.project.opening_types ?? [])].sort(byMark);
  const untyped = untypedCount(doc);
  if (types.length === 0 && untyped === 0) return null;
  const counts = typeCounts(doc);
  const assign = async () => {
    const result = await useApp.getState().dispatch({ type: "assign_opening_types", level_id: null });
    if (result?.diff.summary) useApp.getState().toast("success", result.diff.summary);
  };
  return (
    <Section
      title="Door and window types"
      icon="door"
      count={types.length}
      open={open}
      onToggle={toggle}
      aside={untyped > 0 ? <span className={s.sectionAside}>{untyped} untyped</span> : null}
    >
      {types.length > 0 ? (
        <ul className={css.sectionRows}>
          {types.map((t) => (
            <TypeRow key={t.id} t={t} count={counts.get(t.id) ?? 0} onPick={() => openTypesPanel(t.id)} />
          ))}
        </ul>
      ) : (
        <p className={s.note}>No types yet. Assign types to group the doors and windows by size and style, D1, W1 and on.</p>
      )}
      <div className={s.actionsRow}>
        {untyped > 0 ? (
          <Button size="sm" icon="check" onClick={() => void assign()}>
            Assign types
          </Button>
        ) : null}
        <Button size="sm" variant="ghost" icon="pencil" onClick={() => openTypesPanel(null)}>
          Edit
        </Button>
      </div>
      {untyped > 0 ? (
        <p className={s.note}>
          {untyped === 1 ? "1 door or window" : `${untyped} doors and windows`} without a type. Assign types gives each a matching type, or a new one.
        </p>
      ) : null}
    </Section>
  );
}
