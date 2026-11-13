// The Chat button in the top bar, next to the people, with the unread count
// (DECISIONS D41). Shown while a live session runs. Opens Chat in the dock
// and expands the dock; with Chat already on screen it puts the cursor in the
// message field instead. The count is UnreadBadge: it pops in, bumps on each
// new message and shrinks away once read.
import { useEffect, useState } from "react";
import { useShell } from "../shell/shellStore";
import { IconButton, cx } from "../ui/controls";
import { dur } from "../ui/motion";
import { UnreadBadge } from "./UnreadBadge";
import { chatButtonLabel } from "./chatRules";
import { focusChatInput } from "./chatNotify";
import { canChat } from "./format";
import { useLive } from "./liveStore";
import { chatOnScreen } from "./wiring";
import s from "./ChatButton.module.css";

/**
 * True from the render that wants the button until its exit animation has
 * played. It mounts in the same render (no effect in between), so the top
 * bar's fit measuring sees it.
 */
function useKept(show: boolean): boolean {
  const [mounted, setMounted] = useState(show);
  if (show && !mounted) setMounted(true);
  useEffect(() => {
    if (show) return;
    const id = window.setTimeout(() => setMounted(false), Math.round(dur("base") * 0.7) + 30);
    return () => window.clearTimeout(id);
  }, [show]);
  return mounted;
}

function open(): void {
  const wasOnScreen = chatOnScreen();
  useShell.getState().setDockTab("chat");
  if (wasOnScreen) focusChatInput();
}

export function ChatButton() {
  const live = useLive((st) => canChat(st.status.mode));
  const unread = useLive((st) => st.unread);
  const mounted = useKept(live);
  if (!mounted) return null;
  const label = chatButtonLabel(unread);
  return (
    <span className={cx(s.wrap, !live && s.leaving)}>
      <IconButton icon="chat" tone="chrome" label={label} tip={label} disabled={!live} onClick={open} />
      <span className={s.badge}>
        <UnreadBadge />
      </span>
    </span>
  );
}
