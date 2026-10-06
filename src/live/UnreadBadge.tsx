// Messages from others that arrived while the Chat tab was not on screen.
// Pops in, bumps on each new message, and shrinks away once read.
import { usePresence } from "../ui/motion";
import { useLastTruthy } from "../ui/motionDom";
import { useLive } from "./liveStore";
import s from "./live.module.css";

export function UnreadBadge() {
  const unread = useLive((st) => st.unread);
  const shown = useLastTruthy(unread > 0 ? unread : null);
  const p = usePresence(unread > 0, "hover");
  if (!p.mounted || shown === null) return null;
  return (
    <span className={s.unread} data-stage={p.stage} aria-label={`${shown} unread ${shown === 1 ? "message" : "messages"}`}>
      {/* Keyed by the count, so each new message bumps it once. */}
      <span key={shown} className={s.unreadCount}>
        {shown > 99 ? "99+" : shown}
      </span>
    </span>
  );
}
