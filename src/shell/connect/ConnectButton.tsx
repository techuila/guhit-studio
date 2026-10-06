// "Connect agent" with a status dot: green when agents can connect, amber when
// allowed but not listening, grey when off. Opens the dialog (DECISIONS D35).
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
      className={s.trigger}
      data-tip={TIPS[dot]}
      data-tip-side={tipSide}
      onClick={() => useConnect.getState().openDialog()}
    >
      <i className={cx(s.dot, s[`dot_${dot}`])} aria-hidden />
      Connect agent
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
