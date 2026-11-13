// Pure words and rules for the swarm panel and the draft bar: who is the
// lead, what each part's pill says, what the bar says, who depends on whom,
// and when Accept or Discard has to ask first. Tested in view.test.ts.
// In these words the building is "the plan" or "the project", never "the
// model", which would read as the AI model.

import { clientLabel } from "../shell/connect/agents";
import type { SwarmConflict, SwarmCost, SwarmPart, SwarmPartState, SwarmStatus } from "./types";

export const LEAD_KEY = "lead";

/** The lock's banner and the disabled tools' hint while a draft is open (D57). */
export const LOCK_HINT = "A swarm draft is open. Accept or discard it to edit the plan";

/** What waits for Accept, said wherever the swarm costs or runs. */
export const NOTHING_UNTIL_ACCEPT = "Nothing goes into the project until you accept it";

export function leadOf(status: SwarmStatus | null): SwarmPart | null {
  return status?.parts.find((p) => p.key === LEAD_KEY) ?? null;
}

/** The parts that draw: every part but the lead. */
export function workersOf(status: SwarmStatus | null): SwarmPart[] {
  return status ? status.parts.filter((p) => p.key !== LEAD_KEY) : [];
}

export function partName(status: SwarmStatus | null, key: string): string {
  return status?.parts.find((p) => p.key === key)?.name ?? key;
}

const PILL: Record<SwarmPartState, string> = {
  waiting: "Waiting",
  working: "Working",
  paused: "Paused",
  blocked: "Blocked",
  done: "Done",
  stopped: "Stopped",
  failed: "Failed",
  accepted: "Accepted",
};

/** The state pill: "Waiting for Shell" names what it waits for. */
export function pillText(part: SwarmPart, status: SwarmStatus | null): string {
  if (part.state === "waiting" && part.waiting_for.length > 0) {
    const w = part.waiting_for[0];
    return `Waiting for ${partName(status, w.part)}`;
  }
  return PILL[part.state];
}

/** A part still drawing or about to: Stop applies. */
export function isLive(part: SwarmPart): boolean {
  return part.state === "waiting" || part.state === "working" || part.state === "paused";
}

/** A part with work in the draft that a person can accept or discard. */
export function isOpen(part: SwarmPart): boolean {
  return part.state !== "accepted" && part.steps > 0;
}

/** The draft holds something to accept: Accept all is on. */
export function hasDraftWork(status: SwarmStatus): boolean {
  return status.added + status.modified + status.removed > 0 || workersOf(status).some(isOpen);
}

/** Why Start swarm is off, else its shortcut, for the button's tip. `mod` is the platform's ⌘ or Ctrl+. */
export function startTip(o: { hasDoc: boolean; hasKey: boolean; brief: string }, mod: string): string {
  if (!o.hasDoc) return "Open a project first";
  if (!o.hasKey) return "Add a Claude API key in the copilot settings";
  if (o.brief.trim() === "") return "Write a brief first";
  return `${mod}Enter starts the swarm`;
}

/** "The swarm drew 48 elements in a draft". */
export function draftLine(status: SwarmStatus): string {
  const drew = status.added + status.modified;
  const base = `The swarm drew ${drew} ${drew === 1 ? "element" : "elements"} in a draft`;
  return status.removed > 0 ? `${base}, and removes ${status.removed}` : base;
}

/** "Claude Code is driving 3 sub-agents": an MCP swarm in place of the prompt. */
export function drivingLine(status: SwarmStatus): string | null {
  if (status.source.type !== "mcp") return null;
  const n = workersOf(status).length;
  // "claude-code" reads "Claude Code", as everywhere else in the app.
  const who = clientLabel(status.source.client.trim()) || "Your AI";
  return `${who} is driving ${n} ${n === 1 ? "sub-agent" : "sub-agents"}`;
}

export function usd(v: number): string {
  return `US$${v.toFixed(2)}`;
}

/** The model the swarm runs on, as people read it ("Claude Opus 5.5"), else its id. */
export function modelName(cost: SwarmCost | null): string | null {
  if (!cost) return null;
  return (cost.model_label ?? cost.model).trim() || null;
}

/**
 * The estimate before a start, one phrase per span (the panel sets them
 * apart with a gap, not middle dots): "4 agents on Claude Opus 5.5", "About
 * US$0.76 to US$2.42 on your Claude key (estimate)", "Nothing goes into the
 * project until you accept it". Before the lead plans, the count is not
 * known: "Agents on Claude Opus 5.5".
 */
export function estimateParts(cost: SwarmCost | null, agents: number | null): string[] {
  const model = modelName(cost);
  const on = model ? ` on ${model}` : "";
  const who = agents ? `${agents} agents${on}` : model ? `Agents${on}` : null;
  const price = cost && cost.low_usd !== null && cost.high_usd !== null ? `About ${usd(cost.low_usd)} to ${usd(cost.high_usd)} on your Claude key (estimate)` : "Runs on your Claude key";
  return [...(who ? [who] : []), price, NOTHING_UNTIL_ACCEPT];
}

/** While it runs: what it spent against the cap. */
export function spendLine(cost: SwarmCost | null): string | null {
  if (!cost) return null;
  return `Spent ${usd(cost.spent_usd)} of the ${usd(cost.cap_usd)} cap`;
}

/**
 * The engine's note when the price table does not know the model: it bills
 * at the table's highest rate so the cap still holds. Null when priced.
 */
export function priceNote(cost: SwarmCost | null): string | null {
  return cost?.price_note?.trim() || null;
}

/** "Waiting for the AI service, 12 s" while a part backs off a rate limit. */
export function aiWaitLine(until: string | null, now: number): string | null {
  if (!until) return null;
  const at = Date.parse(until);
  if (!Number.isFinite(at)) return null;
  const s = Math.ceil((at - now) / 1000);
  return s > 0 ? `Waiting for the AI service, ${s} s` : null;
}

/**
 * The parts `key` builds on, from what each part waits for, nearest last:
 * accepting `key` brings them in first. Accepted parts are already in.
 */
export function dependsOn(parts: readonly SwarmPart[], key: string): string[] {
  const byKey = new Map(parts.map((p) => [p.key, p]));
  const out: string[] = [];
  const seen = new Set<string>([key]);
  const visit = (k: string) => {
    for (const w of byKey.get(k)?.waiting_for ?? []) {
      if (seen.has(w.part)) continue;
      seen.add(w.part);
      const p = byKey.get(w.part);
      if (!p || p.state === "accepted") continue;
      visit(w.part);
      out.push(w.part);
    }
  };
  visit(key);
  return out;
}

/** The parts that build on `key`, directly or not: discarding `key` takes them too. */
export function dependents(parts: readonly SwarmPart[], key: string): string[] {
  const out: string[] = [];
  const seen = new Set<string>([key]);
  let frontier = [key];
  while (frontier.length > 0) {
    const next: string[] = [];
    for (const p of parts) {
      if (seen.has(p.key) || p.state === "accepted") continue;
      if (p.waiting_for.some((w) => frontier.includes(w.part))) {
        seen.add(p.key);
        out.push(p.key);
        next.push(p.key);
      }
    }
    frontier = next;
  }
  return out;
}

/** Feed rows shown, newest first. */
export const FEED_ROWS = 14;

export interface FeedRow {
  /** Stable across updates, unique even for two items alike in the same second. */
  key: string;
  part: string;
  name: string;
  text: string;
}

/**
 * The feed, newest first, at most 14 rows. A line that already starts with
 * the part's name ("Shell: added 1 wall") loses it, since the row shows it.
 */
export function feedRows(status: SwarmStatus): FeedRow[] {
  const seen = new Map<string, number>();
  const rows = status.feed.map((f) => {
    const base = `${f.at}|${f.part}|${f.text}`;
    const n = (seen.get(base) ?? 0) + 1;
    seen.set(base, n);
    const name = partName(status, f.part);
    const text = f.text.startsWith(`${name}: `) ? f.text.slice(name.length + 2) : f.text;
    return { key: `${base}|${n}`, part: f.part, name, text, at: f.at };
  });
  // Newest first; the feed's own order breaks ties (later is newer).
  return rows
    .map((r, i) => ({ r, i }))
    .sort((a, b) => (a.r.at < b.r.at ? 1 : a.r.at > b.r.at ? -1 : b.i - a.i))
    .slice(0, FEED_ROWS)
    .map(({ r }) => ({ key: r.key, part: r.part, name: r.name, text: r.text }));
}

/** "Shell", "Shell and Rooms", "Shell, Rooms and Roof". */
export function nameList(status: SwarmStatus | null, keys: readonly string[]): string {
  const names = keys.map((k) => partName(status, k));
  if (names.length <= 1) return names[0] ?? "";
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/** An Accept or Discard that asks first. */
export interface Ask {
  kind: "accept" | "discard";
  /** The part asked for, null for the whole draft. */
  part: string | null;
  /** Every part the action takes, in order. */
  parts: string[];
  text: string;
  confirm: string;
  /** What the accept would leave differently from the draft view, one line each (at most 3 shown). */
  differs?: string[];
  /** Steps that no longer apply and would stay out, one line each (at most 3 shown). */
  skipped?: string[];
  /** The confirm accepts the rest without those steps (`accept_rest`): they stay in the draft as conflicts. */
  rest?: boolean;
  /** An accept: each part's last step as the person saw it when they asked. The confirm sends these, not newer ones. */
  reviewed?: Record<string, number>;
  /** Where it was asked: the draft bar or the panel shows it there. */
  origin?: AskOrigin;
}

export type AskOrigin = "bar" | "panel";

/** "2 elements will come out differently from the draft." */
function differLine(differs: readonly SwarmConflict[]): string {
  const n = differs.length;
  return `${n} ${n === 1 ? "element" : "elements"} will come out differently from the draft.`;
}

/** "2 steps no longer apply. Accepting the rest leaves them in the draft as conflicts." */
function skipLine(skipped: readonly SwarmConflict[]): string {
  const n = skipped.length;
  return `${n} ${n === 1 ? "step no longer applies" : "steps no longer apply"}. Accepting the rest leaves ${n === 1 ? "it" : "them"} in the draft as conflicts.`;
}

/** "Rooms: Wall 4000 mm changed in the plan since the draft." One conflict under the draft bar, by part. */
export function conflictLine(status: SwarmStatus | null, c: SwarmConflict): string {
  return c.part ? `${partName(status, c.part)}: ${c.message}` : c.message;
}

/** "Dims, Auto dimension: Nothing to dimension." One skipped step for the confirm's list. */
export function skippedLine(status: SwarmStatus | null, c: SwarmConflict): string {
  const who = [c.part ? partName(status, c.part) : "", c.label].filter(Boolean).join(", ");
  return who ? `${who}: ${c.message}` : c.message;
}

/**
 * Accepting a part asks first when it pulls in parts it depends on, when
 * the engine says something would come out differently from the draft view
 * (`swarm_check` `differs`), or when steps no longer apply (`skipped`): then
 * the confirm is "Accept the rest". `pulled` comes from `swarm_check` when
 * the engine knows, else from `dependsOn`.
 */
export function acceptAsk(status: SwarmStatus, key: string, pulled: readonly string[], differs: readonly SwarmConflict[] = [], skipped: readonly SwarmConflict[] = []): Ask | null {
  const extra = pulled.filter((k) => k !== key);
  const parts = [...extra, key];
  const drawing = drawingIn(status, parts);
  if (extra.length === 0 && differs.length === 0 && skipped.length === 0 && drawing.length === 0) return null;
  const builds = extra.length > 0 ? `${partName(status, key)} builds on ${nameList(status, extra)}. Accepting it accepts ${extra.length === 1 ? "that part" : "those parts"} first, each as its own undo step.` : "";
  const rest = skipped.length > 0;
  return {
    kind: "accept",
    part: key,
    parts,
    text: [builds, stillDrawing(status, drawing, "it"), rest ? skipLine(skipped) : "", differs.length > 0 ? differLine(differs) : ""].filter(Boolean).join(" "),
    confirm: rest ? "Accept the rest" : extra.length > 0 ? `Accept ${parts.length} parts` : `Accept ${partName(status, key)}`,
    differs: differs.map((d) => d.message),
    skipped: skipped.map((c) => skippedLine(status, c)),
    rest,
  };
}

/**
 * Accept all asks first when a part is still drawing (accepting ends the
 * swarm), when something would come out differently from the draft view, or
 * when steps no longer apply ("Accept the rest").
 */
export function acceptAllAsk(status: SwarmStatus, differs: readonly SwarmConflict[], skipped: readonly SwarmConflict[] = []): Ask | null {
  const open = status.parts.filter(isOpen).map((p) => p.key);
  const drawing = drawingIn(status, open);
  if (differs.length === 0 && skipped.length === 0 && drawing.length === 0) return null;
  const rest = skipped.length > 0;
  return {
    kind: "accept",
    part: null,
    parts: open,
    text: [stillDrawing(status, drawing, "all"), rest ? skipLine(skipped) : "", differs.length > 0 ? differLine(differs) : ""].filter(Boolean).join(" "),
    confirm: rest ? "Accept the rest" : drawing.length > 0 ? "Accept all now" : "Accept all anyway",
    differs: differs.map((d) => d.message),
    skipped: skipped.map((c) => skippedLine(status, c)),
    rest,
  };
}

/** The parts among `keys` still drawing or about to. */
function drawingIn(status: SwarmStatus, keys: readonly string[]): string[] {
  return keys.filter((k) => {
    const p = status.parts.find((x) => x.key === k);
    return !!p && isLive(p);
  });
}

/** "Shell is still drawing: accepting it now takes its steps so far and stops it." */
function stillDrawing(status: SwarmStatus, drawing: readonly string[], what: "it" | "all"): string {
  if (drawing.length === 0) return "";
  const who = nameList(status, drawing);
  const is = drawing.length === 1 ? "is" : "are";
  if (what === "all") return `${who} ${is} still drawing. Accepting all now takes the steps so far and ends the swarm.`;
  return `${who} ${is} still drawing. Accepting now takes ${drawing.length === 1 ? "its" : "their"} steps so far and stops ${drawing.length === 1 ? "it" : "them"}.`;
}

/** Discarding a part asks first when other parts depend on it. */
export function discardAsk(status: SwarmStatus, key: string, takes: readonly string[]): Ask | null {
  const extra = takes.filter((k) => k !== key);
  if (extra.length === 0) return null;
  return {
    kind: "discard",
    part: key,
    parts: [key, ...extra],
    text: `${nameList(status, extra)} ${extra.length === 1 ? "builds" : "build"} on ${partName(status, key)}. Discarding it discards ${extra.length === 1 ? "that part" : "those parts"} too.`,
    confirm: `Discard ${extra.length + 1} parts`,
  };
}

/** Discarding the whole draft always asks: the work is gone after it. */
export function discardAllAsk(status: SwarmStatus): Ask {
  return {
    kind: "discard",
    part: null,
    parts: workersOf(status).filter(isOpen).map((p) => p.key),
    text: "Discard the whole draft? Nothing was added to the project, and the draft cannot be brought back.",
    confirm: "Discard the draft",
  };
}

/**
 * Each part's newest step as the person saw it: Accept is pinned to these.
 * `seen` is per part the newest step the drawn view shows (0 when none). The
 * engine accepts only when every pin equals the part's `last_seq`
 * (`check_pins`), so a step that never reached the screen, and an undo since
 * the person looked, make the accept `stale` instead of slipping in or out.
 * Without `seen`, the parts' own `last_seq`.
 */
export function pinsOf(status: SwarmStatus, seen: Readonly<Record<string, number>> | null = null): Record<string, number> {
  const pins: Record<string, number> = {};
  for (const p of status.parts) pins[p.key] = seen ? (seen[p.key] ?? 0) : p.last_seq;
  return pins;
}
