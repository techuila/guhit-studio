// The swarm store follows the backend (wiring.ts): the draft is fetched only
// when its revision moves, one fetch in flight at a time.
import { afterEach, describe, expect, it } from "vitest";
import type { DocState } from "../contract/bindings";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import { useShell } from "../shell/shellStore";
import { useApp } from "../state/store";
import { setSwarmApi } from "./api";
import { acceptAll } from "./actions";
import { mockApi, mockPart, mockStatus } from "./mock";
import { reveal, useSwarm } from "./swarmStore";
import type { SwarmDraftReply, SwarmDraftState, SwarmStatus, SwarmStep } from "./types";
import { startSwarmWiring, swarmWiring } from "./wiring";
import { bus } from "../state/bus";

const doc = fixture as unknown as DocState;
const walls = doc.project.elements.filter((e) => e.kind === "wall");

function step(seq: number, id: string): SwarmStep {
  return { seq, part: "shell", agent_id: "agent-shell", label: "Add wall", status: "Laying out walls", added: [id], modified: [], removed: [], at: "" };
}

function draft(revision: number, steps: SwarmStep[]): SwarmDraftState {
  return { swarm_id: "sw-1", draft_revision: revision, state: doc, parts: [{ part: "shell", color: 0, ids: steps.flatMap((s) => s.added) }], removed: [], steps, display_modified: [] };
}

/** A backend whose draft answers wait until the test lets them go. */
function slowDrafts() {
  const asked: number[] = [];
  const pending: Array<(d: SwarmDraftState) => void> = [];
  const api = { ...mockApi(() => null), draft: (since: number) => (asked.push(since), new Promise<SwarmDraftState>((res) => pending.push(res))) };
  return { api, asked, pending };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

afterEach(() => {
  setSwarmApi(null);
  reveal.reset();
  useSwarm.setState({ status: null, draft: null, lastSeq: 0, seen: {}, watching: false, mode: "chat", ask: null, note: null, acting: null });
  useApp.setState({ doc: null });
});

describe("the cheaper reply and the accept pins", () => {
  it("a remounted dock names the revision it drew; a current answer keeps the view and takes only the status", async () => {
    useApp.setState({ doc });
    const asked: Array<{ since: number; drawn: unknown }> = [];
    let reply: SwarmDraftReply = draft(3, [step(1, walls[0].id), step(2, walls[1].id), step(3, walls[2].id)]);
    setSwarmApi({ ...mockApi(() => null), draft: (since, drawn) => (asked.push({ since, drawn: drawn ?? null }), Promise.resolve(reply)) });
    const first = swarmWiring();
    first.onStatus(mockStatus({ project_id: doc.project.id, draft_revision: 3 }), true);
    await flush();
    const view = useSwarm.getState().draft;
    expect(view?.draft_revision).toBe(3);
    // Nothing drawn yet: the whole view is asked for.
    expect(asked[0]).toEqual({ since: 0, drawn: null });
    first.stop();

    // The dock remounts on the same revision: it names what it drew, and the engine answers the status alone.
    reply = { swarm_id: "sw-1", draft_revision: 3, status: mockStatus({ project_id: doc.project.id, draft_revision: 3, state: "ready" }) };
    const again = swarmWiring();
    again.onStatus(mockStatus({ project_id: doc.project.id, draft_revision: 3 }), false);
    await flush();
    expect(asked[1]).toEqual({ since: 3, drawn: { draftRevision: 3, swarmId: "sw-1" } });
    expect(useSwarm.getState().draft).toBe(view);
    expect(useSwarm.getState().lastSeq).toBe(3);
    expect(useSwarm.getState().status?.state).toBe("ready");
    // Current now: no fetch after it.
    expect(asked).toHaveLength(2);
    again.stop();
  });

  // Gate P2 "pins hide undos": the engine accepts only when each pin equals the part's last_seq (check_pins).
  it("pins an accept to what the window drew, so an agent's undo since is stale and the person looks again", async () => {
    useApp.setState({ doc });
    const statusAt = (revision: number, shell: number): SwarmStatus =>
      mockStatus({ project_id: doc.project.id, state: "ready", draft_revision: revision, parts: [mockPart("lead", "Lead", 6, "done", 0), mockPart("shell", "Shell", 0, "done", shell)] });
    let engine = statusAt(3, 3);
    const sent: Array<Record<string, number>> = [];
    const { api, pending } = slowDrafts();
    setSwarmApi({
      ...api,
      check: mockApi(() => engine).check,
      accept: (_id, _parts, reviewed) => {
        sent.push(reviewed);
        if (engine.parts.some((p) => (reviewed[p.key] ?? 0) !== p.last_seq)) return Promise.reject({ code: "stale", message: "Shell drew more after you reviewed the draft. Look at it again, then accept.", element_ids: [] });
        return Promise.resolve({ applied: [{ part: "shell", revision: 1, summary: "" }], skipped: [], state: null, dependents: [], differs: [] });
      },
    });
    const w = swarmWiring();
    w.onStatus(engine, true);
    pending[0](draft(3, [step(1, walls[0].id), step(2, walls[1].id), step(3, walls[2].id)]));
    await flush();
    expect(useSwarm.getState().seen).toEqual({ lead: 0, shell: 3 });

    // Shell's agent undoes step 3; its draft fetch is still in flight when the person presses Accept all.
    engine = statusAt(4, 2);
    w.onStatus(engine, true);
    await acceptAll("bar");
    expect(sent[0]).toEqual({ lead: 0, shell: 3 });
    expect(useSwarm.getState().note?.text).toContain("Look at it again");

    // The view without step 3 lands: now the person has seen the undo, and the accept goes in.
    pending[1](draft(4, []));
    await flush();
    expect(useSwarm.getState().seen).toEqual({ lead: 0, shell: 2 });
    await acceptAll("bar");
    expect(sent[1]).toEqual({ lead: 0, shell: 2 });
    expect(useSwarm.getState().note).toBeNull();
    w.stop();
  });
});

describe("following the backend", () => {
  it("fetches the draft only when its revision moves, one fetch at a time, and once more after a move mid-fetch", async () => {
    useApp.setState({ doc });
    const { api, asked, pending } = slowDrafts();
    setSwarmApi(api);
    const w = swarmWiring();
    const at = (rev: number): SwarmStatus => mockStatus({ project_id: doc.project.id, draft_revision: rev });
    w.onStatus(at(1), true);
    expect(asked).toEqual([0]);
    // Two more moves while the first fetch runs: no second fetch yet.
    w.onStatus(at(2), true);
    w.onStatus(at(3), true);
    expect(asked).toHaveLength(1);
    pending[0](draft(1, [step(1, walls[0].id)]));
    await flush();
    // The revision moved past what came back: exactly one more, from the last step seen.
    expect(asked).toEqual([0, 1]);
    pending[1](draft(3, [step(2, walls[1].id), step(3, walls[2].id)]));
    await flush();
    expect(asked).toHaveLength(2);
    // The same revision again: nothing to fetch.
    w.onStatus(at(3), true);
    expect(asked).toHaveLength(2);
    expect(useSwarm.getState().lastSeq).toBe(3);
    w.stop();
  });

  it("reveals the steps of a swarm it watched start, and shows a swarm found drafting at once", async () => {
    useApp.setState({ doc });
    setSwarmApi({ ...mockApi(() => null), draft: () => Promise.resolve(draft(1, [step(1, walls[0].id)])) });
    const watched = swarmWiring();
    watched.onStatus(mockStatus({ project_id: doc.project.id, draft_revision: 1 }), true);
    await flush();
    expect(reveal.progress(walls[0].id, performance.now())).toBeLessThan(1);
    watched.stop();

    reveal.reset();
    useSwarm.setState({ status: null, draft: null, lastSeq: 0, watching: false });
    const found = swarmWiring();
    found.onStatus(mockStatus({ project_id: doc.project.id, draft_revision: 1 }), false);
    await flush();
    expect(reveal.progress(walls[0].id, performance.now())).toBe(1);
    found.stop();
  });

  it("opens the dock on Swarm when a swarm starts while the window watches", () => {
    useApp.setState({ doc });
    setSwarmApi(mockApi(() => null));
    useShell.setState({ dockTab: "visuals", dockCollapsed: true });
    const w = swarmWiring();
    w.onStatus(mockStatus({ project_id: doc.project.id }), true);
    expect(useSwarm.getState().mode).toBe("swarm");
    expect(useShell.getState().dockTab).toBe("copilot");
    expect(useShell.getState().dockCollapsed).toBe(false);
    w.stop();
  });

  it("ignores another project's swarm, and clears when the swarm ends", () => {
    useApp.setState({ doc });
    setSwarmApi(mockApi(() => null));
    const w = swarmWiring();
    w.onStatus(mockStatus({ project_id: "another" }), true);
    expect(useSwarm.getState().status).toBeNull();
    w.onStatus(mockStatus({ project_id: doc.project.id }), true);
    expect(useSwarm.getState().status).not.toBeNull();
    w.onStatus(null, true);
    expect(useSwarm.getState().status).toBeNull();
    expect(useSwarm.getState().draft).toBeNull();
    w.stop();
  });

  // Gate P1-4: a fetch that lands after the swarm ended must not paint a ghost.
  it("a draft answer that lands after the discard event paints nothing", async () => {
    useApp.setState({ doc });
    const { api, pending } = slowDrafts();
    setSwarmApi(api);
    const w = swarmWiring();
    w.onStatus(mockStatus({ project_id: doc.project.id, draft_revision: 1 }), true);
    expect(pending).toHaveLength(1);
    // Discard (or accept) ends the swarm while the fetch is in flight.
    w.onStatus(null, true);
    pending[0](draft(1, [step(1, walls[0].id)]));
    await flush();
    expect(useSwarm.getState().status).toBeNull();
    expect(useSwarm.getState().draft).toBeNull();
    expect(useSwarm.getState().lastSeq).toBe(0);
    w.stop();
  });

  it("drops a draft answer for the swarm before, and fetches the new swarm's", async () => {
    useApp.setState({ doc });
    const { api, asked, pending } = slowDrafts();
    setSwarmApi(api);
    const w = swarmWiring();
    w.onStatus(mockStatus({ project_id: doc.project.id, draft_revision: 1 }), true);
    // The first swarm ends and a second starts while its fetch is in flight.
    w.onStatus(null, true);
    w.onStatus(mockStatus({ id: "sw-2", project_id: doc.project.id, draft_revision: 1 }), true);
    pending[0](draft(1, [step(1, walls[0].id)]));
    await flush();
    expect(useSwarm.getState().status?.id).toBe("sw-2");
    expect(useSwarm.getState().draft).toBeNull();
    // The new swarm's draft is asked for once the stale answer is dropped.
    expect(asked).toEqual([0, 0]);
    pending[1]({ ...draft(1, [step(1, walls[1].id)]), swarm_id: "sw-2" });
    await flush();
    expect(useSwarm.getState().draft?.swarm_id).toBe("sw-2");
    expect(useSwarm.getState().lastSeq).toBe(1);
    w.stop();
  });

  it("the store takes no draft while no swarm is open, nor one of another swarm", () => {
    useSwarm.getState().applyDraft(draft(1, [step(1, walls[0].id)]), doc);
    expect(useSwarm.getState().draft).toBeNull();
    useSwarm.getState().applyStatus(mockStatus({ id: "sw-2", project_id: doc.project.id }), true);
    useSwarm.getState().applyDraft(draft(1, [step(1, walls[0].id)]), doc);
    expect(useSwarm.getState().draft).toBeNull();
    expect(useSwarm.getState().seen).toEqual({});
  });

  it("answers the swarm bus event: the dock opens on Swarm, at the part's row", () => {
    setSwarmApi(mockApi(() => null));
    useShell.setState({ dockTab: "chat", dockCollapsed: true });
    const stop = startSwarmWiring();
    bus.emit("swarm", { part: "rooms" });
    expect(useSwarm.getState().mode).toBe("swarm");
    expect(useSwarm.getState().focus?.part).toBe("rooms");
    expect(useShell.getState().dockTab).toBe("copilot");
    expect(useShell.getState().dockCollapsed).toBe(false);
    const token = useSwarm.getState().focus?.token;
    bus.emit("swarm", { part: "rooms" });
    expect(useSwarm.getState().focus?.token).toBe((token ?? 0) + 1);
    stop();
    useSwarm.setState({ mode: "chat" });
    bus.emit("swarm", { part: null });
    expect(useSwarm.getState().mode).toBe("chat");
  });
});
