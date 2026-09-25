// Pure helpers for the live session UI (DECISIONS D29): participant names
// and colors, who is where, chat grouping and time labels, invites.
// No DOM, no store. Tested in format.test.ts.

import type { ChatMessage, LiveMode, LiveStatus, Participant, Presence } from "../contract/bindings";

/** Participant colors: `Participant::color` indexes `--peer-0` to `--peer-7`. */
export const PEER_COLORS = 8;

/** Longest cursor chat message, as the engine cuts it (`MAX_CURSOR_CHAT_CHARS`). */
export const CURSOR_CHAT_MAX = 160;

/** Longest chat panel message (`MAX_CHAT_CHARS`). */
export const CHAT_MAX = 2000;

/** Longest display name (`MAX_NAME_CHARS`). */
export const NAME_MAX = 40;

/** The token index of a participant color, wrapped into 0..7. */
export function peerIndex(color: number): number {
  const n = Number.isFinite(color) ? Math.trunc(color) : 0;
  return ((n % PEER_COLORS) + PEER_COLORS) % PEER_COLORS;
}

/** The CSS color of a participant: `var(--peer-N)`. */
export function peerVar(color: number): string {
  return `var(--peer-${peerIndex(color)})`;
}

/** First code point of a word, upper case. Keeps emoji and accents whole. */
function firstChar(word: string): string {
  return ([...word][0] ?? "").toUpperCase();
}

/** Up to two initials for an avatar: "Ana Reyes" is "AR", "ana" is "A". */
export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  if (words.length === 1) return firstChar(words[0]);
  return firstChar(words[0]) + firstChar(words[words.length - 1]);
}

/** The first word of a name, for tight spots: "Ana Reyes" is "Ana". */
export function firstName(name: string): string {
  return name.trim().split(/\s+/)[0] || name.trim();
}

/** A name as the profile stores it: control characters out, spaces folded, at most 40 characters. */
export function cleanName(raw: string): string {
  // eslint-disable-next-line no-control-regex
  const text = raw.replace(/[\u0000-\u001f\u007f]/g, "").replace(/\s+/g, " ").trim();
  return [...text].slice(0, NAME_MAX).join("");
}

/** Hosting or joined: chat and cursor chat work. Reconnecting edits and messages wait. */
export function canChat(mode: LiveMode): boolean {
  return mode === "hosting" || mode === "joined";
}

/** In a live session in any way, reconnecting included. */
export function isLive(mode: LiveMode): boolean {
  return mode !== "off";
}

export function participant(status: LiveStatus, id: string | null | undefined): Participant | null {
  if (!id) return null;
  return status.participants.find((p) => p.id === id) ?? null;
}

/** Everyone in the session but this computer, in the host's order. */
export function others(status: LiveStatus): Participant[] {
  return status.participants.filter((p) => p.id !== status.self_id);
}

export function host(status: LiveStatus): Participant | null {
  return status.participants.find((p) => p.role === "host") ?? null;
}

/** "Ana" as a possessive for sentences: "Ana's". */
export function possessive(name: string): string {
  const n = firstName(name);
  return /s$/i.test(n) ? `${n}'` : `${n}'s`;
}

/** Tooltip of an avatar: "Ana Reyes, host", "Ben", "Mara, you". */
export function avatarTip(p: Participant, selfId: string | null): string {
  const tags = [p.role === "host" ? "host" : null, p.id === selfId ? "you" : null].filter(Boolean);
  return tags.length > 0 ? `${p.name}, ${tags.join(", ")}` : p.name;
}

/**
 * Who made the step on top of the history, for the undo and redo tooltips:
 * a name only when it is someone else in a live session. A participant who
 * left is still named from their chat messages.
 */
export function stepAuthor(status: LiveStatus, by: string | null | undefined, messages: readonly ChatMessage[]): string | null {
  if (!by || status.mode === "off" || by === status.self_id) return null;
  const p = participant(status, by);
  if (p) return p.name;
  for (let i = messages.length - 1; i >= 0; i--) if (messages[i].author_id === by) return messages[i].author_name;
  return "someone else";
}

// ---------------------------------------------------------------- presence

/** Another participant's latest presence and when it last changed (ms). */
export interface PeerPresence {
  presence: Presence;
  changedAt: number;
}

export type Peers = Readonly<Record<string, PeerPresence>>;

/**
 * Participants whose pointer is on the plan of `levelId`, in the host's
 * order. Someone on another level or off the plan shows no cursor.
 */
export function cursorIds(status: LiveStatus, peers: Peers, levelId: string | null): string[] {
  if (status.mode === "off" || !levelId) return [];
  const out: string[] = [];
  for (const p of status.participants) {
    if (p.id === status.self_id) continue;
    const pr = peers[p.id]?.presence;
    if (pr?.cursor && pr.level_id === levelId) out.push(p.id);
  }
  return out;
}

export interface PeerSelection {
  id: string;
  color: number;
  ids: string[];
}

/** What each other participant has selected, for the plan's outlines. */
export function peerSelections(status: LiveStatus, peers: Peers): PeerSelection[] {
  if (status.mode === "off") return [];
  const out: PeerSelection[] = [];
  for (const p of status.participants) {
    if (p.id === status.self_id) continue;
    const ids = peers[p.id]?.presence.selection ?? [];
    if (ids.length > 0) out.push({ id: p.id, color: p.color, ids });
  }
  return out;
}

/** Same selections in the same colors: the plan need not redraw. */
export function sameSelections(a: readonly PeerSelection[], b: readonly PeerSelection[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const x = a[i];
    const y = b[i];
    if (x.id !== y.id || x.color !== y.color || x.ids.length !== y.ids.length) return false;
    for (let j = 0; j < x.ids.length; j++) if (x.ids[j] !== y.ids[j]) return false;
  }
  return true;
}

// ---------------------------------------------------------------- chat

export interface ChatGroup {
  /** The first message's id. */
  key: string;
  authorId: string;
  name: string;
  color: number;
  viaAi: boolean;
  mine: boolean;
  /** When the first message of the run was sent. */
  sentAt: string;
  messages: ChatMessage[];
}

/** Messages this close together from one person share a header. */
export const GROUP_GAP_MS = 5 * 60_000;

function dayKey(t: number): string {
  const d = new Date(t);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

/**
 * Runs of messages from one person group under one header. A run breaks on
 * another author, an AI message among human ones, a pause longer than
 * `gapMs` or a new day.
 */
export function groupMessages(messages: readonly ChatMessage[], selfId: string | null, gapMs = GROUP_GAP_MS): ChatGroup[] {
  const out: ChatGroup[] = [];
  let lastAt = Number.NaN;
  for (const m of messages) {
    const t = Date.parse(m.sent_at);
    const prev = out[out.length - 1];
    const joins =
      prev !== undefined &&
      prev.authorId === m.author_id &&
      prev.viaAi === m.via_ai &&
      Number.isFinite(t) &&
      Number.isFinite(lastAt) &&
      t - lastAt <= gapMs &&
      t >= lastAt &&
      dayKey(t) === dayKey(lastAt);
    if (joins) prev.messages.push(m);
    else
      out.push({
        key: m.id,
        authorId: m.author_id,
        name: m.author_name,
        color: m.color,
        viaAi: m.via_ai,
        mine: selfId !== null && m.author_id === selfId,
        sentAt: m.sent_at,
        messages: [m],
      });
    lastAt = t;
  }
  return out;
}

/** Adds messages by id, keeping the order they were sent in. */
export function mergeMessages(current: readonly ChatMessage[], incoming: readonly ChatMessage[], max = 500): ChatMessage[] {
  const seen = new Set(current.map((m) => m.id));
  const added: ChatMessage[] = [];
  for (const m of incoming) {
    if (seen.has(m.id)) continue;
    seen.add(m.id);
    added.push(m);
  }
  if (added.length === 0) return current as ChatMessage[];
  const all = [...current, ...added];
  // Stable by time: equal or unreadable stamps keep their arrival order.
  const indexed = all.map((m, i) => ({ m, i, t: Date.parse(m.sent_at) }));
  indexed.sort((a, b) => (Number.isFinite(a.t) && Number.isFinite(b.t) && a.t !== b.t ? a.t - b.t : a.i - b.i));
  const sorted = indexed.map((x) => x.m);
  return sorted.length > max ? sorted.slice(sorted.length - max) : sorted;
}

function clock(d: Date): string {
  // Newer ICU puts a narrow no-break space before AM and PM.
  return d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" }).replace(/\s/g, " ");
}

/**
 * When a message was sent, as the chat header shows it: "9:41 AM" today,
 * "Yesterday 9:41 AM", "Sep 20, 9:41 AM" this year, "Sep 20, 2025" before.
 */
export function timeLabel(iso: string, now: Date = new Date()): string {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return "";
  const d = new Date(t);
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const day = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  if (day === today) return clock(d);
  // Rounded, so a day with a daylight saving change still counts as one.
  if (Math.round((today - day) / 86_400_000) === 1) return `Yesterday ${clock(d)}`;
  if (d.getFullYear() === now.getFullYear()) return `${d.toLocaleDateString("en-US", { month: "short", day: "numeric" })}, ${clock(d)}`;
  return d.toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });
}

/** The full stamp for a tooltip: "Sep 25, 2026, 9:41 AM". */
export function fullTime(iso: string): string {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return "";
  const d = new Date(t);
  return `${d.toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" })}, ${clock(d)}`;
}

// ---------------------------------------------------------------- invites

export const INVITE_PREFIX = "guhit-live:";

export interface InviteInfo {
  project: string | null;
  addrs: string[];
}

function fromBase64Url(text: string): string | null {
  if (!/^[A-Za-z0-9_-]+$/.test(text)) return null;
  const b64 = text.replace(/-/g, "+").replace(/_/g, "/");
  const padded = b64 + "=".repeat((4 - (b64.length % 4)) % 4);
  try {
    const bin = atob(padded);
    return new TextDecoder().decode(Uint8Array.from(bin, (c) => c.charCodeAt(0)));
  } catch {
    return null;
  }
}

/**
 * Reads what an invite says about the session, to show before joining:
 * the project name and the host's addresses. Null when the text is not a
 * Guhit invite. The secret and pin are checked by the engine, not here.
 */
export function parseInvite(text: string): InviteInfo | null {
  const t = text.trim();
  if (!t.startsWith(INVITE_PREFIX)) return null;
  const json = fromBase64Url(t.slice(INVITE_PREFIX.length).trim());
  if (json === null) return null;
  try {
    const v = JSON.parse(json) as Record<string, unknown>;
    if (typeof v !== "object" || v === null || typeof v.secret !== "string" || typeof v.pin !== "string") return null;
    const addrs = Array.isArray(v.addrs) ? v.addrs.filter((a): a is string => typeof a === "string") : [];
    return { project: typeof v.project === "string" && v.project.trim() !== "" ? v.project : null, addrs };
  } catch {
    return null;
  }
}
