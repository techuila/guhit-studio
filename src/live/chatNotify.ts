// Unread chat (DECISIONS D41). A message counts as unread unless the window
// is focused with Chat on screen. Owns the window focus, the peek card's
// state, the window's attention request and the macOS Dock badge. The rules
// are pure functions in chatRules.ts.
//
//   - `noteIncomingChat` runs for every message from someone else
//     (wiring.ts `onChat`): counts it, adds or updates a peek card, and asks
//     the window for attention once per burst while it is not focused.
//   - `startChatNotify` runs once from `startLive`: tracks window focus
//     (Tauri's `onFocusChanged` in the desktop app, window focus and blur in
//     the browser) and mirrors the unread count onto the Dock badge.
//
// Desktop only, and no notification plugin (D41): `requestUserAttention`
// bounces the Dock icon or flashes the taskbar, `setBadgeCount` shows the
// count on the macOS Dock icon.

import { create } from "zustand";
import type { ChatMessage } from "../contract/bindings";
import { isTauri } from "../contract/ipc";
import { useShell } from "../shell/shellStore";
import { useApp } from "../state/store";
import { addPeek, badgeCount, dropPeek, isNewBurst, isUnread, skipPeek, type PeekCard, type PeekView } from "./chatRules";
import { isLive, participant } from "./format";
import { useLive } from "./liveStore";
import { chatOnScreen } from "./wiring";

interface NotifyState {
  /** This window has focus. Starts from the browser's answer, then follows the events. */
  focused: boolean;
  /** The peek cards on screen, oldest first. */
  cards: PeekCard[];
}

export const useNotify = create<NotifyState>(() => ({
  focused: typeof document === "undefined" ? true : document.hasFocus(),
  cards: [],
}));

/** When the previous unread message arrived while unfocused (ms). Null after focus or a read. */
let lastUnreadAt: number | null = null;

// ---------------------------------------------------------------- Tauri window

let tauriWindow: Promise<typeof import("@tauri-apps/api/window")> | null = null;

/** The window API, loaded on first use and only in the desktop app. */
function windowApi(): Promise<typeof import("@tauri-apps/api/window")> | null {
  if (!isTauri) return null;
  tauriWindow ??= import("@tauri-apps/api/window");
  return tauriWindow;
}

/** Dock icon bounce or taskbar flash, once. No effect while the window has focus. */
function askAttention(): void {
  void windowApi()
    ?.then(({ getCurrentWindow, UserAttentionType }) => getCurrentWindow().requestUserAttention(UserAttentionType.Informational))
    .catch(() => undefined);
}

/** The count on the macOS Dock icon, cleared at zero. Windows has no badge count and ignores it. */
function showBadge(unread: number): void {
  void windowApi()
    ?.then(({ getCurrentWindow }) => getCurrentWindow().setBadgeCount(badgeCount(unread)))
    .catch(() => undefined);
}

// ---------------------------------------------------------------- messages

/** Someone else's chat message arrived (wiring.ts `onChat`). */
export function noteIncomingChat(message: ChatMessage): void {
  const { focused } = useNotify.getState();
  if (!isUnread(focused, chatOnScreen())) return;

  const live = useLive.getState();
  live.setUnread(live.unread + 1);

  const app = useApp.getState();
  const inEditor = app.screen === "editor";
  const view: PeekView = { focused, planOnScreen: inEditor && app.viewMode !== "3d", levelId: inEditor ? app.activeLevelId : null };
  if (!skipPeek(message, view)) {
    const role = participant(live.status, message.author_id)?.role ?? "guest";
    useNotify.setState((s) => ({ cards: addPeek(s.cards, message, role) }));
  }

  // One request per burst, not per message, and only while a session runs.
  if (!focused && isLive(live.status.mode)) {
    const now = Date.now();
    const fresh = isNewBurst(lastUnreadAt, now);
    lastUnreadAt = now;
    if (fresh) askAttention();
  }
}

/** Closes one sender's card. */
export function dismissPeek(authorId: string): void {
  useNotify.setState((s) => {
    const cards = dropPeek(s.cards, authorId);
    return cards === s.cards ? s : { cards };
  });
}

/** Opens Chat in the dock, expands the dock and puts the cursor in the message field. */
export function replyFromPeek(authorId: string): void {
  dismissPeek(authorId);
  useShell.getState().setDockTab("chat");
  focusChatInput();
}

// ---------------------------------------------------------------- chat input

/** The Chat tab's message field while it is mounted (ChatPanel's Composer registers it). */
export const chatInput: { current: HTMLTextAreaElement | null } = { current: null };

/**
 * Focuses the message field, waiting a few frames for it to be on screen:
 * the dock may still be collapsed or on another tab when this is called.
 */
export function focusChatInput(): void {
  let frames = 0;
  const tick = () => {
    const el = chatInput.current;
    if (el && !el.disabled && el.offsetParent !== null) {
      el.focus({ preventScroll: true });
      return;
    }
    if (++frames < 30) requestAnimationFrame(tick);
  };
  tick();
}

// ---------------------------------------------------------------- focus

function setFocused(focused: boolean): void {
  if (useNotify.getState().focused === focused) return;
  useNotify.setState({ focused });
  if (!focused) return;
  // Back in the window: the next burst may ask again, and Chat already on
  // screen means everything shown there counts as read.
  lastUnreadAt = null;
  if (chatOnScreen()) useLive.getState().setUnread(0);
}

/**
 * What the Dock icon shows: the unread count while a session runs, nothing
 * once it ended. The count itself stays (the Chat tab still shows what there
 * is to read), the Dock badge does not outlive the session.
 */
function dockCount(): number {
  const live = useLive.getState();
  return isLive(live.status.mode) ? live.unread : 0;
}

/** The unread count changed: the Dock badge follows, and a read clears the cards. */
function onUnread(unread: number): void {
  showBadge(dockCount());
  if (unread > 0) return;
  lastUnreadAt = null;
  if (useNotify.getState().cards.length > 0) useNotify.setState({ cards: [] });
}

/** The session ended: no Dock badge, no card to reply to, a fresh burst next time. */
function onSessionEnd(): void {
  showBadge(0);
  lastUnreadAt = null;
  if (useNotify.getState().cards.length > 0) useNotify.setState({ cards: [] });
}

/** Starts focus tracking. Runs once from `startLive`; returns its cleanup. */
export function startChatNotify(): () => void {
  let stopped = false;
  const stops: Array<() => void> = [];

  if (isTauri) {
    void windowApi()
      ?.then(async ({ getCurrentWindow }) => {
        const win = getCurrentWindow();
        const unlisten = await win.onFocusChanged(({ payload }) => !stopped && setFocused(payload));
        if (stopped) return unlisten();
        stops.push(unlisten);
        const now = await win.isFocused();
        if (!stopped) setFocused(now);
      })
      .catch(() => undefined);
  } else if (typeof window !== "undefined") {
    const on = () => setFocused(true);
    const off = () => setFocused(false);
    window.addEventListener("focus", on);
    window.addEventListener("blur", off);
    stops.push(() => {
      window.removeEventListener("focus", on);
      window.removeEventListener("blur", off);
    });
    setFocused(document.hasFocus());
  }

  stops.push(
    useLive.subscribe((s, p) => {
      const was = isLive(p.status.mode);
      const is = isLive(s.status.mode);
      if (was && !is) onSessionEnd();
      else if (s.unread !== p.unread || was !== is) onUnread(s.unread);
    }),
  );
  // The count may already be above zero when this starts (a hot reload).
  showBadge(dockCount());

  return () => {
    stopped = true;
    for (const stop of stops) stop();
    showBadge(0);
    useNotify.setState({ cards: [] });
    lastUnreadAt = null;
  };
}
