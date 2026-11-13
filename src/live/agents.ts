// Pure helpers for agents in the window (DECISIONS D42, D47, D50): one list
// of every agent to show, made from this computer's agents (`agents_list`,
// the `agents` event) and, in a live session, other computers' agents
// (participants with `agent` set and their presence); names, status lines,
// tooltips, limit chips, who is on the bar and on the plan, and the review
// of what the agents changed. No DOM, no store. Tested in agents.test.ts.

import type { AgentActivity, AgentClaim, AgentLimit, AgentSummary, ElementKind, HistoryStep, Level, LiveStatus, Presence, SnapshotMeta } from "../contract/bindings";
import { clientLabel } from "../shell/connect/agents";
import type { HistoryRow } from "../shell/history";
import { firstName, possessive, type Peers } from "./format";

/** An agent quiet this long leaves the top bar and the plan (it stays in the Agents panel). */
export const OFF_BAR_MS = 10 * 60_000;

/** Another computer's agent counts as working while its status line changed this recently. */
export const REMOTE_WORKING_MS = 60_000;

/** One agent as the window shows it. */
export interface AgentView {
  /** Its participant id: history steps, presence and `following` use it. */
  id: string;
  name: string;
  /** Index into `--peer-0` to `--peer-7`, like a person's. */
  color: number;
  /** The MCP client as it named itself: "claude-code". */
  client: string;
  clientVersion: string;
  /** The person it works for. Null on this computer outside a live session (that is you). */
  ownerId: string | null;
  ownerName: string;
  presence: Presence;
  activity: AgentActivity;
  limit: AgentLimit | null;
  claim: AgentClaim | null;
  /** Undo steps it made since the app started. Null for another computer's agent. */
  steps: number | null;
  /** RFC 3339 of its latest call; empty when unknown. */
  lastCallAt: string;
  /** This computer runs it: pause, stop, resume, rename and limits go to this app. */
  local: boolean;
}

export const EMPTY_PRESENCE: Presence = {
  cursor: null,
  level_id: null,
  selection: [],
  typing: null,
  ai_scope: false,
  following: null,
  view_mode: null,
  plan_view: null,
  camera: null,
  pointer_3d: null,
  status: null,
};

/**
 * The app lists this computer's agents and, in a live session, everyone
 * else's (`agents_list`). This computer's have no owner (outside a session)
 * or this window's participant id; only they take pause, stop and limits.
 */
function fromSummary(a: AgentSummary, selfId: string | null): AgentView {
  const p = a.participant;
  const owner = p.agent?.owner_id ?? null;
  return {
    id: p.id,
    name: p.name,
    color: p.color,
    client: p.agent?.client ?? "",
    clientVersion: p.agent?.client_version ?? "",
    ownerId: p.agent?.owner_id ?? null,
    ownerName: p.agent?.owner_name ?? "",
    presence: a.presence,
    activity: a.activity,
    limit: a.limit,
    claim: a.claim,
    steps: a.steps,
    lastCallAt: a.last_call_at,
    local: owner === null || owner === selfId,
  };
}

/**
 * Every agent to show: the app's list first, in its order (newest call
 * first), then agents among the session's participants it did not list, in
 * the host's order, with their presence. An agent in both comes from the list.
 */
export function agentViews(listed: readonly AgentSummary[], status: LiveStatus, peers: Peers, now: number): AgentView[] {
  const out = listed.map((a) => fromSummary(a, status.mode === "off" ? null : status.self_id));
  if (status.mode === "off") return out;
  const seen = new Set(out.map((a) => a.id));
  for (const p of status.participants) {
    if (!p.agent || p.id === status.self_id || seen.has(p.id)) continue;
    const peer = peers[p.id];
    const presence = peer?.presence ?? EMPTY_PRESENCE;
    const busy = peer !== undefined && presence.status !== null && now - peer.changedAt < REMOTE_WORKING_MS;
    out.push({
      id: p.id,
      name: p.name,
      color: p.color,
      client: p.agent.client,
      clientVersion: p.agent.client_version,
      ownerId: p.agent.owner_id,
      ownerName: p.agent.owner_name,
      presence,
      activity: busy ? "working" : "idle",
      limit: null,
      claim: null,
      steps: null,
      lastCallAt: peer ? new Date(peer.changedAt).toISOString() : "",
      local: false,
    });
  }
  return out;
}

/**
 * The agents in the order this window first saw them, so avatars, rows and
 * buttons never trade places under the pointer when another agent makes a
 * call (the app sends newest call first). `seen` remembers the order and
 * grows as new agents arrive.
 */
export function inFirstSeenOrder(views: AgentView[], seen: Map<string, number>): AgentView[] {
  for (const a of views) if (!seen.has(a.id)) seen.set(a.id, seen.size);
  return [...views].sort((x, y) => (seen.get(x.id) ?? 0) - (seen.get(y.id) ?? 0));
}

/** Same agents showing the same things: nothing to redraw. Presence is compared by reference. */
export function sameViews(a: readonly AgentView[], b: readonly AgentView[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const x = a[i];
    const y = b[i];
    if (
      x.id !== y.id ||
      x.name !== y.name ||
      x.color !== y.color ||
      x.activity !== y.activity ||
      x.steps !== y.steps ||
      x.lastCallAt !== y.lastCallAt ||
      x.presence !== y.presence ||
      x.limit !== y.limit ||
      x.claim !== y.claim ||
      x.local !== y.local ||
      x.ownerName !== y.ownerName
    )
      return false;
  }
  return true;
}

export function agentById(views: readonly AgentView[], id: string | null | undefined): AgentView | null {
  if (!id) return null;
  return views.find((a) => a.id === id) ?? null;
}

/** The client's own name: "claude-code" reads "Claude Code". Empty when unknown. */
export function clientName(a: Pick<AgentView, "client">): string {
  return a.client ? clientLabel(a.client) : "";
}

/** "Ground floor (Claude Code)", or just the name when it is the client's. */
export function agentTitle(a: Pick<AgentView, "name" | "client">): string {
  const client = clientName(a);
  if (!client || client.toLowerCase() === a.name.trim().toLowerCase()) return a.name;
  return `${a.name} (${client})`;
}

/** "your agent", or "Ana's agent" for someone else's in a live session. */
export function ownerLine(a: Pick<AgentView, "ownerId" | "ownerName">, selfId: string | null): string {
  if (a.ownerId === null || a.ownerId === selfId || !a.ownerName) return "your agent";
  return `${possessive(a.ownerName)} agent`;
}

/** What it is doing, in a few words: its status line, or what its activity means. */
export function statusText(a: Pick<AgentView, "activity" | "presence">): string {
  const line = a.presence.status?.trim() || null;
  switch (a.activity) {
    case "working":
      return line ?? "Working";
    case "looking":
      return line ?? "Looking at the plan";
    case "idle":
      return "Idle";
    case "paused":
      return "Paused";
    case "stopped":
      return "Stopped";
  }
}

export function stepsText(n: number | null): string {
  if (n === null) return "";
  return n === 1 ? "1 step" : `${n} steps`;
}

/**
 * The avatar's tooltip: "Ground floor (Claude Code), your agent: Placing
 * doors. 31 steps. Click to follow".
 */
export function agentTip(a: AgentView, selfId: string | null, followed: boolean): string {
  if (followed) return `Following ${a.name}. Click to stop`;
  const steps = stepsText(a.steps);
  return `${agentTitle(a)}, ${ownerLine(a, selfId)}: ${statusText(a)}.${steps ? ` ${steps}.` : ""} Click to follow`;
}

/** Shown on the top bar and the plan: working lately, paused (so it is not forgotten), or another computer's. */
export function onBar(a: Pick<AgentView, "local" | "activity" | "lastCallAt">, now: number): boolean {
  if (!a.local || a.activity === "paused" || a.activity === "working" || a.activity === "looking") return true;
  const t = Date.parse(a.lastCallAt);
  return Number.isFinite(t) && now - t < OFF_BAR_MS;
}

/** Agents whose plan point is on `levelId`, for their cursors. */
export function agentCursorIds(views: readonly AgentView[], levelId: string | null, now: number): string[] {
  if (!levelId) return [];
  return views.filter((a) => a.presence.cursor && a.presence.level_id === levelId && onBar(a, now)).map((a) => a.id);
}

/** Agents with a point on the model, for their 3D pointers. Every level shows, like people's. */
export function agentPointerIds(views: readonly AgentView[], now: number): string[] {
  return views.filter((a) => a.presence.pointer_3d && onBar(a, now)).map((a) => a.id);
}

/** Agents working on each level now, for the Levels section's dots. */
export function agentsByLevel(views: readonly AgentView[], now: number): Map<string, AgentView[]> {
  const out = new Map<string, AgentView[]>();
  for (const a of views) {
    const level = a.presence.level_id;
    if (!level || !onBar(a, now) || a.activity === "stopped") continue;
    const list = out.get(level);
    if (list) list.push(a);
    else out.set(level, [a]);
  }
  return out;
}

/** Pause, Stop and Resume as they apply now. */
export function controlsFor(a: Pick<AgentView, "activity" | "local">): { pause: boolean; stop: boolean; resume: boolean } {
  if (!a.local) return { pause: false, stop: false, resume: false };
  const held = a.activity === "paused" || a.activity === "stopped";
  return { pause: !held, stop: a.activity !== "stopped", resume: held };
}

// ---------------------------------------------------------------- limits

export const KIND_LABEL: Record<ElementKind, string> = {
  wall: "Walls",
  opening: "Doors and windows",
  room: "Rooms",
  column: "Columns",
  stair: "Stairs",
  asset: "Objects",
  annotation: "Text",
  dimension: "Dimensions",
  camera: "Cameras",
  underlay: "Underlays",
  linework: "Linework",
  reference_model: "3D models",
  pipe: "Services",
  slab: "Slabs",
  roof: "Roofs",
  grid: "Grid lines",
  view_mark: "Section and elevation marks",
  lot: "The lot",
  tour: "Tours",
};

/** The kinds offered in the limit menu, in the order an architect thinks of them. */
export const LIMIT_KINDS: ElementKind[] = ["wall", "opening", "room", "slab", "stair", "column", "asset", "pipe", "dimension", "annotation"];

export interface LimitChip {
  key: "levels" | "ids" | "kinds";
  label: string;
  tip: string;
}

/**
 * One chip per part of a limit: "Ground Floor", "Selection: 3", "Walls,
 * Rooms". None without a limit. Levels and elements the open plan does not
 * have are left out (`elementIds` when given): right after a project
 * switch, a limit can still name the last project's until the app sends
 * the new one, and a chip for it would say something untrue.
 */
export function limitChips(limit: AgentLimit | null, levels: readonly Pick<Level, "id" | "name">[], elementIds?: ReadonlySet<string>): LimitChip[] {
  if (!limit) return [];
  const out: LimitChip[] = [];
  const names = limit.level_ids.flatMap((id) => levels.find((l) => l.id === id)?.name ?? []);
  if (names.length > 0) {
    out.push({ key: "levels", label: names.length <= 2 ? names.join(", ") : `${names.length} levels`, tip: `Only ${names.join(", ")}` });
  }
  const n = elementIds ? limit.ids.filter((id) => elementIds.has(id)).length : limit.ids.length;
  if (n > 0) {
    out.push({ key: "ids", label: `Selection: ${n}`, tip: `Only ${n === 1 ? "1 element" : `${n} elements`} and what they reach` });
  }
  if (limit.kinds.length > 0) {
    const names = limit.kinds.map((k) => KIND_LABEL[k] ?? k);
    out.push({ key: "kinds", label: names.length <= 2 ? names.join(", ") : `${names.length} kinds`, tip: `Only ${names.join(", ").toLowerCase()}` });
  }
  return out;
}

/** The limit without one part; null once nothing is left. */
export function withoutPart(limit: AgentLimit | null, key: LimitChip["key"]): AgentLimit | null {
  if (!limit) return null;
  const next: AgentLimit = {
    level_ids: key === "levels" ? [] : limit.level_ids,
    ids: key === "ids" ? [] : limit.ids,
    kinds: key === "kinds" ? [] : limit.kinds,
  };
  return next.level_ids.length + next.ids.length + next.kinds.length === 0 ? null : next;
}

/** The limit with one part set, the other parts kept. */
export function withPart(limit: AgentLimit | null, patch: Partial<AgentLimit>): AgentLimit | null {
  const next: AgentLimit = { level_ids: limit?.level_ids ?? [], ids: limit?.ids ?? [], kinds: limit?.kinds ?? [], ...patch };
  return next.level_ids.length + next.ids.length + next.kinds.length === 0 ? null : next;
}

/** A kind switched on or off in the limit. */
export function toggleKind(limit: AgentLimit | null, kind: ElementKind): AgentLimit | null {
  const kinds = limit?.kinds ?? [];
  return withPart(limit, { kinds: kinds.includes(kind) ? kinds.filter((k) => k !== kind) : [...kinds, kind] });
}

/** A name as the app keeps it: control characters out, spaces folded, 1 to 40 characters. */
export function cleanAgentName(raw: string): string {
  // eslint-disable-next-line no-control-regex
  return [...raw.replace(/[\u0000-\u001f\u007f]/g, "").replace(/\s+/g, " ").trim()].slice(0, 40).join("");
}

/** "Claimed by Ground floor", for the faint outline on the plan. */
export function claimLine(name: string): string {
  return `Claimed by ${name}`;
}

// ---------------------------------------------------------------- history and review

/** A step an agent made: by a known agent, or for someone (`on_behalf_*`). */
export function isAgentStep(step: Pick<HistoryStep, "author_id" | "on_behalf_of" | "on_behalf_name">, agentIds: ReadonlySet<string>): boolean {
  if (step.author_id && agentIds.has(step.author_id)) return true;
  // `!=`: a step from before these fields has neither.
  return step.on_behalf_of != null || step.on_behalf_name != null;
}

/** What the app names the owner when this computer has no profile name yet. */
export const THIS_COMPUTER = "this computer";

/**
 * "for Axl", "for you", or null for a person's own step. The app marks
 * every agent step with this computer's host participant id and its
 * profile name (or "this computer"), session or not. Outside a session the
 * window has no participant id (`selfId` null), so the name says it: the
 * profile name, or "this computer", is you. In a session, the id does.
 */
export function forLine(step: Pick<HistoryStep, "on_behalf_of" | "on_behalf_name">, selfId: string | null, selfName: string | null): string | null {
  const name = step.on_behalf_name?.trim() || null;
  if (step.on_behalf_of == null && name === null) return null;
  if (selfId !== null && step.on_behalf_of === selfId) return "for you";
  if (name !== null && selfName !== null && name === selfName.trim()) return "for you";
  if (name !== null && name.toLowerCase() === THIS_COMPUTER) return selfId === null ? "for you" : null;
  return name ? `for ${firstName(name)}` : null;
}

/** The label the app gives the version it saves before agents start ("Before agents, 3:42 PM"). */
export const BEFORE_AGENTS = /^before agents\b/i;

/**
 * Where the review starts: the newest automatic "Before agents" version, or
 * null when there is none (then the review covers the kept history).
 */
export function autoVersion(snapshots: readonly SnapshotMeta[]): SnapshotMeta | null {
  let best: SnapshotMeta | null = null;
  for (const s of snapshots) {
    if (!BEFORE_AGENTS.test(s.label.trim())) continue;
    if (!best || Date.parse(s.created_at) > Date.parse(best.created_at)) best = s;
  }
  return best;
}

export interface ReviewGroup {
  /** The agent's participant id, or its name when the step has no id. */
  key: string;
  name: string;
  color: number | null;
  /** Its done steps since the review starts, newest first. */
  rows: HistoryRow[];
  /** Every element its steps touched, once each, newest first. */
  touched: string[];
}

/**
 * What the agents changed since `sinceIso` (null: the whole kept history):
 * done steps only (an undone step is not on the plan), grouped by agent,
 * the agent with the newest step first.
 */
export function reviewGroups(rows: readonly HistoryRow[], sinceIso: string | null, agentIds: ReadonlySet<string>): ReviewGroup[] {
  const since = sinceIso ? Date.parse(sinceIso) : Number.NEGATIVE_INFINITY;
  const out: ReviewGroup[] = [];
  for (const r of rows) {
    if (r.stack !== "undo" || !isAgentStep(r.step, agentIds)) continue;
    const at = Date.parse(r.step.at);
    // A step without a time (older hosts) counts: it cannot be placed before the version.
    if (Number.isFinite(at) && at < since) continue;
    const key = r.step.author_id ?? `name:${r.name}`;
    let g = out.find((x) => x.key === key);
    if (!g) {
      g = { key, name: r.name, color: r.color, rows: [], touched: [] };
      out.push(g);
    }
    g.rows.push(r);
    for (const id of [...r.step.ids, ...r.step.removed]) if (!g.touched.includes(id)) g.touched.push(id);
  }
  return out;
}

/** Whose steps an "Undo to here" takes back, one line per author. */
export interface UndoLine {
  key: string;
  name: string;
  agent: boolean;
  count: number;
}

/**
 * Every step "Undo to here" on a row `steps` deep takes back, by author,
 * in the order they first come (newest first): someone else's later steps
 * go too, and the review says so before it acts.
 */
export function undoLines(rows: readonly HistoryRow[], steps: number): UndoLine[] {
  const out: UndoLine[] = [];
  for (const r of rows) {
    if (r.stack !== "undo" || r.steps > steps) continue;
    const line = out.find((x) => x.key === r.person);
    if (line) line.count++;
    else out.push({ key: r.person, name: r.name, agent: r.agent, count: 1 });
  }
  return out;
}

/** "Ground floor, an AI agent: 3 steps", "You: 1 step". */
export function undoLineText(line: UndoLine): string {
  return `${line.name}${line.agent ? ", an AI agent" : ""}: ${stepsText(line.count)}`;
}

/** "3 steps, 12 elements". */
export function groupLine(g: Pick<ReviewGroup, "rows" | "touched">): string {
  const n = g.touched.length;
  return `${stepsText(g.rows.length)}, ${n === 1 ? "1 element" : `${n} elements`}`;
}

/** A key per step, stable while it sits in the undo stack (like history.ts's row keys). */
export function stepKey(step: Pick<HistoryStep, "at" | "author_id" | "label" | "ids" | "removed">): string {
  return `${step.at}|${step.author_id ?? ""}|${step.label}|${step.ids[0] ?? step.removed[0] ?? ""}`;
}

/**
 * The newest steps of each agent that are not in `seen`, newest first: what
 * to tint on the plan as its trail. Stops at the first step already seen,
 * so an undo never brings old steps back as new.
 */
export function newAgentSteps(undo: readonly HistoryStep[], seen: ReadonlySet<string>, agentIds: ReadonlySet<string>, max = 5): Map<string, HistoryStep[]> {
  const out = new Map<string, HistoryStep[]>();
  for (const step of undo) {
    if (seen.has(stepKey(step))) break;
    if (!step.author_id || !isAgentStep(step, agentIds)) continue;
    const list = out.get(step.author_id) ?? [];
    if (list.length < max) list.push(step);
    out.set(step.author_id, list);
  }
  return out;
}

// ---------------------------------------------------------------- color

/** A `#rrggbb` color at `alpha`, for faint outlines; other colors come back as they are. */
export function withAlpha(color: string, alpha: number): string {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(color.trim());
  if (!m) return color;
  const a = Math.round(Math.min(1, Math.max(0, alpha)) * 100) / 100;
  return `rgba(${parseInt(m[1], 16)}, ${parseInt(m[2], 16)}, ${parseInt(m[3], 16)}, ${a})`;
}
