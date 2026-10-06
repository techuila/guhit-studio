// The live session in the top bar: everyone else's avatar (click one to see
// their pointer) and the Share button that opens the live session dialog.
import { useMemo } from "react";
import { useShallow } from "zustand/react/shallow";
import { useApp } from "../state/store";
import { Button, cx } from "../ui/controls";
import { useListPresence } from "../ui/useListPresence";
import { Avatar } from "./Avatar";
import { avatarTip, others, possessive, stepAuthor } from "./format";
import { useLive } from "./liveStore";
import { focusParticipant, openShare } from "./session";
import s from "./live.module.css";

/** Avatars shown before the rest fold into "+N". */
const MAX_AVATARS = 4;

export function LiveAvatars() {
  const list = useLive(useShallow((st) => others(st.status)));
  const selfId = useLive((st) => st.status.self_id);
  const shown = useMemo(() => list.slice(0, MAX_AVATARS), [list]);
  const rest = list.slice(MAX_AVATARS);
  const rows = useListPresence(shown, (p) => p.id, "base");
  if (rows.length === 0 && rest.length === 0) return null;
  return (
    <div className={s.avatars} role="group" aria-label="People in the live session">
      {rows.map((row, i) => (
        <button
          key={row.key}
          type="button"
          className={cx(s.avatarButton, row.entering && s.avatarEnter, row.leaving && s.avatarLeave)}
          style={{ zIndex: rows.length - i }}
          data-tip={avatarTip(row.item, selfId)}
          data-tip-side="bottom"
          aria-label={`${avatarTip(row.item, selfId)}. Show ${possessive(row.item.name)} pointer`}
          disabled={row.leaving}
          onClick={() => focusParticipant(row.item.id)}
        >
          <Avatar p={row.item} size={24} />
        </button>
      ))}
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
  const count = useLive((st) => st.status.participants.length);
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
      disabled={!hasDoc}
      onClick={() => openShare()}
    >
      {SHARE_LABEL[mode]}
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
