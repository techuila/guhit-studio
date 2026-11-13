// Inspector edits that send a whole object (the project settings, the roof, a
// level, a layer, an element) build that object when the edit commits, from
// the latest model, not from the render that drew the field.
//
// Commits also run one after another: each waits for the one before it to
// come back from the engine, then reads the model again. Typing Client, Tab,
// Location, Tab, Designer, Tab without a pause sends three commands, and each
// keeps what the ones before it set. Before this, every field sent the
// settings it was drawn with, so the last commit undid the first two.
//
// Each edit is given as `base` (the object the field was drawn with) and
// `next` (base with the edit). Only what differs between the two is laid over
// the latest object, field by field, into nested objects (the site, a light)
// and item by item into arrays (a sheet's viewports, a lot's sides).
import type { ApplyResult, Command, DocState, Element, Layer, Level, ProjectSettings, Roof, Sheet, Signatory, TitleBlock } from "../contract/bindings";
import { topRoof } from "../contract/roof";
import { useApp } from "../state/store";

let chain: Promise<unknown> = Promise.resolve();

/**
 * Queues `build` after every earlier queued commit, then dispatches what it
 * returns. `build` sees the model as the engine last returned it; null sends
 * nothing.
 */
export function commitLatest(build: (doc: DocState) => Command | null): Promise<ApplyResult | null> {
  const run = async (): Promise<ApplyResult | null> => {
    const doc = useApp.getState().doc;
    if (!doc) return null;
    const command = build(doc);
    return command ? useApp.getState().dispatch(command) : null;
  };
  const next = chain.then(run, run);
  chain = next.catch(() => null);
  return next;
}

const same = (a: unknown, b: unknown) => a === b || JSON.stringify(a) === JSON.stringify(b);
const isPlain = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === "object" && !Array.isArray(v);
type WithId = Record<string, unknown> & { id: string };
const hasIds = (a: unknown[]): a is WithId[] => a.every((v) => isPlain(v) && typeof v.id === "string");

/**
 * `current` with the changes from `base` to `next`. Plain objects merge key
 * by key, arrays item by item (`rebaseArray`); anything else (numbers,
 * strings, null) is replaced whole.
 */
export function rebase<T>(current: T, base: T, next: T): T {
  if (same(base, next)) return current;
  if (Array.isArray(current) && Array.isArray(base) && Array.isArray(next)) return rebaseArray(current, base, next) as T;
  if (!isPlain(base) || !isPlain(next) || !isPlain(current)) return next;
  const out: Record<string, unknown> = { ...current };
  for (const key of Object.keys(next)) out[key] = rebase(current[key], base[key], next[key]);
  return out as T;
}

/**
 * Items with ids (a sheet's viewports, a tour's shots) merge by id: one the
 * edit changed is rebased, one it removed goes, one added since stays, and
 * one it added goes in after the item it follows in the edit; an edit that
 * reorders them wins whole. Items without ids (a lot's sides, a pipe's
 * points) merge by place while no one changed the length; otherwise the
 * edit's array wins whole.
 */
function rebaseArray(current: unknown[], base: unknown[], next: unknown[]): unknown[] {
  if (hasIds(current) && hasIds(base) && hasIds(next)) {
    const was = new Map(base.map((x) => [x.id, x]));
    const now = new Map(next.map((x) => [x.id, x]));
    const kept = base.filter((x) => now.has(x.id)).map((x) => x.id);
    const order = next.filter((x) => was.has(x.id)).map((x) => x.id);
    if (!same(kept, order)) return next;
    const out: WithId[] = [];
    for (const c of current) {
      const from = was.get(c.id);
      const to = now.get(c.id);
      if (!from) out.push(c);
      else if (to) out.push(rebase(c, from, to));
    }
    // A new item has no id yet (""): it is found by itself, the others by id.
    const at = (x: WithId) => (was.has(x.id) ? out.findIndex((o) => o.id === x.id) : out.indexOf(x));
    next.forEach((x, i) => {
      if (was.has(x.id)) return;
      let after = -1;
      for (let j = i - 1; j >= 0 && after < 0; j--) after = at(next[j]);
      out.splice(after + 1, 0, x);
    });
    return out;
  }
  if (current.length === base.length && base.length === next.length) return current.map((c, i) => rebase(c, base[i], next[i]));
  return next;
}

export function editSettings(base: ProjectSettings, next: ProjectSettings): Promise<ApplyResult | null> {
  if (same(base, next)) return Promise.resolve(null);
  return commitLatest((doc) => ({ type: "set_project_settings", settings: rebase(doc.project.settings, base, next) }));
}

/** `base` is `topRoof` as the field drew it; the edit lands on the latest roof over the top floor (D54). */
export function editRoof(base: Roof, next: Roof): Promise<ApplyResult | null> {
  if (same(base, next)) return Promise.resolve(null);
  return commitLatest((doc) => ({ type: "set_roof", roof: rebase(topRoof(doc.project), base, next) }));
}

export function editLevel(base: Level, next: Level): Promise<ApplyResult | null> {
  if (same(base, next)) return Promise.resolve(null);
  return commitLatest((doc) => {
    const level = doc.project.levels.find((l) => l.id === base.id);
    return level ? { type: "update_level", level: rebase(level, base, next) } : null;
  });
}

/** `set_title_block` for an edit made on `base`, the title block the form was drawn with. The engine keeps the stored signatory. */
export function editTitleBlock(base: TitleBlock, next: TitleBlock): Promise<ApplyResult | null> {
  if (same(base, next)) return Promise.resolve(null);
  return commitLatest((doc) => ({ type: "set_title_block", title_block: rebase(doc.project.title_block, base, next) }));
}

/** `set_signatory` for an edit made on `base`. Only people send it (D53). */
export function editSignatory(base: Signatory, next: Signatory): Promise<ApplyResult | null> {
  if (same(base, next)) return Promise.resolve(null);
  return commitLatest((doc) => ({ type: "set_signatory", signatory: rebase(doc.project.title_block.signatory, base, next) }));
}

/**
 * `upsert_sheet` for an edit made on `base`, the sheet the panel was drawn
 * with; `index` also moves it in the set. A sheet deleted since is skipped.
 */
export function editSheet(base: Sheet, next: Sheet, index: number | null = null): Promise<ApplyResult | null> {
  if (same(base, next) && index === null) return Promise.resolve(null);
  return commitLatest((doc) => {
    const sheet = doc.project.sheet_set.sheets.find((sh) => sh.id === base.id);
    return sheet ? { type: "upsert_sheet", sheet: rebase(sheet, base, next), index } : null;
  });
}

export function editLayer(base: Layer, next: Layer): Promise<ApplyResult | null> {
  if (same(base, next)) return Promise.resolve(null);
  return commitLatest((doc) => {
    const layer = doc.project.layers.find((l) => l.key === base.key);
    return layer ? { type: "set_layer", layer: rebase(layer, base, next) } : null;
  });
}

/** The element as it stands now, with the changes from `base` to `next`. */
export function rebaseElement(doc: DocState, base: Element, next: Element): Element | null {
  const current = doc.project.elements.find((e) => e.id === base.id);
  if (!current || current.kind !== next.kind) return null;
  return rebase(current, base, next);
}

/** `update_element` for an edit made on `base`, the element the field was drawn with. */
export function updateElement(base: Element, next: Element): Promise<ApplyResult | null> {
  if (same(base, next)) return Promise.resolve(null);
  return commitLatest((doc) => {
    const element = rebaseElement(doc, base, next);
    return element ? { type: "update_element", element } : null;
  });
}

/**
 * Several element edits as one batch, one undo step. Elements deleted since
 * are skipped; nothing is sent when nothing changes.
 */
export function updateElements(label: string, pairs: Array<{ base: Element; next: Element }>): Promise<ApplyResult | null> {
  return commitLatest((doc) => {
    const commands: Command[] = [];
    for (const { base, next } of pairs) {
      if (same(base, next)) continue;
      const element = rebaseElement(doc, base, next);
      if (element) commands.push({ type: "update_element", element });
    }
    if (commands.length === 0) return null;
    return { type: "batch", label, commands };
  });
}
