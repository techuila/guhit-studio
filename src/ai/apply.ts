// Apply on the copilot's proposal card (D8): the engine commits the proposal
// as one undo step. While a swarm draft is open the model is locked (D57):
// the engine refuses `swarm_running` and keeps the proposal, so the card
// stays pending and applies once the draft is accepted or discarded.

import type { AiProposal } from "../contract/bindings";
import { ipc, toIpcError } from "../contract/ipc";
import { useApp } from "../state/store";
import { useCopilot } from "./copilotStore";

export const STALE_TEXT = "The plan changed after this proposal was made, so it was not applied. Ask again and I will work from the current plan.";

/** The toast when Apply meets the swarm lock. */
export const LOCKED_APPLY_TEXT = "Not applied yet: a swarm is drafting. Accept or discard the draft, then apply.";

export async function applyProposal(itemId: number, proposal: AiProposal): Promise<void> {
  const copilot = useCopilot.getState();
  copilot.patch(itemId, { proposalStatus: "applying" });
  try {
    const result = await ipc.aiResolve(proposal.id, true);
    const app = useApp.getState();
    app.setPreview(null);
    app.setHover(null);
    if (result.applied) {
      app.setDoc(result.applied.state);
      app.toast("success", "Applied. Undo reverts it in one step.");
      copilot.patch(itemId, { proposalStatus: "applied", proposalNote: "Applied. Undo reverts it in one step." });
    } else {
      copilot.patch(itemId, { proposalStatus: "discarded", proposalNote: "Nothing was applied." });
    }
  } catch (e) {
    const err = toIpcError(e);
    const app = useApp.getState();
    if (err.code === "swarm_running") {
      // The engine still holds it: the card and its preview stay for later.
      copilot.patch(itemId, { proposalStatus: "pending" });
      app.toast("info", LOCKED_APPLY_TEXT);
      return;
    }
    app.setPreview(null);
    app.setHover(null);
    const stale = err.code === "stale" || err.code === "not_found";
    copilot.patch(itemId, { proposalStatus: "stale", proposalNote: stale ? STALE_TEXT : `Not applied: ${err.message}` });
    // Make sure the mirror matches the engine before the user asks again.
    try {
      const state = await ipc.docState();
      if (state) app.setDoc(state);
    } catch {
      // The next action will surface a bridge problem.
    }
  }
}
