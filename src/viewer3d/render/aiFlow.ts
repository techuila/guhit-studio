// One AI render from the studio, in order (lane L6, DECISIONS D53):
//
// 1. The estimate: what it costs and sends. Shown before anything is spent.
// 2. The maps, when the capture has none yet: drawn in the window, then the
//    estimate again (it now lists them).
// 3. The draft: the provider's image with its drift check, not saved yet.
// 4. A clean image is kept at once. One the drift check flags waits for the
//    person: keep it anyway, or discard it.
//
// Pure of React and IPC so the order is tested (aiFlow.test.ts).

import type { RenderAiRequest, RenderRecord } from "../../contract/bindings";
import type { Draft, Estimate } from "./aiRender";

export type FlowPhase = "pricing" | "maps" | "rendering";

export interface FlowDeps {
  estimate: (request: RenderAiRequest) => Promise<Estimate>;
  /** Draws and saves the source's maps. */
  drawMaps: () => Promise<void>;
  draft: (request: RenderAiRequest) => Promise<Draft>;
  keep: (pendingId: string) => Promise<RenderRecord>;
}

export interface FlowHooks {
  /** The price, before the call it prices. */
  onPrice: (estimate: Estimate) => void;
  onPhase: (phase: FlowPhase) => void;
}

export type FlowResult = { kind: "saved"; record: RenderRecord; draft: Draft } | { kind: "held"; draft: Draft };

export async function renderWithAi(request: RenderAiRequest, deps: FlowDeps, hooks: FlowHooks): Promise<FlowResult> {
  hooks.onPhase("pricing");
  let estimate = await deps.estimate(request);
  hooks.onPrice(estimate);
  if (estimate.maps_missing && request.use_maps !== false) {
    hooks.onPhase("maps");
    try {
      await deps.drawMaps();
      estimate = await deps.estimate(request);
      hooks.onPrice(estimate);
    } catch {
      // Without maps the render still runs, unchecked; the estimate says so.
    }
  }
  hooks.onPhase("rendering");
  const draft = await deps.draft(request);
  if (draft.drift?.warn) return { kind: "held", draft };
  const record = await deps.keep(draft.pending_id);
  return { kind: "saved", record, draft };
}
