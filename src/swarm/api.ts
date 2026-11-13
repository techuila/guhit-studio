// The swarm's backend calls (src/contract/ipc.ts), behind one interface so
// the panel and the store can be tested with a mock (`setSwarmApi`).

import { ipc } from "../contract/ipc";
import type { SwarmAcceptResult, SwarmCost, SwarmDraftCurrent, SwarmDraftReply, SwarmRequest, SwarmStatus } from "./types";

export interface SwarmApi {
  status(): Promise<SwarmStatus | null>;
  /**
   * The draft view and the steps after `sinceStep`. With `drawn` (the
   * revision of the swarm the window has drawn), only the status when the
   * draft has not moved since (`SwarmDraftCurrent`).
   */
  draft(sinceStep: number, drawn?: { draftRevision: number; swarmId: string } | null): Promise<SwarmDraftReply | null>;
  /** What a brief would cost. Spends nothing. */
  estimate(request: SwarmRequest): Promise<SwarmCost>;
  start(request: SwarmRequest): Promise<SwarmStatus>;
  pause(): Promise<SwarmStatus>;
  resume(): Promise<SwarmStatus>;
  /** One part, or every part when null. */
  stop(part: string | null): Promise<SwarmStatus>;
  /** What accepting these parts (all when null) would pull in, and who depends on them. */
  check(parts: string[] | null): Promise<SwarmAcceptResult>;
  /**
   * `reviewed`: each part's `last_seq` as the person saw it. Refused
   * `steps_skipped` when steps no longer apply, unless `acceptRest`.
   */
  accept(swarmId: string, parts: string[] | null, reviewed: Record<string, number>, acceptRest?: boolean): Promise<SwarmAcceptResult>;
  discard(swarmId: string): Promise<null>;
  /** Discards a part, with the parts that depend on it. Null when the draft is left empty. */
  discardPart(part: string, withDependents: boolean): Promise<SwarmStatus | null>;
  setCap(usd: number): Promise<SwarmStatus>;
}

const real: SwarmApi = {
  status: () => ipc.swarmStatus(),
  draft: (sinceStep, drawn) => ipc.swarmDraft(sinceStep, drawn ?? null),
  estimate: (request) => ipc.swarmEstimate(request),
  start: (request) => ipc.swarmStart(request),
  pause: () => ipc.swarmPause(null),
  resume: () => ipc.swarmResume(null),
  stop: (part) => ipc.swarmStop(part),
  check: (parts) => ipc.swarmCheck(parts),
  accept: (swarmId, parts, reviewed, acceptRest) => ipc.swarmAccept(swarmId, parts, reviewed, acceptRest ?? false),
  discard: (swarmId) => ipc.swarmDiscard(swarmId),
  discardPart: (part, withDependents) => ipc.swarmPartDiscard(part, withDependents),
  setCap: (usd) => ipc.swarmSetCap(usd),
};

/** The window is current: the reply has only the status, not the view. */
export function isDraftCurrent(reply: SwarmDraftReply): reply is SwarmDraftCurrent {
  return "status" in reply && !("state" in reply);
}

let impl: SwarmApi = real;

export function swarmApi(): SwarmApi {
  return impl;
}

/** Tests and the dev page swap the backend. Null puts the real one back. */
export function setSwarmApi(api: SwarmApi | null): void {
  impl = api ?? real;
}
