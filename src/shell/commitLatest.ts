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
// the latest object, field by field, into nested objects (the site, a light).
import type { ApplyResult, Command, DocState, Element, Layer, Level, ProjectSettings, Roof } from "../contract/bindings";
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

/**
 * `current` with the changes from `base` to `next`. Plain objects merge key
 * by key; anything else (numbers, strings, arrays, null) is replaced whole.
 */
export function rebase<T>(current: T, base: T, next: T): T {
  if (same(base, next)) return current;
  if (!isPlain(base) || !isPlain(next) || !isPlain(current)) return next;
  const out: Record<string, unknown> = { ...current };
  for (const key of Object.keys(next)) out[key] = rebase(current[key], base[key], next[key]);
  return out as T;
}

export function editSettings(base: ProjectSettings, next: ProjectSettings): Promise<ApplyResult | null> {
  if (same(base, next)) return Promise.resolve(null);
  return commitLatest((doc) => ({ type: "set_project_settings", settings: rebase(doc.project.settings, base, next) }));
}

export function editRoof(base: Roof, next: Roof): Promise<ApplyResult | null> {
  if (same(base, next)) return Promise.resolve(null);
  return commitLatest((doc) => ({ type: "set_roof", roof: rebase(doc.project.roof, base, next) }));
}

export function editLevel(base: Level, next: Level): Promise<ApplyResult | null> {
  if (same(base, next)) return Promise.resolve(null);
  return commitLatest((doc) => {
    const level = doc.project.levels.find((l) => l.id === base.id);
    return level ? { type: "update_level", level: rebase(level, base, next) } : null;
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
