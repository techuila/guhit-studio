// Swarm statuses for tests and the dev page: one per panel state. Not used by
// the app itself.

import type { SwarmApi } from "./api";
import type { SwarmConflict, SwarmPart, SwarmStatus } from "./types";
import { dependents, dependsOn } from "./view";

export function mockPart(key: string, name: string, color: number, state: SwarmPart["state"], steps: number, more: Partial<SwarmPart> = {}): SwarmPart {
  return {
    key,
    name,
    role: more.role ?? name,
    color,
    agent_id: more.agent_id ?? `agent-${key}`,
    state,
    status: more.status ?? null,
    steps,
    last_seq: more.last_seq ?? steps,
    milestones: more.milestones ?? [],
    waiting_for: more.waiting_for ?? [],
    conflicts: more.conflicts ?? 0,
  };
}

const BRIEF = "Two storey house for a 12 x 15 m lot in Quezon City: 3 bedrooms, 2.5 baths, open living and dining, carport, lanai.";

export function mockStatus(more: Partial<SwarmStatus> = {}): SwarmStatus {
  return {
    id: "sw-1",
    project_id: "p-1",
    source: { type: "app" },
    state: "running",
    brief: BRIEF,
    plan: null,
    parts: [
      mockPart("lead", "Lead", 6, "done", 1, { role: "Splits the brief", status: "Watching the team" }),
      mockPart("shell", "Shell", 0, "working", 7, { role: "Walls and columns", status: "Laying out the outer walls" }),
      mockPart("rooms", "Rooms", 2, "working", 4, { role: "Rooms, doors, windows", status: "Front door, 900 mm", waiting_for: [{ part: "shell", milestone: "walls" }] }),
      mockPart("roof", "Roof", 1, "waiting", 0, { role: "Roof and stairs", waiting_for: [{ part: "shell", milestone: "done" }] }),
    ],
    feed: [
      { at: "2026-11-10T03:42:01Z", part: "lead", text: "split the work: Shell, Rooms, Roof" },
      { at: "2026-11-10T03:42:05Z", part: "shell", text: "drew the outer walls" },
      { at: "2026-11-10T03:42:09Z", part: "rooms", text: "named the living room" },
    ],
    draft_revision: 12,
    main_revision: 40,
    added: 21,
    modified: 2,
    removed: 0,
    conflicts: [],
    cost: { model: "claude-opus-5-5", model_label: "Claude Opus 5.5", low_usd: 1.6, high_usd: 2.4, spent_usd: 0.42, cap_usd: 3, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0 },
    ai_wait_until: null,
    ...more,
  };
}

/** Every panel state, by name. */
export function panelStates(now = Date.now()): Record<string, SwarmStatus | null> {
  const running = mockStatus();
  return {
    idle: null,
    planning: mockStatus({
      state: "planning",
      parts: [mockPart("lead", "Lead", 6, "working", 0, { role: "Splits the brief", status: "Reading the brief" })],
      feed: [],
      added: 0,
      modified: 0,
      draft_revision: 0,
    }),
    running,
    paused: mockStatus({ state: "paused", parts: running.parts.map((p) => (p.key === "lead" ? p : { ...p, state: p.state === "working" ? "paused" : p.state })) }),
    waiting_ai: mockStatus({ ai_wait_until: new Date(now + 12_000).toISOString() }),
    ready: mockStatus({
      state: "ready",
      parts: running.parts.map((p) => ({ ...p, state: "done", steps: Math.max(p.steps, 3), status: "Done" })),
      feed: [{ at: "2026-11-10T03:43:30Z", part: "roof", text: "finished: 5 steps" }, ...running.feed],
      added: 48,
    }),
    conflicts: mockStatus({
      state: "ready",
      parts: running.parts.map((p) => ({ ...p, state: "done", steps: Math.max(p.steps, 3) })),
      conflicts: [{ part: "rooms", step: 9, label: "Add door", code: "changed_on_main", message: "Wall 4000 mm changed in the model since the draft.", element_ids: [] }],
    }),
    mcp: mockStatus({
      source: { type: "mcp", client: "Claude Code" },
      cost: null,
      parts: [
        mockPart("lead", "Lead", 6, "working", 1, { role: "Claude Code", status: "Watching the team" }),
        mockPart("shell", "Shell", 0, "done", 9, { status: "Done" }),
        mockPart("rooms", "Rooms", 2, "working", 5, { status: "Kitchen window, 1200 mm" }),
        mockPart("sheets", "Sheets", 4, "working", 2, { status: "Grid lines 1 to 3" }),
      ],
    }),
    failed: mockStatus({ state: "failed", parts: running.parts.map((p) => (p.key === "rooms" ? { ...p, state: "failed", status: "The plan could not be split" } : p)) }),
    accepted_one: mockStatus({ state: "ready", parts: running.parts.map((p) => (p.key === "shell" ? { ...p, state: "accepted" } : { ...p, state: "done" })) }),
  };
}

/**
 * A backend that answers from memory, for tests and the dev page. Like the
 * engine, an accept whose steps would be skipped is refused `steps_skipped`
 * unless it says `acceptRest`.
 */
export function mockApi(status: () => SwarmStatus | null, calls: string[] = [], differs: () => SwarmConflict[] = () => [], skipped: () => SwarmConflict[] = () => []): SwarmApi {
  const ok = <T,>(name: string, v: T) => {
    calls.push(name);
    return Promise.resolve(v);
  };
  const st = () => status() ?? mockStatus();
  return {
    status: () => ok("status", status()),
    draft: () => ok("draft", null),
    estimate: () => ok("estimate", { model: "claude-opus-5-5", model_label: "Claude Opus 5.5", low_usd: 1.6, high_usd: 2.4, spent_usd: 0, cap_usd: 3, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0 }),
    start: () => ok("start", st()),
    pause: () => ok("pause", { ...st(), state: "paused" }),
    resume: () => ok("resume", { ...st(), state: "running" }),
    stop: (part) => ok(`stop:${part ?? "all"}`, st()),
    // Like the engine: what accepting pulls in, in order, and who builds on it.
    check: (parts) => {
      const keys = parts ?? st().parts.map((p) => p.key);
      const pulled = [...new Set(keys.flatMap((k) => [...dependsOn(st().parts, k), k]))];
      const takes = [...new Set(keys.flatMap((k) => dependents(st().parts, k)))];
      return ok(`check:${keys.join(",")}`, { applied: pulled.map((part) => ({ part, revision: 0, summary: "" })), skipped: skipped(), state: null, dependents: takes, differs: differs() });
    },
    accept: (_id, parts, _reviewed, acceptRest) => {
      const name = `accept:${parts === null ? "all" : parts.join(",")}${acceptRest ? "+rest" : ""}`;
      const left = skipped();
      if (left.length > 0 && !acceptRest) {
        calls.push(`${name}:refused`);
        return Promise.reject({ code: "steps_skipped", message: `${left.length} steps no longer apply.`, element_ids: [] });
      }
      return ok(name, { applied: (parts ?? st().parts.map((p) => p.key)).map((part) => ({ part, revision: 1, summary: "" })), skipped: left, state: null, dependents: [], differs: [] });
    },
    discard: () => ok("discard", null),
    discardPart: (part, withDependents) => ok(`discard:${part}${withDependents ? "+dependents" : ""}`, st()),
    setCap: () => ok("cap", st()),
  };
}
