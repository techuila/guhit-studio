// "Connect agent" with a link mark and a status dot: green when agents can
// connect, amber when allowed but not listening, grey when off. Opens the
// dialog (DECISIONS D35). Folded in the top bar, the mark and the dot stay.

import { Button, cx } from "../../ui/controls";
import { dotState, useConnect, useMcpPolling } from "./connectStore";
import s from "./connect.module.css";

const TIPS = {
  on: "Connect an AI agent. Agents can connect now",
  warn: "Connect an AI agent. Agents are allowed but the app is not listening",
  off: "Connect an AI agent",
} as const;

export function ConnectButton({ variant = "default", tipSide = "bottom" }: { variant?: "default" | "chrome"; tipSide?: string }) {
  useMcpPolling(15000);
  const dot = useConnect((st) => dotState(st.status));
  return (
    <Button
      variant={variant}
      icon="link"
      className={s.trigger}
      aria-label="Connect agent"
      data-tip={TIPS[dot]}
      data-tip-side={tipSide}
      onClick={() => useConnect.getState().openDialog()}
    >
      <i className={cx(s.dot, s[`dot_${dot}`])} aria-hidden />
      {/* The top bar folds the words away when it runs out of room. */}
      <span data-fold-label>Connect agent</span>
    </Button>
  );
}

/** The link in the copilot's no-key notes. */
export function ConnectLink() {
  return (
    <button type="button" className={s.linkBtn} onClick={() => useConnect.getState().openDialog()}>
      Use your own AI agent instead
    </button>
  );
}
