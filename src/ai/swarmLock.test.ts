// The copilot while a swarm draft locks the model (D57, gate P1-5): Send and
// Furnish are off with the lock's hint, and Apply refused `swarm_running`
// keeps the proposal for after the draft.
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AiProposal, DocState } from "../contract/bindings";

let swarmOpen = false;
const resolved: string[] = [];

vi.mock("../contract/ipc", async (orig) => ({
  ...(await orig<typeof import("../contract/ipc")>()),
  ipc: {
    aiResolve: vi.fn(async (id: string) => {
      resolved.push(id);
      if (swarmOpen) throw { code: "swarm_running", message: "A swarm draft is open.", element_ids: [] };
      return { applied: { state: { revision: 6, project: { id: "p", levels: [], elements: [] }, derived: {} } } };
    }),
    docState: vi.fn(async () => ({ revision: 5 })),
  },
}));

import { useApp } from "../state/store";
import { LOCK_HINT } from "../swarm/view";
import { LOCKED_APPLY_TEXT, applyProposal } from "./apply";
import { composerGate, useCopilot } from "./copilotStore";

const proposal = { id: "p1", base_revision: 5, preview: { state: {}, diff: { added: ["w1"], modified: [], removed: [], summary: "" } } } as unknown as AiProposal;

describe("the copilot during a swarm draft", () => {
  beforeEach(() => {
    swarmOpen = false;
    resolved.length = 0;
    useCopilot.getState().reset("p");
    useApp.setState({ doc: { revision: 5 } as unknown as DocState, preview: proposal.preview, toasts: [] });
  });

  it("Send and Furnish are off with the lock's hint while a draft is open", () => {
    const open = { hasDoc: true, busy: false, noKey: false, text: "Add a window" };
    expect(composerGate({ ...open, locked: false })).toEqual({ canSend: true, canFurnish: true, tip: null });
    expect(composerGate({ ...open, locked: true })).toEqual({ canSend: false, canFurnish: false, tip: LOCK_HINT });
  });

  it("Apply refused swarm_running keeps the card pending with its preview, and applies after the draft", async () => {
    const item = useCopilot.getState().push({ role: "assistant", text: "Here it is", proposal, proposalStatus: "pending" });
    swarmOpen = true;
    await applyProposal(item, proposal);
    const card = useCopilot.getState().items.find((m) => m.id === item)!;
    expect(card.proposalStatus).toBe("pending");
    expect(card.proposal?.id).toBe("p1");
    expect(useApp.getState().preview).toBe(proposal.preview);
    expect(useApp.getState().toasts.slice(-1)[0]).toMatchObject({ kind: "info", message: LOCKED_APPLY_TEXT });
    // Discarded: the same proposal applies.
    swarmOpen = false;
    await applyProposal(item, proposal);
    expect(resolved).toEqual(["p1", "p1"]);
    expect(useCopilot.getState().items.find((m) => m.id === item)!.proposalStatus).toBe("applied");
  });
});
