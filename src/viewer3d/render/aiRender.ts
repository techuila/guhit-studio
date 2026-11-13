// AI render v2 calls (lane L6, DECISIONS D53): the estimate shown before
// every call, the draft a person keeps or discards after the drift check,
// the conditioning maps, the project look and the agents' daily cap.
//
// These commands live in the Rust render_ai module (`render_ai::OWNS`); the
// typed calls are in contract/ipc.ts and the types in the generated
// bindings, re-exported here for the render panel.

import type { AgentUsage, Estimate, MapKind, MapsMeta, RenderAiRequest, RenderRecord } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";

export type { AgentUsage, Draft, Estimate, LegendEntry, MapsMeta } from "../../contract/bindings";

type Id = string;

export const aiRender = {
  estimate: (request: RenderAiRequest) => ipc.renderAiEstimate(request),
  draft: (request: RenderAiRequest) => ipc.renderAiDraft(request),
  keep: (pendingId: Id) => ipc.renderAiKeep(pendingId),
  discard: (pendingId: Id) => ipc.renderAiDiscard(pendingId),
  saveMaps: (renderId: Id, maps: Array<{ kind: MapKind; png: string }>, meta: MapsMeta) =>
    ipc.renderAiMapsSave(renderId, maps, meta),
  usage: () => ipc.renderAiUsage(),
  setCap: (cap: number) => ipc.renderAiUsageSet(cap),
  driftOverlay: (id: Id) => ipc.renderAiDriftOverlay(id),
  mapData: (id: Id, kind: MapKind) => ipc.renderAiMapData(id, kind),
  setLook: (id: Id | null) => ipc.renderSetLook(id),
};

/** "$0.10", the way the price line shows it. */
export function priceLabel(e: Pick<Estimate, "price_usd"> | null): string {
  if (!e) return "";
  return `$${e.price_usd}`;
}

/** "Gemini Flash, 2K, about $0.10" for the button and the price line. */
export function estimateLine(e: Estimate | null): string {
  if (!e) return "";
  return `${e.model_label}, ${e.image_size}, about ${priceLabel(e)}`;
}

/** "3 of 20 left today" for the agents' cap. */
export function capLine(u: AgentUsage | null): string {
  if (!u) return "";
  if (u.cap === 0) return "Agents may not make AI images";
  const left = Math.max(0, u.cap - u.used);
  return `${left} of ${u.cap} left today`;
}

/** Share of the cap used, 0 to 1, for the meter. */
export function capShare(u: AgentUsage | null): number {
  if (!u || u.cap <= 0) return 1;
  return Math.min(1, Math.max(0, u.used / u.cap));
}

/** "drift" when a record's drift check warned, "ok" when it passed, null when it was not checked. */
export function driftState(r: Pick<RenderRecord, "drift"> | null | undefined): "drift" | "ok" | null {
  const d = r?.drift;
  if (!d) return null;
  return d.warn ? "drift" : "ok";
}

/** The steps the cap control offers. */
export const CAP_STEPS = [0, 5, 10, 20, 40, 60, 100] as const;
