// Pure helpers for the History panel (DECISIONS D38): rows from the undo
// and redo stacks, who made them, the people to filter by, what a step
// changed and relative times. No DOM, no store. Tested in history.test.ts.

import type { Element, HistoryList, HistoryStep, LiveStatus } from "../contract/bindings";

/** One step as the panel lists it. */
export interface HistoryRow {
  key: string;
  step: HistoryStep;
  /** "undo": done, newest first. "redo": undone, the next redo first. */
  stack: "undo" | "redo";
  /** Steps an "Undo to here" (or "Redo to here") on this row takes: its place in its stack plus one. */
  steps: number;
  /** This computer's own step: no author (docs/CONTRACT.md, "Live sessions"). */
  mine: boolean;
  /** "You", the author's name, or "Someone" when the name is lost. */
  name: string;
  /** Participant color index, null for your own steps (they tint in the accent). */
  color: number | null;
  /** Filter key: "you", or the author's participant id. */
  person: string;
}

/** The filter chip for everyone. */
export const EVERYONE = "everyone";

export interface HistoryPerson {
  key: string;
  name: string;
  color: number | null;
  mine: boolean;
  count: number;
}

function nameOf(step: HistoryStep, status: LiveStatus | null): string {
  if (step.author_id === null) return "You";
  if (step.author_name) return step.author_name;
  return status?.participants.find((p) => p.id === step.author_id)?.name ?? "Someone";
}

/**
 * A key per step that stays while the step moves between the stacks, so its
 * row slides instead of being replaced. Steps have no id: the key is what
 * the step is, and steps that look the same (one wall moved three times in
 * a second) are numbered in the order they were made, oldest first, which
 * undo and redo do not change.
 */
function stepKeys(list: HistoryList): { undo: string[]; redo: string[] } {
  const seen = new Map<string, number>();
  const key = (step: HistoryStep) => {
    const base = `${step.at}|${step.author_id ?? ""}|${step.label}|${step.ids[0] ?? step.removed[0] ?? ""}`;
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    return n === 0 ? base : `${base}#${n}`;
  };
  const undo = [...list.undo].reverse().map(key).reverse();
  return { undo, redo: list.redo.map(key) };
}

/** The undo stack newest first, then the redo stack in the order redo brings it back. */
export function historyRows(list: HistoryList, status: LiveStatus | null = null): HistoryRow[] {
  const keys = stepKeys(list);
  const row = (step: HistoryStep, stack: "undo" | "redo", i: number): HistoryRow => {
    const mine = step.author_id === null;
    return {
      key: keys[stack][i],
      step,
      stack,
      steps: i + 1,
      mine,
      name: nameOf(step, status),
      color: mine ? null : (step.color ?? status?.participants.find((p) => p.id === step.author_id)?.color ?? null),
      person: mine ? "you" : (step.author_id ?? "you"),
    };
  };
  return [...list.undo.map((s, i) => row(s, "undo", i)), ...list.redo.map((s, i) => row(s, "redo", i))];
}

/** Everyone who made a step, you first, then in the order they first appear. */
export function historyPeople(rows: readonly HistoryRow[]): HistoryPerson[] {
  const out: HistoryPerson[] = [];
  for (const r of rows) {
    const known = out.find((p) => p.key === r.person);
    if (known) known.count++;
    else out.push({ key: r.person, name: r.name, color: r.color, mine: r.mine, count: 1 });
  }
  return out.sort((a, b) => Number(b.mine) - Number(a.mine));
}

export function filterRows(rows: readonly HistoryRow[], person: string): HistoryRow[] {
  return person === EVERYONE ? [...rows] : rows.filter((r) => r.person === person);
}

/** The step's elements that exist now: what it added or changed, and what it removed (still there once it is undone). */
export function liveIds(step: HistoryStep, elements: readonly Pick<Element, "id">[]): string[] {
  const exists = new Set(elements.map((e) => e.id));
  const out: string[] = [];
  for (const id of [...step.ids, ...step.removed]) if (exists.has(id) && !out.includes(id)) out.push(id);
  return out;
}

/**
 * What a step changed, without its label: "Delete: removed 1 light" is
 * "Removed 1 light". Empty when the summary says no more than the label.
 */
export function stepDetail(step: Pick<HistoryStep, "label" | "summary">): string {
  let text = step.summary.trim();
  if (step.label && text.startsWith(`${step.label}:`)) text = text.slice(step.label.length + 1).trim();
  if (!text || text === step.label) return "";
  return text[0].toUpperCase() + text.slice(1);
}

/**
 * When a step was made, for a row: "just now", "4 min ago", "2 hr ago",
 * "Yesterday", "Oct 5". Empty for a step without a time (older hosts).
 */
export function relativeTime(iso: string, now: Date = new Date()): string {
  const t = Date.parse(iso);
  if (!iso || !Number.isFinite(t)) return "";
  const s = Math.max(0, Math.round((now.getTime() - t) / 1000));
  if (s < 45) return "just now";
  const min = Math.round(s / 60);
  if (min < 60) return `${min} min ago`;
  const hr = Math.round(min / 60);
  const d = new Date(t);
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const day = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  if (day === today && hr < 24) return `${hr} hr ago`;
  if (Math.round((today - day) / 86_400_000) === 1) return "Yesterday";
  const opts: Intl.DateTimeFormatOptions =
    d.getFullYear() === now.getFullYear() ? { month: "short", day: "numeric" } : { month: "short", day: "numeric", year: "numeric" };
  return d.toLocaleDateString("en-US", opts);
}

/** How often relative times are redrawn while the panel is open. */
export const RELATIVE_TICK_MS = 30_000;
