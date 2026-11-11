// Over the canvas while a swarm draft is open (DECISIONS D56, D57): the draft
// bar at the top centre ("The swarm drew 48 elements in a draft", Accept all,
// Discard all, Show draft, "Ready for review", a conflicts button that opens
// their list) and, under it, the lock's quiet banner. Mounted once over the
// plan and 3D panes (`SwarmOverlay`). Motion: docs/MOTION.md, "Swarm".

import { useEffect, useId, useState } from "react";
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
import { LOCK_HINT, conflictLine, draftLine, hasDraftWork } from "./view";
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

/** The conflicts listed under the bar: at most 3, then how many more. */
const CONFLICTS_SHOWN = 3;

export function DraftBar({ status, showDraft, acting, ask, conflictsOpen = false }: BarState & { status: SwarmStatus; conflictsOpen?: boolean }) {
  const mine = ask && ask.origin !== "panel" ? ask : null;
  const ready = status.state === "ready";
  const conflicts = status.conflicts.length;
  // The conflicts are words a keyboard user needs too: a button opens them under the bar.
  const [listOpen, setListOpen] = useState(conflictsOpen);
  const listId = useId();
  const open = listOpen && conflicts > 0;
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
          <button
            type="button"
            className={cx(s.conflicts, open && s.conflictsOn)}
            aria-expanded={open}
            aria-controls={listId}
            onClick={() => setListOpen((v) => !v)}
            data-testid="swarm-conflicts"
          >
            <Icon name="warning" size={12} />
            {conflicts} {conflicts === 1 ? "conflict" : "conflicts"}
            <Icon name="chevronDown" size={12} className={s.conflictsChevron} />
          </button>
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
        <button type="button" className={cx(s.btn, s.small, s.primary)} disabled={acting !== null || !hasDraftWork(status)} onClick={() => void acceptAll()} data-testid="swarm-accept-all">
          {acting === "accept" ? "Accepting" : "Accept all"}
        </button>
        <button type="button" className={cx(s.btn, s.small)} disabled={acting !== null} onClick={() => discardAll("bar")} data-testid="swarm-bar-discard">
          Discard all
        </button>
      </div>
      {conflicts > 0 ? (
        // Opens by height like an ask. The words stay in the page while closed, hidden from readers.
        <div className={cx(s.ask, open && s.askOpen)} id={listId} aria-hidden={!open} data-testid="swarm-conflict-list">
          <div className={s.askInner}>
            <ul className={s.conflictList}>
              {status.conflicts.slice(0, CONFLICTS_SHOWN).map((c, i) => (
                <li key={`${c.part}-${c.step}-${i}`}>{conflictLine(status, c)}</li>
              ))}
              {conflicts > CONFLICTS_SHOWN ? <li>And {conflicts - CONFLICTS_SHOWN} more.</li> : null}
            </ul>
          </div>
        </div>
      ) : null}
      <AskRow ask={mine} acting={acting} compact />
    </div>
  );
}

/**
 * The lock (D57): quiet, under the bar, while a draft is open. It is a
 * button that opens the swarm; a hidden status line says the lock once,
 * after it comes on, for a screen reader.
 */
export function LockBanner() {
  const [said, setSaid] = useState("");
  useEffect(() => {
    // A live region speaks a change, not what it holds when it mounts.
    const t = window.setTimeout(() => setSaid(LOCK_HINT), 100);
    return () => window.clearTimeout(t);
  }, []);
  return (
    <>
      <button type="button" className={s.lock} onClick={() => bus.emit("swarm", { part: null })} data-testid="swarm-lock">
        <Icon name="lock" size={12} />
        <span>{LOCK_HINT}</span>
      </button>
      <span className={s.srOnly} role="status">
        {said}
      </span>
    </>
  );
}
