// The live session in the top bar: everyone else's avatar (click one to
// follow them, DECISIONS D37; again to stop) and the Share button that opens
// the live session dialog. The person followed wears a ring in their color;
// someone who follows you wears an eye mark.
//
// After the people, the agents working on the project (DECISIONS D47), with
// or without a live session: rounded squares with a status dot (the shape
// tells them from people). Clicking one follows it too. The last button,
// with the agent mark, opens the Agents panel.
import { useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { useShallow } from "zustand/react/shallow";
import { useApp } from "../state/store";
import { Button, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import { useListPresence } from "../ui/useListPresence";
import { openAgents } from "./agentActions";
import { AgentAvatar, agentStyle } from "./AgentAvatar";
import { agentTip, onBar, type AgentView } from "./agents";
import { useAgents } from "./agentsStore";
import { Avatar, peerStyle } from "./Avatar";
import { toggleFollow } from "./follow";
import { avatarFollowTip, canFollow, followerIds, others, possessive, stepAuthor } from "./format";
import { useLive } from "./liveStore";
import { openShare } from "./session";
import s from "./live.module.css";
import a from "./agents.module.css";

/** Avatars shown before the rest fold into "+N". */
const MAX_AVATARS = 4;

/** Agent avatars shown before the rest fold into the Agents button's count. */
const MAX_AGENTS = 4;

/** People, then agents. */
export function LiveAvatars() {
  return (
    <>
      <PeopleAvatars />
      <AgentAvatars />
    </>
  );
}

/** The agents on the bar: working lately, paused, or another computer's in the session. */
function barAgents(views: readonly AgentView[], now: number): AgentView[] {
  return views.filter((x) => onBar(x, now));
}

/** The top bar's last fold level (TopBar `useFoldToFit`, chrome.module.css `data-fold`). */
const MAX_FOLD = 2;

/**
 * The agents arrive after the top bar measured itself, so they make room
 * the way the bar does: first its words fold away (`data-fold`, one step at
 * a time, as `useFoldToFit` would), and only when nothing is left to fold
 * do the agents fold too, into the Agents button with a small square per
 * agent. They unfold when the room comes back.
 */
function useCompact(ref: RefObject<HTMLDivElement | null>, key: string): boolean {
  const [compact, setCompact] = useState(false);
  const state = useRef({ compact: false, full: 0 });
  useLayoutEffect(() => {
    const el = ref.current;
    const bar = el?.parentElement;
    if (!el || !bar) return;
    let frame = 0;
    const slack = () => {
      const first = bar.firstElementChild;
      return first ? first.getBoundingClientRect().left - bar.getBoundingClientRect().left : 0;
    };
    const fold = () => Number(bar.dataset.fold ?? 0) || 0;
    const fit = () => {
      frame = 0;
      const st = state.current;
      const width = el.getBoundingClientRect().width;
      if (!st.compact) {
        st.full = width;
        while (slack() < -0.5 && fold() < MAX_FOLD) bar.dataset.fold = String(fold() + 1);
        if (slack() < -0.5) setCompact((st.compact = true));
        return;
      }
      const need = st.full - width + 2;
      if (slack() >= need) {
        setCompact((st.compact = false));
        return;
      }
      // The bar may have unfolded its words since: folding them is better than folding the agents.
      const was = fold();
      if (was < MAX_FOLD) {
        bar.dataset.fold = String(MAX_FOLD);
        if (slack() >= need) setCompact((st.compact = false));
        else bar.dataset.fold = String(was);
      }
    };
    // After the top bar's own fold has run this frame.
    const later = () => {
      if (!frame) frame = requestAnimationFrame(fit);
    };
    later();
    const ro = new ResizeObserver(later);
    ro.observe(bar);
    window.addEventListener("resize", later);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", later);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [ref, key]);
  return compact;
}

function AgentAvatars() {
  const list = useAgents(useShallow((st) => barAgents(st.views, Date.now())));
  const total = useAgents((st) => st.views.length);
  const panelOpen = useAgents((st) => st.panel !== null);
  const selfId = useLive((st) => st.status.self_id);
  const following = useLive((st) => st.following);
  const hasPeople = useLive((st) => others(st.status).length > 0);
  const people = useLive((st) => others(st.status).length);
  const groupRef = useRef<HTMLDivElement>(null);
  const compact = useCompact(groupRef, `${list.length}:${people}`);
  const shown = useMemo(() => (compact ? [] : list.slice(0, MAX_AGENTS)), [list, compact]);
  const rows = useListPresence(shown, (x) => x.id, "base");
  if (total === 0) return null;
  const more = list.length - shown.length;
  return (
    <div ref={groupRef} className={cx(a.group, hasPeople && a.groupAfterPeople)} role="group" aria-label="Agents working on the plan">
      {rows.map((row, i) => {
        const followed = following === row.item.id;
        const tip = agentTip(row.item, selfId, followed);
        return (
          <button
            key={row.key}
            type="button"
            className={cx(a.agentButton, followed && a.agentFollowed, row.entering && a.agentEnter, row.leaving && a.agentLeave)}
            style={{ ...agentStyle(row.item.color), zIndex: followed ? rows.length + 1 : rows.length - i }}
            data-tip={tip}
            data-tip-side="bottom"
            aria-label={tip}
            aria-pressed={followed}
            disabled={row.leaving}
            data-testid={`agent-avatar-${row.item.id}`}
            onClick={() => toggleFollow(row.item.id)}
          >
            <AgentAvatar name={row.item.name} color={row.item.color} activity={row.item.activity} size={22} />
          </button>
        );
      })}
      <button
        type="button"
        className={cx(a.panelButton, panelOpen && a.panelButtonOn)}
        data-tip={more > 0 ? `${more} more. Agents: pause, stop, limit and review them` : "Agents: pause, stop, limit and review them"}
        data-tip-side="bottom-end"
        aria-label={`Agents panel, ${total} ${total === 1 ? "agent" : "agents"}`}
        aria-expanded={panelOpen}
        data-testid="agents-panel-button"
        onClick={() => (panelOpen ? useAgents.getState().closePanel() : openAgents())}
      >
        <Icon name="agent" size={14} />
        {compact && list.length > 0 ? (
          <span className={a.miniStack} aria-hidden>
            {list.slice(0, MAX_AGENTS).map((x) => (
              <i key={x.id} className={a.mini} data-activity={x.activity} style={agentStyle(x.color)} />
            ))}
          </span>
        ) : null}
        {more > 0 && !compact ? `+${more}` : total}
      </button>
    </div>
  );
}

function PeopleAvatars() {
  const list = useLive(useShallow((st) => others(st.status)));
  const selfId = useLive((st) => st.status.self_id);
  const following = useLive((st) => st.following);
  const followers = useLive(useShallow((st) => followerIds(st.status, st.peers, st.status.self_id)));
  const followable = useLive(useShallow((st) => others(st.status).filter((p) => canFollow(st.peers, p.id)).map((p) => p.id)));
  const shown = useMemo(() => list.slice(0, MAX_AVATARS), [list]);
  const rest = list.slice(MAX_AVATARS);
  const rows = useListPresence(shown, (p) => p.id, "base");
  if (rows.length === 0 && rest.length === 0) return null;
  return (
    <div className={s.avatars} role="group" aria-label="People in the live session">
      {rows.map((row, i) => {
        const followed = following === row.item.id;
        const followsMe = followers.includes(row.item.id);
        const tip = avatarFollowTip(row.item, selfId, followed, followsMe, followable.includes(row.item.id));
        return (
          <button
            key={row.key}
            type="button"
            className={cx(s.avatarButton, followed && s.avatarFollowed, row.entering && s.avatarEnter, row.leaving && s.avatarLeave)}
            style={{ ...peerStyle(row.item.color), zIndex: followed ? rows.length + 1 : rows.length - i }}
            data-tip={tip}
            data-tip-side="bottom"
            aria-label={tip}
            aria-pressed={followed}
            disabled={row.leaving}
            onClick={() => toggleFollow(row.item.id)}
          >
            <Avatar p={row.item} size={24} />
            <i className={s.followMark} data-on={followsMe} aria-hidden>
              <Icon name="eye" size={10} />
            </i>
          </button>
        );
      })}
      {rest.length > 0 ? (
        <button
          type="button"
          className={cx(s.avatarButton, s.avatarMore)}
          data-tip={rest.map((p) => p.name).join(", ")}
          data-tip-side="bottom-end"
          aria-label={`${rest.length} more people in the live session`}
          onClick={() => openShare()}
        >
          +{rest.length}
        </button>
      ) : null}
    </div>
  );
}

/** Short on purpose: the dot's color and the tooltip say the rest. */
const SHARE_LABEL = { off: "Share", hosting: "Share", joined: "Live", reconnecting: "Live" } as const;

export function ShareButton() {
  const mode = useLive((st) => st.status.mode);
  // People only: agents in the session are not people.
  const count = useLive((st) => st.status.participants.filter((p) => !p.agent).length);
  const hostName = useLive((st) => st.status.participants.find((p) => p.role === "host")?.name ?? null);
  const hasDoc = useApp((st) => st.doc !== null);
  const tip =
    mode === "off"
      ? "Work on this plan together, live"
      : mode === "hosting"
        ? `Live session, ${count} ${count === 1 ? "person" : "people"}. Copy the invite`
        : mode === "joined"
          ? `In ${hostName ? possessive(hostName) : "a"} live session`
          : "Reconnecting: the connection to the host dropped. Trying again";
  return (
    <Button
      variant="chrome"
      icon="people"
      className={cx(s.share, mode !== "off" && s.shareLive)}
      data-tip={tip}
      data-tip-side="bottom-end"
      // The label folds away on a narrow bar: the name stays (WCAG 4.1.2).
      aria-label={SHARE_LABEL[mode]}
      disabled={!hasDoc}
      onClick={() => openShare()}
    >
      <span data-fold-label>{SHARE_LABEL[mode]}</span>
      {mode !== "off" ? <i className={s.liveDot} data-mode={mode} aria-hidden /> : null}
    </Button>
  );
}

/**
 * The person whose step an undo (or redo) would take back, when it is
 * someone else in the live session: "Undo Move wall, by Ana".
 */
export function useStepAuthor(which: "undo" | "redo"): string | null {
  const by = useApp((st) => (which === "undo" ? st.doc?.undo_by : st.doc?.redo_by) ?? null);
  return useLive((st) => stepAuthor(st.status, by, st.messages));
}
