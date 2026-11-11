// The Sheet tab of the Sheets panel (lane L4): number, title, paper and
// orientation, and the sheet's viewports, each with its scale, title and
// what it shows; the Add view menu. Every change is one `UpsertSheet`, laid
// over the latest sheet viewport by viewport (commitLatest.ts), so quick
// edits never undo each other.
import { useEffect, useRef, useState } from "react";
import type { DocState, Orientation, PaperSize, RenderRecord, Sheet, ViewRef, Viewport } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";
import { bus } from "../../state/bus";
import { useApp } from "../../state/store";
import { Button, IconButton, NumberField, Segmented, Select, TextField, cx } from "../../ui/controls";
import { Icon } from "../../ui/icons";
import { Presence } from "../../ui/motionDom";
import { editSheet } from "../commitLatest";
import { pickImage } from "./pickImage";
import { PAPERS, SCALES, projectViews, viewKey, viewLabel } from "./sheetModel";
import s from "./sheets.module.css";

const KIND_LABEL: Record<ViewRef["type"], string> = {
  plan: "Plan",
  elevation: "Elevation",
  section: "Section",
  roof_plan: "Roof plan",
  site_plan: "Site plan",
  vicinity_map: "Vicinity map",
  perspective: "Perspective",
  schedule: "Schedule",
  notes: "Notes",
};

const scaled = (v: ViewRef) => !["vicinity_map", "perspective", "schedule", "notes"].includes(v.type);

export function SheetTab({
  doc,
  sheet,
  selectedViewport,
  onSelectViewport,
  onDelete,
}: {
  doc: DocState;
  sheet: Sheet;
  selectedViewport: string | null;
  onSelectViewport: (id: string | null) => void;
  onDelete: () => void;
}) {
  const reportError = useApp((st) => st.reportError);
  const save = (next: Sheet) => editSheet(sheet, next);
  const setViewport = (id: string, patch: Partial<Viewport>) =>
    save({ ...sheet, viewports: sheet.viewports.map((v) => (v.id === id ? { ...v, ...patch } : v)) });

  const scaleOptions = [{ value: "fit", label: "Fit (largest common)" }, ...SCALES.map((n) => ({ value: String(n), label: `1:${n}` }))];

  return (
    <>
      <div className={s.group}>
        <div className={s.grid2}>
          <label className={s.cell}>
            <span className={s.cellLabel}>Sheet number</span>
            <TextField label="Sheet number" value={sheet.number} required onCommit={(number) => save({ ...sheet, number })} />
          </label>
          <label className={s.cell}>
            <span className={s.cellLabel}>Paper</span>
            <Select<PaperSize> label="Paper" value={sheet.paper} options={PAPERS} onChange={(paper) => save({ ...sheet, paper })} />
          </label>
        </div>
        <label className={s.cell}>
          <span className={s.cellLabel}>Sheet title</span>
          <TextField label="Sheet title" value={sheet.title} placeholder="Floor plans" onCommit={(title) => save({ ...sheet, title })} />
        </label>
        <Segmented<Orientation>
          label="Orientation"
          stretch
          value={sheet.orientation}
          onChange={(orientation) => save({ ...sheet, orientation })}
          options={[
            { value: "landscape", label: "Landscape" },
            { value: "portrait", label: "Portrait" },
          ]}
        />
      </div>

      <div className={s.group}>
        <div className={s.groupHead}>
          <span>Views on this sheet</span>
          <AddView doc={doc} onAdd={(view) => save({ ...sheet, viewports: [...sheet.viewports, { id: "", view, scale_denominator: null, position: null, title: null }] })} />
        </div>
        {sheet.viewports.length === 0 ? <p className={s.note}>Add a plan, an elevation, a schedule or notes. Views without a scale share the largest scale that fits.</p> : null}
        {sheet.viewports.map((vp) => (
          <div key={vp.id} className={cx(s.vpRow, vp.id === selectedViewport && s.vpRowOn)} onClick={() => onSelectViewport(vp.id)}>
            <div className={s.vpRowHead}>
              {/* The row's own button, so the keyboard can pick the view on
                  the sheet too (Enter or Space); a click anywhere on the row
                  still does. */}
              <button
                type="button"
                className={s.vpRowTitle}
                aria-pressed={vp.id === selectedViewport}
                title={viewLabel(doc, vp.view)}
                data-testid="viewport-row"
                onClick={(e) => {
                  e.stopPropagation();
                  onSelectViewport(vp.id);
                }}
              >
                {vp.title?.trim() || viewLabel(doc, vp.view)}
              </button>
              {/* The kind, unless the title already says it ("Vicinity map", "Roof plan"). */}
              {KIND_LABEL[vp.view.type].toLowerCase() !== (vp.title?.trim() || viewLabel(doc, vp.view)).toLowerCase() ? (
                <span className={s.pill}>{KIND_LABEL[vp.view.type]}</span>
              ) : null}
              {vp.view.type !== "notes" && vp.view.type !== "vicinity_map" && vp.view.type !== "perspective" ? (
                <IconButton icon="eye" size={15} label="Open full screen" tip="Open full screen" onClick={() => bus.emit("open_view", { view: vp.view })} />
              ) : null}
              {vp.position ? (
                <IconButton icon="fit" size={15} label="Place automatically" tip="Place automatically" onClick={() => setViewport(vp.id, { position: null })} />
              ) : null}
              <IconButton
                icon="trash"
                size={15}
                label="Remove from the sheet"
                tip="Remove from the sheet"
                onClick={(e) => {
                  e.stopPropagation();
                  save({ ...sheet, viewports: sheet.viewports.filter((v) => v.id !== vp.id) });
                  if (selectedViewport === vp.id) onSelectViewport(null);
                }}
              />
            </div>
            <div className={s.vpRowMeta}>
              {scaled(vp.view) ? (
                <Select
                  label="Scale"
                  value={vp.scale_denominator ? String(vp.scale_denominator) : "fit"}
                  options={scaleOptions}
                  onChange={(v) => setViewport(vp.id, { scale_denominator: v === "fit" ? null : Number(v) })}
                />
              ) : (
                <span className={s.readOnly}>{vp.view.type === "schedule" || vp.view.type === "notes" ? "No scale" : "Not to scale"}</span>
              )}
              <TextField label="Viewport title" value={vp.title ?? ""} placeholder="Title from the view" onCommit={(t) => setViewport(vp.id, { title: t.trim() === "" ? null : t })} />
            </div>
            {vp.view.type === "notes" ? (
              <TextField
                label="Notes"
                multiline
                value={vp.view.text}
                placeholder="One note per line"
                onCommit={(text) => setViewport(vp.id, { view: { type: "notes", text } })}
              />
            ) : null}
            {vp.view.type === "vicinity_map" ? (
              <div className={s.vpRowMeta}>
                <Button
                  size="sm"
                  icon="import"
                  onClick={async (e) => {
                    e.stopPropagation();
                    try {
                      const file = await pickImage();
                      if (file && vp.view.type === "vicinity_map") setViewport(vp.id, { view: { ...vp.view, file_name: file } });
                    } catch (err) {
                      reportError(err);
                    }
                  }}
                >
                  {vp.view.file_name ? "Replace image" : "Choose image"}
                </Button>
                <NumberField
                  label="Radius"
                  value={vp.view.radius_m}
                  suffix="m"
                  min={50}
                  max={5000}
                  integer
                  onCommit={(radius_m) => vp.view.type === "vicinity_map" && setViewport(vp.id, { view: { ...vp.view, radius_m } })}
                />
              </div>
            ) : null}
          </div>
        ))}
      </div>

      <div className={s.group}>
        <Button variant="danger" size="sm" icon="trash" onClick={onDelete}>
          Delete sheet {sheet.number}
        </Button>
      </div>
    </>
  );
}

/** The Add view menu: every drawing, a vicinity map, a model render, notes. */
function AddView({ doc, onAdd }: { doc: DocState; onAdd: (view: ViewRef) => void }) {
  const [open, setOpen] = useState(false);
  const [renders, setRenders] = useState<RenderRecord[] | null>(null);
  const wrap = useRef<HTMLSpanElement>(null);
  // A press outside the menu closes it.
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", down, true);
    return () => window.removeEventListener("mousedown", down, true);
  }, [open]);
  const toggle = () => {
    const next = !open;
    setOpen(next);
    if (next && renders === null) {
      // Model views only: an AI image never goes on a sheet (D54).
      void ipc
        .renderList()
        .then((all) => setRenders(all.filter((r) => r.source === "model_view" && !r.video)))
        .catch(() => setRenders([]));
    }
  };
  const pick = (view: ViewRef) => {
    onAdd(view);
    setOpen(false);
  };
  const choices = projectViews(doc);
  const groups = [...new Set(choices.map((c) => c.group))];
  return (
    <span ref={wrap} className={s.addWrap}>
      <Button size="sm" icon="plus" onClick={toggle} aria-expanded={open}>
        Add view
      </Button>
      <Presence open={open} exit="base">
        {(stage) => (
          <div
            className={s.addMenu}
            data-stage={stage}
            style={{ width: 300, left: "auto" }}
            role="menu"
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.stopPropagation();
                setOpen(false);
              }
            }}
          >
            {groups.map((g) => (
              <div key={g}>
                <div className={s.addGroup}>{g}</div>
                {choices
                  .filter((c) => c.group === g)
                  .map((c) => (
                    <button key={viewKey(c.view)} type="button" role="menuitem" className={s.addItem} onClick={() => pick(c.view)}>
                      {c.label}
                    </button>
                  ))}
              </div>
            ))}
            <div className={s.addGroup}>Site and pictures</div>
            <button type="button" role="menuitem" className={s.addItem} onClick={() => pick({ type: "vicinity_map", file_name: null, radius_m: 500 })}>
              <Icon name="pin" size={14} />
              Vicinity map
            </button>
            {renders === null ? (
              <button type="button" className={s.addItem} disabled>
                Loading model renders
              </button>
            ) : renders.length === 0 ? (
              <button type="button" className={s.addItem} disabled>
                No model renders in Visuals yet
              </button>
            ) : (
              renders.map((r) => (
                <button key={r.id} type="button" role="menuitem" className={s.addItem} onClick={() => pick({ type: "perspective", render_id: r.id })}>
                  <Icon name="visuals" size={14} />
                  Perspective: {r.camera.name || new Date(r.created_at).toLocaleString()}
                </button>
              ))
            )}
            <div className={s.addGroup}>Text</div>
            <button type="button" role="menuitem" className={s.addItem} onClick={() => pick({ type: "notes", text: "All dimensions are in millimeters unless noted.\nVerify all dimensions on site." })}>
              <Icon name="text" size={14} />
              General notes
            </button>
          </div>
        )}
      </Presence>
    </span>
  );
}
