// Pure helpers for the live session UI (DECISIONS D29): participant names
// and colors, who is where, chat grouping and time labels, invites.
// No DOM, no store. Tested in format.test.ts.

import type { ChatMessage, LiveMode, LiveRelay, LiveStatus, NavMode, Participant, Presence } from "../contract/bindings";

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

/**
 * Titles that come before a name in the Philippines (G4 problem 10): a name
 * that starts with one keeps it and the name after it.
 */
const TITLES = new Set([
  "arch", "archt", "ar", "engr", "eng", "atty", "dr", "dra", "doc", "mr", "mrs", "ms", "miss", "sir", "maam", "prof", "hon", "rev", "fr", "sr", "sis", "gen", "col", "capt", "lt", "sgt", "pres", "gov", "mayor", "kap", "enp", "jd",
]);

function isTitle(word: string): boolean {
  return TITLES.has(word.toLowerCase().replace(/[.'’]/g, ""));
}

/**
 * The first word of a name, for tight spots: "Ana Reyes" is "Ana". A name
 * that starts with a title keeps the whole name, as people are addressed:
 * "Engr. Dela Cruz", "Arch. Ana Reyes" (G4 problem 10).
 */
export function firstName(name: string): string {
  const text = name.trim().replace(/\s+/g, " ");
  const words = text.split(" ");
  if (words.length > 1 && isTitle(words[0])) return text;
  return words[0] || text;
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

/**
 * Everyone in the session but this computer, in the host's order. People
 * only: agents (`Participant::agent`, DECISIONS D47) show apart, from
 * agents.ts, and never count as people.
 */
export function others(status: LiveStatus): Participant[] {
  return status.participants.filter((p) => p.id !== status.self_id && !p.agent);
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
 * People whose pointer is on the plan of `levelId`, in the host's order.
 * Someone on another level or off the plan shows no cursor. Agents have
 * their own cursors (`agentCursorIds`).
 */
export function cursorIds(status: LiveStatus, peers: Peers, levelId: string | null): string[] {
  if (status.mode === "off" || !levelId) return [];
  const out: string[] = [];
  for (const p of status.participants) {
    if (p.id === status.self_id || p.agent) continue;
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

/** What each other person has selected, for the plan's outlines. Agents' focus comes from agents.ts, in a session or not. */
export function peerSelections(status: LiveStatus, peers: Peers): PeerSelection[] {
  if (status.mode === "off") return [];
  const out: PeerSelection[] = [];
  for (const p of status.participants) {
    if (p.id === status.self_id || p.agent) continue;
    const ids = peers[p.id]?.presence.selection ?? [];
    if (ids.length > 0) out.push({ id: p.id, color: p.color, ids });
  }
  return out;
}

/**
 * Who follows participant `id` (DECISIONS D37), in the host's order. The
 * person followed sees them, and sends its plan area and camera while
 * there is anyone.
 */
export function followerIds(status: LiveStatus, peers: Peers, id: string | null): string[] {
  if (status.mode === "off" || !id) return [];
  const out: string[] = [];
  for (const p of status.participants) {
    if (p.id === status.self_id || p.id === id) continue;
    if (peers[p.id]?.presence.following === id) out.push(p.id);
  }
  return out;
}

/**
 * Following `from`'s chain of who follows whom reaches `target`. Following
 * someone whose chain comes back to you would loop: each window fits the
 * other's area to its own shape, so the views would keep growing.
 */
export function followLoops(status: LiveStatus, peers: Peers, from: string | null, target: string | null): boolean {
  if (!from || !target) return false;
  let at: string | null = from;
  for (let i = 0; at && i <= status.participants.length; i++) {
    if (at === target) return true;
    at = at === status.self_id ? null : (peers[at]?.presence.following ?? null);
  }
  return false;
}

/**
 * Someone can be followed: their presence says what they show. A version
 * before follow mode (0.3) never sends `view_mode`, so its presence has none
 * while live, and it would never send its plan area or camera either. Until
 * their first presence arrives, they count as followable.
 */
export function canFollow(peers: Peers, id: string): boolean {
  const pr = peers[id]?.presence;
  return !pr || (pr.view_mode ?? null) !== null;
}

/**
 * Two people clicked each other at once, so following loops back to this
 * window through `following`. Exactly one of the loop stops, the same one on
 * every computer: the one with the lowest id. True when that is this window.
 */
export function breaksFollowLoop(status: LiveStatus, peers: Peers, following: string | null): boolean {
  const self = status.self_id;
  if (!self || !following) return false;
  const ids = [self];
  let at: string | null = following;
  while (at && at !== self && ids.length <= status.participants.length) {
    ids.push(at);
    at = peers[at]?.presence.following ?? null;
  }
  return at === self && ids.every((id) => self <= id);
}

/**
 * Tooltip of an avatar in the top bar: "Ana Reyes, host. Click to follow",
 * "Following Ana. Click to stop", "Ben is following you", or that their
 * version cannot be followed.
 */
export function avatarFollowTip(p: Participant, selfId: string | null, followed: boolean, followsMe: boolean, followable = true): string {
  const first = firstName(p.name);
  if (followed) return `Following ${first}. Click to stop`;
  if (followsMe) return `${first} is following you`;
  if (!followable) return `${avatarTip(p, selfId)}. ${possessive(first)} version of Guhit Studio cannot be followed`;
  return `${avatarTip(p, selfId)}. Click to follow`;
}

/** The toast when someone's version cannot be followed. */
export function oldVersionLine(name: string): string {
  const first = firstName(name);
  return `${possessive(first)} version of Guhit Studio cannot be followed. Ask ${first} to update.`;
}

/** "Ana is walking" while the person followed walks or flies in 3D, null otherwise. */
export function navLine(name: string, nav: NavMode | null | undefined): string | null {
  if (nav === "walk") return `${firstName(name)} is walking`;
  if (nav === "fly") return `${firstName(name)} is flying`;
  return null;
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
  /** The host also takes guests through a relay (DECISIONS D32): the invite works over the internet. */
  relay: boolean;
  /** The host takes direct connections. Where it listens is never shown: version 3 invites seal the addresses. */
  direct: boolean;
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
 * the project name, whether a relay carries it and whether the host takes
 * direct connections, never where the host is. Null when the text is not a
 * Guhit invite. The secret, pin, seal and relay are checked by the engine,
 * not here.
 */
export function parseInvite(text: string): InviteInfo | null {
  const t = text.trim();
  if (!t.startsWith(INVITE_PREFIX)) return null;
  const json = fromBase64Url(t.slice(INVITE_PREFIX.length).trim());
  if (json === null) return null;
  try {
    const v = JSON.parse(json) as Record<string, unknown>;
    if (typeof v !== "object" || v === null || typeof v.secret !== "string" || typeof v.pin !== "string") return null;
    // Version 3 seals the addresses; versions 1 and 2 list them in plain text.
    const sealed = typeof v.v === "number" && v.v >= 3;
    const direct = sealed
      ? typeof v.sealed === "string" && v.sealed !== ""
      : Array.isArray(v.addrs) && v.addrs.some((a) => typeof a === "string");
    const r = v.relay as Record<string, unknown> | null | undefined;
    const relay = typeof r === "object" && r !== null && typeof r.url === "string" && typeof r.room === "string";
    return { project: typeof v.project === "string" && v.project.trim() !== "" ? v.project : null, relay, direct };
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------- reach

/**
 * Who can use a host's invite right now (DECISIONS D32): anyone over the
 * internet once the relay has the session, people on the host's network or
 * VPN when it listens for direct connections, else no one until the relay
 * is back.
 */
export type Reach = "internet" | "connecting" | "network" | "nowhere";

export function reachOf(relay: LiveRelay, direct: boolean): Reach {
  if (relay === "ready") return "internet";
  if (relay === "connecting") return "connecting";
  return direct ? "network" : "nowhere";
}
