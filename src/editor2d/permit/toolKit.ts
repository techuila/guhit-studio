// What the permit plan tools share (lane L1): their own small animator (the
// controller's is not lent to lane tools), a typed number buffer, the click
// ring, the refusal bump and the pills they draw beside the pointer. Motion
// follows docs/MOTION.md: ghosts fade in, a placed point rings once, a
// refusal bumps once beside the pointer, a dropped draft fades out, the
// typed box scales in. Reduced motion jumps to the end state (`dur` is 0).

import { dur, ease } from "../../ui/motion";
import { Anim } from "../anim";
import type { P } from "../geom";
import type { PlanToolHost } from "../planTool";
import type { RenderContext } from "../render";
import { toScreen } from "../view";

/** Exits run at about 70% of the enter (MOTION.md rule 4). */
export const exitMs = (key: Parameters<typeof dur>[0]): number => Math.round(dur(key) * 0.7);

const clock = (): number => (typeof performance !== "undefined" ? performance.now() : Date.now());

/** How long a refusal's reason stays readable beside the pointer, motion or not. */
const REASON_HOLD_MS = 1600;

/** A lane tool's animated values, on the plan's own frame loop through `host.invalidate`. */
export class ToolMotion {
  readonly anim = new Anim();
  /** Where the last placed point rings. */
  ring: P | null = null;
  /** The reason the last click was refused, shown beside the pointer. */
  reason: string | null = null;

  to(key: string, to: number, ms: number, spec: Parameters<Anim["to"]>[3] = {}): void {
    this.anim.setClock(clock());
    this.anim.to(key, to, ms, spec);
  }

  value(key: string, rest = 0): number {
    return this.anim.value(key, rest);
  }

  /** The ghost fades in once, when a draft starts or the pointer first gives it a target. */
  ghostIn(): void {
    if (this.anim.has("ghost")) return;
    this.to("ghost", 1, dur("hover"), { from: 0, easing: ease.out });
  }

  ghostOut(): void {
    this.anim.clear("ghost");
  }

  /** A placed point rings once. */
  pulse(at: P): void {
    this.ring = at;
    this.anim.clear("ring");
    this.to("ring", 0, dur("base"), { from: 1, easing: ease.out, drop: true });
  }

  /** A refused click: the reason bumps once beside the pointer, stays a moment, then fades. */
  refuse(reason: string): void {
    this.reason = reason;
    this.anim.clear("bump");
    this.anim.clear("reason");
    this.to("bump", 1, dur("press"), { from: 1.18, easing: ease.spring });
    // Held for REASON_HOLD_MS, then faded; with reduced motion it is held and then gone.
    const fade = dur("panel");
    const hold = REASON_HOLD_MS / (REASON_HOLD_MS + fade);
    const easing = (x: number): number => (x < hold ? 0 : fade > 0 ? ease.in((x - hold) / (1 - hold)) : 1);
    this.to("reason", 0, REASON_HOLD_MS + fade, { from: 1, easing, drop: true });
  }

  /** The typed box scales in at the pointer. */
  typedIn(): void {
    if (this.anim.has("typed")) return;
    this.to("typed", 1, dur("base"), { from: 0, easing: ease.out });
  }

  typedOut(): void {
    this.anim.clear("typed");
  }

  /** Advances the clock; asks the plan for another frame while anything still moves. */
  frame(host: PlanToolHost): void {
    if (this.anim.tick(clock()) > 0) host.invalidate();
  }

  clear(): void {
    this.anim.clearAll();
    this.ring = null;
    this.reason = null;
  }
}

// ------------------------------------------------------------------ typed numbers

/** A number typed while drawing, in the display unit. Digits, one point, a leading minus. */
export function typeKey(buffer: string, key: string, signed = false): string | null {
  if (/^[0-9]$/.test(key)) return buffer + key;
  if (key === ".") return buffer.includes(".") ? buffer : buffer === "" || buffer === "-" ? `${buffer}0.` : `${buffer}.`;
  if (key === "-" && signed) return buffer === "" ? "-" : buffer;
  if (key === "Backspace") return buffer.slice(0, -1);
  return null;
}

/** The typed value in mm, or null while it is not a number yet. */
export function typedMm(buffer: string, unit: "mm" | "m"): number | null {
  if (buffer === "" || buffer === "-" || buffer === "." || buffer === "-.") return null;
  const v = Number(buffer);
  return Number.isFinite(v) ? v * (unit === "m" ? 1000 : 1) : null;
}

// ------------------------------------------------------------------ drawing

const PILL_H = 18;

/** A dark pill with mono text, as the plan's own readouts. Kept inside the canvas. */
export function pill(rc: RenderContext, at: P, text: string, o: { bg?: string; fg?: string; alpha?: number; scale?: number } = {}): void {
  const { ctx, palette } = rc;
  ctx.save();
  ctx.font = `11px ${palette.fontMono}`;
  const w = ctx.measureText(text).width + 12;
  const x = Math.max(4, Math.min(rc.width - w - 4, at.x));
  const y = Math.max(4, Math.min(rc.height - PILL_H - 4, at.y - PILL_H / 2));
  if (o.scale !== undefined && o.scale !== 1) {
    // Grows from its left edge, where it hangs off the pointer.
    ctx.translate(x, y + PILL_H / 2);
    ctx.scale(o.scale, o.scale);
    ctx.translate(-x, -(y + PILL_H / 2));
  }
  const a = o.alpha ?? 1;
  ctx.globalAlpha = 0.92 * a;
  ctx.fillStyle = o.bg ?? palette.ink;
  ctx.beginPath();
  ctx.roundRect(x, y, w, PILL_H, 4);
  ctx.fill();
  ctx.globalAlpha = a;
  ctx.fillStyle = o.fg ?? "#ffffff";
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  ctx.fillText(text, x + 6, y + PILL_H / 2 + 0.5);
  ctx.restore();
}

/** The click ring and the refusal reason: call at the end of a tool's `draw`. */
export function drawFeedback(rc: RenderContext, m: ToolMotion, cursor: P | null): void {
  const { ctx, palette, view } = rc;
  const ring = m.value("ring", 0);
  if (m.ring && ring > 0.002) {
    const s = toScreen(view, m.ring);
    ctx.save();
    ctx.globalAlpha = ring * 0.9;
    ctx.strokeStyle = palette.selection;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.arc(s.x, s.y, 4 + (1 - ring) * 12, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
  }
  const reason = m.value("reason", 0);
  if (m.reason && reason > 0.002 && cursor) {
    const s = toScreen(view, cursor);
    pill(rc, { x: s.x + 16, y: s.y - 22 }, m.reason, { bg: palette.danger, alpha: reason, scale: m.value("bump", 1) });
  } else if (reason <= 0.002) m.reason = null;
}

/** The typed value's box beside the pointer: what is typed, its unit and what Enter does. */
export function drawTyped(rc: RenderContext, m: ToolMotion, cursor: P | null, label: string, buffer: string, unit: string, help: string): void {
  if (!cursor || buffer === "") return;
  const t = m.value("typed", 1);
  const s = toScreen(rc.view, cursor);
  const text = `${label} ${buffer}${unit === "m" ? " m" : " mm"}   ${help}`;
  // Scales in at the cursor (0.96 to 1) as it fades up.
  pill(rc, { x: s.x + 18, y: s.y + 26 }, text, { bg: rc.palette.selection, alpha: t, scale: 0.96 + 0.04 * t });
}

/** True on macOS, for the shortcut named in hints. */
export function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent || "");
}
