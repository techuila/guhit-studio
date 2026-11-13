// The door or window type picker of an opening (lane L1, DECISIONS D53):
// which type (D1, W1) it is, a new type made from it, and the way to the
// types panel. OpeningFields mounts it at the top of the opening's fields.
// Choosing a type sends `UpdateElement` with its id; the engine gives the
// opening the type's style, size and sill in the same step.
import { useEffect, useRef, useState } from "react";
import type { DocState } from "../../contract/bindings";
import { Button, Field, Select, cx } from "../../ui/controls";
import { openTypesPanel } from "../TypesPanel";
import { byMark, newTypeFrom, typeLabel } from "./permitCommon";
import { update, type Of } from "./common";
import css from "./permit.module.css";
import s from "../Inspector.module.css";

export function OpeningTypeFields({ el, doc }: { el: Of<"opening">; doc: DocState }) {
  const [busy, setBusy] = useState(false);
  const door = el.opening_type === "door";
  const types = (doc.project.opening_types ?? []).filter((t) => t.opening_type === el.opening_type).sort(byMark);
  const type = el.type_id ? types.find((t) => t.id === el.type_id) : undefined;
  // A size, style or sill edit moved it to another type (the engine's `follow`): say so here too.
  const seen = useRef({ id: el.id, type: el.type_id ?? null });
  const picked = useRef(false);
  const [moved, setMoved] = useState<{ from: string; to: string } | null>(null);
  useEffect(() => {
    const was = seen.current;
    const now = el.type_id ?? null;
    if (was.id !== el.id) setMoved(null);
    else if (was.type && now && was.type !== now && !picked.current) {
      const mark = (id: string) => doc.project.opening_types.find((t) => t.id === id)?.mark ?? "";
      setMoved({ from: mark(was.type), to: mark(now) });
    }
    seen.current = { id: el.id, type: now };
    picked.current = false;
  }, [el.id, el.type_id, doc]);
  const options = [{ value: "", label: "None" }, ...types.map((t) => ({ value: t.id, label: typeLabel(t) }))];
  // A type id that is not listed (deleted meanwhile): keep it showing until the engine answers.
  if (el.type_id && !type) options.push({ value: el.type_id, label: "Unknown type" });

  const makeType = async () => {
    setBusy(true);
    try {
      await newTypeFrom(el);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Field label={door ? "Door type" : "Window type"} hint="Openings of one type share its style, size and sill">
        <Select
          label={door ? "Door type" : "Window type"}
          value={el.type_id ?? ""}
          options={options}
          onChange={(id) => {
            picked.current = true;
            setMoved(null);
            update(el, { ...el, type_id: id === "" ? null : id });
          }}
        />
      </Field>
      {type ? (
        <p className={css.typeLine}>
          <span key={type.mark} className={cx(css.markPill, !door && css.markWindow)}>
            {type.mark}
          </span>
          <span key={moved ? "moved" : "rule"} className={css.typeNote}>
            {moved && moved.from && moved.to ? `Moved from ${moved.from} to ${moved.to}` : "Editing the size moves it to another type"}
          </span>
        </p>
      ) : null}
      <div className={s.actionsRow}>
        <Button size="sm" icon="plus" disabled={busy} onClick={() => void makeType()}>
          New type from this
        </Button>
        {types.length > 0 ? (
          <Button size="sm" variant="ghost" icon="pencil" onClick={() => openTypesPanel(el.type_id ?? null)}>
            Edit types
          </Button>
        ) : null}
      </div>
    </>
  );
}
