// The Revisions tab of the Sheets panel (lane L4): the revision log and a
// form to add one, marked on the sheets it touched. Each sheet prints its
// own revisions in its title block's table.
import { useState } from "react";
import type { DocState } from "../../contract/bindings";
import { useApp } from "../../state/store";
import { Button, CheckRow, IconButton } from "../../ui/controls";
import { nextRevisionMark, today } from "./sheetModel";
import s from "./sheets.module.css";

export function RevisionsTab({ doc, current }: { doc: DocState; current: string | null }) {
  const dispatch = useApp((st) => st.dispatch);
  const set = doc.project.sheet_set;
  const [mark, setMark] = useState<string | null>(null);
  const input = (label: string, value: string, onChange: (v: string) => void, placeholder?: string) => (
    <input
      type="text"
      aria-label={label}
      className={s.textInput}
      value={value}
      placeholder={placeholder}
      spellCheck={false}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") void add();
      }}
    />
  );
  const [date, setDate] = useState(today());
  const [description, setDescription] = useState("");
  const [picked, setPicked] = useState<string[] | null>(null);
  const sheetIds = picked ?? (current ? [current] : []);
  const markValue = mark ?? nextRevisionMark(set.revisions.map((r) => r.mark));

  async function add() {
    if (sheetIds.length === 0 || description.trim() === "" || markValue.trim() === "") return;
    const ok = await dispatch({
      type: "upsert_revision",
      revision: { id: "", mark: markValue, date, description: description.trim() },
      sheet_ids: sheetIds,
    });
    if (ok) {
      setMark(null);
      setDescription("");
      setPicked(null);
    }
  }

  return (
    <>
      <div className={s.group}>
        <div className={s.groupHead}>Revision log</div>
        {set.revisions.length === 0 ? <p className={s.note}>No revisions yet. A revision is marked on the sheets it changed and prints in their title block.</p> : null}
        {set.revisions.map((r) => {
          const on = set.sheets.filter((sh) => sh.revision_ids.includes(r.id)).map((sh) => sh.number);
          return (
            <div key={r.id} className={s.revRow}>
              <span className={s.revMark}>{r.mark}</span>
              <span className={s.revText}>
                <span>{r.description || "No description"}</span>
                <small>
                  {r.date || "No date"}
                  {on.length ? ` on ${on.join(", ")}` : ", on no sheet"}
                </small>
              </span>
              <IconButton icon="trash" size={15} label={`Delete revision ${r.mark}`} tip="Delete revision" onClick={() => void dispatch({ type: "delete_revision", id: r.id })} />
            </div>
          );
        })}
      </div>

      <div className={s.group}>
        <div className={s.groupHead}>Add a revision</div>
        <div className={s.grid2}>
          <label className={s.cell}>
            <span className={s.cellLabel}>Mark</span>
            {input("Revision mark", markValue, setMark)}
          </label>
          <label className={s.cell}>
            <span className={s.cellLabel}>Date</span>
            {input("Revision date", date, setDate, "YYYY-MM-DD")}
          </label>
        </div>
        <label className={s.cell}>
          <span className={s.cellLabel}>Description</span>
          {input("Revision description", description, setDescription, "Revised per the building official's comments")}
        </label>
        <span className={s.cellLabel}>On sheets</span>
        <div className={s.checks}>
          {set.sheets.map((sh) => (
            <CheckRow
              key={sh.id}
              checked={sheetIds.includes(sh.id)}
              onChange={(on) => setPicked(on ? [...sheetIds, sh.id] : sheetIds.filter((x) => x !== sh.id))}
            >
              {sh.number} {sh.title}
            </CheckRow>
          ))}
        </div>
        <Button variant="primary" size="sm" icon="plus" disabled={sheetIds.length === 0 || description.trim() === ""} onClick={() => void add()}>
          Add revision {markValue}
        </Button>
      </div>
    </>
  );
}
