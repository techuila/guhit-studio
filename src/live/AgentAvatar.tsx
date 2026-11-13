// An agent's avatar (DECISIONS D47): a rounded square in its color with its
// initials, the AI mark, and a status dot: working pulses, looking is a
// ring, idle dims, paused shows a pause bar, stopped a square. People's
// avatars are round (Avatar.tsx), so the shape tells them apart.
import type { CSSProperties } from "react";
import type { AgentActivity } from "../contract/bindings";
import { cx } from "../ui/controls";
import { initials, peerVar } from "./format";
import s from "./agents.module.css";

/** The AI mark: a four-point sparkle, drawn in a 12 by 12 box. */
export function AiMark({ size = 12, className }: { size?: number; className?: string }) {
  return (
    <svg className={className} width={size} height={size} viewBox="0 0 12 12" aria-hidden>
      <path d="M6 .6c.5 2.9 2.2 4.6 5.4 5.4C8.2 6.8 6.5 8.5 6 11.4 5.5 8.5 3.8 6.8.6 6 3.8 5.2 5.5 3.5 6 .6z" fill="currentColor" />
    </svg>
  );
}

export function agentStyle(color: number): CSSProperties {
  return { "--peer": peerVar(color) } as CSSProperties;
}

export function AgentAvatar({
  name,
  color,
  activity,
  size = 24,
  ring = "var(--chrome)",
  className,
}: {
  name: string;
  color: number;
  activity: AgentActivity | null;
  size?: number;
  /** What the avatar sits on: the corner marks wear a ring of it. The top bar's chrome by default. */
  ring?: string;
  className?: string;
}) {
  return (
    <span
      className={cx(s.avatar, className)}
      data-activity={activity ?? undefined}
      style={{ ...agentStyle(color), "--avatar-ring": ring, width: size, height: size, fontSize: Math.round(size * 0.38), borderRadius: Math.round(size * 0.3) } as CSSProperties}
      aria-hidden
    >
      {initials(name)}
      <span className={s.mark}>
        <AiMark size={Math.max(7, Math.round(size * 0.32))} />
      </span>
      {activity ? <StatusDot activity={activity} /> : null}
    </span>
  );
}

/** The status dot: on an avatar's corner, or `inline` in a line of text on a light surface. */
export function StatusDot({ activity, inline, className }: { activity: AgentActivity; inline?: boolean; className?: string }) {
  return (
    <span className={cx(s.status, inline && s.statusInline, className)} data-activity={activity} aria-hidden>
      {activity === "paused" ? (
        <svg width="6" height="6" viewBox="0 0 6 6">
          <path d="M1.2 .6v4.8M4.8 .6v4.8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
        </svg>
      ) : null}
    </span>
  );
}
