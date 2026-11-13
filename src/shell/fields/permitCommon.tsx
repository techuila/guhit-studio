// What the permit fields, the types panel and the schedules share (lane L1):
// door and window type wording, a new type made from an opening, the lot
// set from a pasted description, and a number that bumps when it changes.
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { Command, Element, OpeningSpec, OpeningStyle, OpeningType, Point } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";
import { parseTct } from "../../editor2d/permit/tct";
import { useApp } from "../../state/store";
import css from "./permit.module.css";

type OpeningEl = Extract<Element, { kind: "opening" }>;

export const STYLE_NAME: Record<OpeningStyle, string> = {
  swing_single: "Single swing",
  swing_double: "Double swing",
  sliding: "Sliding",
  fixed: "Fixed glass",
  casement: "Casement",
  jalousie: "Jalousie",
};

const STYLE_SHORT: Record<OpeningStyle, string> = {
  swing_single: "swing",
  swing_double: "double swing",
  sliding: "sliding",
  fixed: "fixed",
  casement: "casement",
  jalousie: "jalousie",
};

export const TYPE_NOUN: Record<OpeningType, string> = { door: "door", window: "window" };

/** "900 x 2100", always in mm as schedules write it. */
export function sizeText(w: number, h: number): string {
  return `${Math.round(w)} x ${Math.round(h)}`;
}

/** "D1 - 900 x 2100 swing". */
export function typeLabel(t: Pick<OpeningSpec, "mark" | "width_mm" | "height_mm" | "style">): string {
  return `${t.mark} - ${sizeText(t.width_mm, t.height_mm)} ${STYLE_SHORT[t.style]}`;
}

/** Marks sort D1, D2, D10, then W1. */
export function byMark(a: Pick<OpeningSpec, "mark">, b: Pick<OpeningSpec, "mark">): number {
  return a.mark.localeCompare(b.mark, "en", { numeric: true });
}

/** A type's spec as an opening has it now: size, style, sill (windows) and material. */
export function specFrom(o: OpeningEl): OpeningSpec {
  const door = o.opening_type === "door";
  return {
    id: "",
    mark: "",
    opening_type: o.opening_type,
    style: o.style,
    width_mm: o.width_mm,
    height_mm: o.height_mm,
    sill_mm: door ? null : o.sill_mm,
    material_id: o.material_id,
    description: `${STYLE_NAME[o.style]} ${TYPE_NOUN[o.opening_type]}`,
    remarks: "",
  };
}

/**
 * A new type from this opening, and the opening given it, as one undo step.
 * The engine names the type (empty id and mark); a preview of the same
 * command tells which id it gets, because a leaf's ids depend only on the
 * project before it and the command itself, never on the batch around it.
 */
export async function newTypeFrom(o: OpeningEl): Promise<boolean> {
  const app = useApp.getState();
  const doc = app.doc;
  if (!doc) return false;
  const upsert: Command = { type: "upsert_opening_spec", spec: specFrom(o) };
  const before = new Set(doc.project.opening_types.map((t) => t.id));
  let made: OpeningSpec | undefined;
  try {
    const preview = await ipc.docPreview(upsert);
    made = preview.state.project.opening_types.find((t) => !before.has(t.id));
  } catch (e) {
    app.reportError(e);
    return false;
  }
  if (!made) {
    app.toast("error", "The engine made no new type. Try again.");
    return false;
  }
  const now = useApp.getState().doc?.project.elements.find((e): e is OpeningEl => e.id === o.id && e.kind === "opening");
  if (!now) return false;
  const result = await app.dispatch({
    type: "batch",
    label: `New ${TYPE_NOUN[o.opening_type]} type ${made.mark}`,
    commands: [upsert, { type: "update_element", element: { ...now, type_id: made.id } }],
  });
  return !!result;
}

/**
 * Sets the lot from a pasted description. Null when the lot was set; the
 * reason when the text has no legs; empty when the engine refused them (its
 * toast already says why, naming the leg).
 */
export async function setLotFromText(text: string, start: Point): Promise<string | null> {
  const parsed = parseTct(text);
  if (!parsed.ok) return parsed.error;
  const app = useApp.getState();
  const result = await app.dispatch({ type: "set_lot_from_survey", start, legs: parsed.legs });
  if (!result) return "";
  app.toast("success", result.diff.summary || `Lot set from ${parsed.legs.length} legs`);
  return null;
}

/** Its children, with a short teal bump whenever `value` changes (never on first show). */
export function Bump({ value, children }: { value: string | number; children?: ReactNode }) {
  const prev = useRef(value);
  const [n, setN] = useState(0);
  useEffect(() => {
    if (prev.current === value) return;
    prev.current = value;
    setN((x) => x + 1);
  }, [value]);
  return (
    <span key={n} className={n > 0 ? css.bump : undefined}>
      {children ?? value}
    </span>
  );
}
