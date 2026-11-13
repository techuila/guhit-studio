// The swarm panel, the draft bar and the lock banner in each state, and
// what their buttons do, against a backend in memory.
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useApp } from "../state/store";
import { acceptAll, acceptPart, confirmAsk, discardAll, discardPart } from "./actions";
import { setSwarmApi } from "./api";
import { DraftBar, SwarmOverlayView, pickBar } from "./DraftBar";
import { mockApi, mockStatus, panelStates } from "./mock";
import { SwarmPanelView, pickPanel } from "./SwarmPanel";
import { reveal, useSwarm } from "./swarmStore";
import type { SwarmStatus } from "./types";

function show(status: SwarmStatus | null, more: Partial<ReturnType<typeof useSwarm.getState>> = {}): void {
  reveal.reset();
  useSwarm.setState({ status, draft: null, lastSeq: 0, seen: {}, ask: null, note: null, acting: null, brief: "", estimate: null, ...more });
}

// Server rendering reads a store's first state, so the views take the current one as props.
const panel = (hasKey = true) => renderToStaticMarkup(<SwarmPanelView {...pickPanel(useSwarm.getState())} hasKey={hasKey} hasDoc />);
const bar = (status: SwarmStatus) => renderToStaticMarkup(<DraftBar {...pickBar(useSwarm.getState())} status={status} />);
const overlay = () => renderToStaticMarkup(<SwarmOverlayView {...pickBar(useSwarm.getState())} />);

afterEach(() => {
  setSwarmApi(null);
  show(null);
});

describe("swarm panel states", () => {
  const states = panelStates(Date.parse("2026-11-10T03:00:00Z"));

  it("idle: the prompt, Start, the estimate line and the cap", () => {
    show(null, { brief: "A bungalow" });
    const html = panel();
    expect(html).toContain('aria-label="Brief for the swarm"');
    expect(html).toContain("Start");
    expect(html).toContain("nothing enters the model until you accept");
    expect(html).toContain('aria-label="Spending cap in US dollars"');
    expect(html).not.toContain("Pause all");
  });

  it("idle: the estimate names the model, and an unpriced model says it is billed at the highest rate", () => {
    const estimate = { model: "claude-opus-5-5", model_label: "Claude Opus 5.5", low_usd: 0.76, high_usd: 2.42, spent_usd: 0, cap_usd: 3, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0 };
    show(null, { brief: "A bungalow", estimate });
    expect(panel()).toContain("Agents on Claude Opus 5.5 · about US$0.76 to $2.42 on your Claude key (estimate)");
    expect(panel()).not.toContain('data-testid="swarm-price-note"');
    show(null, { brief: "A bungalow", estimate: { ...estimate, model: "claude-opus-5-5-20260401", model_label: "claude-opus-5-5-20260401", price_note: "Billed at the highest rate: price unknown" } });
    const html = panel();
    expect(html).toContain("Agents on claude-opus-5-5-20260401");
    expect(html).toContain('data-testid="swarm-price-note"');
    expect(html).toContain("Billed at the highest rate: price unknown");
  });

  it("running on an unpriced model: the spend line says so", () => {
    const st = states.running as SwarmStatus;
    show({ ...st, cost: { ...st.cost!, price_note: "Billed at the highest rate: price unknown" } });
    const html = panel();
    expect(html).toContain("Spent US$0.42 of the US$3.00 cap");
    expect(html).toContain("Billed at the highest rate: price unknown");
  });

  it("idle without a key: Start is off and MCP is offered", () => {
    show(null, { brief: "A bungalow" });
    const html = panel(false);
    expect(html).toMatch(/data-testid="swarm-start"[^>]*disabled|disabled[^>]*data-testid="swarm-start"/);
    expect(html).toContain("Claude Code or Codex");
  });

  it("planning: the lead reads the brief", () => {
    show(states.planning);
    const html = panel();
    expect(html).toContain("Reading the brief");
    expect(html).toContain("Pause all");
    expect(html).not.toContain('data-testid="swarm-start"');
  });

  it("running: the lead row, a row per part with its pill and Stop, the feed, spend", () => {
    show(states.running);
    const html = panel();
    for (const key of ["lead", "shell", "rooms", "roof"]) expect(html).toContain(`data-testid="swarm-part-${key}"`);
    expect(html).toContain("Waiting for Shell");
    expect(html).toContain('data-testid="swarm-stop-shell"');
    expect(html).not.toContain('data-testid="swarm-stop-lead"');
    expect(html).toContain("What they did");
    expect(html).toContain("split the work: Shell, Rooms, Roof");
    expect(html).toContain("Spent US$0.42 of the US$3.00 cap");
    expect(html).toContain('aria-label="Reveal speed"');
    expect(html).toContain("Pause all");
  });

  it("paused: Resume all", () => {
    show(states.paused);
    const html = panel();
    expect(html).toContain("Resume all");
    expect(html).toContain(">Paused<");
  });

  it("waiting for the AI service: the countdown", () => {
    // The panel counts down from the real clock, so the wait starts now.
    show(panelStates().waiting_ai);
    expect(panel()).toContain("Waiting for the AI service");
  });

  it("ready: no Pause, per-part Accept and Discard", () => {
    show(states.ready);
    const html = panel();
    expect(html).not.toContain("Pause all");
    expect(html).toContain('data-testid="swarm-accept-shell"');
    expect(html).toContain('data-testid="swarm-discard-rooms"');
    expect(html).toContain("one undo step, with its agent as the author");
  });

  it("an MCP swarm shows who drives it in place of the prompt", () => {
    show(states.mcp);
    const html = panel();
    expect(html).toContain("Claude Code is driving 3 sub-agents");
    expect(html).not.toContain('aria-label="Brief for the swarm"');
    expect(html).toContain("Runs on your own AI plan");
  });

  it("failed: the part's pill says so", () => {
    show(states.failed);
    expect(panel()).toContain(">Failed<");
  });

  it("an accepted part leaves the review list", () => {
    show(states.accepted_one);
    const html = panel();
    expect(html).toContain(">Accepted<");
    expect(html).not.toContain('data-testid="swarm-accept-shell"');
    expect(html).toContain('data-testid="swarm-accept-rooms"');
  });

  it("Skip to the end shows only while the reveal is behind", () => {
    show(states.running);
    expect(panel()).not.toContain("Skip to the end");
    reveal.arrive("shell", [{ id: "w", part: "shell", kind: "line", from: { x: 0, y: 0 }, to: { x: 9000, y: 0 }, lengthMm: 9000, hosts: [], label: "Wall", levelId: null, removed: false }], performance.now());
    expect(panel()).toContain("Skip to the end");
  });
});

describe("draft bar and lock banner", () => {
  it("says what the draft holds, with Accept all, Discard and Show draft", () => {
    show(mockStatus());
    const html = bar(mockStatus());
    expect(html).toContain("The swarm drew 23 elements in a draft");
    expect(html).toContain("Accept all");
    expect(html).toContain('data-testid="swarm-bar-discard"');
    expect(html).toContain('aria-pressed="true"');
    expect(html).not.toContain("Ready for review");
    expect(html).not.toContain('data-testid="swarm-conflicts"');
  });

  it("ready: the Ready for review pill", () => {
    const st = panelStates().ready as SwarmStatus;
    show(st);
    expect(bar(st)).toContain("Ready for review");
  });

  it("conflicts: the badge with the count", () => {
    const st = panelStates().conflicts as SwarmStatus;
    show(st);
    const html = bar(st);
    expect(html).toContain("1 conflict");
    expect(html).toContain("changed in the model since the draft");
  });

  it("the draft hidden: the eye says so", () => {
    show(mockStatus(), { showDraft: false });
    expect(bar(mockStatus())).toContain('aria-label="Show the draft"');
    useSwarm.setState({ showDraft: true });
  });

  it("the overlay holds the bar and the lock banner while a draft is open, and nothing otherwise", () => {
    show(mockStatus());
    const html = overlay();
    expect(html).toContain('data-testid="swarm-draft-bar"');
    expect(html).toContain("A swarm is drafting. Accept or discard to edit");
    show(null);
    expect(overlay()).toBe("");
  });
});

describe("what the buttons do", () => {
  let calls: string[];
  let current: SwarmStatus;

  beforeEach(() => {
    calls = [];
    current = mockStatus({ state: "ready", parts: mockStatus().parts.map((p) => ({ ...p, state: "done", steps: Math.max(1, p.steps) })) });
    setSwarmApi(mockApi(() => current, calls));
    show(current);
    useApp.setState({ toasts: [] });
  });

  it("Accept all skips the reveal to the end first, then accepts every part pinned to its last step", async () => {
    reveal.arrive("shell", [{ id: "w", part: "shell", kind: "line", from: { x: 0, y: 0 }, to: { x: 9000, y: 0 }, lengthMm: 9000, hosts: [], label: "Wall", levelId: null, removed: false }], performance.now());
    expect(reveal.progress("w", performance.now())).toBeLessThan(1);
    await acceptAll();
    expect(reveal.progress("w", performance.now())).toBe(1);
    expect(calls).toEqual(["check:lead,shell,rooms,roof", "accept:all"]);
    expect(useApp.getState().toasts.slice(-1)[0]?.message).toContain("Each part is one undo step");
  });

  it("Accept on a part that builds on another asks first, then accepts both", async () => {
    await acceptPart("rooms", "bar");
    const ask = useSwarm.getState().ask;
    expect(ask?.parts).toEqual(["shell", "rooms"]);
    expect(ask?.origin).toBe("bar");
    expect(calls).toEqual(["check:rooms"]);
    expect(bar(current)).toContain("Rooms builds on Shell");
    await confirmAsk(ask!);
    expect(calls).toEqual(["check:rooms", "accept:shell,rooms"]);
  });

  it("Accept on a part that builds on nothing goes straight in", async () => {
    await acceptPart("shell");
    expect(useSwarm.getState().ask).toBeNull();
    expect(calls).toEqual(["check:shell", "accept:shell"]);
  });

  it("Discard on a part others build on asks first, naming them", async () => {
    await discardPart("shell", "panel");
    const ask = useSwarm.getState().ask;
    expect(ask?.kind).toBe("discard");
    expect(ask?.text).toContain("Rooms and Roof build on Shell");
    expect(panel()).toContain("Discard 3 parts");
    await confirmAsk(ask!);
    expect(calls.slice(-1)[0]).toBe("discard:shell+dependents");
  });

  it("Discard on a part nobody builds on goes straight through", async () => {
    await discardPart("roof");
    expect(useSwarm.getState().ask).toBeNull();
    expect(calls).toEqual(["check:roof", "discard:roof"]);
  });

  it("Discard for the whole draft asks, and closes the swarm once confirmed", async () => {
    discardAll("bar");
    const ask = useSwarm.getState().ask;
    expect(ask?.confirm).toBe("Discard the draft");
    expect(calls).toEqual([]);
    await confirmAsk(ask!);
    expect(calls).toEqual(["discard"]);
    expect(useSwarm.getState().status).toBeNull();
  });

  it("Accept asks first, listing them, when the engine says elements would come out differently", async () => {
    const differ = { part: "shell", step: 3, label: "Add walls", code: "differs", message: "Room 1 in the draft comes in as Room 3.", element_ids: [] };
    setSwarmApi(mockApi(() => current, calls, () => [differ]));
    await acceptPart("shell", "bar");
    const ask = useSwarm.getState().ask;
    expect(ask?.text).toBe("1 element will come out differently from the draft.");
    expect(ask?.confirm).toBe("Accept Shell");
    const html = bar(current);
    expect(html).toContain('data-testid="swarm-ask-differs"');
    expect(html).toContain("Room 1 in the draft comes in as Room 3.");
    await confirmAsk(ask!);
    expect(calls.slice(-1)[0]).toBe("accept:shell");

    calls.length = 0;
    await acceptAll();
    const all = useSwarm.getState().ask;
    expect(all?.confirm).toBe("Accept all anyway");
    expect(calls).toEqual(["check:lead,shell,rooms,roof"]);
    await confirmAsk(all!);
    expect(calls.slice(-1)[0]).toBe("accept:all");
  });

  it("Accept names the swarm and pins every part to the step the window showed", async () => {
    let sent: { id: string; reviewed: Record<string, number> } | null = null;
    setSwarmApi({ ...mockApi(() => current, calls), accept: (id, _parts, reviewed) => ((sent = { id, reviewed }), Promise.resolve({ applied: [], skipped: [], state: null, dependents: [], differs: [] })) });
    useSwarm.setState({ seen: { lead: 1, shell: 7, rooms: 3 } });
    await acceptAll();
    expect(sent).toEqual({ id: "sw-1", reviewed: { lead: 1, shell: 7, rooms: 3, roof: 0 } });
  });

  it("accepts a part that is still drawing after a confirm that says so, pinned to what was seen when Accept was pressed", async () => {
    current = mockStatus();
    let sent: Record<string, number> | null = null;
    setSwarmApi({ ...mockApi(() => current, calls), accept: (_id, _parts, reviewed) => ((sent = reviewed), Promise.resolve({ applied: [{ part: "shell", revision: 1, summary: "" }], skipped: [], state: null, dependents: [], differs: [] })) });
    show(current, { seen: { lead: 1, shell: 7 } });
    await acceptPart("shell", "panel");
    const ask = useSwarm.getState().ask;
    expect(ask?.text).toContain("Shell is still drawing");
    expect(panel()).toContain("Accepting now takes its steps so far and stops it.");
    // Shell draws more while the confirm is open: the accept still names what the person saw.
    useSwarm.setState({ seen: { lead: 1, shell: 9 } });
    await confirmAsk(ask!);
    expect(sent).toEqual({ lead: 1, shell: 7, rooms: 0, roof: 0 });
  });

  // Gate P1-3: an accept never drops steps without asking. The reviewer's
  // repro: Dims ran Auto dimension, which no longer applies on the model.
  const nothing = { part: "shell", step: 4, label: "Auto dimension", code: "nothing_to_dimension", message: "Nothing to dimension.", element_ids: [] };

  it("Accept asks first, listing the steps that would stay out, and Accept the rest sends accept_rest", async () => {
    setSwarmApi(mockApi(() => current, calls, () => [], () => [nothing]));
    await acceptPart("shell", "bar");
    const ask = useSwarm.getState().ask;
    expect(ask?.confirm).toBe("Accept the rest");
    expect(ask?.rest).toBe(true);
    expect(ask?.text).toBe("1 step no longer applies. Accepting the rest leaves it in the draft as conflicts.");
    const html = bar(current);
    expect(html).toContain('data-testid="swarm-ask-skipped"');
    expect(html).toContain("Shell, Auto dimension: Nothing to dimension.");
    expect(html).toContain("Cancel");
    expect(calls).toEqual(["check:shell"]);
    await confirmAsk(ask!);
    expect(calls.slice(-1)[0]).toBe("accept:shell+rest");

    calls.length = 0;
    await acceptAll("panel");
    const all = useSwarm.getState().ask;
    expect(all?.confirm).toBe("Accept the rest");
    expect(panel()).toContain("Shell, Auto dimension: Nothing to dimension.");
    await confirmAsk(all!);
    expect(calls).toEqual(["check:lead,shell,rooms,roof", "accept:all+rest"]);
  });

  it("a part none of whose steps apply any more asks first, though the check says no part commits", async () => {
    const base = mockApi(() => current, calls, () => [], () => [nothing]);
    setSwarmApi({ ...base, check: (parts) => base.check(parts).then((r) => ({ ...r, applied: [] })) });
    await acceptPart("shell", "panel");
    const ask = useSwarm.getState().ask;
    expect(ask?.confirm).toBe("Accept the rest");
    expect(ask?.skipped).toEqual(["Shell, Auto dimension: Nothing to dimension."]);
    // Asked from the check: no accept was sent to be refused.
    expect(calls).toEqual(["check:shell"]);
  });

  it("an accept refused steps_skipped shows that confirm instead of an error, then accepts the rest", async () => {
    // The check said nothing would stay out; by the accept a step had stopped applying.
    let stopped = false;
    const base = mockApi(() => current, calls, () => [], () => (stopped ? [nothing] : []));
    setSwarmApi({
      ...base,
      accept: (id, parts, reviewed, rest) => {
        stopped = true;
        return base.accept(id, parts, reviewed, rest);
      },
    });
    await acceptPart("shell", "panel");
    expect(useSwarm.getState().note).toBeNull();
    const ask = useSwarm.getState().ask;
    expect(ask?.confirm).toBe("Accept the rest");
    expect(ask?.origin).toBe("panel");
    expect(ask?.skipped).toEqual(["Shell, Auto dimension: Nothing to dimension."]);
    expect(calls).toEqual(["check:shell", "accept:shell:refused", "check:shell"]);
    await confirmAsk(ask!);
    expect(calls.slice(-1)[0]).toBe("accept:shell+rest");
  });

  it("an engine that has not built a call yet says so in the panel, not as a crash", async () => {
    setSwarmApi({ ...mockApi(() => current, calls), accept: () => Promise.reject({ code: "not_built", message: "not built", element_ids: [] }) });
    await acceptAll();
    expect(useSwarm.getState().note?.text).toBe("Accepting is not ready in this build yet.");
  });
});
