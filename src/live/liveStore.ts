// Live session state for the window (DECISIONS D29, docs/CONTRACT.md "Live
// sessions"). The backend owns the session; this is what the UI shows of it:
// the status, this computer's profile, everyone else's presence, the chat,
// and the local cursor chat. Filled by wiring.ts from `onAppEvent` and the
// `live_*`, `presence_*`, `chat_*` and `profile_*` calls.
//
// Presence changes many times a second: read it with narrow selectors (one
// participant, one field) or with `useLive.subscribe`, never the whole map
// in a component that renders much.

import { create } from "zustand";
import type { ChatMessage, LiveStatus, Presence, Profile } from "../contract/bindings";
import { mergeMessages, type PeerPresence } from "./format";
import { samePresence } from "./presenceSync";

export const LIVE_OFF: LiveStatus = {
  mode: "off",
  self_id: null,
  participants: [],
  invite: null,
  addresses: [],
  relay: "off",
  project_id: null,
  project_name: null,
  notice: null,
};

/** How long a sent message shows beside its author's cursor. */
export const SAID_MS = 6000;

/** How long your own cursor chat message stays in the bubble after Enter. */
export const SENT_MS = 4000;

export type LiveDialog = "share" | "join";

export interface CursorChat {
  /** The bubble is open with its text field. */
  open: boolean;
  draft: string;
  /** Your last message, shown in the bubble for a moment after Enter. */
  sent: { text: string; token: number } | null;
  /** "/" outside a live session: a short hint at the pointer instead. */
  hint: number;
}

/** A message someone just sent, shown beside their cursor. */
export interface Said {
  id: string;
  text: string;
}

export interface LiveState {
  status: LiveStatus;
  /** This computer's name. Null until loaded. */
  profile: Profile | null;
  /** Everyone else's latest presence, by participant id. */
  peers: Record<string, PeerPresence>;
  /** The chat of the open project, oldest first. */
  messages: ChatMessage[];
  /** The chat was loaded at least once for the current project or session. */
  chatLoaded: boolean;
  /** Messages from others that arrived while the Chat tab was not on screen. */
  unread: number;
  /** The last message of each participant, while it shows beside their cursor. */
  said: Record<string, Said>;
  chat: CursorChat;
  dialog: LiveDialog | null;
  /** The Share dialog opens with "End the session?" already asked. */
  askEnd: boolean;
  /** Going back to the projects while hosting asks first: it ends the session. */
  askLeave: boolean;
  /** A join is in flight. Its failure shows in the join dialog, not as a toast. */
  joining: boolean;
  /**
   * A session this computer had joined ended. Its last copy can still be
   * saved (`live_save_copy`) until another project opens.
   */
  ended: { projectName: string | null } | null;

  setStatus: (status: LiveStatus) => void;
  setProfile: (profile: Profile) => void;
  /** One participant's presence; null when they left. */
  setPeer: (id: string, presence: Presence | null, now: number) => void;
  setPeers: (entries: Array<{ participant_id: string; presence: Presence }>, now: number) => void;
  setMessages: (messages: ChatMessage[]) => void;
  addMessage: (message: ChatMessage) => void;
  setUnread: (n: number) => void;
  setSaid: (authorId: string, said: Said) => void;
  /** Clears what shows beside a cursor, unless a newer message replaced it. */
  clearSaid: (authorId: string, messageId: string) => void;
  openCursorChat: () => void;
  setDraft: (draft: string) => void;
  closeCursorChat: () => void;
  markSent: (text: string) => number;
  clearSent: (token: number) => void;
  showHint: () => void;
  openDialog: (dialog: LiveDialog, opts?: { askEnd?: boolean }) => void;
  closeDialog: () => void;
  setAskEnd: (on: boolean) => void;
  setAskLeave: (on: boolean) => void;
  setJoining: (on: boolean) => void;
  setEnded: (ended: { projectName: string | null } | null) => void;
}

let sentSeq = 1;

export const useLive = create<LiveState>((set, get) => ({
  status: LIVE_OFF,
  profile: null,
  peers: {},
  messages: [],
  chatLoaded: false,
  unread: 0,
  said: {},
  chat: { open: false, draft: "", sent: null, hint: 0 },
  dialog: null,
  askEnd: false,
  askLeave: false,
  joining: false,
  ended: null,

  setStatus: (status) =>
    set((s) => {
      const ids = new Set(status.mode === "off" ? [] : status.participants.map((p) => p.id));
      const peers: Record<string, PeerPresence> = {};
      let pruned = false;
      for (const [id, peer] of Object.entries(s.peers)) {
        if (ids.has(id) && id !== status.self_id) peers[id] = peer;
        else pruned = true;
      }
      let said = s.said;
      if (Object.keys(said).some((id) => !ids.has(id))) {
        said = {};
        for (const [id, x] of Object.entries(s.said)) if (ids.has(id)) said[id] = x;
      }
      const live = status.mode === "hosting" || status.mode === "joined";
      return {
        status,
        peers: pruned ? peers : s.peers,
        said,
        // Cursor chat needs a running session.
        chat: live || !s.chat.open ? s.chat : { ...s.chat, open: false, draft: "" },
        askEnd: status.mode === "hosting" ? s.askEnd : false,
        askLeave: status.mode === "hosting" ? s.askLeave : false,
      };
    }),

  setProfile: (profile) => set({ profile }),

  setPeer: (id, presence, now) =>
    set((s) => {
      // Returning the same state object is what skips the notification.
      if (id === s.status.self_id) return s;
      if (!presence) {
        if (!(id in s.peers)) return s;
        const peers = { ...s.peers };
        delete peers[id];
        return { peers };
      }
      const before = s.peers[id];
      if (before && samePresence(before.presence, presence)) return s;
      return { peers: { ...s.peers, [id]: { presence, changedAt: now } } };
    }),

  setPeers: (entries, now) =>
    set((s) => {
      const peers: Record<string, PeerPresence> = {};
      for (const e of entries) {
        if (e.participant_id === s.status.self_id) continue;
        const before = s.peers[e.participant_id];
        peers[e.participant_id] = before && samePresence(before.presence, e.presence) ? before : { presence: e.presence, changedAt: now };
      }
      return { peers };
    }),

  setMessages: (messages) => set({ messages: mergeMessages([], messages), chatLoaded: true }),

  addMessage: (message) =>
    set((s) => {
      const messages = mergeMessages(s.messages, [message]);
      return messages === s.messages ? s : { messages };
    }),

  setUnread: (unread) => {
    if (get().unread !== unread) set({ unread });
  },

  setSaid: (authorId, said) => set((s) => ({ said: { ...s.said, [authorId]: said } })),

  clearSaid: (authorId, messageId) =>
    set((s) => {
      if (s.said[authorId]?.id !== messageId) return s;
      const said = { ...s.said };
      delete said[authorId];
      return { said };
    }),

  openCursorChat: () => set((s) => ({ chat: { ...s.chat, open: true, draft: "", sent: null } })),
  setDraft: (draft) => set((s) => ({ chat: { ...s.chat, draft } })),
  closeCursorChat: () => set((s) => (s.chat.open ? { chat: { ...s.chat, open: false, draft: "" } } : s)),
  markSent: (text) => {
    const token = sentSeq++;
    set((s) => ({ chat: { ...s.chat, open: false, draft: "", sent: { text, token } } }));
    return token;
  },
  clearSent: (token) => set((s) => (s.chat.sent?.token === token ? { chat: { ...s.chat, sent: null } } : s)),
  showHint: () => set((s) => ({ chat: { ...s.chat, hint: s.chat.hint + 1 } })),

  openDialog: (dialog, opts) => set({ dialog, askEnd: opts?.askEnd ?? false }),
  closeDialog: () => set({ dialog: null, askEnd: false }),
  setAskEnd: (askEnd) => set({ askEnd }),
  setAskLeave: (askLeave) => set({ askLeave }),
  setJoining: (joining) => set({ joining }),
  setEnded: (ended) => set({ ended }),
}));
