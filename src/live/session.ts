// What the live session UI can do: start, join, leave or end a session, copy
// the invite, save a copy, and bring someone's pointer into view. Shared by
// the Share and Join dialogs, the top bar, the chat panel and the palette.

import type { Point } from "../contract/bindings";
import { ipc, toIpcError } from "../contract/ipc";
import { getActiveController } from "../editor2d/controller";
import type { PaletteAction } from "../shell/actions";
import { useShell } from "../shell/shellStore";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { copyText } from "../ui/clipboard";
import { cleanName, firstName, others, participant, possessive } from "./format";
import { useLive } from "./liveStore";

/** Saves the profile name when it changed. Hosting and joining need one. */
export async function ensureName(raw: string): Promise<boolean> {
  const name = cleanName(raw);
  if (name === "") return false;
  const current = useLive.getState().profile?.name ?? "";
  if (name === current) return true;
  const profile = await ipc.profileSet(name);
  useLive.getState().setProfile(profile);
  return true;
}

/** Shares the open project. Throws the engine's error for the dialog to show. */
export async function startHosting(name: string): Promise<void> {
  if (!(await ensureName(name))) throw { code: "bad_args", message: "Add your name first. Others see it next to your pointer.", element_ids: [] };
  const status = await ipc.liveHost();
  useLive.getState().setStatus(status);
}

/**
 * Joins the session an invite shares and opens its project, like opening a
 * project from the hub. Throws the engine's error for the dialog to show.
 */
export async function joinSession(invite: string, name: string): Promise<void> {
  if (!(await ensureName(name))) throw { code: "bad_args", message: "Add your name first. Others see it next to your pointer.", element_ids: [] };
  const live = useLive.getState();
  live.setJoining(true);
  try {
    const doc = await ipc.liveJoin(invite.trim());
    const app = useApp.getState();
    app.setDoc(doc);
    useApp.setState({ screen: "editor", tool: "select", selection: [], preview: null });
    useShell.getState().close();
    // The status event follows; ask now so the top bar is right at once.
    void ipc
      .liveStatus()
      .then((st) => useLive.getState().setStatus(st))
      .catch(() => undefined);
  } finally {
    useLive.getState().setJoining(false);
  }
}

/** Host: ends the session for everyone. The project stays open here. */
export async function endSession(): Promise<void> {
  try {
    useLive.getState().setStatus(await ipc.liveLeave());
    useApp.getState().toast("info", "Live session ended. The project stays open on this computer.");
  } catch (e) {
    useApp.getState().reportError(e);
  }
}

/** Guest: leaves the session. The shared project closes and the window goes to the hub. */
export async function leaveSession(): Promise<void> {
  const host = useLive.getState().status.participants.find((p) => p.role === "host");
  useLive.getState().closeDialog();
  try {
    useLive.getState().setStatus(await ipc.liveLeave());
  } catch (e) {
    useApp.getState().reportError(e);
    return;
  }
  useShell.getState().close();
  const doc = await ipc.docState().catch(() => null);
  useApp.getState().setDoc(doc);
  if (!doc) useApp.setState({ screen: "hub" });
  useApp.getState().toast("info", host ? `You left ${possessive(host.name)} live session.` : "You left the live session.");
}

/** Guest: the shared project as a new project on this computer. True when saved. */
export async function saveCopy(): Promise<boolean> {
  try {
    const meta = await ipc.liveSaveCopy();
    useApp.getState().toast("success", `Saved a copy as "${meta.name}" in your projects.`);
    return true;
  } catch (e) {
    useApp.getState().reportError(e);
    return false;
  }
}

export async function copyInvite(): Promise<boolean> {
  const invite = useLive.getState().status.invite;
  if (!invite) return false;
  const ok = await copyText(invite);
  if (!ok) useApp.getState().toast("error", "The invite could not be copied. Select it and copy it by hand.");
  return ok;
}

export function openShare(opts?: { askEnd?: boolean }): void {
  useLive.getState().openDialog("share", opts);
}

export function openJoin(): void {
  useLive.getState().openDialog("join");
}

export function openChat(): void {
  useShell.getState().setDockTab("chat");
}

/** Waits a few frames for the plan to mount after the view mode changed. */
function whenPlanReady(fn: () => void): void {
  let frames = 0;
  const tick = () => {
    if (getActiveController()) return fn();
    if (++frames < 90) requestAnimationFrame(tick);
  };
  tick();
}

/**
 * Centers the plan on a point, on its level: a participant's pointer or a
 * cursor chat message. The 3D-only view opens the plan beside it first.
 */
export function focusPlanPoint(point: Point, levelId: string | null): void {
  const app = useApp.getState();
  if (app.viewMode === "3d") app.setViewMode("split");
  whenPlanReady(() => bus.emit("focus_point", { point, level_id: levelId }));
}

/** Clicking someone's avatar: their pointer comes into view, or a line says where they are. */
export function focusParticipant(id: string): void {
  const st = useLive.getState();
  const p = participant(st.status, id);
  if (!p) return;
  const pr = st.peers[id]?.presence;
  if (pr?.cursor) {
    focusPlanPoint(pr.cursor, pr.level_id);
    return;
  }
  const level = pr?.level_id ? useApp.getState().doc?.project.levels.find((l) => l.id === pr.level_id) : null;
  const where = level && level.id !== useApp.getState().activeLevelId ? ` They are on ${level.name}.` : "";
  useApp.getState().toast("info", `${possessive(p.name)} pointer is not on the plan right now.${where}`);
}

/** The palette's live session commands (spread into `paletteActions`). */
export function liveActions(): PaletteAction[] {
  const st = useLive.getState();
  const mode = st.status.mode;
  const hasDoc = useApp.getState().doc !== null;
  const out: PaletteAction[] = [];
  if (mode === "off") {
    out.push({
      id: "live-start",
      title: "Start live session",
      group: "Project",
      icon: "people",
      keywords: "share invite multiplayer collaborate together live session host network",
      disabled: !hasDoc,
      run: () => openShare(),
    });
  } else {
    out.push({ id: "live-share", title: "Show the live session", group: "Project", icon: "people", keywords: "share invite participants people live session", run: () => openShare() });
  }
  if (mode === "hosting") {
    out.push({
      id: "live-copy",
      title: "Copy invite",
      group: "Project",
      icon: "copy",
      keywords: "share invite link live session clipboard",
      run: () =>
        void copyInvite().then((ok) => {
          if (ok) useApp.getState().toast("success", "Invite copied. Anyone with it can join while the session runs.");
        }),
    });
  }
  out.push({ id: "live-join", title: "Join a live session", group: "Project", icon: "join", keywords: "join invite multiplayer collaborate live session guest", run: openJoin });
  out.push({ id: "live-chat", title: "Open chat", group: "Panels", icon: "chat", keywords: "chat messages live session talk", run: openChat });
  if (mode === "hosting") {
    const n = others(st.status).length;
    out.push({
      id: "live-end",
      title: "End the live session",
      group: "Project",
      icon: "close",
      keywords: `stop end live session host ${n > 0 ? "everyone" : ""}`,
      run: () => openShare({ askEnd: true }),
    });
  } else if (mode === "joined" || mode === "reconnecting") {
    const host = st.status.participants.find((p) => p.role === "host");
    out.push({
      id: "live-leave",
      title: host ? `Leave ${possessive(host.name)} live session` : "Leave the live session",
      group: "Project",
      icon: "back",
      keywords: "leave exit live session guest",
      run: () => void leaveSession(),
    });
  }
  return out;
}

/** "Ana" or "Ana and 2 others", for short status lines. */
export function peopleLine(names: string[]): string {
  if (names.length === 0) return "No one else yet";
  const first = firstName(names[0]);
  if (names.length === 1) return first;
  if (names.length === 2) return `${first} and ${firstName(names[1])}`;
  return `${first} and ${names.length - 1} others`;
}

/** The message of an error, for inline display. */
export function errorText(e: unknown): string {
  return toIpcError(e).message;
}
