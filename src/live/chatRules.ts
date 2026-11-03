// The rules of unread chat notifications (DECISIONS D41), as pure functions:
// what counts as unread, when a burst of messages starts, what the peek card
// shows and how its cards merge. No DOM, no store. Tested in chatRules.test.ts.

import type { ChatMessage, ParticipantRole } from "../contract/bindings";
import { firstName } from "./format";

/** How much of a message the peek card shows. */
export const PEEK_CHARS = 80;

/** How long a peek card stays once nothing holds it (hover, focus, an unfocused window). */
export const PEEK_MS = 6000;

/** At most this many cards at once: a third sender pushes the oldest out. */
export const PEEK_MAX = 2;

/**
 * Messages that come within this long of the previous unread one belong to
 * the same burst: the window asks for attention once for all of them.
 */
export const BURST_GAP_MS = 30_000;

/** A message is read only when the window is focused and Chat is on screen. */
export function isUnread(focused: boolean, chatOnScreen: boolean): boolean {
  return !(focused && chatOnScreen);
}

/**
 * True when this message starts a new burst: it is the first unread one, or
 * the last one was more than `gapMs` ago. `lastAt` is when the previous
 * unread message arrived (ms), null after the window was focused.
 */
export function isNewBurst(lastAt: number | null, now: number, gapMs: number = BURST_GAP_MS): boolean {
  if (lastAt === null) return true;
  // A clock that went back starts a new burst rather than waiting for it.
  return now < lastAt || now - lastAt > gapMs;
}

/** What the Dock icon shows: the count, or nothing at zero. */
export function badgeCount(unread: number): number | undefined {
  return unread > 0 ? unread : undefined;
}

/**
 * The start of a message for the card: whitespace folded to single spaces,
 * cut at `max` characters (code points, so emoji stay whole) with "...".
 */
export function peekText(text: string, max: number = PEEK_CHARS): string {
  const flat = text.replace(/\s+/g, " ").trim();
  const chars = [...flat];
  if (chars.length <= max) return flat;
  return `${chars.slice(0, max).join("").trimEnd()}...`;
}

export interface PeekView {
  /** The window has focus (a cursor chat bubble is only seen then). */
  focused: boolean;
  /** The editor shows the plan: the 2D or the split view. */
  planOnScreen: boolean;
  /** The level the plan shows. */
  levelId: string | null;
}

/**
 * A cursor chat message already shows beside its author's pointer while the
 * plan is on screen on its level and someone is looking: no card for it.
 */
export function skipPeek(message: Pick<ChatMessage, "at" | "level_id">, view: PeekView): boolean {
  if (!message.at) return false;
  return view.focused && view.planOnScreen && view.levelId !== null && message.level_id === view.levelId;
}

/** One card: a sender and what they sent while it showed. */
export interface PeekCard {
  authorId: string;
  name: string;
  color: number;
  role: ParticipantRole;
  /** Sent by an AI client on the author's behalf. */
  viaAi: boolean;
  /** The start of the latest message. */
  text: string;
  /** Messages from this sender since the card appeared. */
  count: number;
  /** The latest message's id: a new one re-keys the text so it fades in. */
  lastId: string;
  /** Counts up on every update: restarts the card's timer. */
  stamp: number;
}

/**
 * The cards after a message arrived. The same sender updates their card in
 * place (more messages, the newest text); another sender adds a card below,
 * and the oldest leaves when there are more than `max`.
 */
export function addPeek(cards: readonly PeekCard[], message: ChatMessage, role: ParticipantRole, max: number = PEEK_MAX): PeekCard[] {
  const at = cards.findIndex((c) => c.authorId === message.author_id);
  const text = peekText(message.text);
  if (at >= 0) {
    const old = cards[at];
    const next = cards.slice();
    next[at] = {
      ...old,
      name: message.author_name,
      color: message.color,
      role,
      viaAi: message.via_ai,
      text,
      count: old.count + 1,
      lastId: message.id,
      stamp: old.stamp + 1,
    };
    return next;
  }
  const card: PeekCard = {
    authorId: message.author_id,
    name: message.author_name,
    color: message.color,
    role,
    viaAi: message.via_ai,
    text,
    count: 1,
    lastId: message.id,
    stamp: 0,
  };
  const all = [...cards, card];
  return all.length > max ? all.slice(all.length - max) : all;
}

/** Removes a sender's card. The same array comes back when there was none. */
export function dropPeek(cards: readonly PeekCard[], authorId: string): PeekCard[] {
  return cards.some((c) => c.authorId === authorId) ? cards.filter((c) => c.authorId !== authorId) : (cards as PeekCard[]);
}

/** The card's heading for a screen reader: "Ana Reyes", "Ana Reyes, 3 new". */
export function peekHeading(card: Pick<PeekCard, "name" | "count">): string {
  return card.count > 1 ? `${card.name}, ${card.count} new` : card.name;
}

/** The count pill's text, shown from the second message on: "3 new". */
export function newLabel(count: number): string | null {
  return count > 1 ? `${count} new` : null;
}

/** The Chat button's label and tooltip: "Chat", "Chat, 3 unread messages". */
export function chatButtonLabel(unread: number): string {
  if (unread <= 0) return "Chat";
  return `Chat, ${unread} unread ${unread === 1 ? "message" : "messages"}`;
}

/** Short name for a tight spot: the card's close button label. */
export function dismissLabel(card: Pick<PeekCard, "name">): string {
  return `Dismiss the message from ${firstName(card.name)}`;
}
