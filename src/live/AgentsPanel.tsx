// The Agents panel (DECISIONS D47): every agent working on the project, with
// its name (rename in place), client, owner, status and steps, its limit as
// chips (levels, the selection, kinds of element), and Pause, Stop, Resume,
// Follow, "Steps in History" and "Show on plan". Pause all and Stop all sit
// on top. A second tab, "What they changed", is the review (AgentChanges.tsx).
//
// Not modal: it floats at the top right under the top bar, so the plan and
// 3D stay usable while agents work. Esc inside it and the close button close
// it; a click on the plan does not. Opened from the top bar's agents button,
// the Connect agent dialog and the command palette. Mounted by LiveRoot.

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { AgentLimit, ElementKind } from "../contract/bindings";
import { useConnect } from "../shell/connect/connectStore";
import { useApp } from "../state/store";
import { Button, IconButton, Segmented, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import type { PresenceStage } from "../ui/motion";
import { ConfirmDialog } from "../ui/Dialog";
import { Presence, useLastTruthy } from "../ui/motionDom";
import { relativeTime } from "../shell/history";
import { useListPresence } from "../ui/useListPresence";
import { AgentChanges } from "./AgentChanges";
import { limitAgent, pauseAgent, renameAgent, resumeAgent, showSteps, showWork, stopAgent } from "./agentActions";
import { AgentAvatar, StatusDot, agentStyle } from "./AgentAvatar";
import {
  KIND_LABEL,
  LIMIT_KINDS,
  clientName,
  controlsFor,
  limitChips,
  ownerLine,
  statusText,
  stepsText,
  toggleKind,
  undoLineText,
  withPart,
  withoutPart,
  type AgentView,
} from "./agents";
import { useAgents, type AgentsTab } from "./agentsStore";
import { toggleFollow } from "./follow";
import { useLive } from "./liveStore";
import s from "./AgentsPanel.module.css";

/** One empty list, so a store selector without a project returns the same value each time. */
const NO_LEVELS: never[] = [];

const TABS: Array<{ value: AgentsTab; label: string }> = [
  { value: "agents", label: "Agents" },
  { value: "changes", label: "What they changed" },
];

export function AgentsPanelRoot() {
  const open = useAgents((st) => st.panel !== null);
  return (
    <>
      <Presence open={open} exit="base">{(stage) => <AgentsPanel stage={stage} />}</Presence>
      <UndoAsk />
    </>
  );
}

/**
 * Closes the panel and puts the focus back where it came from: the top
 * bar's Agents button, as History returns it to its clock, else what had
 * the focus before (the palette, the Connect dialog).
 */
function closeAndRefocus(before: HTMLElement | null): void {
  useAgents.getState().closePanel();
  const button = document.querySelector<HTMLElement>('[data-testid="agents-panel-button"]');
  const target = button ?? (before && before.isConnected ? before : null);
  target?.focus({ preventScroll: true });
}

function AgentsPanel({ stage }: { stage: PresenceStage }) {
  const tab = useAgents((st) => st.panel) ?? "agents";
  const lastTab = useRef<AgentsTab>(tab);
  if (stage !== "exit") lastTab.current = tab;
  const panel = useRef<HTMLDivElement>(null);
  const before = useRef<HTMLElement | null>(null);
  const exiting = stage === "exit";

  useEffect(() => {
    before.current = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
    panel.current?.focus({ preventScroll: true });
  }, []);

  return (
    <div
      ref={panel}
      className={s.panel}
      data-stage={stage}
      role="dialog"
      aria-label="Agents"
      tabIndex={-1}
      data-testid="agents-panel"
      onKeyDown={(e) => {
        if (e.key !== "Escape" || exiting) return;
        e.preventDefault();
        e.stopPropagation();
        closeAndRefocus(before.current);
      }}
    >
      <header className={s.head}>
        <Icon name="agent" size={16} className={s.headMark} />
        <h2 className={s.title}>Agents</h2>
        <div className={s.tabs}>
          <Segmented label="Agents panel" options={TABS} value={lastTab.current} onChange={(v) => useAgents.getState().openPanel(v)} />
        </div>
        <IconButton icon="close" label="Close agents" tip="Close (Esc)" tipSide="bottom-end" size={15} className={s.close} onClick={() => closeAndRefocus(before.current)} />
      </header>
      {/* A tab change cross-fades the content in place (opacity only). */}
      <div className={s.body} key={lastTab.current}>
        {lastTab.current === "agents" ? <AgentsList /> : <AgentChanges />}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- undo to here

/**
 * The review's "Undo to here" asks first and names every author whose
 * steps go with it, another agent's later steps among them. The answer
 * stands for the engine's own question (`other_author`), so it is asked
 * once; it carries the revision the steps were counted at, so a step that
 * lands meanwhile is never taken back unseen (DECISIONS D38).
 */
function UndoAsk() {
  const ask = useAgents((st) => st.undoAsk);
  const last = useLastTruthy(ask);
  const close = () => useAgents.getState().askUndo(null);
  const steps = last?.steps ?? 1;
  return (
    <Presence open={ask !== null} exit="panel">
      {(stage) =>
        last ? (
          <ConfirmDialog
            title={steps === 1 ? "Undo 1 step?" : `Undo ${steps} steps?`}
            message={
              <>
                <span className={s.askLead}>{steps === 1 ? "This takes back:" : "This takes back, as one step:"}</span>
                {last.lines.map((line) => (
                  <span key={line.key} className={s.askLine} data-testid="undo-ask-line">
                    {undoLineText(line)}
                  </span>
                ))}
              </>
            }
            confirmLabel={steps === 1 ? "Undo it" : `Undo ${steps} steps`}
            onCancel={close}
            onConfirm={() => {
              close();
              // Answered here, so the engine's "someone else's step" question is not asked again.
              useApp.setState({ undoConfirm: { redo: false, message: "", steps, revision: last.revision ?? undefined } });
              void useApp.getState().resolveUndoConfirm(true);
            }}
            stage={stage}
          />
        ) : null
      }
    </Presence>
  );
}

// ---------------------------------------------------------------- the list

function AgentsList() {
  const views = useAgents((st) => st.views);
  const available = useAgents((st) => st.available);
  const local = useMemo(() => views.filter((a) => a.local), [views]);
  // The rows show as one block when the panel opens (MOTION.md rule 8); an agent that comes later rises alone.
  const rows = useListPresence(views, (a) => a.id, "base");
  const canPause = local.some((a) => controlsFor(a).pause);
  const canStop = local.some((a) => controlsFor(a).stop);
  const canResume = local.some((a) => controlsFor(a).resume);

  if (views.length === 0) {
    return (
      <div className={s.empty}>
        <span className={s.emptyMark} aria-hidden>
          <Icon name="agent" size={22} />
        </span>
        <p className={s.emptyTitle}>No agents yet</p>
        <p className={s.emptyText}>
          {available
            ? "Connect your own AI agent. Each one shows here and on the plan with its name, cursor, status and steps, and you can pause, stop or limit it."
            : "This app does not report agents. Update Guhit Studio to see agents here."}
        </p>
        <Button variant="primary" size="sm" icon="link" onClick={() => useConnect.getState().openDialog()}>
          Connect agent
        </Button>
      </div>
    );
  }

  return (
    <>
      {local.length > 0 ? (
        <div className={s.toolbar}>
          <Button size="sm" disabled={!canPause} onClick={() => void pauseAgent(null)} data-tip="Their edits are refused until you resume them. Reading still works" data-tip-side="bottom-start">
            <PauseGlyph />
            Pause all
          </Button>
          <Button size="sm" disabled={!canStop} onClick={() => void stopAgent(null)} data-tip="They cannot read or edit until you resume them" data-tip-side="bottom">
            <StopGlyph />
            Stop all
          </Button>
          {canResume ? (
            <Button size="sm" variant="ghost" onClick={() => void resumeAgent(null)}>
              <PlayGlyph />
              Resume all
            </Button>
          ) : null}
          <button type="button" className={s.linkButton} onClick={() => useConnect.getState().openDialog()}>
            Add another agent
          </button>
        </div>
      ) : null}
      <ul className={s.list} aria-label="Agents">
        {rows.map((row) => (
          <AgentRow key={row.key} agent={row.item} leaving={row.leaving} />
        ))}
      </ul>
      <p className={s.foot}>
        Agents use their own subscription or AI model. Each edit is one undo step with the agent as its author. A version is saved before agents start.
      </p>
    </>
  );
}

function PauseGlyph() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden className={s.glyph}>
      <path d="M4 2.5v7M8 2.5v7" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

function StopGlyph() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden className={s.glyph}>
      <rect x="2.75" y="2.75" width="6.5" height="6.5" rx="1.2" fill="currentColor" />
    </svg>
  );
}

function PlayGlyph() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden className={s.glyph}>
      <path d="M3.5 2.4v7.2L9.6 6z" fill="currentColor" strokeLinejoin="round" />
    </svg>
  );
}

function AgentRow({ agent, leaving }: { agent: AgentView; leaving: boolean }) {
  const selfId = useLive((st) => st.status.self_id);
  const following = useLive((st) => st.following === agent.id);
  const levels = useApp((st) => st.doc?.project.levels ?? null);
  const elements = useApp((st) => st.doc?.project.elements ?? null);
  // What the open plan has: a limit naming another project's levels or elements shows no chip for them.
  const elementIds = useMemo(() => new Set((elements ?? []).map((e) => e.id)), [elements]);
  const [limitOpen, setLimitOpen] = useState(false);
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const t = window.setInterval(() => setNow(new Date()), 30_000);
    return () => window.clearInterval(t);
  }, []);
  const controls = controlsFor(agent);
  const chips = limitChips(agent.limit, levels ?? [], elementIds);
  const client = clientName(agent);
  const when = agent.lastCallAt ? relativeTime(agent.lastCallAt, now) : "";
  const steps = stepsText(agent.steps);
  return (
    <li
      className={cx(s.row, leaving && s.rowLeave)}
      style={agentStyle(agent.color)}
      data-activity={agent.activity}
      data-testid={`agent-row-${agent.id}`}
      inert={leaving}
    >
      <div className={s.rowInner}>
        <div className={s.rowTop}>
          <AgentAvatar name={agent.name} color={agent.color} activity={agent.activity} size={32} ring="var(--surface)" />
          <div className={s.who}>
            <NameField agent={agent} />
            <span className={s.whoMeta}>
              {client ? <span className={s.client}>{client}{agent.clientVersion ? ` ${agent.clientVersion}` : ""}</span> : null}
              <span>{ownerLine(agent, selfId)}</span>
            </span>
          </div>
          <IconButton
            icon="eye"
            label={following ? `Stop following ${agent.name}` : `Follow ${agent.name}`}
            tip={following ? "Stop following" : "Follow: take its level, glide to its work"}
            tipSide="bottom-end"
            size={15}
            active={following}
            className={s.follow}
            onClick={() => toggleFollow(agent.id)}
          />
        </div>
        <div className={s.statusLine}>
          <StatusDot activity={agent.activity} inline />
          <span key={statusText(agent)} className={s.statusText}>
            {statusText(agent)}
          </span>
          {when || steps ? (
            <span className={s.statusMeta}>
              {when ? <span>{when}</span> : null}
              {steps ? <span>{steps}</span> : null}
            </span>
          ) : null}
        </div>
        {chips.length > 0 || agent.local ? (
          <div className={s.limits}>
            {chips.map((c) => (
              // Focusable, so the keyboard shows the tip too; the tip is also in the text for screen readers.
              <span key={c.key} className={s.limitChip} data-tip={c.tip} data-tip-side="top" tabIndex={0}>
                {c.label}
                <span className={s.srOnly}>. {c.tip}</span>
                {agent.local ? (
                  <button type="button" className={s.limitRemove} aria-label={`Remove the limit ${c.label}`} onClick={() => void limitAgent(agent.id, withoutPart(agent.limit, c.key))}>
                    <Icon name="close" size={10} />
                  </button>
                ) : null}
              </span>
            ))}
            {agent.local ? (
              <button type="button" className={cx(s.limitAdd, limitOpen && s.limitAddOn)} aria-expanded={limitOpen} onClick={() => setLimitOpen((v) => !v)}>
                <Icon name={limitOpen ? "chevronUp" : "plus"} size={11} />
                {chips.length > 0 ? "Change limit" : "Limit"}
              </button>
            ) : null}
            {chips.length === 0 ? <span className={s.noLimit}>Can change anything</span> : null}
          </div>
        ) : null}
        {agent.local ? <LimitEditor agent={agent} open={limitOpen} /> : null}
        <div className={s.actions}>
          {controls.pause ? (
            <Button size="sm" onClick={() => void pauseAgent(agent.id)} data-tip="Its edits are refused until you resume it. Reading still works" data-tip-side="top-start">
              <PauseGlyph />
              Pause
            </Button>
          ) : null}
          {controls.resume ? (
            <Button size="sm" variant="primary" onClick={() => void resumeAgent(agent.id)}>
              <PlayGlyph />
              Resume
            </Button>
          ) : null}
          {controls.stop ? (
            <Button size="sm" variant="ghost" onClick={() => void stopAgent(agent.id)} data-tip="It cannot read or edit until you resume it" data-tip-side="top">
              <StopGlyph />
              Stop
            </Button>
          ) : null}
          {!agent.local ? <span className={s.remote}>Only its owner can pause it</span> : null}
          <span className={s.actionsEnd}>
            <button type="button" className={s.linkButton} onClick={() => showSteps(agent.id)}>
              Steps in History
            </button>
            <button type="button" className={s.linkButton} onClick={() => void showWork(agent.id)}>
              Show on plan
            </button>
          </span>
        </div>
      </div>
    </li>
  );
}

function NameField({ agent }: { agent: AgentView }) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(agent.name);
  const input = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  useLayoutEffect(() => {
    if (!editing) return;
    done.current = false;
    input.current?.focus();
    input.current?.select();
  }, [editing]);
  if (!agent.local) return <span className={s.name}>{agent.name}</span>;
  if (!editing) {
    return (
      <button
        type="button"
        className={s.nameButton}
        data-tip="Rename"
        data-tip-side="top-start"
        onClick={() => {
          setValue(agent.name);
          setEditing(true);
        }}
      >
        <span className={s.name}>{agent.name}</span>
        <Icon name="pencil" size={11} className={s.namePencil} />
      </button>
    );
  }
  const commit = async () => {
    if (done.current) return;
    done.current = true;
    await renameAgent(agent.id, value);
    setEditing(false);
  };
  return (
    <input
      ref={input}
      className={s.nameInput}
      value={value}
      maxLength={40}
      aria-label={`New name for ${agent.name}`}
      spellCheck={false}
      onChange={(e) => setValue(e.target.value)}
      onBlur={() => void commit()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") void commit();
        else if (e.key === "Escape") {
          done.current = true;
          setEditing(false);
        }
      }}
    />
  );
}

/** Where the agent may work: levels, the selection, kinds. Each change applies at once. */
function LimitEditor({ agent, open }: { agent: AgentView; open: boolean }) {
  const levels = useApp((st) => st.doc?.project.levels ?? NO_LEVELS);
  const elements = useApp((st) => st.doc?.project.elements ?? null);
  const selection = useApp((st) => st.selection);
  const limit = agent.limit;
  const set = (next: AgentLimit | null) => void limitAgent(agent.id, next);
  const levelOn = (id: string) => limit?.level_ids.includes(id) ?? false;
  const kindOn = (k: ElementKind) => limit?.kinds.includes(k) ?? false;
  // Only elements the open plan has: right after a project switch the limit may still name the last one's.
  const ids = useMemo(() => {
    const here = new Set((elements ?? []).map((e) => e.id));
    return (limit?.ids ?? []).filter((id) => here.has(id));
  }, [limit, elements]);
  return (
    <div className={cx(s.reveal, open && s.revealOpen)} inert={!open}>
      <div className={s.revealClip}>
        <div className={s.limitEditor}>
          {levels.length > 0 ? (
            <div className={s.limitGroup}>
              <span className={s.limitLabel}>Levels</span>
              <div className={s.toggles}>
                {[...levels]
                  .sort((x, y) => y.elevation_mm - x.elevation_mm)
                  .map((l) => (
                    <button
                      key={l.id}
                      type="button"
                      className={cx(s.toggle, levelOn(l.id) && s.toggleOn)}
                      aria-pressed={levelOn(l.id)}
                      onClick={() => set(withPart(limit, { level_ids: levelOn(l.id) ? (limit?.level_ids ?? []).filter((x) => x !== l.id) : [...(limit?.level_ids ?? []), l.id] }))}
                    >
                      {l.name}
                    </button>
                  ))}
              </div>
            </div>
          ) : null}
          <div className={s.limitGroup}>
            <span className={s.limitLabel}>Selection</span>
            <div className={s.toggles}>
              <button
                type="button"
                className={s.toggle}
                disabled={selection.length === 0}
                data-tip={selection.length === 0 ? "Select elements on the plan first" : "Only what is selected now, and what it reaches"}
                data-tip-side="top"
                onClick={() => set(withPart(limit, { ids: [...selection] }))}
              >
                {selection.length === 0 ? "Use the selection" : `Use the selection (${selection.length})`}
              </button>
              {ids.length > 0 ? (
                <button type="button" className={cx(s.toggle, s.toggleOn)} aria-pressed onClick={() => set(withPart(limit, { ids: [] }))}>
                  {ids.length === 1 ? "1 element" : `${ids.length} elements`}
                  <Icon name="close" size={10} />
                </button>
              ) : null}
            </div>
          </div>
          <div className={s.limitGroup}>
            <span className={s.limitLabel}>Kinds</span>
            <div className={s.toggles}>
              {LIMIT_KINDS.map((k) => (
                <button key={k} type="button" className={cx(s.toggle, kindOn(k) && s.toggleOn)} aria-pressed={kindOn(k)} onClick={() => set(toggleKind(limit, k))}>
                  {KIND_LABEL[k]}
                </button>
              ))}
            </div>
          </div>
          <p className={s.limitHint}>
            An edit outside the limit is refused, and the agent is told why, such as "Only the selection". With nothing picked, it can change anything.
            {limit ? (
              <>
                {" "}
                <button type="button" className={s.linkButton} onClick={() => set(null)}>
                  Clear the limit
                </button>
              </>
            ) : null}
          </p>
        </div>
      </div>
    </div>
  );
}
