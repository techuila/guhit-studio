// Copilot conversation state. Lives outside the component so a chat survives
// a remount of the dock. The document itself stays in the app store.
import { create } from "zustand";
import type { AiMessage, AiProposal, AiSettings } from "../contract/bindings";
import { LOCK_HINT } from "../swarm/view";

export type ProposalStatus = "pending" | "applying" | "applied" | "discarded" | "stale";

/** One room of a "Furnish rooms" answer: its own proposal, applied on its own (D8). */
export interface FurnishRoom {
  roomId: string;
  roomName: string;
  proposal: AiProposal;
  status: ProposalStatus;
  note: string | null;
}

export interface ChatItem {
  id: number;
  /** "note" is a local system line (errors, stale notices). Never sent to the model. */
  role: "user" | "assistant" | "note";
  text: string;
  toolsUsed: string[];
  proposal: AiProposal | null;
  proposalStatus: ProposalStatus | null;
  /** Shown under a proposal card after it was resolved. */
  proposalNote: string | null;
  tone: "normal" | "error";
  /** A "Furnish rooms" answer: one proposal per room. Null for chat turns. */
  furnish?: FurnishRoom[] | null;
  /** The style the rooms were furnished in, to apply each one in it. */
  furnishStyle?: string;
}

interface CopilotState {
  /** Project the conversation belongs to. A different project starts clean. */
  projectId: string | null;
  items: ChatItem[];
  busy: boolean;
  /** Bumped to abandon the request in flight. Its answer is then ignored. */
  requestSeq: number;
  settings: AiSettings | null;
  settingsError: string | null;

  reset: (projectId: string | null) => void;
  push: (item: Omit<ChatItem, "id" | "toolsUsed" | "proposal" | "proposalStatus" | "proposalNote" | "tone"> & Partial<ChatItem>) => number;
  patch: (id: number, patch: Partial<ChatItem>) => void;
  setBusy: (busy: boolean) => void;
  nextRequest: () => number;
  setSettings: (settings: AiSettings | null, error?: string | null) => void;
}

let seq = 1;

export const useCopilot = create<CopilotState>((set, get) => ({
  projectId: null,
  items: [],
  busy: false,
  requestSeq: 0,
  settings: null,
  settingsError: null,

  reset: (projectId) => set({ projectId, items: [], busy: false, requestSeq: get().requestSeq + 1 }),

  push: (item) => {
    const id = seq++;
    set((s) => ({
      items: [
        ...s.items,
        {
          toolsUsed: [],
          proposal: null,
          proposalStatus: null,
          proposalNote: null,
          tone: "normal",
          ...item,
          id,
        },
      ],
    }));
    return id;
  },

  patch: (id, patch) => set((s) => ({ items: s.items.map((m) => (m.id === id ? { ...m, ...patch } : m)) })),
  setBusy: (busy) => set({ busy }),
  nextRequest: () => {
    const next = get().requestSeq + 1;
    set({ requestSeq: next });
    return next;
  },
  setSettings: (settings, error = null) => set({ settings, settingsError: error }),
}));

/** The pending proposal, if any. There is at most one. */
export function pendingItem(items: ChatItem[]): ChatItem | undefined {
  return items.find((m) => m.proposalStatus === "pending" || m.proposalStatus === "applying");
}

/** What the composer may start now, and why not, for the buttons' tip. */
export interface ComposerGate {
  canSend: boolean;
  canFurnish: boolean;
  tip: string | null;
}

/**
 * While a swarm draft is open the model is locked (D57) and the engine
 * refuses chat and furnish before any request, so Send and Furnish are off
 * with the lock's hint and nothing is spent.
 */
export function composerGate(o: { hasDoc: boolean; busy: boolean; noKey: boolean; locked: boolean; text: string }): ComposerGate {
  const can = o.hasDoc && !o.busy && !o.noKey && !o.locked;
  return { canSend: can && o.text.trim() !== "", canFurnish: can, tip: o.locked ? LOCK_HINT : null };
}

/** Conversation as the backend wants it: user and assistant text only. */
export function toHistory(items: ChatItem[]): AiMessage[] {
  return items
    .filter((m) => m.role !== "note" && m.text.trim() !== "")
    .map((m) => ({ role: m.role === "user" ? "user" : "assistant", text: m.text }));
}
