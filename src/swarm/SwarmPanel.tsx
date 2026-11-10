// The swarm panel in the AI dock (DECISIONS D56, D57), behind the dock's
// Chat | Swarm switch: the prompt (or, for an MCP swarm, who drives it), the
// estimate and the cap, Start, Pause all, the speed and Skip to the end, a
// row per part with its progress and state, the feed, and per-part Accept
// and Discard. Motion: docs/MOTION.md, "Swarm".
//
// `SwarmPanel` reads the store; `SwarmPanelView` draws from props only, so
// every state renders the same in a test.

import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { useShallow } from "zustand/react/shallow";
import { AgentAvatar } from "../live/AgentAvatar";
import { peerVar } from "../live/format";
import { cx } from "../ui/controls";
import { dur, motionOK } from "../ui/motion";
import { useListPresence } from "../ui/useListPresence";
import { acceptPart, cancelAsk, confirmAsk, discardAll, discardPart, estimateBrief, pauseAll, resumeAll, startSwarm, stopPart } from "./actions";
import { SPEED_MAX, SPEED_MIN } from "./reveal";
import { reveal, useRevealNow, useSwarm, type SwarmUiState } from "./swarmStore";
import type { SwarmPart, SwarmStatus } from "./types";
import { Icon } from "../ui/icons";
import { aiWaitLine, drivingLine, estimateLine, feedRows, isLive, isOpen, leadOf, pillText, priceNote, spendLine, workersOf, type Ask } from "./view";
import s from "./swarm.module.css";

/** How long the brief rests before the estimate is asked for. */
const ESTIMATE_DEBOUNCE_MS = 500;

/** What the panel draws from the store. */
export type PanelState = Pick<SwarmUiState, "status" | "note" | "ask" | "acting" | "brief" | "capUsd" | "estimate" | "speed"> & { focus?: SwarmUiState["focus"] };

export function pickPanel(st: SwarmUiState): PanelState {
  return { status: st.status, note: st.note, ask: st.ask, acting: st.acting, brief: st.brief, capUsd: st.capUsd, estimate: st.estimate, speed: st.speed, focus: st.focus };
}

export interface SwarmPanelProps {
  /** The in-app swarm needs a Claude API key; an MCP swarm does not. */
  hasKey: boolean;
  hasDoc: boolean;
}

export function SwarmPanel({ hasKey, hasDoc }: SwarmPanelProps) {
  const state = useSwarm(useShallow(pickPanel));
  // The estimate follows the brief once typing rests. It spends nothing.
  useEffect(() => {
    if (state.status || !hasKey) return;
    const brief = state.brief;
    const t = window.setTimeout(() => void estimateBrief(brief), ESTIMATE_DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [state.brief, state.status, hasKey]);
  return <SwarmPanelView {...state} hasKey={hasKey} hasDoc={hasDoc} />;
}

export function SwarmPanelView(props: PanelState & SwarmPanelProps) {
  const { status, note, ask, hasKey } = props;
  // An ask shows where it was asked: the whole draft by the controls, a part in the review.
  const mine = ask?.origin === "panel" ? ask : null;
  return (
    <div className={s.panel} data-testid="swarm-panel">
      <BriefSection {...props} ask={mine?.part === null ? mine : null} />
      {note ? (
        <p key={note.text} className={cx(s.note, note.tone === "error" && s.noteError)} role="status">
          {note.text}
        </p>
      ) : null}
      <section className={s.section} aria-label="Swarm parts">
        <div className={s.label}>Swarm</div>
        {status ? <Parts status={status} acting={props.acting} focus={props.focus ?? null} /> : <EmptyParts hasKey={hasKey} />}
      </section>
      {status ? <Feed status={status} /> : null}
      {status ? <Review status={status} acting={props.acting} ask={mine && mine.part !== null ? mine : null} /> : null}
    </div>
  );
}

// ---------------------------------------------------------------- brief

function BriefSection({ status, hasKey, hasDoc, ask, brief, capUsd, estimate, acting, speed }: PanelState & SwarmPanelProps) {
  const now = useRevealNow();
  const driving = status ? drivingLine(status) : null;
  const canStart = !status && hasDoc && hasKey && brief.trim() !== "" && acting === null;
  const paused = status?.state === "paused";
  const running = status?.state === "running" || status?.state === "planning";
  const behind = reveal.behindMs(Math.max(now, performance.now())) > 0;
  const wait = status ? aiWaitLine(status.ai_wait_until, Date.now()) : null;

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && canStart) {
      e.preventDefault();
      void startSwarm();
    }
  };

  return (
    <section className={s.section} aria-label="Brief">
      <div className={s.label}>{driving ? "Your AI, over MCP" : status ? "The brief" : "One prompt, several agents"}</div>
      {driving ? (
        <div className={s.driving} data-testid="swarm-driving">
          <b>{driving}</b>
          {status?.brief ? <span>{status.brief}</span> : null}
        </div>
      ) : status ? (
        <p className={s.briefShown}>{status.brief}</p>
      ) : (
        <textarea
          className={s.prompt}
          rows={3}
          value={brief}
          placeholder={hasKey ? "Two storey house for a 12 x 15 m lot: 3 bedrooms, 2.5 baths, open living and dining, carport" : "Add a Claude API key in the copilot settings to start a swarm here"}
          disabled={!hasDoc || !hasKey || acting !== null}
          aria-label="Brief for the swarm"
          onChange={(e) => useSwarm.getState().setBrief(e.target.value)}
          onKeyDown={onKeyDown}
        />
      )}
      <div className={s.controls}>
        {!status ? (
          <button type="button" className={cx(s.btn, s.primary)} disabled={!canStart} onClick={() => void startSwarm()} data-testid="swarm-start">
            {acting === "start" ? "Starting" : "Start"}
          </button>
        ) : null}
        {status && (running || paused) ? (
          <button type="button" className={s.btn} disabled={acting !== null} onClick={() => void (paused ? resumeAll() : pauseAll())} data-testid="swarm-pause">
            {paused ? "Resume all" : "Pause all"}
          </button>
        ) : null}
        {status ? (
          <label className={s.speed}>
            <span>Speed</span>
            <input
              type="range"
              min={SPEED_MIN}
              max={SPEED_MAX}
              step={0.25}
              value={speed}
              aria-label="Reveal speed"
              onChange={(e) => useSwarm.getState().setSpeed(Number(e.target.value))}
            />
            <span className={s.speedValue}>{speed.toFixed(2).replace(/0$/, "")}x</span>
          </label>
        ) : null}
        {status && behind ? (
          <button type="button" className={cx(s.btn, s.small)} onClick={() => useSwarm.getState().skip()} data-testid="swarm-skip">
            Skip to the end
          </button>
        ) : null}
        {status ? (
          <button type="button" className={cx(s.btn, s.small, s.quiet)} disabled={acting !== null} onClick={() => discardAll("panel")} data-testid="swarm-discard">
            Discard
          </button>
        ) : null}
      </div>
      {!status ? (
        <div className={s.capRow}>
          <span className={s.cost} data-testid="swarm-estimate">
            {estimateLine(estimate, null)}
            <PriceNote note={priceNote(estimate)} />
          </span>
          <label className={s.cap}>
            <span>Stop at US$</span>
            <input
              type="number"
              min={0.5}
              step={0.5}
              value={capUsd ?? 3}
              aria-label="Spending cap in US dollars"
              disabled={!hasKey}
              onChange={(e) => {
                const v = Number(e.target.value);
                useSwarm.getState().setCap(Number.isFinite(v) && v > 0 ? v : null);
              }}
            />
          </label>
        </div>
      ) : (
        <p className={s.cost}>
          {status.source.type === "app" ? (spendLine(status.cost) ?? "Nothing enters the model until you accept") : "Runs on your own AI plan. Nothing enters the model until you accept"}
          {wait ? <span className={s.wait}> · {wait}</span> : null}
          {status.source.type === "app" ? <PriceNote note={priceNote(status.cost)} /> : null}
        </p>
      )}
      {status ? <AskRow ask={ask} acting={acting} /> : null}
    </section>
  );
}

/** "Billed at the highest rate: price unknown", when the price table does not know the model. */
function PriceNote({ note }: { note: string | null }) {
  if (!note) return null;
  return (
    <span className={s.priceNote} data-testid="swarm-price-note">
      <Icon name="warning" size={12} />
      {note}
    </span>
  );
}

function EmptyParts({ hasKey }: { hasKey: boolean }) {
  return (
    <div className={s.empty}>
      <p>A lead agent splits the brief into parts, and each agent draws its part at the same time, with its own cursor. They draw into a draft: nothing enters the model until you accept all of it, one part, or discard it.</p>
      <p>{hasKey ? "Or ask Claude Code or Codex to run a swarm over MCP. It shows here the same way." : "Or ask Claude Code or Codex to run a swarm over MCP, on your own plan. It shows here the same way."}</p>
    </div>
  );
}

// ---------------------------------------------------------------- parts

function Parts({ status, acting, focus }: { status: SwarmStatus; acting: string | null; focus: SwarmUiState["focus"] }) {
  const lead = leadOf(status);
  const workers = workersOf(status);
  const all = lead ? [lead, ...workers] : workers;
  const rows = useListPresence(all, (p) => p.key, "base");
  const most = Math.max(1, ...all.map((p) => p.steps));
  return (
    <div className={s.rows}>
      {rows.map((row, i) => (
        <PartRow
          key={row.key}
          part={row.item}
          status={status}
          acting={acting}
          most={most}
          lead={row.item.key === lead?.key}
          index={i}
          entering={row.entering}
          leaving={row.leaving}
          focusToken={focus?.part === row.key ? focus.token : 0}
        />
      ))}
    </div>
  );
}

function activityOf(part: SwarmPart) {
  switch (part.state) {
    case "working":
      return "working" as const;
    case "paused":
      return "paused" as const;
    case "stopped":
    case "failed":
      return "stopped" as const;
    default:
      return "idle" as const;
  }
}

interface PartRowProps {
  part: SwarmPart;
  status: SwarmStatus;
  acting: string | null;
  most: number;
  lead: boolean;
  index: number;
  entering: boolean;
  leaving: boolean;
  /** Bumps when "show the swarm" asks for this row. */
  focusToken: number;
}

function PartRow({ part, status, acting, most, lead, index, entering, leaving, focusToken }: PartRowProps) {
  const tick = useRevealNow();
  const rowRef = useRef<HTMLDivElement>(null);
  const [washing, setWashing] = useState(false);
  // Asked for by name: it scrolls into view and washes in its color once.
  useEffect(() => {
    if (!focusToken) return;
    rowRef.current?.scrollIntoView?.({ block: "nearest", behavior: motionOK() ? "smooth" : "auto" });
    setWashing(true);
    const t = window.setTimeout(() => setWashing(false), dur("panel") * 2 + 40);
    return () => window.clearTimeout(t);
  }, [focusToken]);
  const now = Math.max(tick, performance.now());
  const seg = reveal.segment(part.key, now);
  // While its cursor works, the line is what it draws; else what the part says.
  const line = seg && seg.id !== null && seg.label ? seg.label : (part.status ?? part.role);
  const revealed = Math.min(part.steps, reveal.revealedSteps(part.key, now));
  const finished = part.state === "done" || part.state === "accepted";
  const landed = finished ? 1 : part.steps / most;
  const shown = part.state === "accepted" ? 1 : finished && part.steps > 0 ? revealed / part.steps : revealed / most;
  return (
    <div
      ref={rowRef}
      className={cx(s.row, entering && s.rowEnter, leaving && s.rowLeave, washing && s.rowFocus)}
      data-state={part.state}
      data-testid={`swarm-part-${part.key}`}
      style={{ "--peer": peerVar(part.color), "--stagger": `${Math.min(index, 7) * 30}ms` } as CSSProperties}
    >
      <AgentAvatar name={part.name} color={part.color} activity={activityOf(part)} size={26} ring="var(--surface)" />
      <div className={s.who}>
        <b>
          {part.name}
          <span className={s.aiTag}>AI</span>
          {lead && part.name !== "Lead" ? <span className={s.leadTag}>Lead</span> : null}
        </b>
        <small key={line} className={s.line} title={line}>
          {line}
        </small>
        <div className={s.prog} role="progressbar" aria-label={`${part.name}: ${part.steps} steps landed, ${revealed} shown`} aria-valuenow={revealed} aria-valuemax={part.steps}>
          <i className={s.landed} style={{ transform: `scaleX(${Math.min(1, landed)})` }} />
          <i className={s.revealed} style={{ transform: `scaleX(${Math.min(1, shown)})` }} />
        </div>
      </div>
      <div className={s.side}>
        <span key={part.state} className={s.pill} data-state={part.state}>
          {pillText(part, status)}
        </span>
        {!lead && isLive(part) ? (
          <button type="button" className={cx(s.btn, s.small)} disabled={acting !== null} onClick={() => void stopPart(part.key)} data-testid={`swarm-stop-${part.key}`}>
            Stop
          </button>
        ) : null}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- feed

function Feed({ status }: { status: SwarmStatus }) {
  const items = feedRows(status);
  const colorOf = (key: string) => status.parts.find((p) => p.key === key)?.color ?? 0;
  // Rows on screen when the feed mounts do not replay their rise.
  const first = useRef(true);
  useEffect(() => {
    first.current = false;
  }, []);
  return (
    <section className={cx(s.section, s.feedBox)} aria-label="What they did">
      <div className={s.label}>What they did</div>
      {items.length === 0 ? (
        <p className={s.quietLine}>Nothing yet.</p>
      ) : (
        <ul className={s.feed}>
          {items.map((f) => (
            <li key={f.key} className={first.current ? undefined : s.feedIn} style={{ "--peer": peerVar(colorOf(f.part)) } as CSSProperties}>
              <i aria-hidden />
              <span>
                <b>{f.name}</b> {f.text}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

// ---------------------------------------------------------------- review

function Review({ status, ask, acting }: { status: SwarmStatus; ask: Ask | null; acting: string | null }) {
  const lead = leadOf(status);
  const open = [...(lead && isOpen(lead) ? [lead] : []), ...workersOf(status).filter(isOpen)];
  if (open.length === 0) return null;
  return (
    <section className={cx(s.section, s.review)} aria-label="Accept or discard a part">
      <div className={s.label}>Accept one part</div>
      <ul className={s.reviewList}>
        {open.map((p) => (
          <li key={p.key} style={{ "--peer": peerVar(p.color) } as CSSProperties} data-testid={`swarm-review-${p.key}`}>
            <i className={s.swatch} aria-hidden />
            <span className={s.reviewName}>{p.name}</span>
            <span className={s.reviewMeta}>
              {p.steps} {p.steps === 1 ? "step" : "steps"}
            </span>
            <button type="button" className={cx(s.btn, s.small)} disabled={acting !== null} onClick={() => void acceptPart(p.key, "panel")} data-testid={`swarm-accept-${p.key}`}>
              Accept
            </button>
            <button type="button" className={cx(s.btn, s.small, s.quiet)} disabled={acting !== null} onClick={() => void discardPart(p.key, "panel")} data-testid={`swarm-discard-${p.key}`}>
              Discard
            </button>
          </li>
        ))}
      </ul>
      <AskRow ask={ask} acting={acting} />
      <p className={s.quietLine}>Each accepted part is one undo step, with its agent as the author.</p>
    </section>
  );
}

/** At most 3 lines of an ask, then how many more. */
function AskList({ lines, testid }: { lines: string[] | undefined; testid: string }) {
  if (!lines || lines.length === 0) return null;
  return (
    <ul className={s.askDiffers} data-testid={testid}>
      {lines.slice(0, 3).map((d, i) => (
        <li key={i}>{d}</li>
      ))}
      {lines.length > 3 ? <li>And {lines.length - 3} more.</li> : null}
    </ul>
  );
}

/** An Accept or Discard that asks first, inline. Opens by height. */
export function AskRow({ ask, acting, compact }: { ask: Ask | null; acting: string | null; compact?: boolean }) {
  return (
    <div className={cx(s.ask, ask && s.askOpen, compact && s.askCompact)} aria-hidden={!ask}>
      <div className={s.askInner}>
        {ask ? (
          <div className={s.askBody} role="alertdialog" aria-label={ask.confirm} data-testid="swarm-ask">
            <p>{ask.text}</p>
            <AskList lines={ask.skipped} testid="swarm-ask-skipped" />
            <AskList lines={ask.differs} testid="swarm-ask-differs" />
            <div className={s.askActions}>
              <button type="button" className={cx(s.btn, s.small, ask.kind === "discard" ? s.danger : s.primary)} disabled={acting !== null} onClick={() => void confirmAsk(ask)} data-testid="swarm-ask-confirm">
                {ask.confirm}
              </button>
              <button type="button" className={cx(s.btn, s.small, s.quiet)} onClick={cancelAsk}>
                Cancel
              </button>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
