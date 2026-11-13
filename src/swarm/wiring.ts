// Connects the swarm store to the backend (DECISIONS D56, D57). Started once
// by the AI dock, which the editor always mounts:
//   - `swarm_status` at start and when the project changes, then the `swarm`
//     app event for every change;
//   - the draft is fetched only when `draft_revision` moves, one fetch in
//     flight at a time; a move during a fetch fetches once more after it; an
//     answer that lands after its swarm ended or was replaced is dropped; the
//     fetch names the revision drawn, and an engine that has nothing newer
//     answers the status alone (`SwarmDraftCurrent`), which keeps the view;
//   - a swarm that starts, or that the window finds open, opens the dock on Swarm;
//   - the `swarm` bus event (the draft bar, the lock banner, a refusal toast)
//     opens the dock on Swarm, at a part's row when it names one.

import { onAppEvent } from "../contract/ipc";
import { useShell } from "../shell/shellStore";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { isDraftCurrent, swarmApi } from "./api";
import { useSwarm } from "./swarmStore";
import type { SwarmStatus } from "./types";


/** What the window does with each status, apart from where statuses come from (tests drive it directly). */
export interface SwarmWiring {
  onStatus: (status: SwarmStatus | null, event: boolean) => void;
  /** Reads `swarm_status` (at start, and when the project changes). */
  read: () => Promise<void>;
  stop: () => void;
}

/** Opens the AI dock on its Swarm tab, at a part's row when given. */
export function showSwarm(part: string | null): void {
  useSwarm.getState().showSwarm(part);
  const shell = useShell.getState();
  if (shell.dockTab !== "copilot" || shell.dockCollapsed) shell.setDockTab("copilot");
}

export function startSwarmWiring(): () => void {
  const w = swarmWiring();
  const offBus = bus.on("swarm", ({ part }) => showSwarm(part));
  const offEvent = onAppEvent((e) => {
    if (e.type === "swarm") w.onStatus(e.swarm, true);
  });
  let project = useApp.getState().doc?.project.id ?? null;
  const offApp = useApp.subscribe((s) => {
    const now = s.doc?.project.id ?? null;
    if (now === project) return;
    project = now;
    w.onStatus(null, false);
    if (now) void w.read();
  });
  if (project) void w.read();
  return () => {
    w.stop();
    offBus();
    offEvent();
    offApp();
  };
}

export function swarmWiring(): SwarmWiring {
  let stopped = false;
  let inFlight = false;
  let fetched = -1;

  const fetchDraft = async (): Promise<void> => {
    if (inFlight || stopped) return;
    const status = useSwarm.getState().status;
    if (!status || status.draft_revision === fetched) return;
    inFlight = true;
    const asked = status.draft_revision;
    // The swarm this fetch is for: an answer that lands after it ended or was replaced is dropped.
    const swarmId = status.id;
    const current = () => useSwarm.getState().status?.id === swarmId;
    // The view drawn, so an engine with nothing newer answers the status alone.
    const shown = useSwarm.getState().draft;
    const drawn = shown && shown.swarm_id === swarmId ? { draftRevision: shown.draft_revision, swarmId } : null;
    try {
      const reply = await swarmApi().draft(useSwarm.getState().lastSeq, drawn);
      if (stopped) return;
      // Dropped when the swarm ended or was replaced meanwhile: `fetched`
      // stays as the new status set it, so the swarm now open is fetched below.
      if (current() && (!reply || reply.swarm_id === swarmId)) {
        fetched = reply ? reply.draft_revision : asked;
        if (reply && isDraftCurrent(reply)) {
          // The view drawn is the newest: only the status moves, never back.
          const now = useSwarm.getState().status;
          if (!now || reply.status.draft_revision >= now.draft_revision) useSwarm.getState().applyStatus(reply.status, true);
        } else if (reply) {
          useSwarm.getState().applyDraft(reply, useApp.getState().doc);
        }
      }
    } catch {
      // The next status change asks again.
      if (current()) fetched = asked;
    } finally {
      inFlight = false;
    }
    // The draft moved while this fetch ran: one more.
    const now = useSwarm.getState().status;
    if (!stopped && now && now.draft_revision !== fetched) void fetchDraft();
  };

  const onStatus = (status: SwarmStatus | null, event: boolean) => {
    const before = useSwarm.getState().status;
    const projectId = useApp.getState().doc?.project.id ?? null;
    // Another project's swarm is not this window's business.
    const mine = status && (projectId === null || status.project_id === projectId) ? status : null;
    useSwarm.getState().applyStatus(mine, event);
    if (!mine) {
      fetched = -1;
      return;
    }
    if (before?.id !== mine.id) fetched = -1;
    // A swarm started, or one found drafting: show it. The model is locked, so the panel is where to act.
    if (!before || before.id !== mine.id) showSwarm(null);
    void fetchDraft();
  };

  const read = async () => {
    try {
      const status = await swarmApi().status();
      if (!stopped) onStatus(status, false);
    } catch {
      // An engine before swarms: nothing to show.
    }
  };

  return {
    onStatus,
    read,
    stop: () => {
      stopped = true;
    },
  };
}
