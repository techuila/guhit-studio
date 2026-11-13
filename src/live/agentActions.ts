// What the window can do with agents (DECISIONS D47): pause, stop, resume,
// rename and limit them, show their steps (History filtered to one agent)
// and their work (everything they touched, framed), and the palette's
// commands. Shared by the top bar, the Agents panel, the review and the
// command palette.

import type { AgentLimit, AgentSummary } from "../contract/bindings";
import { ipc } from "../contract/ipc";
import type { PaletteAction } from "../shell/actions";
import { levelOfElement } from "../shell/levels";
import { useConnect } from "../shell/connect/connectStore";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { agentById, cleanAgentName, controlsFor } from "./agents";
import { useAgents, type AgentsTab } from "./agentsStore";

/** The app answers with every agent after a pause, stop or resume: show it at once. */
function took(list: AgentSummary[]): void {
  useAgents.getState().setLocal(list);
}

function name(id: string | null): string {
  if (id === null) return "every agent";
  return agentById(useAgents.getState().views, id)?.name ?? "the agent";
}

/** Pause: its edits are refused, reads still work. `id` null: every agent. */
export async function pauseAgent(id: string | null): Promise<void> {
  try {
    took(await ipc.agentPause(id));
    useApp.getState().toast("info", id === null ? "Paused every agent. Their edits wait until you resume them." : `Paused ${name(id)}. Its edits wait until you resume it.`);
  } catch (e) {
    useApp.getState().reportError(e);
  }
}

/** Stop: every call is refused until resumed. `id` null: every agent. */
export async function stopAgent(id: string | null): Promise<void> {
  try {
    took(await ipc.agentStop(id));
    useApp.getState().toast("info", id === null ? "Stopped every agent. Resume one to let it work again." : `Stopped ${name(id)}. Resume it to let it work again.`);
  } catch (e) {
    useApp.getState().reportError(e);
  }
}

/** Ends a pause or a stop. `id` null: every agent. */
export async function resumeAgent(id: string | null): Promise<void> {
  try {
    took(await ipc.agentResume(id));
    useApp.getState().toast("success", id === null ? "Every agent can work again." : `${name(id)} can work again.`);
  } catch (e) {
    useApp.getState().reportError(e);
  }
}

/** Saves a new name. False when it was empty or the app refused it. */
export async function renameAgent(id: string, raw: string): Promise<boolean> {
  const next = cleanAgentName(raw);
  if (next === "") return false;
  if (next === agentById(useAgents.getState().views, id)?.name) return true;
  try {
    const one = await ipc.agentUpdate(id, { name: next });
    replaceOne(one);
    return true;
  } catch (e) {
    useApp.getState().reportError(e);
    return false;
  }
}

/** Sets or clears (null) an agent's limit. The engine enforces it on its next call. */
export async function limitAgent(id: string, limit: AgentLimit | null): Promise<void> {
  try {
    replaceOne(await ipc.agentUpdate(id, { limit }));
  } catch (e) {
    useApp.getState().reportError(e);
  }
}

function replaceOne(one: AgentSummary): void {
  const st = useAgents.getState();
  const local = st.local.map((a) => (a.participant.id === one.participant.id ? one : a));
  st.setLocal(local.some((a) => a.participant.id === one.participant.id) ? local : [one, ...local]);
}

/** "Show its steps": History opens with only this agent's steps. */
export function showSteps(id: string): void {
  useAgents.getState().requestHistory(id);
}

/** The ids of elements an agent's done steps touched that are on the plan now, newest first. */
export async function touchedBy(id: string): Promise<string[]> {
  const list = await ipc.historyList();
  const exists = new Set((useApp.getState().doc?.project.elements ?? []).map((e) => e.id));
  const out: string[] = [];
  for (const step of list.undo) {
    if (step.author_id !== id) continue;
    for (const x of step.ids) if (exists.has(x) && !out.includes(x)) out.push(x);
  }
  return out;
}

/**
 * Selects these elements and frames them, on the level most of them are on.
 * `none` is said when nothing is left to show.
 */
export function showElements(ids: string[], none: string): void {
  const app = useApp.getState();
  if (ids.length === 0) {
    app.toast("info", none);
    return;
  }
  const all = app.doc?.project.elements ?? [];
  const byId = new Map(all.map((e) => [e.id, e]));
  const count = new Map<string, number>();
  for (const id of ids) {
    const el = byId.get(id);
    const level = el ? levelOfElement(el, byId) : null;
    if (level) count.set(level, (count.get(level) ?? 0) + 1);
  }
  const here = app.activeLevelId ? (count.get(app.activeLevelId) ?? 0) : 0;
  let best: string | null = null;
  for (const [level, n] of count) if (best === null || n > (count.get(best) ?? 0)) best = level;
  if (best && best !== app.activeLevelId && (count.get(best) ?? 0) > here) app.setActiveLevel(best);
  const level = useApp.getState().activeLevelId;
  const shown = ids.filter((id) => {
    const el = byId.get(id);
    const l = el ? levelOfElement(el, byId) : null;
    return l === null || l === level;
  });
  app.select(shown.length > 0 ? shown : ids);
  bus.emit("focus_elements", shown.length > 0 ? shown : ids);
}

/** "Show its work": frames everything the agent touched that is still on the plan. */
export async function showWork(id: string): Promise<void> {
  try {
    const ids = await touchedBy(id);
    showElements(ids, `Nothing ${name(id)} made or changed is on the plan now.`);
  } catch (e) {
    useApp.getState().reportError(e);
  }
}

export function openAgents(tab?: AgentsTab): void {
  useAgents.getState().openPanel(tab);
}

/** The palette's agent commands (spread into `liveActions`). */
export function agentActions(): PaletteAction[] {
  const st = useAgents.getState();
  const local = st.views.filter((a) => a.local);
  const canPause = local.some((a) => controlsFor(a).pause);
  const canStop = local.some((a) => controlsFor(a).stop);
  const canResume = local.some((a) => controlsFor(a).resume);
  const hasDoc = useApp.getState().doc !== null;
  const out: PaletteAction[] = [
    { id: "agents-show", title: "Show agents", group: "Panels", icon: "agent", keywords: "agents mcp ai claude codex cursor pause stop limit participants", run: () => openAgents("agents") },
    {
      id: "agents-review",
      title: "What the agents changed",
      group: "Panels",
      icon: "clock",
      keywords: "agents review changes history undo mcp ai since version",
      disabled: !hasDoc,
      run: () => openAgents("changes"),
    },
  ];
  if (local.length > 0) {
    out.push({ id: "agents-pause-all", title: "Pause all agents", group: "Project", icon: "lock", keywords: "agents pause hold wait every all mcp ai", disabled: !canPause, run: () => void pauseAgent(null) });
    out.push({ id: "agents-stop-all", title: "Stop all agents", group: "Project", icon: "close", keywords: "agents stop halt every all mcp ai", disabled: !canStop, run: () => void stopAgent(null) });
    if (canResume) out.push({ id: "agents-resume-all", title: "Resume all agents", group: "Project", icon: "unlock", keywords: "agents resume continue every all mcp ai", run: () => void resumeAgent(null) });
  }
  out.push({ id: "agents-connect", title: "Connect an AI agent", group: "Project", icon: "link", keywords: "agents mcp connect claude codex cursor add another", run: () => useConnect.getState().openDialog() });
  return out;
}
