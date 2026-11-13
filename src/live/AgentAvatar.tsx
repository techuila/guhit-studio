// An agent's avatar (DECISIONS D47): a rounded square in its color with its
// initials and a status dot. Working is the "on" color with a steady ring,
// looking is a hollow ring, idle a grey dot, paused a pause bar, stopped a
// square. The avatar keeps its color and initials in every state. People's
// avatars are round (Avatar.tsx), so the shape alone tells them apart: no
// corner mark, no sparkle (tokens.css header).
import type { CSSProperties } from "react";
import type { AgentActivity } from "../contract/bindings";
import { cx } from "../ui/controls";
import { initials, peerVar } from "./format";
import s from "./agents.module.css";

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
  /** What the avatar sits on: the status dot wears a ring of it. The top bar's chrome by default. */
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
