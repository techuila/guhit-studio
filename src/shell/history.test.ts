import { describe, expect, it } from "vitest";
import type { HistoryList, HistoryStep, LiveStatus } from "../contract/bindings";
import { EVERYONE, filterRows, historyPeople, historyRows, liveIds, relativeTime, stepDetail } from "./history";

const step = (label: string, author: [string, string, number] | null, extra: Partial<HistoryStep> = {}): HistoryStep => ({
  label,
  summary: `${label}: changed 1 wall`,
  origin: "user",
  author_id: author?.[0] ?? null,
  author_name: author?.[1] ?? null,
  color: author?.[2] ?? null,
  at: "2026-10-07T01:00:00Z",
  ids: [`${label}-id`],
  removed: [],
  ...extra,
});

const ANA: [string, string, number] = ["p-ana", "Ana", 0];
const BEN: [string, string, number] = ["p-ben", "Ben", 1];

// Newest first: yours, Ana's, Ben's, Ben's, yours. Undone: Ana's, then yours.
const LIST: HistoryList = {
  undo: [step("Move wall", null), step("Add door", ANA), step("Add wall", BEN), step("Add room", BEN), step("Add stair", null)],
  redo: [step("Delete", ANA), step("Rotate", null)],
};

describe("history rows", () => {
  it("lists the undo stack newest first, then the undone steps, with who and how far", () => {
    const rows = historyRows(LIST);
    expect(rows.map((r) => [r.stack, r.steps, r.name, r.color, r.mine])).toEqual([
      ["undo", 1, "You", null, true],
      ["undo", 2, "Ana", 0, false],
      ["undo", 3, "Ben", 1, false],
      ["undo", 4, "Ben", 1, false],
      ["undo", 5, "You", null, true],
      ["redo", 1, "Ana", 0, false],
      ["redo", 2, "You", null, true],
    ]);
    expect(new Set(rows.map((r) => r.key)).size, "keys are unique").toBe(rows.length);
  });

  it("keeps a step's key as it moves from done to undone", () => {
    const before = historyRows(LIST);
    const after = historyRows({ undo: LIST.undo.slice(1), redo: [LIST.undo[0], ...LIST.redo] });
    expect(after.find((r) => r.stack === "redo" && r.steps === 1)?.key).toBe(before[0].key);
  });

  it("tells apart steps that look the same, and keeps them apart through undo", () => {
    // One wall moved three times in the same second.
    const moves = [step("Move", null), step("Move", null), step("Move", null)];
    const before = historyRows({ undo: moves, redo: [] });
    expect(new Set(before.map((r) => r.key)).size).toBe(3);
    // Two undone: the newest two go to the redo stack, the next redo first.
    const after = historyRows({ undo: [moves[2]], redo: [moves[1], moves[0]] });
    expect(after.map((r) => r.key)).toEqual([before[2].key, before[1].key, before[0].key]);
  });

  it("names a step from the session when the host sent no name, else Someone", () => {
    const status = { participants: [{ id: "p-cy", name: "Cy", color: 3, role: "guest" }] } as unknown as LiveStatus;
    const rows = historyRows({ undo: [step("A", ["p-cy", "", 0], { author_name: null, color: null }), step("B", ["p-x", "", 0], { author_name: null, color: null })], redo: [] }, status);
    expect(rows.map((r) => [r.name, r.color])).toEqual([
      ["Cy", 3],
      ["Someone", null],
    ]);
  });

  it("offers a chip per person, you first, and filters by it", () => {
    const rows = historyRows(LIST);
    const people = historyPeople(rows);
    expect(people.map((p) => [p.key, p.name, p.count])).toEqual([
      ["you", "You", 3],
      ["p-ana", "Ana", 2],
      ["p-ben", "Ben", 2],
    ]);
    expect(filterRows(rows, "p-ben").map((r) => r.step.label)).toEqual(["Add wall", "Add room"]);
    expect(filterRows(rows, EVERYONE)).toHaveLength(rows.length);
    // The steps count from the whole stack, not the filtered list.
    expect(filterRows(rows, "p-ben")[1].steps).toBe(4);
  });

  it("says what a step changed without repeating its label", () => {
    expect(stepDetail({ label: "Delete", summary: "Delete: changed 1 switch; removed 1 light" })).toBe("Changed 1 switch; removed 1 light");
    expect(stepDetail({ label: "MCP: add wall", summary: "MCP: add wall: added 1 wall" })).toBe("Added 1 wall");
    expect(stepDetail({ label: "Move", summary: "Moved 1 other element" })).toBe("Moved 1 other element");
    expect(stepDetail({ label: "Move wall", summary: "Move wall" })).toBe("");
    expect(stepDetail({ label: "Move wall", summary: "" })).toBe("");
  });

  it("shows only the elements that still exist", () => {
    const s = step("Delete", null, { ids: ["a", "b"], removed: ["c", "a"] });
    expect(liveIds(s, [{ id: "b" }, { id: "c" }, { id: "a" }])).toEqual(["a", "b", "c"]);
    expect(liveIds(s, [{ id: "z" }])).toEqual([]);
  });
});

describe("relative time", () => {
  const now = new Date(2026, 9, 7, 15, 30, 0);
  const ago = (ms: number) => new Date(now.getTime() - ms).toISOString();

  it("counts minutes, then hours today", () => {
    expect(relativeTime(ago(10_000), now)).toBe("just now");
    expect(relativeTime(ago(-5_000), now), "a clock a little ahead").toBe("just now");
    expect(relativeTime(ago(60_000), now)).toBe("1 min ago");
    expect(relativeTime(ago(14 * 60_000), now)).toBe("14 min ago");
    expect(relativeTime(ago(2 * 3_600_000), now)).toBe("2 hr ago");
  });

  it("then days and dates", () => {
    expect(relativeTime(new Date(2026, 9, 6, 22, 0).toISOString(), now)).toBe("Yesterday");
    expect(relativeTime(new Date(2026, 9, 2, 9, 0).toISOString(), now)).toBe("Oct 2");
    expect(relativeTime(new Date(2025, 11, 30, 9, 0).toISOString(), now)).toBe("Dec 30, 2025");
  });

  it("is empty without a time", () => {
    expect(relativeTime("", now)).toBe("");
    expect(relativeTime("not a time", now)).toBe("");
  });
});
