// Connects the live store to the backend. `startLive` runs once from
// LiveRoot's effect (after boot, so the dev harness is in place) and returns
// its cleanup:
//   - one `onAppEvent` subscription for `live`, `presence` and `chat`
//     (window requests are answered in src/shell/windowTasks.ts),
//   - status, profile, presence and chat loaded at start and whenever the
//     session or the open project changes,
//   - this window's presence pushed with `presence_set` (presenceSync.ts),
//   - the unread count and the one-time notice toast.

import type { ChatMessage, LiveStatus } from "../contract/bindings";
import { ipc, onAppEvent } from "../contract/ipc";
import { useShell } from "../shell/shellStore";
import { useApp } from "../state/store";
import { canChat, isLive } from "./format";
import { SAID_MS, useLive } from "./liveStore";
import { PRESENCE_INTERVAL_MS, PresenceThrottle, buildPresence } from "./presenceSync";

/** Changes when a session starts, ends, moves to another project or reconnects. */
export function sessionKey(s: LiveStatus): string {
  return `${s.mode}|${s.project_id ?? ""}|${s.self_id ?? ""}`;
}

/** The Chat tab is what the dock shows, in the editor. */
export function chatOnScreen(): boolean {
  const shell = useShell.getState();
  return useApp.getState().screen === "editor" && shell.dockTab === "chat" && !shell.dockCollapsed;
}

function projectOf(): string | null {
  return useApp.getState().doc?.project.id ?? null;
}

export function startLive(): () => void {
  let stopped = false;
  const stops: Array<() => void> = [];
  const timers = new Set<number>();
  const later = (fn: () => void, ms: number) => {
    const id = window.setTimeout(() => {
      timers.delete(id);
      fn();
    }, ms);
    timers.add(id);
  };

  // ------------------------------------------------ presence and chat loads

  let loadSeq = 0;
  /**
   * Everyone's presence (in a session) and the chat (with a project open:
   * `chat_list` answers `no_document` otherwise, which the browser would log
   * as a failed request).
   */
  const reload = async () => {
    const seq = ++loadSeq;
    const inSession = isLive(useLive.getState().status.mode);
    const hasDoc = useApp.getState().doc !== null;
    const [peers, chat] = await Promise.all([
      inSession ? ipc.presenceList().catch(() => []) : Promise.resolve([]),
      hasDoc ? ipc.chatList().catch(() => [] as ChatMessage[]) : Promise.resolve([] as ChatMessage[]),
    ]);
    if (stopped || seq !== loadSeq) return;
    const live = useLive.getState();
    live.setPeers(isLive(live.status.mode) ? peers : [], Date.now());
    // Merge: a message that arrived while the list loaded stays.
    live.setMessages([...chat, ...useLive.getState().messages]);
  };

  let shownProject = projectOf();
  /** Another project (or none) is open: its chat replaces the old one. */
  const onProject = () => {
    const now = projectOf();
    if (now === shownProject) return;
    shownProject = now;
    // Another project open: the ended session's copy can no longer be saved.
    if (now !== null) useLive.getState().setEnded(null);
    useLive.setState({ messages: [], chatLoaded: false, unread: 0, said: {} });
    void reload();
  };

  // ------------------------------------------------ app events

  const onStatus = (next: LiveStatus) => {
    const live = useLive.getState();
    const prev = live.status;
    live.setStatus(next);
    // Why a session ended or a join failed, once. A failed join shows its
    // error in the join dialog instead.
    if (next.notice && next.notice !== prev.notice && !live.joining && live.dialog !== "join") {
      useApp.getState().toast("info", next.notice);
    }
  };

  /**
   * The session started, ended, reconnected or moved to another project,
   * whoever set the status (an event, or a call's answer): load presence
   * and chat again. A joined session that the host ended, or whose
   * connection was lost, closed the shared project here: back to the hub,
   * with its last copy still savable from there.
   */
  const onSession = (prev: LiveStatus, next: LiveStatus) => {
    void reload();
    void ipc
      .profileGet()
      .then((profile) => !stopped && useLive.getState().setProfile(profile))
      .catch(() => undefined);
    const wasGuest = prev.mode === "joined" || prev.mode === "reconnecting";
    const endedHere = wasGuest && next.mode === "off" && next.notice !== null;
    useLive.getState().setEnded(endedHere ? { projectName: prev.project_name } : null);
    if (!endedHere) return;
    if (useLive.getState().dialog === "share") useLive.getState().closeDialog();
    void ipc
      .docState()
      .then((doc) => {
        if (doc || stopped) return;
        useShell.getState().close();
        useApp.getState().setDoc(null);
        useApp.setState({ screen: "hub" });
      })
      .catch(() => undefined);
  };

  const onChat = (message: ChatMessage) => {
    const live = useLive.getState();
    live.addMessage(message);
    if (message.author_id === live.status.self_id) return;
    if (!chatOnScreen()) live.setUnread(live.unread + 1);
    // Shown beside the author's cursor for a moment.
    live.setSaid(message.author_id, { id: message.id, text: message.text });
    later(() => useLive.getState().clearSaid(message.author_id, message.id), SAID_MS);
  };

  stops.push(
    onAppEvent((event) => {
      switch (event.type) {
        case "live":
          onStatus(event.status);
          break;
        case "presence":
          useLive.getState().setPeer(event.participant_id, event.presence, Date.now());
          break;
        case "chat":
          onChat(event.message);
          break;
        default:
          // Window requests: src/shell/windowTasks.ts.
          break;
      }
    }),
  );

  // ------------------------------------------------ this window's presence

  const throttle = new PresenceThrottle({
    intervalMs: PRESENCE_INTERVAL_MS,
    send: (presence) => {
      void ipc.presenceSet(presence).catch(() => undefined);
    },
    now: () => performance.now(),
    setTimer: (fn, ms) => window.setTimeout(fn, ms),
    clearTimer: (id) => window.clearTimeout(id as number),
  });
  const push = () => {
    const app = useApp.getState();
    const live = useLive.getState();
    throttle.update(
      buildPresence({
        selection: app.selection,
        levelId: app.doc ? app.activeLevelId : null,
        aiScope: app.aiScope,
        cursor: app.cursor,
        typing: live.chat.open ? live.chat.draft : null,
        live: canChat(live.status.mode),
      }),
    );
  };
  stops.push(() => throttle.dispose());

  stops.push(
    useApp.subscribe((s, p) => {
      if (s.selection !== p.selection || s.activeLevelId !== p.activeLevelId || s.aiScope !== p.aiScope || s.cursor !== p.cursor || s.doc !== p.doc) push();
      if (s.doc?.project.id !== p.doc?.project.id) onProject();
      if (s.screen !== p.screen && chatOnScreen()) useLive.getState().setUnread(0);
    }),
  );
  stops.push(
    useLive.subscribe((s, p) => {
      if (sessionKey(s.status) !== sessionKey(p.status)) {
        // A new session: the others need this window's state even if it did not change.
        throttle.reset();
        push();
        onSession(p.status, s.status);
      } else if (s.chat !== p.chat) push();
    }),
  );
  stops.push(
    useShell.subscribe((s, p) => {
      if ((s.dockTab !== p.dockTab || s.dockCollapsed !== p.dockCollapsed) && chatOnScreen()) useLive.getState().setUnread(0);
    }),
  );

  // ------------------------------------------------ first load

  void (async () => {
    const [status, profile] = await Promise.all([ipc.liveStatus().catch(() => null), ipc.profileGet().catch(() => null)]);
    if (stopped) return;
    // An old notice is not news: it was shown when it happened.
    if (status) useLive.getState().setStatus(status);
    if (profile) useLive.getState().setProfile(profile);
    await reload();
    push();
  })();

  return () => {
    stopped = true;
    for (const stop of stops) stop();
    for (const id of timers) window.clearTimeout(id);
    timers.clear();
  };
}
