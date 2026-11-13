// "Connect an AI agent" (DECISIONS D35): agents down the left, the chosen
// agent's setup on the right, and a status line with the "Allow agents" switch.
// With several agents (DECISIONS D47): who is connected now, with a link to
// the Agents panel, and "Add another agent", a labelled URL per agent
// (`/mcp/agent/<label>`) with the toolsets it asks for.
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { useShallow } from "zustand/react/shallow";
import { toIpcError } from "../../contract/ipc";
import { openAgents } from "../../live/agentActions";
import { AgentAvatar } from "../../live/AgentAvatar";
import { agentTitle, onBar, statusText } from "../../live/agents";
import { useAgents } from "../../live/agentsStore";
import { copyText } from "../../ui/clipboard";
import { Button, Switch, cx } from "../../ui/controls";
import { Dialog } from "../../ui/Dialog";
import type { PresenceStage } from "../../ui/motion";
import { relativeTime } from "../../ui/units";
import { useListPresence } from "../../ui/useListPresence";
import { Icon } from "../../ui/icons";
import {
  AGENTS,
  CORE_TOOLS,
  CURSOR_TOOL_WARNING,
  DOCS_URL,
  FALLBACK_URL,
  GROUPS,
  TOOLSETS,
  agentById,
  agentForClient,
  agentSlug,
  clientLabel,
  labelledUrl,
  sameToolsets,
  serverNameFor,
  setupFor,
  stdioArgsFor,
  toolCount,
  type AgentDef,
  type Setup,
  type SetupContext,
} from "./agents";
import { dotState, useConnect, useMcpPolling } from "./connectStore";
import { openClaudeDesktopBundle, openErrorText, openLink } from "./open";
import s from "./connect.module.css";

function Mark({ agent, big }: { agent: AgentDef; big?: boolean }) {
  return (
    <span className={cx(s.mark, big && s.markBig)} style={{ background: agent.color }} aria-hidden>
      {agent.mark}
    </span>
  );
}

function useTimedFlag(ms: number): [boolean, () => void] {
  const [on, setOn] = useState(false);
  const timer = useRef<number>(0);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  return [
    on,
    () => {
      setOn(true);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setOn(false), ms);
    },
  ];
}

function CodeBlock({ code }: { code: string }) {
  const [copied, flash] = useTimedFlag(1600);
  const [failed, setFailed] = useState(false);
  const pre = useRef<HTMLPreElement>(null);
  const copy = async () => {
    const ok = await copyText(code);
    setFailed(!ok);
    if (!ok && pre.current) {
      const r = document.createRange();
      r.selectNodeContents(pre.current);
      const sel = window.getSelection();
      sel?.removeAllRanges();
      sel?.addRange(r);
    }
    flash();
  };
  return (
    <div className={s.code}>
      <pre ref={pre}>{code}</pre>
      <button type="button" className={cx(s.copy, copied && s.copyOk)} onClick={() => void copy()}>
        {copied ? (failed ? "Selected" : "Copied") : "Copy"}
      </button>
    </div>
  );
}

function ActionButton({ setup }: { setup: Extract<Setup, { kind: "link" | "bundle" }> }) {
  const [done, flash] = useTimedFlag(1800);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const click = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      if (setup.kind === "link") await openLink(setup.link);
      else await openClaudeDesktopBundle();
      flash();
    } catch (e) {
      setError(openErrorText(e instanceof Error ? e : toIpcError(e), setup.app));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className={s.action}>
      <Button variant="primary" className={cx(s.cta, done && s.ctaDone)} icon={done ? "check" : undefined} disabled={busy} onClick={() => void click()}>
        {done ? `Opened in ${setup.app}` : setup.button}
      </Button>
      {error ? (
        <p className={s.error} role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

function Detail({ agent, ctx }: { agent: AgentDef; ctx: SetupContext }) {
  const setup = setupFor(agent.id, ctx);
  return (
    <div className={s.detail} key={agent.id}>
      <div className={s.detailTop}>
        <Mark agent={agent} big />
        <div>
          <h3>{agent.name}</h3>
          <p className={s.sub}>{setup.lead}</p>
        </div>
      </div>
      {setup.kind === "snippet" ? (
        <>
          {setup.where ? (
            <p className={s.where}>
              File: <code>{setup.where}</code>
            </p>
          ) : null}
          <CodeBlock code={setup.code} />
        </>
      ) : (
        <ActionButton setup={setup} />
      )}
      {setup.steps.length > 0 ? (
        <ol className={s.steps}>
          {setup.steps.map((t) => (
            <li key={t}>{t}</li>
          ))}
        </ol>
      ) : null}
      <AnotherAgent agent={agent} ctx={ctx} />
    </div>
  );
}

/**
 * "Add another agent": a name gives a labelled URL, so each agent (or each
 * subagent of one client) is its own participant with its own name, cursor
 * and limits. The toolsets add tool groups beside the core tools.
 */
function AnotherAgent({ agent, ctx }: { agent: AgentDef; ctx: SetupContext }) {
  const [open, setOpen] = useState(false);
  const [label, setLabel] = useState("");
  const defaults = useConnect((st) => st.toolsets) ?? [];
  // Null: this agent gets the defaults ("Tools agents get"). A list of its own goes in its URL or command.
  const [own, setOwn] = useState<string[] | null>(null);
  const toolsets = own ?? defaults;
  const name = label.trim() || "Second agent";
  const url = labelledUrl(ctx.url, name, own);
  const setup = setupFor(agent.id, { ...ctx, url, name: serverNameFor(name), stdioArgs: stdioArgsFor(name, own) });
  const slug = agentSlug(label);
  const toggle = (id: string) => {
    const next = toolsets.includes(id) ? toolsets.filter((x) => x !== id) : [...toolsets, id];
    // Back to the same as the defaults: it follows them again.
    setOwn(sameToolsets(next, defaults) ? null : next);
  };
  return (
    <div className={s.another}>
      <button type="button" className={cx(s.anotherToggle, open && s.anotherToggleOn)} aria-expanded={open} onClick={() => setOpen((v) => !v)} data-testid="connect-add-another">
        <span className={s.anotherPlus} aria-hidden>
          +
        </span>
        Add another agent
        <span className={s.anotherHint}>Its own name, cursor and limits</span>
      </button>
      <div className={cx(s.reveal, open && s.revealOpen)} inert={!open}>
        <div className={s.revealClip}>
          <div className={s.anotherBody}>
            <label className={s.anotherField}>
              <span>Name it</span>
              <input
                className={s.anotherInput}
                value={label}
                maxLength={40}
                placeholder="Upper floor"
                spellCheck={false}
                onChange={(e) => setLabel(e.target.value)}
                onKeyDown={(e) => e.stopPropagation()}
              />
            </label>
            <div className={s.anotherField}>
              <span>Tools</span>
              <div className={s.toolsets} role="group" aria-label="Extra tools for this agent">
                <span className={cx(s.toolset, s.toolsetCore)} data-tip="Walls, rooms, doors, windows, levels, stairs, slabs, objects and reading the plan" data-tip-side="top">
                  Core
                </span>
                {TOOLSETS.map((t) => {
                  const on = toolsets.includes(t.id);
                  return (
                    <button
                      key={t.id}
                      type="button"
                      className={cx(s.toolset, on && s.toolsetOn)}
                      aria-pressed={on}
                      data-tip={`${t.hint} (${t.tools === 1 ? "1 tool" : `${t.tools} tools`})`}
                      data-tip-side="top"
                      onClick={() => toggle(t.id)}
                    >
                      {t.label}
                    </button>
                  );
                })}
              </div>
            </div>
            <p className={s.toolsetNote}>
              {own === null ? "The defaults above, " : "Its own tools, "}
              {toolCount(toolsets)} tools.
              {own !== null ? (
                <>
                  {" "}
                  <button type="button" className={s.linkBtn} onClick={() => setOwn(null)}>
                    Use the defaults
                  </button>
                </>
              ) : null}
            </p>
            {agent.id === "claude-desktop" ? null : (
              <>
                <p className={s.where}>Its address{slug ? "" : " (name it first)"}:</p>
                <CodeBlock code={url} />
              </>
            )}
            <p className={s.sub}>{setup.lead.replace(/\bGuhit\b/, `"${name}"`)}</p>
            {setup.kind === "snippet" && setup.where ? (
              <p className={s.where}>
                File: <code>{setup.where}</code>
              </p>
            ) : null}
            {setup.kind === "snippet" ? <CodeBlock code={setup.code} /> : <ActionButton setup={setup} />}
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * "Tools agents get" (MCP v2): the core tools are always there; each
 * extended toolset is a switch, saved as the default for every agent
 * (`mcp_set_toolsets`). An agent's own `?toolsets=` or `--toolsets` wins.
 * Hidden against an app that cannot say (`mcp_toolsets` fails).
 */
function ToolsetsSection() {
  const toolsets = useConnect((st) => st.toolsets);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    void useConnect.getState().loadToolsets();
  }, []);
  if (toolsets === null) return null;
  const count = toolCount(toolsets);
  const names = TOOLSETS.filter((t) => toolsets.includes(t.id)).map((t) => t.label);
  const summary = names.length === 0 ? "Core only" : `Core, ${names.join(", ")}`;
  const flip = async (id: string, on: boolean) => {
    setError(null);
    const next = on ? [...toolsets, id] : toolsets.filter((x) => x !== id);
    try {
      await useConnect.getState().setToolsets(TOOLSETS.filter((t) => next.includes(t.id)).map((t) => t.id));
    } catch (e) {
      setError(toIpcError(e).message);
    }
  };
  return (
    <div className={s.tools} data-testid="connect-toolsets">
      <button type="button" className={cx(s.toolsHead, open && s.toolsHeadOn)} aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        <span className={s.toolsTitle}>Tools agents get</span>
        <span className={s.toolsSummary}>
          {summary}: <strong key={count} className={s.toolsCount}>{count} tools</strong>
        </span>
        <Icon name="chevronDown" size={14} className={s.toolsChevron} />
      </button>
      <div className={cx(s.reveal, open && s.revealOpen)} inert={!open}>
        <div className={s.revealClip}>
          <div className={s.toolsBody}>
            <div className={s.toolRow}>
              <Switch checked disabled label="Core tools, always on" onChange={() => undefined} />
              <span className={s.toolText}>
                <span className={s.toolName}>Core</span>
                <span className={s.toolHint}>Draft and check the plan: walls, rooms, openings, stairs, slabs, objects, levels, history</span>
              </span>
              <span className={s.toolNum}>{CORE_TOOLS}</span>
            </div>
            {TOOLSETS.map((t) => {
              const on = toolsets.includes(t.id);
              return (
                <div key={t.id} className={s.toolRow}>
                  <Switch checked={on} label={`${t.label} tools for every agent`} onChange={(v) => void flip(t.id, v)} />
                  <span className={s.toolText}>
                    <span className={s.toolName}>{t.label}</span>
                    <span className={s.toolHint}>{t.hint}</span>
                  </span>
                  <span className={s.toolNum}>+{t.tools}</span>
                </div>
              );
            })}
            <p className={s.toolsNote}>
              Fewer tools keep an agent sharp. Cursor warns above {CURSOR_TOOL_WARNING} tools: core is {CORE_TOOLS}, all on is {toolCount(TOOLSETS.map((t) => t.id))}. An agent's own address or command can ask for others.
            </p>
            {error ? (
              <p className={s.error} role="alert">
                {error}
              </p>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}

/** Who is connected now (DECISIONS D47): their avatars and status, and the way to the Agents panel. */
function ConnectedNow() {
  const list = useAgents(useShallow((st) => st.views.filter((x) => x.local && onBar(x, Date.now()))));
  const rows = useListPresence(list, (x) => x.id, "base");
  if (rows.length === 0) return null;
  return (
    <div className={s.connected} data-testid="connect-connected-now">
      <span className={s.connectedLabel}>Working now</span>
      <ul className={s.connectedList}>
        {rows.map((row) => (
          <li
            key={row.key}
            className={cx(s.connectedItem, row.entering && s.connectedIn, row.leaving && s.connectedOut)}
            data-tip={`${agentTitle(row.item)}: ${statusText(row.item)}`}
            data-tip-side="bottom"
            style={{ "--i": 0 } as CSSProperties}
          >
            <AgentAvatar name={row.item.name} color={row.item.color} activity={row.item.activity} size={20} ring="var(--surface-2)" />
            <span className={s.connectedName}>{row.item.name}</span>
          </li>
        ))}
      </ul>
      <button
        type="button"
        className={s.connectedOpen}
        onClick={() => {
          useConnect.getState().closeDialog();
          openAgents("agents");
        }}
      >
        Manage agents
      </button>
    </div>
  );
}

function StatusLine() {
  const status = useConnect((st) => st.status);
  const unreachable = useConnect((st) => st.unreachable);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dot = dotState(status);
  const port = status?.port ?? 1450;
  const text = !status
    ? unreachable
      ? "Could not read the agent status"
      : "Checking"
    : !status.enabled
      ? "Agents are off"
      : status.listening
        ? "Ready for agents"
        : `Not listening: port ${port} is taken`;
  const last = status?.last_client;
  const lastText = !status
    ? ""
    : !status.enabled
      ? "Turn it on to let agents read and change your plans"
      : last
        ? `Last used by ${clientLabel(last.name)}, ${relativeTime(last.at) || "a while ago"}`
        : "No agent has connected yet";

  const toggle = async (next: boolean) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await useConnect.getState().setEnabled(next);
    } catch (e) {
      setError(toIpcError(e).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={s.status}>
      <span className={s.statusMain}>
        <i key={dot} className={cx(s.dot, s[`dot_${dot}`])} aria-hidden />
        {text}
      </span>
      {status ? <span className={cx(s.url, !status.enabled && s.urlOff)}>{status.url}</span> : null}
      <span className={s.muted}>{lastText}</span>
      <label className={s.allow}>
        <span>Allow agents</span>
        <Switch checked={status?.enabled ?? false} disabled={!status || busy} label="Allow agents" onChange={(v) => void toggle(v)} />
      </label>
      {error ? (
        <p className={cx(s.error, s.statusError)} role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

export function ConnectDialog({ stage }: { stage?: PresenceStage }) {
  useMcpPolling(3000);
  const status = useConnect((st) => st.status);
  const [picked, setPicked] = useState<string | null>(null);
  const inUse = agentForClient(status?.last_client?.name);
  const selected = agentById(picked ?? inUse ?? "claude-code");
  const ctx: SetupContext = {
    url: status?.url ?? FALLBACK_URL,
    stdioCommand: status?.stdio_command ?? null,
    claudeDesktopBundle: status?.claude_desktop_bundle ?? false,
  };

  return (
    <Dialog
      title="Connect an AI agent"
      width={780}
      stage={stage}
      onClose={() => useConnect.getState().closeDialog()}
      footer={
        <div className={s.foot}>
          <span>Your agent's own subscription or local model does the thinking. Guhit never sees a key. Every change is one undo step.</span>
          <a className={s.docsLink} href={DOCS_URL} target="_blank" rel="noreferrer">
            Setup for every agent
          </a>
        </div>
      }
    >
      <div className={s.frame}>
        <StatusLine />
        <ConnectedNow />
        <ToolsetsSection />
        <div className={s.split}>
          <div className={s.agents} role="listbox" aria-label="Agents">
            {GROUPS.map((g) => (
              <div key={g} className={s.group}>
                <div className={s.groupName}>{g}</div>
                {AGENTS.filter((a) => a.group === g).map((a) => (
                  <button
                    key={a.id}
                    type="button"
                    role="option"
                    aria-selected={a.id === selected.id}
                    className={s.agent}
                    onClick={() => setPicked(a.id)}
                  >
                    <Mark agent={a} />
                    <span className={s.name}>{a.name}</span>
                    {inUse === a.id ? <span className={cx(s.pill, s.pillUsed)}>in use</span> : a.free ? <span className={cx(s.pill, s.pillFree)}>free</span> : null}
                  </button>
                ))}
              </div>
            ))}
          </div>
          <Detail agent={selected} ctx={ctx} />
        </div>
      </div>
    </Dialog>
  );
}
