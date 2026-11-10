// Over the canvas while a swarm draft is open (DECISIONS D56, D57): the draft
// bar at the top centre ("The swarm drew 48 elements in a draft", Accept all,
// Discard, Show draft, "Ready for review", a conflicts badge) and, under it,
// the lock's quiet banner. Mounted once over the plan and 3D panes
// (`SwarmOverlay`). Motion: docs/MOTION.md, "Swarm".

import { useShallow } from "zustand/react/shallow";
import { bus } from "../state/bus";
import { Icon } from "../ui/icons";
import { cx } from "../ui/controls";
import { usePresence } from "../ui/motion";
import { useLastTruthy } from "../ui/motionDom";
import { acceptAll, discardAll } from "./actions";
import { AskRow } from "./SwarmPanel";
import { useSwarm, type SwarmUiState } from "./swarmStore";
import type { SwarmStatus } from "./types";
import { LOCK_HINT, draftLine, isOpen, workersOf } from "./view";
import s from "./swarm.module.css";

/** What the bar draws from the store. */
export type BarState = Pick<SwarmUiState, "status" | "showDraft" | "acting" | "ask">;

export function pickBar(st: SwarmUiState): BarState {
  return { status: st.status, showDraft: st.showDraft, acting: st.acting, ask: st.ask };
}

export function SwarmOverlay() {
  const state = useSwarm(useShallow(pickBar));
  return <SwarmOverlayView {...state} />;
}

export function SwarmOverlayView(state: BarState) {
  const p = usePresence(state.status !== null, "panel");
  const shown = useLastTruthy(state.status);
  if (!p.mounted || !shown) return null;
  return (
    <div className={s.overlay} data-stage={p.stage} data-testid="swarm-overlay">
      <DraftBar {...state} status={shown} />
      <LockBanner />
    </div>
  );
}

export function DraftBar({ status, showDraft, acting, ask }: BarState & { status: SwarmStatus }) {
  const mine = ask && ask.origin !== "panel" ? ask : null;
  const ready = status.state === "ready";
  const conflicts = status.conflicts.length;
  const anything = status.added + status.modified + status.removed > 0 || workersOf(status).some(isOpen);
  return (
    <div className={s.bar} role="region" aria-label="Swarm draft" data-testid="swarm-draft-bar">
      <div className={s.barRow}>
        <button type="button" className={s.barText} data-tip="Show the swarm" onClick={() => bus.emit("swarm", { part: null })} data-testid="swarm-draft-line">
          {draftLine(status)}
        </button>
        {ready ? (
          <span className={s.readyPill} data-testid="swarm-ready">
            Ready for review
          </span>
        ) : null}
        {conflicts > 0 ? (
          <span
            className={s.conflicts}
            data-tip={status.conflicts
              .slice(0, 3)
              .map((c) => c.message)
              .join(" ")}
            data-testid="swarm-conflicts"
          >
            <Icon name="warning" size={12} />
            {conflicts} {conflicts === 1 ? "conflict" : "conflicts"}
          </span>
        ) : null}
        <button
          type="button"
          className={cx(s.eye, showDraft && s.eyeOn)}
          aria-pressed={showDraft}
          aria-label={showDraft ? "Hide the draft" : "Show the draft"}
          data-tip={showDraft ? "Hide the draft" : "Show the draft"}
          onClick={() => useSwarm.getState().setShowDraft(!showDraft)}
          data-testid="swarm-show-draft"
        >
          <Icon name={showDraft ? "eye" : "eyeOff"} size={15} />
        </button>
        <button type="button" className={cx(s.btn, s.small, s.primary)} disabled={acting !== null || !anything} onClick={() => void acceptAll()} data-testid="swarm-accept-all">
          {acting === "accept" ? "Accepting" : "Accept all"}
        </button>
        <button type="button" className={cx(s.btn, s.small)} disabled={acting !== null} onClick={() => discardAll("bar")} data-testid="swarm-bar-discard">
          Discard
        </button>
      </div>
      <AskRow ask={mine} acting={acting} compact />
    </div>
  );
}

/** The lock (D57): quiet, under the bar, while a draft is open. It opens the swarm. */
export function LockBanner() {
  return (
    <button type="button" className={s.lock} role="status" onClick={() => bus.emit("swarm", { part: null })} data-testid="swarm-lock">
      <Icon name="lock" size={12} />
      <span>{LOCK_HINT}</span>
    </button>
  );
}
