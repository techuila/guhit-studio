// A participant's avatar: initials on their color. The host gets a ring.
import type { CSSProperties } from "react";
import type { Participant } from "../contract/bindings";
import { cx } from "../ui/controls";
import { initials, peerVar } from "./format";
import s from "./live.module.css";

export function peerStyle(color: number): CSSProperties {
  return { "--peer": peerVar(color) } as CSSProperties;
}

export function Avatar({ p, size = 24, className }: { p: Pick<Participant, "name" | "color" | "role">; size?: number; className?: string }) {
  return (
    <span
      className={cx(s.avatar, p.role === "host" && s.avatarHost, className)}
      style={{ ...peerStyle(p.color), width: size, height: size, fontSize: Math.round(size * 0.4) }}
      aria-hidden
    >
      {initials(p.name)}
    </span>
  );
}
