// "What the agents changed" (DECISIONS D47), the review tab of the Agents
// panel: the agents' steps since they started (the "Before agents" version
// the app saves) or since a version the user picks, grouped by agent with
// what each touched. Hovering a step or an agent tints its elements in the
// agent's color, clicking shows them, and "Undo to here" takes back that
// step and everything after it through History's flow (DECISIONS D38): one
// revision, asking first when someone else's steps are among them.

import { useEffect, useMemo, useState, type CSSProperties } from "react";
import type { HistoryList, SnapshotMeta } from "../contract/bindings";
import { ipc, toIpcError } from "../contract/ipc";
import { historyRows, liveIds, relativeTime, stepDetail, type HistoryRow } from "../shell/history";
import { useShell } from "../shell/shellStore";
import { useApp } from "../state/store";
import { Select, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import { showElements } from "./agentActions";
import { AgentAvatar } from "./AgentAvatar";
import { agentById, autoVersion, forLine, groupLine, reviewGroups, undoLines, type ReviewGroup } from "./agents";
import { useAgents } from "./agentsStore";
import { peerVar } from "./format";
import { useLive } from "./liveStore";
import s from "./AgentsPanel.module.css";

/** Steps shown per agent before "N more". */
const FOLD = 6;

const AUTO = "auto";
const ALL = "all";

function versionTime(iso: string): string {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return "";
  return new Date(t).toLocaleString("en-US", { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }).replace(/\s/g, " ");
}

export function AgentChanges() {
  const hasDoc = useApp((st) => st.doc !== null);
  const revision = useApp((st) => st.doc?.revision ?? null);
  const status = useLive((st) => st.status);
  const known = useAgents((st) => st.known);
  const [list, setList] = useState<HistoryList | null>(null);
  const [listRevision, setListRevision] = useState<number | null>(null);
  const [snapshots, setSnapshots] = useState<SnapshotMeta[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [since, setSince] = useState<string | null>(null);
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    if (!hasDoc) return;
    let alive = true;
    ipc
      .historyList()
      .then((next) => {
        if (!alive) return;
        setList(next);
        setListRevision(revision);
        setError(null);
      })
      .catch((e) => alive && setError(toIpcError(e).message));
    return () => {
      alive = false;
    };
  }, [revision, hasDoc]);

  // Versions change rarely: read them on open and when a step lands (the app saves "Before agents" with the first one).
  const stepsTotal = useAgents((st) => st.local.reduce((n, a) => n + a.steps, 0));
  useEffect(() => {
    if (!hasDoc) return;
    let alive = true;
    ipc
      .snapshotList()
      .then((items) => alive && setSnapshots([...items].sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at))))
      .catch(() => alive && setSnapshots([]));
    return () => {
      alive = false;
    };
  }, [hasDoc, stepsTotal]);

  useEffect(() => {
    const t = window.setInterval(() => setNow(new Date()), 30_000);
    return () => window.clearInterval(t);
  }, []);

  // The tint never outlives the tab.
  useEffect(() => () => useApp.getState().setHistoryTint(null), []);

  const auto = useMemo(() => autoVersion(snapshots ?? []), [snapshots]);
  const pick = since ?? (auto ? AUTO : ALL);
  const from: SnapshotMeta | null = pick === AUTO ? auto : pick === ALL ? null : ((snapshots ?? []).find((x) => x.id === pick) ?? null);
  const agentIds = useMemo(() => new Set(Object.keys(known)), [known]);
  const rows = useMemo(() => (list ? historyRows(list, status, agentIds) : []), [list, status, agentIds]);
  const groups = useMemo(() => reviewGroups(rows, from?.created_at ?? null, agentIds), [rows, from, agentIds]);
  const total = groups.reduce((n, g) => n + g.rows.length, 0);

  const options = [
    ...(auto ? [{ value: AUTO, label: `Since agents started (${versionTime(auto.created_at)})` }] : []),
    ...(snapshots ?? []).filter((x) => x.id !== auto?.id).map((x) => ({ value: x.id, label: `Since "${x.label}"` })),
    { value: ALL, label: "Everything kept (the last 200 steps)" },
  ];

  if (!hasDoc) {
    return (
      <div className={s.empty}>
        <p className={s.emptyTitle}>No project open</p>
        <p className={s.emptyText}>Open a project to review what agents changed in it.</p>
      </div>
    );
  }

  const sinceWords = from ? (pick === AUTO ? "the agents started" : `"${from.label}"`) : "the project opened";
  return (
    <div className={s.review} onPointerLeave={() => useApp.getState().setHistoryTint(null)}>
      <div className={s.reviewHead}>
        <Select label="Show the changes since" value={pick} options={options} onChange={(v) => setSince(v)} />
        <span className={s.reviewCount}>{list === null && !error ? "Loading" : total === 1 ? "1 step" : `${total} steps`}</span>
      </div>
      {error ? <p className={s.emptyText}>The history is not available: {error}</p> : null}
      {!error && list !== null && groups.length === 0 ? (
        <div className={s.empty}>
          <span className={s.emptyMark} aria-hidden>
            <Icon name="check" size={20} />
          </span>
          <p className={s.emptyTitle}>No agent has changed the plan since {sinceWords}</p>
          <p className={s.emptyText}>When an agent edits, its steps show here, grouped by agent, with what each one touched. Undo to here takes them back.</p>
        </div>
      ) : null}
      <div className={s.groups}>
        {groups.map((g) => (
          <Group key={g.key} group={g} now={now} listRevision={listRevision} rows={rows} />
        ))}
      </div>
      <p className={s.foot}>
        Undo to here takes back that step and every step after it, as one step, after asking, with everyone whose steps go with it. To go back to before the agents in one move, restore the version in{" "}
        <button type="button" className={s.linkButton} onClick={() => useShell.getState().open("versions")}>
          Versions
        </button>
        .
      </p>
    </div>
  );
}

function Group({ group, now, listRevision, rows }: { group: ReviewGroup; now: Date; listRevision: number | null; rows: readonly HistoryRow[] }) {
  const agent = useAgents((st) => (group.key.startsWith("name:") ? null : agentById(st.views, group.key)));
  const selfId = useLive((st) => st.status.self_id);
  const selfName = useLive((st) => st.profile?.name ?? null);
  const [all, setAll] = useState(false);
  const shown = all ? group.rows : group.rows.slice(0, FOLD);
  const elements = useApp((st) => st.doc?.project.elements);
  const color = group.color ?? agent?.color ?? null;
  // An agent whose color is no longer known tints neutral, never in the AI purple (R4).
  const style = { "--who": color === null ? "var(--ink-3)" : peerVar(color) } as CSSProperties;
  const forText = forLine(group.rows[0].step, selfId, selfName);

  const tintIds = (ids: string[]) => {
    const live = elements ? ids.filter((id) => elements.some((e) => e.id === id)) : [];
    useApp.getState().setHistoryTint(live.length > 0 ? { ids: live, color } : null);
  };
  const showRow = (row: HistoryRow) => {
    const ids = liveIds(row.step, useApp.getState().doc?.project.elements ?? []);
    showElements(ids, "Nothing this step touched is on the plan now.");
  };
  const showAll = () => {
    const exists = new Set((useApp.getState().doc?.project.elements ?? []).map((e) => e.id));
    showElements(
      group.touched.filter((id) => exists.has(id)),
      `Nothing ${group.name} made or changed is on the plan now.`,
    );
  };
  const undoTo = (row: HistoryRow) => {
    // Every step it takes back, whoever made it, is named before it acts (agentsPanel `UndoAsk`).
    useAgents.getState().askUndo({ steps: row.steps, revision: listRevision, lines: undoLines(rows, row.steps) });
  };

  return (
    <section className={s.group} style={style} data-testid={`review-group-${group.key}`}>
      <header className={s.groupHead} onPointerEnter={() => tintIds(group.touched)} onPointerLeave={() => useApp.getState().setHistoryTint(null)}>
        <AgentAvatar name={group.name} color={color ?? 0} activity={agent?.activity ?? null} size={26} ring="var(--surface)" />
        <span className={s.groupText}>
          <span className={s.groupName}>
            {group.name}
            <span className={s.aiTag}>AI</span>
          </span>
          <span className={s.groupMeta}>
            <span>{groupLine(group)}</span>
            {forText ? <span>{forText}</span> : null}
          </span>
        </span>
        <button type="button" className={s.groupShow} onClick={showAll} data-tip="Select and frame everything it touched" data-tip-side="left">
          Show all
        </button>
      </header>
      <ul className={s.steps} aria-label={`Steps by ${group.name}, newest first`}>
        {shown.map((row) => {
          const detail = stepDetail(row.step);
          const time = relativeTime(row.step.at, now);
          return (
            <li key={row.key} className={s.step}>
              <div className={s.stepInner}>
                <button
                  type="button"
                  className={s.stepMain}
                  onPointerEnter={() => tintIds([...row.step.ids, ...row.step.removed])}
                  onPointerLeave={() => useApp.getState().setHistoryTint(null)}
                  onFocus={(e) => e.currentTarget.matches(":focus-visible") && tintIds([...row.step.ids, ...row.step.removed])}
                  onBlur={() => useApp.getState().setHistoryTint(null)}
                  onClick={() => showRow(row)}
                  aria-label={`${row.step.label}${time ? `, ${time}` : ""}${detail ? `. ${detail}` : ""}. Show on the plan`}
                >
                  <span className={s.stepLabel}>{row.step.label || "Change"}</span>
                  <span className={s.stepMeta}>
                    {time ? <span>{time}</span> : null}
                    {detail ? <span className={s.stepDetail}>{detail}</span> : null}
                  </span>
                </button>
                <button
                  type="button"
                  className={s.undoTo}
                  data-tip={row.steps === 1 ? "Undo this step" : `Undo the last ${row.steps} steps, back to before this one`}
                  data-tip-side="left"
                  onClick={() => undoTo(row)}
                >
                  <Icon name="undo" size={12} />
                  Undo to here
                </button>
              </div>
            </li>
          );
        })}
      </ul>
      {group.rows.length > FOLD ? (
        <button type="button" className={cx(s.linkButton, s.more)} onClick={() => setAll((v) => !v)}>
          {all ? "Show fewer" : `${group.rows.length - FOLD} more`}
        </button>
      ) : null}
    </section>
  );
}
