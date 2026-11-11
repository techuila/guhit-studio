// The swarm panel's words and rules (view.ts).
import { describe, expect, it } from "vitest";
import { mockPart, mockStatus } from "./mock";
import { LOCK_HINT, acceptAllAsk, acceptAsk, aiWaitLine, conflictLine, feedRows, dependents, dependsOn, discardAllAsk, discardAsk, draftLine, drivingLine, estimateParts, hasDraftWork, leadOf, nameList, pillText, pinsOf, priceNote, startTip, workersOf } from "./view";

describe("swarm panel words", () => {
  it("keeps the lead apart from the parts that draw", () => {
    const st = mockStatus();
    expect(leadOf(st)?.key).toBe("lead");
    expect(workersOf(st).map((p) => p.key)).toEqual(["shell", "rooms", "roof"]);
  });

  it("names what a waiting part waits for", () => {
    const st = mockStatus();
    expect(pillText(st.parts[3], st)).toBe("Waiting for Shell");
    expect(pillText(st.parts[1], st)).toBe("Working");
    expect(pillText({ ...st.parts[1], state: "accepted" }, st)).toBe("Accepted");
  });

  it("says what the draft holds, counting changes and removals", () => {
    expect(draftLine(mockStatus({ added: 46, modified: 2, removed: 0 }))).toBe("The swarm drew 48 elements in a draft");
    expect(draftLine(mockStatus({ added: 1, modified: 0, removed: 0 }))).toBe("The swarm drew 1 element in a draft");
    expect(draftLine(mockStatus({ added: 10, modified: 0, removed: 2 }))).toBe("The swarm drew 10 elements in a draft, and removes 2");
  });

  it("shows who drives an MCP swarm in place of the prompt, by the client's own name", () => {
    expect(drivingLine(mockStatus({ source: { type: "mcp", client: "Claude Code" } }))).toBe("Claude Code is driving 3 sub-agents");
    // The raw client string reads the way the rest of the app names it.
    expect(drivingLine(mockStatus({ source: { type: "mcp", client: "claude-code" } }))).toBe("Claude Code is driving 3 sub-agents");
    expect(drivingLine(mockStatus({ source: { type: "mcp", client: "  " } }))).toBe("Your AI is driving 3 sub-agents");
    expect(drivingLine(mockStatus())).toBeNull();
  });

  it("gives the estimate as a range on the AI model it runs on, and says nothing goes into the project", () => {
    const cost = { model: "claude-opus-5-5", model_label: "Claude Opus 5.5", low_usd: 0.76, high_usd: 2.42, spent_usd: 0, cap_usd: 3, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0 };
    const tail = "Nothing goes into the project until you accept it";
    // One phrase per span, never joined by middle dots.
    expect(estimateParts(cost, 4)).toEqual(["4 agents on Claude Opus 5.5", "About US$0.76 to US$2.42 on your Claude key (estimate)", tail]);
    // Before the lead plans, the count is not known.
    expect(estimateParts(cost, null)).toEqual(["Agents on Claude Opus 5.5", "About US$0.76 to US$2.42 on your Claude key (estimate)", tail]);
    // An engine without the label: the id.
    expect(estimateParts({ ...cost, model_label: undefined }, null)[0]).toBe("Agents on claude-opus-5-5");
    expect(estimateParts(null, null)).toEqual(["Runs on your Claude key", tail]);
    expect(estimateParts(cost, 4).join(" ")).not.toContain("·");
  });

  it("says why Start swarm is off, else its shortcut", () => {
    const on = { hasDoc: true, hasKey: true, brief: "A bungalow" };
    expect(startTip({ ...on, hasDoc: false }, "⌘")).toBe("Open a project first");
    expect(startTip({ ...on, hasKey: false }, "⌘")).toBe("Add a Claude API key in the copilot settings");
    expect(startTip({ ...on, brief: "  " }, "⌘")).toBe("Write a brief first");
    expect(startTip(on, "⌘")).toBe("⌘Enter starts the swarm");
    expect(startTip(on, "Ctrl+")).toBe("Ctrl+Enter starts the swarm");
  });

  it("knows when the draft has anything for Accept all", () => {
    expect(hasDraftWork(mockStatus())).toBe(true);
    const empty = mockStatus({ added: 0, modified: 0, removed: 0 });
    expect(hasDraftWork({ ...empty, parts: empty.parts.map((p) => ({ ...p, steps: 0 })) })).toBe(false);
  });

  it("names the part on each conflict under the draft bar", () => {
    const st = mockStatus();
    const c = { part: "rooms", step: 9, label: "Add door", code: "changed_on_main", message: "Wall 4000 mm changed in the plan since the draft.", element_ids: [] };
    expect(conflictLine(st, c)).toBe("Rooms: Wall 4000 mm changed in the plan since the draft.");
    expect(conflictLine(st, { ...c, part: "" })).toBe("Wall 4000 mm changed in the plan since the draft.");
  });

  it("calls the building the plan or the project, never the model", () => {
    expect(LOCK_HINT).toBe("A swarm draft is open. Accept or discard it to edit the plan");
    expect(discardAllAsk(mockStatus()).text).toBe("Discard the whole draft? Nothing was added to the project, and the draft cannot be brought back.");
  });

  it("says when the model's price is unknown and the highest rate is billed", () => {
    const cost = { model: "claude-opus-5-5-20260401", model_label: "claude-opus-5-5-20260401", price_note: "Billed at the highest rate: price unknown", low_usd: 1.9, high_usd: 6.05, spent_usd: 0, cap_usd: 3, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0 };
    expect(priceNote(cost)).toBe("Billed at the highest rate: price unknown");
    expect(priceNote({ ...cost, price_note: undefined })).toBeNull();
    expect(priceNote(null)).toBeNull();
  });

  it("counts down a rate limit wait, and drops it once over", () => {
    const now = Date.parse("2026-11-10T03:00:00Z");
    expect(aiWaitLine("2026-11-10T03:00:12Z", now)).toBe("Waiting for the AI service, 12 s");
    expect(aiWaitLine("2026-11-10T02:59:59Z", now)).toBeNull();
    expect(aiWaitLine(null, now)).toBeNull();
  });

  it("lists the feed newest first, 14 rows, with keys unique for lines alike and no doubled names", () => {
    const at = "2026-11-10T03:42:05Z";
    const feed = [
      ...Array.from({ length: 16 }, (_, i) => ({ at: `2026-11-10T03:41:${String(10 + i).padStart(2, "0")}Z`, part: "rooms", text: `step ${i}` })),
      { at, part: "shell", text: "Shell: added 1 wall" },
      { at, part: "shell", text: "Shell: added 1 wall" },
    ];
    const rows = feedRows(mockStatus({ feed }));
    expect(rows).toHaveLength(14);
    expect(new Set(rows.map((r) => r.key)).size).toBe(14);
    expect(rows[0]).toMatchObject({ name: "Shell", text: "added 1 wall" });
    expect(rows[2].text).toBe("step 15");
  });

  it("lists names the way people say them", () => {
    const st = mockStatus();
    expect(nameList(st, ["shell"])).toBe("Shell");
    expect(nameList(st, ["shell", "rooms"])).toBe("Shell and Rooms");
    expect(nameList(st, ["lead", "shell", "rooms"])).toBe("Lead, Shell and Rooms");
  });
});

describe("who depends on whom", () => {
  const parts = [
    mockPart("shell", "Shell", 0, "done", 5),
    mockPart("rooms", "Rooms", 2, "done", 4, { waiting_for: [{ part: "shell", milestone: "walls" }] }),
    mockPart("furnish", "Furnish", 3, "done", 2, { waiting_for: [{ part: "rooms", milestone: "done" }] }),
    mockPart("roof", "Roof", 1, "done", 3, { waiting_for: [{ part: "shell", milestone: "done" }] }),
  ];

  it("brings in what a part builds on, deepest first", () => {
    expect(dependsOn(parts, "furnish")).toEqual(["shell", "rooms"]);
    expect(dependsOn(parts, "shell")).toEqual([]);
  });

  it("skips parts already accepted", () => {
    const accepted = parts.map((p) => (p.key === "shell" ? { ...p, state: "accepted" as const } : p));
    expect(dependsOn(accepted, "furnish")).toEqual(["rooms"]);
  });

  it("finds every part that builds on a part, directly or not", () => {
    expect(dependents(parts, "shell").sort()).toEqual(["furnish", "roof", "rooms"]);
    expect(dependents(parts, "furnish")).toEqual([]);
  });
});

describe("asking first", () => {
  const st = mockStatus();

  it("accepting a part that builds on others asks, naming them", () => {
    const ask = acceptAsk(st, "rooms", ["shell", "rooms"]);
    expect(ask?.parts).toEqual(["shell", "rooms"]);
    expect(ask?.text).toContain("Rooms builds on Shell");
    expect(ask?.confirm).toBe("Accept 2 parts");
    // A finished part that builds on nothing goes straight in.
    const done = mockStatus({ parts: st.parts.map((p) => ({ ...p, state: "done" as const })) });
    expect(acceptAsk(done, "shell", ["shell"])).toBeNull();
  });

  it("accepting asks when elements would come out differently, even with nothing pulled in", () => {
    const differ = { part: "rooms", step: 4, label: "Add room", code: "differs", message: "A wall joins differently.", element_ids: [] };
    const done = mockStatus({ parts: st.parts.map((p) => ({ ...p, state: "done" as const })) });
    const ask = acceptAsk(done, "rooms", ["shell", "rooms"], [differ, differ]);
    expect(ask?.text).toBe("Rooms builds on Shell. Accepting it accepts that part first, each as its own undo step. 2 elements will come out differently from the draft.");
    // Still drawing too: all three things are said, in that order.
    expect(acceptAsk(st, "rooms", ["shell", "rooms"], [differ])?.text).toBe(
      "Rooms builds on Shell. Accepting it accepts that part first, each as its own undo step. Shell and Rooms are still drawing. Accepting now takes their steps so far and stops them. 1 element will come out differently from the draft.",
    );
    expect(ask?.differs).toEqual(["A wall joins differently.", "A wall joins differently."]);
    expect(acceptAsk(st, "shell", ["shell"], [differ])?.confirm).toBe("Accept Shell");
    expect(acceptAllAsk(done, [])).toBeNull();
    expect(acceptAllAsk(done, [differ])?.part).toBeNull();
    expect(acceptAllAsk(done, [differ])?.confirm).toBe("Accept all anyway");
  });

  it("accepting asks when steps no longer apply, listing them, with Accept the rest", () => {
    const done = mockStatus({ parts: st.parts.map((p) => ({ ...p, state: "done" as const })) });
    const skip = (part: string, label: string, message: string) => ({ part, step: 4, label, code: "nothing_to_dimension", message, element_ids: [] });
    const one = acceptAsk(done, "shell", ["shell"], [], [skip("shell", "Auto dimension", "Nothing to dimension.")]);
    expect(one?.text).toBe("1 step no longer applies. Accepting the rest leaves it in the draft as conflicts.");
    expect(one?.skipped).toEqual(["Shell, Auto dimension: Nothing to dimension."]);
    expect(one?.confirm).toBe("Accept the rest");
    expect(one?.rest).toBe(true);
    const two = acceptAllAsk(done, [], [skip("shell", "", "Gone."), skip("", "", "A step stopped applying.")]);
    expect(two?.text).toBe("2 steps no longer apply. Accepting the rest leaves them in the draft as conflicts.");
    expect(two?.skipped).toEqual(["Shell: Gone.", "A step stopped applying."]);
    expect(two?.confirm).toBe("Accept the rest");
    // Nothing stays out: no "rest".
    expect(acceptAsk(st, "shell", ["shell"])?.rest).toBe(false);
  });

  it("discarding a part others build on asks, naming them", () => {
    const ask = discardAsk(st, "shell", ["rooms", "roof"]);
    expect(ask?.parts).toEqual(["shell", "rooms", "roof"]);
    expect(ask?.text).toBe("Rooms and Roof build on Shell. Discarding it discards those parts too.");
    expect(discardAsk(st, "roof", [])).toBeNull();
  });

  it("discarding the whole draft always asks", () => {
    expect(discardAllAsk(st).confirm).toBe("Discard the draft");
  });

  it("pins Accept to each part's newest step as the window showed it, even past the part's own after an undo", () => {
    expect(pinsOf(st)).toEqual({ lead: 1, shell: 7, rooms: 4, roof: 0 });
    // The window showed Shell up to 5 and Rooms up to 9 (its agent undid since): the pins say so, and the engine answers stale.
    expect(pinsOf(st, { shell: 5, rooms: 9 })).toEqual({ lead: 0, shell: 5, rooms: 9, roof: 0 });
  });

  it("says when an accept takes a part that is still drawing", () => {
    const one = acceptAsk(st, "shell", ["shell"]);
    expect(one?.text).toBe("Shell is still drawing. Accepting now takes its steps so far and stops it.");
    expect(one?.confirm).toBe("Accept Shell");
    const all = acceptAllAsk(st, []);
    // Roof has drawn nothing yet: there is nothing of it to accept.
    expect(all?.text).toBe("Shell and Rooms are still drawing. Accepting all now takes the steps so far and ends the swarm.");
    expect(all?.confirm).toBe("Accept all now");
    const done = mockStatus({ parts: st.parts.map((p) => ({ ...p, state: "done" as const })) });
    expect(acceptAsk(done, "shell", ["shell"])).toBeNull();
    expect(acceptAllAsk(done, [])).toBeNull();
  });
});
