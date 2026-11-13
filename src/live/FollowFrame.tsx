// Follow mode on screen (DECISIONS D37): a 2 px frame in the followed
// person's color around the views and the "Following Ana" pill with Stop and
// the Esc hint, "Ana is walking" while they walk or fly in 3D. Mounted in the
// editor's canvas area (EditorShell `CanvasArea`). Only the Stop button takes
// the pointer: the views keep every other click, and a click on them stops
// following. The pill stands on the bottom edge at the right in every view
// mode, the one corner no view uses while you follow (the 3D toolbar is at
// the top, the hints and toasts at the bottom left and middle, the walk
// controls only while walking, which stops following).
import { cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import { usePresence } from "../ui/motion";
import { useLastTruthy } from "../ui/motionDom";
import { Avatar, peerStyle } from "./Avatar";
import { unfollow } from "./follow";
import { firstName, navLine, participant } from "./format";
import { useLive } from "./liveStore";
import s from "./FollowFrame.module.css";

export function FollowFrame() {
  const p = useLive((st) => participant(st.status, st.following));
  const nav = useLive((st) => (st.following ? (st.peers[st.following]?.presence.camera?.nav ?? null) : null));
  const shown = useLastTruthy(p);
  const presence = usePresence(p !== null, "base");
  // The nav line keeps its text while it fades out.
  const navText = shown ? navLine(shown.name, nav) : null;
  const lastNav = useLastTruthy(navText);
  if (!presence.mounted || !shown) return null;
  const name = firstName(shown.name);
  return (
    <div className={s.root} data-stage={presence.stage} style={peerStyle(shown.color)}>
      <div className={s.frame} aria-hidden />
      <div className={s.pill} role="status" aria-label={`Following ${name}. Press Escape to stop`}>
        <Avatar p={shown} size={20} />
        <span className={s.text}>
          <span className={s.title}>Following {name}</span>
          <span className={cx(s.nav, navText !== null && s.navOn)} aria-hidden={navText === null}>
            <span className={s.navBody}>
              <Icon name={lastNav?.endsWith("flying") ? "fly" : "walk"} size={12} />
              {navText ?? lastNav}
            </span>
          </span>
        </span>
        <button type="button" className={s.stop} onClick={() => unfollow()} data-tip="Stop following (Esc)" data-tip-side="top-end">
          Stop
        </button>
        <kbd className={s.key} aria-hidden>
          Esc
        </kbd>
      </div>
    </div>
  );
}
