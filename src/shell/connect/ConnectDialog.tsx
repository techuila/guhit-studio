// "Connect an AI agent" (DECISIONS D35): agents down the left, the chosen
// agent's setup on the right, and a status line with the "Allow agents" switch.
import { useEffect, useRef, useState } from "react";
import { toIpcError } from "../../contract/ipc";
import { copyText } from "../../ui/clipboard";
import { Button, Switch, cx } from "../../ui/controls";
import { Dialog } from "../../ui/Dialog";
import type { PresenceStage } from "../../ui/motion";
import { relativeTime } from "../../ui/units";
import {
  AGENTS,
  DOCS_URL,
  FALLBACK_URL,
  GROUPS,
  agentById,
  agentForClient,
  clientLabel,
  setupFor,
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
