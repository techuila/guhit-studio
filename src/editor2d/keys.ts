// Which keys the plan canvas may take, and what the pipe editing keys do on
// a selected run (DECISIONS D39). Pure: the controller passes in what it
// knows. Tested in keys.test.ts.

import { PIPE_HEIGHT_STEP, PIPE_HEIGHT_STEP_FINE } from "./pipe";

/** Roles of focusable widgets that use the page, arrow and letter keys themselves. */
const WIDGET_ROLES = '[role="slider"], [role="spinbutton"], [role="listbox"], [role="combobox"], [role="textbox"], [role="menu"], [role="menuitem"]';

/** True when the key goes to a focused form control or editable text: an input, a select, a slider, contenteditable. */
export function isTypingTarget(target: EventTarget | null): boolean {
  const el = target as { tagName?: unknown; isContentEditable?: boolean; closest?: (sel: string) => unknown } | null;
  if (!el || typeof el.tagName !== "string") return false;
  const tag = el.tagName.toUpperCase();
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el.isContentEditable) return true;
  return typeof el.closest === "function" && !!el.closest(WIDGET_ROLES);
}

export interface KeyGate {
  /** Another handler already took the key. */
  defaultPrevented: boolean;
  /** A form control or editable text has the focus (`isTypingTarget`). */
  typing: boolean;
  /** A dialog is open (`isModalOpen`). */
  modal: boolean;
  /** Following someone in a live session (`useLive` `following`). */
  following: boolean;
}

/**
 * True when the plan canvas must leave a key alone: a field or a dialog has
 * it, or (Escape) the user is following someone, where Escape stops
 * following first (DECISIONS D37).
 */
export function planKeyBlocked(key: string, g: KeyGate): boolean {
  if (g.defaultPrevented || g.typing || g.modal) return true;
  return key === "Escape" && g.following;
}

export type SelectionKey = { kind: "step"; dz: number } | { kind: "height" } | { kind: "unpick" } | null;

/**
 * A key on a selected run with the select tool: PageUp and PageDown (Shift
 * for the fine step) and `h` need the pointer over the plan, as in the pipe
 * tool; Escape steps out of a picked segment or point. Null: not ours.
 */
export function selectionKey(key: string, mods: { shift: boolean; alt: boolean }, c: { pointerInside: boolean; picked: boolean }): SelectionKey {
  if (mods.alt) return null;
  if ((key === "PageUp" || key === "PageDown") && c.pointerInside) {
    const step = mods.shift ? PIPE_HEIGHT_STEP_FINE : PIPE_HEIGHT_STEP;
    return { kind: "step", dz: key === "PageUp" ? step : -step };
  }
  if (key.toLowerCase() === "h" && c.pointerInside) return { kind: "height" };
  if (key === "Escape" && c.picked) return { kind: "unpick" };
  return null;
}
