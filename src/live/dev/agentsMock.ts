// Dev only, installed by liveMock.ts in mock mode with ?agents=demo. Never
// part of a production build.
//
// Answers the agents IPC calls (`agents_list`, `agent_update`,
// `agent_pause`, `agent_stop`, `agent_resume`) the way the engine does
// (docs/CONTRACT.md, "Agents"), and plays three agents on the sample plan
// so the UI can be looked at without a real agent (DECISIONS D47):
//   Ground floor (Claude Code)  works along the walls, claims the level,
//                               makes a step every few seconds
//   Services (Codex)            reads the plan, limited to services and objects
//   Upper floor (Cursor)        idle for a few minutes
// It also keeps a history (`history_list`) with who made each step, an
// automatic "Before agents" version, and `doc_revision` so the window
// notices the agents' edits. `window.__agentsDemo` drives it: pause, resume,
// step, status, hold (stops the script), go.

import type { AgentActivity, AgentLimit, AgentSummary, Command, DocState, Element, HistoryList, HistoryStep, IpcError, Participant, Point, Presence, SnapshotMeta } from "../../contract/bindings";

export interface AgentsMockHost {
  handlers: Record<string, (args: Record<string, unknown>) => unknown>;
  emit: (event: { type: "agents"; agents: AgentSummary[] }) => void;
  /** This window's participant id in a session, null outside one. */
  selfId: () => string | null;
  profile: () => string;
}

function fail(code: string, message: string): never {
  throw { code, message, element_ids: [] } satisfies IpcError;
}

interface MockAgent {
  id: string;
  name: string;
  color: number;
  client: string;
  version: string;
  activity: AgentActivity;
  /** What it was doing before a pause or stop. */
  before: AgentActivity;
  status: string | null;
  cursor: Point | null;
  levelId: string | null;
  selection: string[];
  limit: AgentLimit | null;
  claimLevels: string[];
  steps: number;
  lastCall: number;
}

const GROUND = "agent-ground";
const SERVICES = "agent-services";
const UPPER = "agent-upper";

const GROUND_LINES = ["Placing doors", "Adding a window to the bedroom", "Checking room sizes", "Moving the kitchen wall"];
const GROUND_LABELS = ["Add door", "Add window", "Resize room", "Move wall", "Add wall"];

export interface AgentsMock {
  /** Agents as participants of a session this window hosts. */
  participants: () => Participant[];
}

export function installAgentsMock(host: AgentsMockHost): AgentsMock {
  const now = Date.now();
  let held = false;
  const agents: MockAgent[] = [
    { id: GROUND, name: "Ground floor", color: 3, client: "claude-code", version: "2.1.4", activity: "working", before: "working", status: GROUND_LINES[0], cursor: null, levelId: null, selection: [], limit: null, claimLevels: [], steps: 0, lastCall: now },
    { id: SERVICES, name: "Services", color: 4, client: "codex", version: "0.46", activity: "looking", before: "looking", status: "Reading the plan", cursor: null, levelId: null, selection: [], limit: { level_ids: [], ids: [], kinds: ["pipe", "asset"] }, claimLevels: [], steps: 0, lastCall: now },
    { id: UPPER, name: "Upper floor", color: 6, client: "cursor", version: "1.7", activity: "idle", before: "idle", status: null, cursor: null, levelId: null, selection: [], limit: null, claimLevels: [], steps: 2, lastCall: now - 4 * 60e3 },
  ];

  // ------------------------------------------------ the plan they work on

  let walls: Array<{ id: string; mid: Point; level: string }> = [];
  let rooms: Array<{ id: string; level: string; c: Point }> = [];
  let levelId = "";
  let projectId: string | null = null;
  const readPlan = () => {
    const doc = host.handlers.doc_state({}) as DocState | null;
    if (!doc) return null;
    projectId = doc.project.id;
    levelId = doc.project.levels[0]?.id ?? "";
    walls = doc.project.elements
      .filter((e): e is Extract<Element, { kind: "wall" }> => e.kind === "wall")
      .map((w) => ({ id: w.id, mid: { x: Math.round((w.start.x + w.end.x) / 2), y: Math.round((w.start.y + w.end.y) / 2) }, level: w.level_id }));
    rooms = doc.project.elements
      .filter((e): e is Extract<Element, { kind: "room" }> => e.kind === "room")
      .map((r) => ({ id: r.id, level: r.level_id, c: { x: Math.round(r.seed.x), y: Math.round(r.seed.y) } }));
    return doc;
  };

  // ------------------------------------------------ history

  let undo: HistoryStep[] = [];
  let redo: HistoryStep[] = [];
  let snapshots: SnapshotMeta[] = [];
  /** An agent's step is being written: the `doc_apply` wrapper must not record it as yours. */
  let applying = false;
  const iso = (ago: number) => new Date(Date.now() - ago).toISOString();
  const stepOf = (label: string, ids: string[], by: MockAgent | null, ago = 0): HistoryStep => ({
    label,
    summary: `${label}: changed ${ids.length === 1 ? "1 element" : `${ids.length} elements`}`,
    origin: by ? "ai" : "user",
    author_id: by?.id ?? null,
    author_name: by?.name ?? null,
    color: by?.color ?? null,
    at: iso(ago),
    ids,
    removed: [],
    on_behalf_of: by ? host.selfId() : null,
    on_behalf_name: by ? host.profile() : null,
  });

  /** A real step in the mock engine: an unchanged element written back, so undo and redo line up. */
  const commit = (label: string, id: string, by: MockAgent | null, ago = 0): boolean => {
    const doc = host.handlers.doc_state({}) as DocState | null;
    const el = doc?.project.elements.find((e) => e.id === id);
    if (!el) return false;
    applying = true;
    try {
      host.handlers.doc_apply({ command: { type: "update_element", element: el } satisfies Command });
    } catch {
      return false;
    } finally {
      applying = false;
    }
    undo = [stepOf(label, [id], by, ago), ...undo].slice(0, 200);
    redo = [];
    return true;
  };

  /** Opening the sample: a short past, with the agents' version and their first steps. */
  const seed = () => {
    if (!readPlan()) return;
    undo = [];
    redo = [];
    const ground = agents[0];
    const upper = agents[2];
    commit("Move wall", walls[0]?.id ?? "", null, 22 * 60e3);
    snapshots = [{ id: "s-agents", label: `Before agents, ${new Date(Date.now() - 14 * 60e3).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}`, created_at: iso(14 * 60e3), revision: 1, auto: true }];
    commit("Add wall", walls[1]?.id ?? "", upper, 12 * 60e3);
    commit("Add door", walls[2]?.id ?? "", upper, 11 * 60e3);
    commit("Resize room", rooms[0]?.id ?? walls[3]?.id ?? "", ground, 6 * 60e3);
    commit("Add window", walls[3]?.id ?? "", ground, 4 * 60e3);
    commit("Change material", walls[4]?.id ?? "", null, 2 * 60e3);
    commit("Move wall", walls[4]?.id ?? "", ground, 60e3);
    ground.steps = 3;
    upper.steps = 2;
    for (const a of agents) {
      a.levelId = levelId;
      a.claimLevels = a.id === GROUND ? [levelId] : [];
    }
    agents[0].limit = { level_ids: [levelId], ids: [], kinds: [] };
    place();
  };

  const origApply = host.handlers.doc_apply;
  const origUndo = host.handlers.doc_undo;
  const origRedo = host.handlers.doc_redo;
  const origOpen = host.handlers.hub_open;
  const origSnapshots = host.handlers.snapshot_list;
  Object.assign(host.handlers, {
    hub_open: (a: Record<string, unknown>) => {
      origOpen(a);
      seed();
      return host.handlers.doc_state({});
    },
    doc_apply: (a: Record<string, unknown>) => {
      const res = origApply(a) as { diff: { summary: string } };
      // A step from the window: yours.
      if (!applying) {
        const cmd = a.command as { element?: { id?: string }; ids?: string[] };
        undo = [stepOf(res.diff.summary || "Change", cmd.element?.id ? [cmd.element.id] : (cmd.ids ?? []), null), ...undo].slice(0, 200);
        redo = [];
      }
      return res;
    },
    doc_undo: (a: Record<string, unknown>) => {
      const n = Math.max(1, Number(a.steps ?? 1));
      let out: unknown = null;
      for (let i = 0; i < n; i++) {
        out = origUndo(a);
        const s = undo.shift();
        if (s) redo = [s, ...redo];
      }
      return out;
    },
    doc_redo: (a: Record<string, unknown>) => {
      const n = Math.max(1, Number(a.steps ?? 1));
      let out: unknown = null;
      for (let i = 0; i < n; i++) {
        out = origRedo(a);
        const s = redo.shift();
        if (s) undo = [s, ...undo];
      }
      return out;
    },
    history_list: (): HistoryList => ({ undo, redo }),
    doc_revision: () => {
      const doc = host.handlers.doc_state({}) as DocState | null;
      return { revision: doc?.revision ?? 0, project_id: doc?.project.id ?? null };
    },
    snapshot_list: () => [...snapshots, ...((origSnapshots({}) as SnapshotMeta[]) ?? [])],
    agents_list: () => summaries(),
    agent_update: (a: Record<string, unknown>) => {
      const agent = agents.find((x) => x.id === a.id) ?? fail("not_found", "No agent with that id.");
      if (typeof a.name === "string") {
        const name = a.name.replace(/\s+/g, " ").trim().slice(0, 40);
        if (!name) fail("bad_args", "A name needs at least one letter or number.");
        agent.name = name;
      }
      if ("limit" in a) agent.limit = (a.limit as AgentLimit | null) ?? null;
      emit();
      return summaries().find((x) => x.participant.id === agent.id);
    },
    agent_pause: (a: Record<string, unknown>) => hold(a.id as string | null, "paused"),
    agent_stop: (a: Record<string, unknown>) => hold(a.id as string | null, "stopped"),
    agent_resume: (a: Record<string, unknown>) => {
      for (const x of pick(a.id as string | null)) {
        if (x.activity !== "paused" && x.activity !== "stopped") continue;
        x.activity = x.before === "paused" || x.before === "stopped" ? "idle" : x.before;
      }
      emit();
      return summaries();
    },
  });

  const pick = (id: string | null) => (id === null ? agents : agents.filter((x) => x.id === id));
  const hold = (id: string | null, to: "paused" | "stopped") => {
    if (id !== null && !agents.some((x) => x.id === id)) fail("not_found", "No agent with that id.");
    for (const x of pick(id)) {
      if (x.activity !== "paused" && x.activity !== "stopped") x.before = x.activity;
      x.activity = to;
    }
    emit();
    return summaries();
  };

  // ------------------------------------------------ what the window sees

  const presenceOf = (a: MockAgent): Presence => ({
    cursor: a.cursor,
    level_id: a.levelId,
    selection: a.activity === "stopped" ? [] : a.selection,
    typing: null,
    ai_scope: false,
    following: null,
    view_mode: null,
    plan_view: a.cursor ? { center: a.cursor, width_mm: 9000, height_mm: 6000 } : null,
    camera: null,
    pointer_3d: a.cursor ? { x: a.cursor.x, y: a.cursor.y, z: 1400 } : null,
    status: a.activity === "working" || a.activity === "looking" ? a.status : null,
  });
  const participantOf = (a: MockAgent): Participant => ({
    id: a.id,
    name: a.name,
    color: a.color,
    role: "guest",
    agent: { owner_id: host.selfId(), owner_name: host.profile(), client: a.client, client_version: a.version },
  });
  const summaries = (): AgentSummary[] =>
    projectId === null
      ? []
      : [...agents]
          .sort((x, y) => y.lastCall - x.lastCall)
          .map((a) => ({
            participant: participantOf(a),
            presence: presenceOf(a),
            activity: a.activity,
            limit: a.limit,
            claim: a.claimLevels.length > 0 ? { level_ids: a.claimLevels, ids: [], expires_at: new Date(a.lastCall + 10 * 60e3).toISOString() } : null,
            steps: a.steps,
            last_call_at: new Date(a.lastCall).toISOString(),
          }));
  const emit = () => host.emit({ type: "agents", agents: summaries() });

  // ------------------------------------------------ the script

  let beat = 0;
  /** Each agent at its next spot. */
  const place = () => {
    if (walls.length === 0) return;
    const g = agents[0];
    const w = walls[beat % walls.length];
    if (g.activity === "working" || g.activity === "looking") {
      g.cursor = w.mid;
      g.selection = [w.id];
      g.status = GROUND_LINES[beat % GROUND_LINES.length];
      g.lastCall = Date.now();
    } else if (!g.cursor) g.cursor = w.mid;
    const sv = agents[1];
    const r = rooms[(beat + 1) % Math.max(1, rooms.length)];
    if (sv.activity === "looking" || sv.activity === "working") {
      sv.cursor = r ? { x: r.c.x + ((beat * 700) % 1800) - 900, y: r.c.y + 400 } : walls[(beat + 3) % walls.length].mid;
      sv.status = beat % 3 === 0 ? "Reading the plan" : beat % 3 === 1 ? "Looking for outlets in the kitchen" : "Taking a picture of the plan";
      sv.lastCall = Date.now();
    } else if (!sv.cursor) sv.cursor = r?.c ?? null;
  };

  const step = () => {
    const g = agents[0];
    if (g.activity !== "working") return;
    const w = walls[beat % walls.length];
    if (w && commit(GROUND_LABELS[beat % GROUND_LABELS.length], w.id, g)) g.steps++;
  };

  window.setInterval(() => {
    if (held || projectId === null) return;
    const doc = host.handlers.doc_state({}) as DocState | null;
    // The project closed: the agents wait for the next one.
    if (!doc) {
      projectId = null;
      return;
    }
    beat++;
    place();
    if (beat % 2 === 0) step();
    emit();
  }, 2600);

  (window as unknown as { __agentsDemo: unknown }).__agentsDemo = {
    pause: (id: string | null = GROUND) => hold(id, "paused"),
    resume: (id: string | null = null) => host.handlers.agent_resume({ id }),
    stop: (id: string | null = GROUND) => hold(id, "stopped"),
    /** One step by Ground floor now. */
    step: () => {
      beat++;
      place();
      step();
      emit();
    },
    status: (text: string, id = GROUND) => {
      const a = agents.find((x) => x.id === id);
      if (a) a.status = text;
      emit();
    },
    /** Stops the script where it is, for still screenshots; `go` starts it again. */
    hold: () => {
      held = true;
    },
    go: () => {
      held = false;
    },
    ids: { ground: GROUND, services: SERVICES, upper: UPPER },
  };

  return {
    participants: () => (projectId === null ? [] : agents.map(participantOf)),
  };
}
