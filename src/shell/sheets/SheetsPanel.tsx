// The Sheets panel (lane L4): the permit set, its sheets, viewports and
// revisions, previews and the export. Mounted once by EditorShell; it opens
// itself on the `open_sheets` bus event (at that sheet, or the whole set
// when `sheet_id` is null).
//
// Left, the sheets in set order (drag to reorder). Middle, the live preview
// from `sheet_preview`, with boxes over the viewports to select and move
// them. Right, the Sheet, Title block and Revisions tabs. The footer exports
// the set as one PDF. Every change is one engine command and one undo step.
import { useEffect, useState } from "react";
import type { PaperSize, Sheet } from "../../contract/bindings";
import { bus } from "../../state/bus";
import { useApp } from "../../state/store";
import { Dialog } from "../../ui/Dialog";
import { Button, IconButton, Segmented, Select, Spinner } from "../../ui/controls";
import { Presence } from "../../ui/motionDom";
import { editSheet } from "../commitLatest";
import { RevisionsTab } from "./RevisionsTab";
import { SheetList } from "./SheetList";
import { SheetPreview } from "./SheetPreview";
import { SheetTab } from "./SheetTab";
import { TitleBlockForm } from "./TitleBlockForm";
import { exportPermitSet, makePermitSet } from "./permitSet";
import { PAPERS } from "./sheetModel";
import s from "./sheets.module.css";

type Tab = "sheet" | "title" | "revisions";

export function SheetsPanel() {
  const [open, setOpen] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);

  useEffect(
    () =>
      bus.on("open_sheets", ({ sheet_id }) => {
        if (sheet_id) setSelected(sheet_id);
        setOpen(true);
      }),
    [],
  );

  return (
    <Presence open={open} exit="panel">
      {(stage) => (
        <Dialog title="Sheets" bare width={Math.min(1680, Math.round(window.innerWidth * 0.96))} stage={stage} onClose={() => setOpen(false)}>
          <Panel selected={selected} onSelect={setSelected} onClose={() => setOpen(false)} />
        </Dialog>
      )}
    </Presence>
  );
}

function Panel({ selected, onSelect, onClose }: { selected: string | null; onSelect: (id: string | null) => void; onClose: () => void }) {
  const doc = useApp((st) => st.doc);
  const dispatch = useApp((st) => st.dispatch);
  const toast = useApp((st) => st.toast);
  const reportError = useApp((st) => st.reportError);
  const [tab, setTab] = useState<Tab>("sheet");
  const [viewport, setViewport] = useState<string | null>(null);
  const [paper, setPaper] = useState<PaperSize>(doc?.project.settings.paper === "a4" ? "a3" : (doc?.project.settings.paper ?? "a3"));
  const [busy, setBusy] = useState<"make" | "export" | null>(null);

  const sheets = doc?.project.sheet_set.sheets ?? [];
  const current = sheets.find((sh) => sh.id === selected) ?? sheets[0] ?? null;

  // Keep a selection that exists.
  useEffect(() => {
    if (current && current.id !== selected) onSelect(current.id);
  }, [current, selected, onSelect]);
  useEffect(() => setViewport(null), [current?.id]);

  if (!doc) return null;

  const save = (sheet: Sheet) => dispatch({ type: "upsert_sheet", sheet, index: null });

  const make = async () => {
    setBusy("make");
    try {
      if (await makePermitSet(paper)) toast("success", "Made the permit set. Check each sheet, then export.");
    } finally {
      setBusy(null);
    }
  };
  const addSheet = async () => {
    const r = await save({ id: "", number: "", title: "New sheet", paper, orientation: "landscape", viewports: [], revision_ids: [] });
    const made = r?.state.project.sheet_set.sheets;
    if (made?.length) onSelect(made[made.length - 1].id);
  };
  const exportSet = async () => {
    setBusy("export");
    try {
      const r = await exportPermitSet([]);
      if (r) toast("success", `Exported the permit set to ${r.path}`);
    } catch (e) {
      reportError(e);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className={s.shell}>
      <header className={s.head}>
        <h2>Sheets</h2>
        <span className={s.headNote}>
          {sheets.length === 0 ? "No permit set yet" : `Permit set, ${sheets.length} ${sheets.length === 1 ? "sheet" : "sheets"}`}
        </span>
        <span className={s.headSpacer} />
        <span className={s.makeGroup}>
          <Select<PaperSize> label="Paper for new sheets" value={paper} options={PAPERS} onChange={setPaper} />
          <Button icon="schedule" disabled={busy !== null} onClick={() => void make()}>
            {busy === "make" ? "Making" : "Make permit set"}
          </Button>
        </span>
        <Button variant="primary" icon="export" disabled={busy !== null} onClick={() => void exportSet()}>
          {busy === "export" ? "Exporting" : "Export PDF"}
        </Button>
        <IconButton icon="close" label="Close" tip="Close (Esc)" tipSide="bottom-end" onClick={onClose} />
      </header>

      <aside className={s.list}>
        <div className={s.listScroll}>
          {sheets.length === 0 ? (
            <div className={s.empty}>
              <strong>No sheets yet</strong>
              <span>Make permit set lays out the site, plans, elevations, sections, roof and schedules with the PH title block. You can move and rescale every view after.</span>
            </div>
          ) : (
            <SheetList
              sheets={sheets}
              selected={current?.id ?? null}
              onSelect={(id) => onSelect(id)}
              onMove={(id, index) => {
                // The latest sheet moves, with any edit still on its way.
                const sheet = sheets.find((sh) => sh.id === id);
                if (sheet) void editSheet(sheet, sheet, index);
              }}
            />
          )}
        </div>
        <div className={s.listFoot}>
          <Button size="sm" icon="plus" onClick={() => void addSheet()}>
            Add sheet
          </Button>
        </div>
      </aside>

      {current ? (
        <SheetPreview
          sheet={current}
          selectedViewport={viewport}
          onSelectViewport={(id) => {
            setViewport(id);
            if (id) setTab("sheet");
          }}
          onMoveViewport={(id, position, shownScale) =>
            // A view moved by hand keeps the scale it shows: what you drag
            // is what prints.
            void editSheet(current, {
              ...current,
              viewports: current.viewports.map((v) =>
                v.id === id ? { ...v, position, scale_denominator: v.scale_denominator ?? shownScale } : v,
              ),
            })
          }
        />
      ) : (
        <div className={s.stage}>
          {busy === "make" ? <Spinner /> : <p className={s.stageEmpty}>The selected sheet shows here as it prints, title block included.</p>}
        </div>
      )}

      <aside className={s.side}>
        <div className={s.tabs}>
          <Segmented<Tab>
            label="Sheet settings"
            stretch
            value={tab}
            onChange={setTab}
            options={[
              { value: "sheet", label: "Sheet" },
              { value: "title", label: "Title block" },
              { value: "revisions", label: "Revisions" },
            ]}
          />
        </div>
        <div key={tab} className={s.tabBody}>
          {tab === "sheet" ? (
            current ? (
              <SheetTab
                doc={doc}
                sheet={current}
                selectedViewport={viewport}
                onSelectViewport={setViewport}
                onDelete={() => {
                  const at = sheets.findIndex((sh) => sh.id === current.id);
                  const next = sheets[at + 1] ?? sheets[at - 1] ?? null;
                  void dispatch({ type: "delete_sheets", ids: [current.id] }).then((r) => {
                    if (r) onSelect(next?.id ?? null);
                  });
                }}
              />
            ) : (
              <p className={s.note}>Make the permit set or add a sheet to start.</p>
            )
          ) : tab === "title" ? (
            <TitleBlockForm doc={doc} />
          ) : (
            <RevisionsTab doc={doc} current={current?.id ?? null} />
          )}
        </div>
      </aside>
    </div>
  );
}
