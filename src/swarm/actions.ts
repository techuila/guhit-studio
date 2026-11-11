// What the swarm panel and the draft bar do (DECISIONS D56, D57). Accept
// always skips the reveal to the end first, so nobody accepts work they have
// not seen, and is pinned to each part's `last_seq` as the person saw it.
// Accept and Discard ask first when other parts come along (view.ts), and
// Accept when steps would stay out: never without "Accept the rest".

import type { IpcError } from "../contract/bindings";
import { toIpcError } from "../contract/ipc";
import { useApp } from "../state/store";
import { swarmApi } from "./api";
import { useSwarm } from "./swarmStore";
import type { SwarmConflict, SwarmRequest } from "./types";
import { acceptAllAsk, acceptAsk, dependents, dependsOn, discardAllAsk, discardAsk, nameList, pinsOf, type Ask, type AskOrigin } from "./view";

function fail(e: unknown, what: string): void {
  const err = toIpcError(e);
  const text = err.code === "not_built" ? `${what} is not ready in this build yet.` : err.message;
  useSwarm.getState().setNote({ tone: "error", text });
}

async function run<T>(acting: string, what: string, fn: () => Promise<T>): Promise<T | null> {
  const sw = useSwarm.getState();
  if (sw.acting) return null;
  sw.setActing(acting);
  sw.setNote(null);
  try {
    return await fn();
  } catch (e) {
    fail(e, what);
    return null;
  } finally {
    useSwarm.getState().setActing(null);
  }
}

/** Starts Guhit Studio's own swarm from the brief in the panel. */
export async function startSwarm(): Promise<void> {
  const sw = useSwarm.getState();
  const brief = sw.brief.trim();
  const app = useApp.getState();
  if (!brief || !app.doc || sw.status) return;
  const status = await run("start", "Starting a swarm", () => swarmApi().start(requestFor(brief)));
  if (status) useSwarm.getState().applyStatus(status, true);
}

/** What starting a swarm sends: the brief, the level on screen, "Only the selection" (D30), the cap. */
function requestFor(brief: string): SwarmRequest {
  const app = useApp.getState();
  const scope = app.aiScope && app.selection.length > 0 ? { ids: app.selection } : null;
  return { brief: brief.trim(), active_level_id: app.activeLevelId, scope, review_plan: false, cap_usd: useSwarm.getState().capUsd };
}

/** The estimate for the brief in the panel. Spends nothing; quiet when it cannot say. */
export async function estimateBrief(brief: string): Promise<void> {
  if (!brief.trim()) {
    useSwarm.getState().setEstimate(null);
    return;
  }
  try {
    const cost = await swarmApi().estimate(requestFor(brief));
    if (useSwarm.getState().brief === brief) useSwarm.getState().setEstimate(cost);
  } catch {
    useSwarm.getState().setEstimate(null);
  }
}

export async function pauseAll(): Promise<void> {
  const status = await run("pause", "Pausing", () => swarmApi().pause());
  if (status) useSwarm.getState().applyStatus(status, true);
}

export async function resumeAll(): Promise<void> {
  const status = await run("pause", "Resuming", () => swarmApi().resume());
  if (status) useSwarm.getState().applyStatus(status, true);
}

export async function stopPart(key: string | null): Promise<void> {
  const status = await run(`stop:${key ?? "all"}`, "Stopping", () => swarmApi().stop(key));
  if (status) useSwarm.getState().applyStatus(status, true);
}

/**
 * The parts accepting `key` brings in, what would come out differently from
 * the draft view, and the steps that would stay out: from the engine when it
 * can say, else the declared order and nothing.
 */
async function closureOf(key: string): Promise<{ parts: string[]; differs: SwarmConflict[]; skipped: SwarmConflict[] }> {
  const status = useSwarm.getState().status;
  if (!status) return { parts: [key], differs: [], skipped: [] };
  try {
    const check = await swarmApi().check([key]);
    // `applied` lists only the parts that commit: none when every step of the part stays out.
    const parts = check.applied.map((a) => a.part);
    return { parts: parts.length > 0 ? parts : [...dependsOn(status.parts, key), key], differs: check.differs ?? [], skipped: check.skipped ?? [] };
  } catch {
    // An engine before swarm_check: the declared order stands in.
  }
  return { parts: [...dependsOn(status.parts, key), key], differs: [], skipped: [] };
}

/** The parts discarding `key` takes with it, from the engine when it can say. */
async function dependentsOf(key: string): Promise<string[]> {
  const status = useSwarm.getState().status;
  if (!status) return [];
  try {
    const check = await swarmApi().check([key]);
    return check.dependents;
  } catch {
    return dependents(status.parts, key);
  }
}

/**
 * Accept all: every part, each as its own undo step, in the order it
 * started. Asks first when the engine says something would come out
 * differently from the draft view, or that steps would stay out.
 */
export async function acceptAll(origin: AskOrigin = "bar"): Promise<void> {
  const sw = useSwarm.getState();
  if (!sw.status) return;
  sw.skip();
  sw.setAsk(null);
  // Pinned to what the person saw when they pressed Accept, not to what lands while the confirm is open.
  const reviewed = pinsOf(sw.status, sw.seen);
  let differs: SwarmConflict[] = [];
  let skipped: SwarmConflict[] = [];
  try {
    const check = await swarmApi().check(null);
    differs = check.differs ?? [];
    skipped = check.skipped ?? [];
  } catch {
    // An engine before swarm_check: nothing to warn about.
  }
  const status = useSwarm.getState().status;
  if (!status) return;
  const ask = acceptAllAsk(status, differs, skipped);
  if (ask) {
    useSwarm.getState().setAsk({ ...ask, origin, reviewed });
    return;
  }
  await accept(null, reviewed, false, origin);
}

/** Accept one part, asking first when it pulls in the parts it builds on or steps would stay out. */
export async function acceptPart(key: string, origin: AskOrigin = "panel"): Promise<void> {
  const sw = useSwarm.getState();
  if (!sw.status) return;
  sw.skip();
  const reviewed = pinsOf(sw.status, sw.seen);
  const { parts, differs, skipped } = await closureOf(key);
  const status = useSwarm.getState().status;
  if (!status) return;
  const ask = acceptAsk(status, key, parts, differs, skipped);
  if (ask) {
    useSwarm.getState().setAsk({ ...ask, origin, reviewed });
    return;
  }
  await accept([key], reviewed, false, origin);
}

/**
 * The engine refused an accept `steps_skipped` (steps stopped applying
 * after the check): ask again, listing them, with "Accept the rest".
 */
async function askRest(parts: string[] | null, reviewed: Record<string, number>, refusal: IpcError, origin: AskOrigin | undefined): Promise<void> {
  let skipped: SwarmConflict[] = [];
  let differs: SwarmConflict[] = [];
  try {
    const check = await swarmApi().check(parts);
    skipped = check.skipped ?? [];
    differs = check.differs ?? [];
  } catch {
    // The refusal's own words stand in below.
  }
  if (skipped.length === 0) skipped = [{ part: "", step: 0, label: "", code: refusal.code, message: refusal.message, element_ids: refusal.element_ids }];
  const status = useSwarm.getState().status;
  if (!status) return;
  const key = parts?.[parts.length - 1] ?? null;
  const ask = key === null ? acceptAllAsk(status, differs, skipped) : acceptAsk(status, key, parts ?? [key], differs, skipped);
  if (ask) useSwarm.getState().setAsk({ ...ask, origin, reviewed });
}

/**
 * `reviewed`: each part's newest step as the person saw it; a part that drew
 * since is refused `stale`. `rest`: steps that no longer apply stay in the
 * draft as conflicts; without it such an accept is refused `steps_skipped`
 * and the person is asked.
 */
async function accept(parts: string[] | null, reviewed: Record<string, number>, rest: boolean, origin?: AskOrigin): Promise<void> {
  const status = useSwarm.getState().status;
  if (!status) return;
  const done = await run("accept", "Accepting", async () => {
    try {
      return { result: await swarmApi().accept(status.id, parts, reviewed, rest), refusal: null };
    } catch (e) {
      const err = toIpcError(e);
      if (err.code !== "steps_skipped" || rest) throw e;
      return { result: null, refusal: err };
    }
  });
  if (done?.refusal) {
    await askRest(parts, reviewed, done.refusal, origin);
    return;
  }
  const result = done?.result;
  if (!result) return;
  const app = useApp.getState();
  if (result.state) app.setDoc(result.state);
  const names = nameList(status, result.applied.map((a) => a.part));
  if (result.applied.length > 0) app.toast("success", `Accepted ${names}. Each part is one undo step.`);
  if (result.skipped.length > 0) {
    useSwarm.getState().setNote({ tone: "info", text: `${result.skipped.length} ${result.skipped.length === 1 ? "step no longer applies" : "steps no longer apply"} and stay in the draft as conflicts: ${result.skipped[0].message}` });
  }
}

/** Discard the whole draft, after the inline confirm. */
export function discardAll(origin: AskOrigin = "bar"): void {
  const status = useSwarm.getState().status;
  if (!status) return;
  useSwarm.getState().setAsk({ ...discardAllAsk(status), origin });
}

/** Discard one part, asking first when other parts build on it. */
export async function discardPart(key: string, origin: AskOrigin = "panel"): Promise<void> {
  const status = useSwarm.getState().status;
  if (!status) return;
  const takes = await dependentsOf(key);
  const now = useSwarm.getState().status;
  if (!now) return;
  const ask = discardAsk(now, key, takes);
  if (ask) {
    useSwarm.getState().setAsk({ ...ask, origin });
    return;
  }
  await dropPart(key, false);
}

async function dropPart(key: string, withDependents: boolean): Promise<void> {
  // A null status is an answer too: the draft had nothing else and closed.
  const done = await run("discard", "Discarding a part", async () => ({ status: await swarmApi().discardPart(key, withDependents) }));
  if (done) useSwarm.getState().applyStatus(done.status, true);
}

/** The person said yes to the open ask. */
export async function confirmAsk(ask: Ask): Promise<void> {
  useSwarm.getState().setAsk(null);
  if (ask.kind === "accept") {
    const sw = useSwarm.getState();
    if (!sw.status) return;
    await accept(ask.part === null ? null : ask.parts, ask.reviewed ?? pinsOf(sw.status, sw.seen), ask.rest === true, ask.origin);
    return;
  }
  if (ask.part !== null) {
    await dropPart(ask.part, ask.parts.length > 1);
    return;
  }
  const status = useSwarm.getState().status;
  if (!status) return;
  const done = await run("discard", "Discarding", async () => {
    await swarmApi().discard(status.id);
    return true;
  });
  if (!done) return;
  useSwarm.getState().applyStatus(null, true);
  useApp.getState().toast("info", "Draft discarded. The project is as it was.");
}

export function cancelAsk(): void {
  useSwarm.getState().setAsk(null);
}
