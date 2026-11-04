import { describe, expect, it } from "vitest";
import type { AgentSummary, HistoryStep, LiveStatus, Presence, SnapshotMeta } from "../contract/bindings";
import { historyRows } from "../shell/history";
import {
  EMPTY_PRESENCE,
  OFF_BAR_MS,
  agentCursorIds,
  agentPointerIds,
  agentTip,
  agentTitle,
  agentViews,
  agentsByLevel,
  autoVersion,
  claimLine,
  cleanAgentName,
  controlsFor,
  forLine,
  groupLine,
  inFirstSeenOrder,
  isAgentStep,
  limitChips,
  newAgentSteps,
  onBar,
  ownerLine,
  reviewGroups,
  sameViews,
  statusText,
  stepKey,
  toggleKind,
  undoLineText,
  undoLines,
  withAlpha,
  withPart,
  withoutPart,
  type AgentView,
} from "./agents";
import { LIVE_OFF } from "./liveStore";

const NOW = Date.parse("2026-11-04T08:00:00Z");
const iso = (agoMs: number) => new Date(NOW - agoMs).toISOString();

const pr = (over: Partial<Presence> = {}): Presence => ({ ...EMPTY_PRESENCE, level_id: "L1", ...over });

const summary = (id: string, name: string, over: Partial<AgentSummary> = {}): AgentSummary => ({
  participant: { id, name, color: 3, role: "guest", agent: { owner_id: null, owner_name: "Axl", client: "claude-code", client_version: "2.1" } },
  presence: pr({ cursor: { x: 1000, y: 2000 }, status: "Placing doors" }),
  activity: "working",
  limit: null,
  claim: null,
  steps: 4,
  last_call_at: iso(5_000),
  ...over,
});

const view = (over: Partial<AgentView> = {}): AgentView => ({
  id: "a1",
  name: "Ground floor",
  color: 3,
  client: "claude-code",
  clientVersion: "2.1",
  ownerId: null,
  ownerName: "Axl",
  presence: pr({ cursor: { x: 1, y: 2 } }),
  activity: "working",
  limit: null,
  claim: null,
  steps: 4,
  lastCallAt: iso(5_000),
  local: true,
  ...over,
});

const hosting = (participants: LiveStatus["participants"]): LiveStatus => ({ ...LIVE_OFF, mode: "hosting", self_id: "me", participants, project_id: "p" });

const step = (label: string, over: Partial<HistoryStep> = {}): HistoryStep => ({
  label,
  summary: `${label}: changed 1 wall`,
  origin: "ai",
  author_id: null,
  author_name: null,
  color: null,
  at: iso(60_000),
  ids: [`${label}-id`],
  removed: [],
  on_behalf_of: null,
  on_behalf_name: null,
  ...over,
});

describe("the agents to show", () => {
  it("lists this computer's agents outside a live session", () => {
    const views = agentViews([summary("a1", "Ground floor"), summary("a2", "Services", { activity: "paused" })], LIVE_OFF, {}, NOW);
    expect(views.map((a) => [a.id, a.name, a.activity, a.local, a.client, a.ownerName])).toEqual([
      ["a1", "Ground floor", "working", true, "claude-code", "Axl"],
      ["a2", "Services", "paused", true, "claude-code", "Axl"],
    ]);
  });

  it("adds other computers' agents from the session, once, with their presence", () => {
    const status = hosting([
      { id: "me", name: "Mara", color: 0, role: "host", agent: null },
      { id: "ana", name: "Ana", color: 1, role: "guest", agent: null },
      { id: "a1", name: "Ground floor", color: 3, role: "guest", agent: { owner_id: "me", owner_name: "Mara", client: "claude-code", client_version: "2" } },
      { id: "k", name: "Kitchen", color: 5, role: "guest", agent: { owner_id: "ana", owner_name: "Ana", client: "cursor", client_version: "1.7" } },
    ]);
    const peers = { k: { presence: pr({ status: "Placing the sink", cursor: { x: 5, y: 6 } }), changedAt: NOW - 1000 } };
    const views = agentViews([summary("a1", "Ground floor")], status, peers, NOW);
    expect(views.map((a) => [a.id, a.local, a.activity])).toEqual([
      ["a1", true, "working"],
      ["k", false, "working"],
    ]);
    expect(views[1].presence.status).toBe("Placing the sink");
    expect(views[1].steps).toBeNull();
    // Quiet for a while: idle.
    const later = agentViews([], status, peers, NOW + 120_000);
    expect(later.find((a) => a.id === "k")?.activity).toBe("idle");
  });

  it("controls only this computer's agents: the app lists everyone's in a session", () => {
    const mine = summary("a1", "Ground floor");
    mine.participant.agent = { owner_id: "me", owner_name: "Mara", client: "claude-code", client_version: "2" };
    const anas = summary("k", "Kitchen");
    anas.participant.agent = { owner_id: "ana", owner_name: "Ana", client: "cursor", client_version: "1.7" };
    const status = hosting([{ id: "me", name: "Mara", color: 0, role: "host", agent: null }]);
    expect(agentViews([mine, anas], status, {}, NOW).map((a) => [a.id, a.local, a.steps])).toEqual([
      ["a1", true, 4],
      ["k", false, 4],
    ]);
  });

  it("keeps the order agents were first seen in, whoever called last", () => {
    const seen = new Map<string, number>();
    const first = inFirstSeenOrder(agentViews([summary("a1", "Ground floor"), summary("a2", "Services")], LIVE_OFF, {}, NOW), seen);
    expect(first.map((a) => a.id)).toEqual(["a1", "a2"]);
    // Services called last: the app lists it first, the window keeps its place.
    const later = inFirstSeenOrder(agentViews([summary("a3", "Upper"), summary("a2", "Services"), summary("a1", "Ground floor")], LIVE_OFF, {}, NOW), seen);
    expect(later.map((a) => a.id)).toEqual(["a1", "a2", "a3"]);
  });

  it("tells a redraw from nothing new", () => {
    const local = [summary("a1", "Ground floor")];
    const a = agentViews(local, LIVE_OFF, {}, NOW);
    expect(sameViews(a, agentViews(local, LIVE_OFF, {}, NOW))).toBe(true);
    expect(sameViews(a, agentViews([{ ...local[0], steps: 5 }], LIVE_OFF, {}, NOW))).toBe(false);
  });
});

describe("names and lines", () => {
  it("names the agent with its client, once", () => {
    expect(agentTitle({ name: "Ground floor", client: "claude-code" })).toBe("Ground floor (Claude Code)");
    expect(agentTitle({ name: "Claude Code", client: "claude-code" })).toBe("Claude Code");
    expect(agentTitle({ name: "Helper", client: "" })).toBe("Helper");
  });

  it("says whose agent it is", () => {
    expect(ownerLine({ ownerId: null, ownerName: "Axl" }, null)).toBe("your agent");
    expect(ownerLine({ ownerId: "me", ownerName: "Mara" }, "me")).toBe("your agent");
    expect(ownerLine({ ownerId: "ana", ownerName: "Ana Reyes" }, "me")).toBe("Ana's agent");
  });

  it("says what it is doing", () => {
    expect(statusText({ activity: "working", presence: pr({ status: "Placing doors" }) })).toBe("Placing doors");
    expect(statusText({ activity: "working", presence: pr() })).toBe("Working");
    expect(statusText({ activity: "looking", presence: pr() })).toBe("Looking at the plan");
    expect(statusText({ activity: "paused", presence: pr({ status: "Placing doors" }) })).toBe("Paused");
    expect(statusText({ activity: "stopped", presence: pr() })).toBe("Stopped");
  });

  it("puts it all in the avatar's tooltip", () => {
    const a = view({ presence: pr({ status: "Placing doors" }), steps: 31 });
    expect(agentTip(a, null, false)).toBe("Ground floor (Claude Code), your agent: Placing doors. 31 steps. Click to follow");
    expect(agentTip(a, null, true)).toBe("Following Ground floor. Click to stop");
    expect(agentTip(view({ steps: null, local: false }), null, false)).not.toContain("steps");
  });

  it("cleans a new name like the app does", () => {
    expect(cleanAgentName("  Upper\u0007   floor  ")).toBe("Upper floor");
    expect(cleanAgentName("x".repeat(60))).toHaveLength(40);
    expect(claimLine("Ground floor")).toBe("Claimed by Ground floor");
  });
});

describe("who shows where", () => {
  it("keeps a quiet agent off the bar after ten minutes, but not a paused one", () => {
    expect(onBar(view({ activity: "idle", lastCallAt: iso(OFF_BAR_MS - 1000) }), NOW)).toBe(true);
    expect(onBar(view({ activity: "idle", lastCallAt: iso(OFF_BAR_MS + 1000) }), NOW)).toBe(false);
    expect(onBar(view({ activity: "paused", lastCallAt: iso(OFF_BAR_MS * 3) }), NOW)).toBe(true);
    expect(onBar(view({ activity: "idle", local: false, lastCallAt: "" }), NOW)).toBe(true);
  });

  it("shows a cursor only on the agent's level, and a 3D pointer on every level", () => {
    const views = [
      view({ id: "a", presence: pr({ cursor: { x: 1, y: 1 }, level_id: "L1", pointer_3d: { x: 1, y: 1, z: 1 } }) }),
      view({ id: "b", presence: pr({ cursor: { x: 1, y: 1 }, level_id: "L2", pointer_3d: { x: 1, y: 1, z: 1 } }) }),
      view({ id: "c", presence: pr({ cursor: null, level_id: "L1" }) }),
    ];
    expect(agentCursorIds(views, "L1", NOW)).toEqual(["a"]);
    expect(agentCursorIds(views, null, NOW)).toEqual([]);
    expect(agentPointerIds(views, NOW)).toEqual(["a", "b"]);
  });

  it("groups agents by level for the Levels dots, leaving stopped ones out", () => {
    const by = agentsByLevel([view({ id: "a" }), view({ id: "b", presence: pr({ level_id: "L2" }) }), view({ id: "c", activity: "stopped" })], NOW);
    expect([...by.entries()].map(([l, list]) => [l, list.map((x) => x.id)])).toEqual([
      ["L1", ["a"]],
      ["L2", ["b"]],
    ]);
  });

  it("offers Pause, Stop and Resume as they apply, and nothing for someone else's agent", () => {
    expect(controlsFor({ activity: "working", local: true })).toEqual({ pause: true, stop: true, resume: false });
    expect(controlsFor({ activity: "paused", local: true })).toEqual({ pause: false, stop: true, resume: true });
    expect(controlsFor({ activity: "stopped", local: true })).toEqual({ pause: false, stop: false, resume: true });
    expect(controlsFor({ activity: "working", local: false })).toEqual({ pause: false, stop: false, resume: false });
  });
});

describe("limits", () => {
  const levels = [
    { id: "L1", name: "Ground Floor" },
    { id: "L2", name: "Second Floor" },
  ];

  it("shows one chip per part of the limit", () => {
    expect(limitChips(null, levels)).toEqual([]);
    const chips = limitChips({ level_ids: ["L1"], ids: ["w1", "w2", "w3"], kinds: ["wall", "opening"] }, levels);
    expect(chips.map((c) => [c.key, c.label])).toEqual([
      ["levels", "Ground Floor"],
      ["ids", "Selection: 3"],
      ["kinds", "Walls, Doors and windows"],
    ]);
    expect(limitChips({ level_ids: ["L1", "L2", "L3"], ids: [], kinds: [] }, [...levels, { id: "L3", name: "Roof deck" }])[0].label).toBe("3 levels");
  });

  it("shows nothing stale after a project switch: levels and elements the plan lacks have no chip", () => {
    // The limit still names the last project's level and walls until the app sends the new one.
    const stale = { level_ids: ["old-L1"], ids: ["old-w1", "w2"], kinds: [] };
    expect(limitChips(stale, levels, new Set(["w2"])).map((c) => c.label)).toEqual(["Selection: 1"]);
    expect(limitChips({ level_ids: ["old-L1"], ids: ["old-w1"], kinds: [] }, levels, new Set(["w2"]))).toEqual([]);
    expect(limitChips({ level_ids: ["L1", "old-L1"], ids: [], kinds: [] }, levels)[0].label).toBe("Ground Floor");
  });

  it("adds and removes parts, and clears to null when nothing is left", () => {
    const one = withPart(null, { level_ids: ["L1"] });
    expect(one).toEqual({ level_ids: ["L1"], ids: [], kinds: [] });
    const two = toggleKind(one, "wall");
    expect(two?.kinds).toEqual(["wall"]);
    expect(toggleKind(two, "wall")?.kinds).toEqual([]);
    expect(withoutPart(two, "levels")).toEqual({ level_ids: [], ids: [], kinds: ["wall"] });
    expect(withoutPart(one, "levels")).toBeNull();
    expect(toggleKind(withPart(null, { kinds: ["wall"] }), "wall")).toBeNull();
  });
});

describe("agent steps and the review", () => {
  const agents = new Set(["a1", "a2"]);

  it("tells an agent's step from a person's", () => {
    expect(isAgentStep(step("x", { author_id: "a1" }), agents)).toBe(true);
    expect(isAgentStep(step("x", { author_id: "ana" }), agents)).toBe(false);
    // An agent the window never saw, but the step says whom it worked for.
    expect(isAgentStep(step("x", { author_id: "zz", on_behalf_name: "Axl" }), agents)).toBe(true);
    // A step from before these fields.
    expect(isAgentStep({ author_id: null } as HistoryStep, agents)).toBe(false);
  });

  it("says whom it worked for", () => {
    expect(forLine(step("x", { on_behalf_of: "ana", on_behalf_name: "Ana Reyes" }), "me", "Mara")).toBe("for Ana");
    expect(forLine(step("x", { on_behalf_of: "me", on_behalf_name: "Mara" }), "me", "Mara")).toBe("for you");
    expect(forLine(step("x", { on_behalf_of: null, on_behalf_name: "Mara" }), null, "Mara")).toBe("for you");
    expect(forLine(step("x"), "me", "Mara")).toBeNull();
  });

  it("says for you for this computer's agents outside a session, where the window has no id", () => {
    // The app marks every agent step with its host participant id and the profile name, session or not.
    const mine = step("x", { author_id: "a1", on_behalf_of: "host-1", on_behalf_name: "Mara" });
    expect(isAgentStep(mine, new Set())).toBe(true);
    expect(forLine(mine, null, "Mara")).toBe("for you");
    // No profile name yet: "this computer".
    const unnamed = step("x", { author_id: "a1", on_behalf_of: "host-1", on_behalf_name: "this computer" });
    expect(isAgentStep(unnamed, new Set())).toBe(true);
    expect(forLine(unnamed, null, null)).toBe("for you");
    expect(forLine(unnamed, null, "Mara")).toBe("for you");
    // In a session the id decides; another computer's unnamed owner is not named at all.
    expect(forLine(step("x", { on_behalf_of: "host-1", on_behalf_name: "Mara" }), "host-1", "Mara")).toBe("for you");
    expect(forLine(step("x", { on_behalf_of: "guest-2", on_behalf_name: "this computer" }), "host-1", "Mara")).toBeNull();
    expect(forLine(step("x", { on_behalf_of: "guest-2", on_behalf_name: "Ana Reyes" }), "host-1", "Mara")).toBe("for Ana");
  });

  it("starts the review at the newest Before agents version", () => {
    const snaps: SnapshotMeta[] = [
      { id: "s1", label: "Before agents, 3:42 PM", created_at: iso(3600e3), revision: 1, auto: true },
      { id: "s2", label: "Kitchen moved", created_at: iso(600e3), revision: 5, auto: false },
      { id: "s3", label: "Before agents, 4:30 PM", created_at: iso(1200e3), revision: 3, auto: true },
    ];
    expect(autoVersion(snaps)?.id).toBe("s3");
    expect(autoVersion(snaps.slice(1, 2))).toBeNull();
  });

  it("groups the agents' done steps since a version, newest agent first, with what each touched", () => {
    const list = {
      undo: [
        step("Add door", { author_id: "a1", author_name: "Ground floor", color: 3, at: iso(10_000), ids: ["d1", "w1"] }),
        step("Move wall", { author_id: null, origin: "user", at: iso(20_000) }),
        step("Add outlet", { author_id: "a2", author_name: "Services", color: 4, at: iso(30_000), ids: ["o1"] }),
        step("Add wall", { author_id: "a1", author_name: "Ground floor", color: 3, at: iso(40_000), ids: ["w1"] }),
        step("Old", { author_id: "a1", author_name: "Ground floor", color: 3, at: iso(900_000) }),
      ],
      redo: [step("Undone", { author_id: "a1", author_name: "Ground floor", color: 3, at: iso(5_000) })],
    };
    const rows = historyRows(list, null, agents);
    const groups = reviewGroups(rows, iso(600_000), agents);
    expect(groups.map((g) => [g.key, g.name, g.rows.map((r) => r.step.label), g.touched])).toEqual([
      ["a1", "Ground floor", ["Add door", "Add wall"], ["d1", "w1"]],
      ["a2", "Services", ["Add outlet"], ["o1"]],
    ]);
    // "Undo to here" on the older Ground floor step takes back four steps.
    expect(groups[0].rows[1].steps).toBe(4);
    expect(groupLine(groups[0])).toBe("2 steps, 2 elements");
    // Without a version, the whole kept history.
    expect(reviewGroups(rows, null, agents)[0].rows).toHaveLength(3);
    expect(reviewGroups(rows, iso(1000), agents)).toEqual([]);
  });

  it("names every author whose steps Undo to here takes back, with a count each", () => {
    const list = {
      undo: [
        step("Add outlet", { author_id: "a2", author_name: "Services", at: iso(5_000) }),
        step("Move wall", { author_id: null, origin: "user", at: iso(10_000) }),
        step("Add door", { author_id: "a2", author_name: "Services", at: iso(15_000) }),
        step("Add wall", { author_id: "a1", author_name: "Ground floor", at: iso(20_000) }),
        step("Add room", { author_id: "a1", author_name: "Ground floor", at: iso(30_000) }),
      ],
      redo: [step("Undone", { author_id: "a1", author_name: "Ground floor" })],
    };
    const rows = historyRows(list, null, agents);
    // Undo to here on Ground floor's "Add wall" (4 deep) also takes back Services' two steps and yours.
    const lines = undoLines(rows, 4);
    expect(lines.map(undoLineText)).toEqual(["Services, an AI agent: 2 steps", "You: 1 step", "Ground floor, an AI agent: 1 step"]);
    expect(lines.reduce((n, l) => n + l.count, 0)).toBe(4);
    expect(undoLines(rows, 1).map(undoLineText)).toEqual(["Services, an AI agent: 1 step"]);
  });

  it("finds the agents' new steps for their trails, and stops at the first one seen", () => {
    const old = step("Add wall", { author_id: "a1", at: iso(40_000) });
    const seen = new Set([stepKey(old)]);
    const undo = [
      step("Add door", { author_id: "a1", at: iso(2_000), ids: ["d1"] }),
      step("Move wall", { author_id: null, origin: "user", at: iso(3_000) }),
      step("Add outlet", { author_id: "a2", at: iso(4_000), ids: ["o1"] }),
      old,
      step("Older", { author_id: "a2", at: iso(50_000) }),
    ];
    const fresh = newAgentSteps(undo, seen, agents);
    expect([...fresh.entries()].map(([id, list]) => [id, list.map((s) => s.label)])).toEqual([
      ["a1", ["Add door"]],
      ["a2", ["Add outlet"]],
    ]);
  });
});

describe("colors", () => {
  it("makes a faint color from a token's hex", () => {
    expect(withAlpha("#c2410c", 0.35)).toBe("rgba(194, 65, 12, 0.35)");
    expect(withAlpha(" #FFFFFF ", 2)).toBe("rgba(255, 255, 255, 1)");
    expect(withAlpha("rgb(1, 2, 3)", 0.5)).toBe("rgb(1, 2, 3)");
  });
});
