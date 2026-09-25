// Dev only, loaded by src/shell/devHarness.ts in mock mode (?mock=1). Never
// part of a production build: devHarness itself only loads in dev.
//
// Answers the live session IPC calls in the browser the way the engine does
// (docs/CONTRACT.md, "Live sessions"), and pushes app events through the
// mock's event stream. With ?live=... it starts in a session and two people
// move, select and chat on the sample plan, so the UI can be looked at and
// screenshotted without a second computer:
//   ?live=demo          hosting, Ana and Ben joined
//   ?live=guest         joined Ana's session
//   ?live=reconnecting  joined, the connection to Ana dropped
//   ?live=off           no session, the project keeps a past chat (default)
// `window.__liveDemo` drives it from the console or a ui-check step file:
// setMode, pause, resume, move, select, type, say, leave.

import type { AppEvent, ChatMessage, DocState, IpcError, LiveMode, LiveStatus, Participant, Point, Presence, ProjectMeta } from "../../contract/bindings";

export interface LiveMockHost {
  /** The mock's IPC handler table: live handlers are added to it. */
  handlers: Record<string, (args: Record<string, unknown>) => unknown>;
  /** Pushes an app event to every open event stream. */
  emit: (event: AppEvent) => void;
  /** Opens the fixture in the mock engine, as `hub_open` does. */
  openShared: () => DocState;
  /** Closes the mock engine's document, as `hub_close` does. */
  closeShared: () => void;
  /** The level and a few element ids of the fixture, for the script. */
  levelId: string;
  projectId: string;
  projectName: string;
  wallIds: string[];
  roomIds: string[];
}

type DemoMode = "off" | "demo" | "guest" | "reconnecting";

function fail(code: string, message: string): never {
  throw { code, message, element_ids: [] } satisfies IpcError;
}

function b64url(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

const SELF = "p-self";
const ANA = "p-ana";
const BEN = "p-ben";

export function installLiveMock(host: LiveMockHost, initial: string | null): void {
  let profile = "Mara Santos";
  let mode: LiveMode = "off";
  let participants: Participant[] = [];
  let notice: string | null = null;
  const peers = new Map<string, Presence>();
  let paused = false;
  let seq = 1;
  const t0 = Date.now();
  const iso = (agoMs: number) => new Date(Date.now() - agoMs).toISOString();

  const invite = `guhit-live:${b64url(JSON.stringify({ v: 1, secret: "q7Zr0cXlUz3k9vJ2m4NwPg", pin: "3uJ0n8x2vVqk1c5Y7mR9sT4wZ6aB0dE2fG4hJ6kL8nP", addrs: ["192.168.1.20:1460", "127.0.0.1:1460"], project: host.projectName }))}`;

  const chat: ChatMessage[] = [
    { id: "m-1", author_id: ANA, author_name: "Ana Reyes", color: 1, text: "I moved the bedroom door so the bed fits the long wall.", sent_at: iso(26 * 3600e3), at: null, level_id: null, via_ai: false },
    { id: "m-2", author_id: SELF, author_name: "Mara Santos", color: 0, text: "Good. Keep the 900 door, the client wants a wider one there.", sent_at: iso(26 * 3600e3 - 60e3), at: null, level_id: null, via_ai: false },
    { id: "m-3", author_id: BEN, author_name: "Ben Cruz", color: 2, text: "Here, the window could go wider for the morning sun.", sent_at: iso(26 * 3600e3 - 3 * 60e3), at: { x: 7200, y: 5600 }, level_id: host.levelId, via_ai: false },
    { id: "m-4", author_id: BEN, author_name: "Ben Cruz", color: 2, text: "Summary: 2 rooms, 48 m² gross. The bedroom has one window.", sent_at: iso(26 * 3600e3 - 2 * 60e3), at: null, level_id: null, via_ai: true },
  ];

  const status = (): LiveStatus => ({
    mode,
    self_id: mode === "off" ? null : SELF,
    participants: mode === "off" ? [] : participants,
    invite: mode === "hosting" ? invite : null,
    addresses: mode === "hosting" ? ["192.168.1.20:1460", "127.0.0.1:1460"] : [],
    project_id: mode === "off" ? null : host.projectId,
    project_name: mode === "off" ? null : host.projectName,
    notice,
  });

  const emitStatus = () => host.emit({ type: "live", status: status() });
  const presence = (over: Partial<Presence>): Presence => ({ cursor: null, level_id: host.levelId, selection: [], typing: null, ai_scope: false, ...over });
  const setPeer = (id: string, over: Partial<Presence>) => {
    if (mode === "off" || !participants.some((p) => p.id === id)) return;
    const next = { ...(peers.get(id) ?? presence({})), ...over };
    peers.set(id, next);
    host.emit({ type: "presence", participant_id: id, presence: next });
  };
  const say = (id: string, text: string, at: Point | null = null, viaAi = false) => {
    const p = participants.find((x) => x.id === id);
    if (!p || (mode !== "hosting" && mode !== "joined")) return;
    const message: ChatMessage = { id: `m-${Date.now()}-${seq++}`, author_id: p.id, author_name: p.name, color: p.color, text, sent_at: new Date().toISOString(), at, level_id: at ? host.levelId : null, via_ai: viaAi };
    chat.push(message);
    host.emit({ type: "chat", message });
  };

  const enter = (next: DemoMode) => {
    notice = null;
    peers.clear();
    if (next === "off") {
      mode = "off";
      participants = [];
    } else if (next === "demo") {
      mode = "hosting";
      participants = [
        { id: SELF, name: profile, color: 0, role: "host" },
        { id: ANA, name: "Ana Reyes", color: 1, role: "guest" },
        { id: BEN, name: "Ben Cruz", color: 2, role: "guest" },
      ];
    } else {
      mode = next === "reconnecting" ? "reconnecting" : "joined";
      participants = [
        { id: ANA, name: "Ana Reyes", color: 1, role: "host" },
        { id: SELF, name: profile, color: 0, role: "guest" },
        { id: BEN, name: "Ben Cruz", color: 2, role: "guest" },
      ];
    }
    emitStatus();
    if (mode !== "off") {
      setPeer(ANA, { cursor: { x: 2200, y: 3600 } });
      setPeer(BEN, { cursor: { x: 6600, y: 1800 } });
    }
  };

  // ------------------------------------------------ IPC

  Object.assign(host.handlers, {
    profile_get: () => ({ name: profile }),
    profile_set: (a: Record<string, unknown>) => {
      const name = String(a.name ?? "").replace(/\s+/g, " ").trim().slice(0, 40);
      if (!name) fail("bad_args", "A name needs at least one letter or number.");
      profile = name;
      participants = participants.map((p) => (p.id === SELF ? { ...p, name } : p));
      return { name };
    },
    presence_set: () => null,
    presence_list: () => (mode === "off" ? [] : [...peers].map(([participant_id, presence]) => ({ participant_id, presence }))),
    live_status: () => status(),
    live_host: async () => {
      if (!profile) fail("bad_args", "Pick a name first. Others see it next to your pointer.");
      await new Promise((r) => setTimeout(r, 450));
      mode = "hosting";
      notice = null;
      participants = [{ id: SELF, name: profile, color: 0, role: "host" }];
      emitStatus();
      // Someone joins a moment later.
      setTimeout(() => {
        if (mode !== "hosting" || participants.some((p) => p.id === ANA)) return;
        participants = [...participants, { id: ANA, name: "Ana Reyes", color: 1, role: "guest" }];
        emitStatus();
        setPeer(ANA, { cursor: { x: 2200, y: 3600 } });
      }, 2500);
      return status();
    },
    live_join: async (a: Record<string, unknown>) => {
      if (!profile) fail("bad_args", "Pick a name first. Others see it next to your pointer.");
      await new Promise((r) => setTimeout(r, 900));
      const text = String(a.invite ?? "");
      if (!text.startsWith("guhit-live:")) fail("live_refused", "That invite is not for a live session.");
      let project = "";
      try {
        const b64 = text.slice("guhit-live:".length).replace(/-/g, "+").replace(/_/g, "/");
        project = String((JSON.parse(atob(b64 + "=".repeat((4 - (b64.length % 4)) % 4))) as { project?: string }).project ?? "");
      } catch {
        fail("live_refused", "That invite is not for a live session.");
      }
      // A project named "... unreachable" tries the failure path.
      if (/unreachable/i.test(project)) fail("live_unreachable", "No address in the invite answered. Check that you are on the same network or VPN as the host.");
      const doc = host.openShared();
      enter("guest");
      return doc;
    },
    live_leave: () => {
      const wasGuest = mode === "joined" || mode === "reconnecting";
      enter("off");
      if (wasGuest) host.closeShared();
      return status();
    },
    live_remove: (a: Record<string, unknown>) => {
      if (mode !== "hosting") fail("host_only", "Only the host can remove people.");
      participants = participants.filter((p) => p.id !== a.participant_id);
      peers.delete(String(a.participant_id));
      host.emit({ type: "presence", participant_id: String(a.participant_id), presence: null });
      emitStatus();
      return status();
    },
    live_save_copy: (): ProjectMeta => ({ id: "p-copy", name: `${host.projectName} (copy)`, created_at: new Date().toISOString(), updated_at: new Date().toISOString(), floor_area_m2: 45.34, room_count: 2, thumbnail: null }),
    chat_send: (a: Record<string, unknown>) => {
      if (mode !== "hosting" && mode !== "joined") fail(mode === "reconnecting" ? "live_lost" : "not_live", mode === "reconnecting" ? "The connection to the host dropped. Trying again." : "Start or join a live session to chat.");
      const text = String(a.text ?? "").trim();
      const at = (a.at as Point | null) ?? null;
      if (!text || [...text].length > (at ? 160 : 2000)) fail("bad_args", "A message is 1 to 2000 characters, 160 in cursor chat.");
      const self = participants.find((p) => p.id === SELF);
      const message: ChatMessage = { id: `m-${Date.now()}-${seq++}`, author_id: SELF, author_name: self?.name ?? profile, color: self?.color ?? 0, text, sent_at: new Date().toISOString(), at, level_id: (a.level_id as string | null) ?? null, via_ai: false };
      chat.push(message);
      setTimeout(() => host.emit({ type: "chat", message }), 30);
      return message;
    },
    chat_list: () => chat.slice(-500),
  });

  // ------------------------------------------------ the demo script

  const wall = host.wallIds[4] ?? host.wallIds[0];
  const room = host.roomIds[1] ?? host.roomIds[0];
  const anaLine = "Can we move this wall 300 to the east?";
  let lastTyped = -1;
  let lastBeat = -1;
  let benSaid = false;

  const tick = () => {
    if (paused || (mode !== "hosting" && mode !== "joined")) return;
    const t = ((Date.now() - t0) / 1000) % 30;
    // Ana: wanders the living room, stops at the partition, asks, moves on.
    if (t < 6 || t >= 12) {
      const u = (Date.now() - t0) / 1000;
      setPeer(ANA, { cursor: { x: Math.round(2500 + 1700 * Math.sin(u * 0.55)), y: Math.round(3000 + 1600 * Math.sin(u * 0.83 + 1)) } });
    } else if (t < 11) {
      if (lastBeat !== 1) {
        lastBeat = 1;
        setPeer(ANA, { cursor: { x: 4920, y: 2400 }, selection: [wall] });
      }
      const n = Math.min(anaLine.length, Math.floor((t - 6.6) * 11));
      if (n >= 0 && n !== lastTyped) {
        lastTyped = n;
        setPeer(ANA, { typing: anaLine.slice(0, n) });
      }
    } else if (lastBeat !== 2) {
      lastBeat = 2;
      lastTyped = -1;
      setPeer(ANA, { typing: null });
      say(ANA, anaLine, { x: 4920, y: 2400 });
    }
    if (t >= 20 && lastBeat === 2) {
      lastBeat = 3;
      setPeer(ANA, { selection: [room] });
    }
    if (t < 1 && lastBeat === 3) {
      lastBeat = 0;
      setPeer(ANA, { selection: [] });
    }
    // Ben: a slow loop in the bedroom, a word in the chat panel, then a long
    // pause (his cursor dims).
    const b = ((Date.now() - t0) / 1000) % 60;
    if (b < 14) {
      benSaid = false;
      const u = (Date.now() - t0) / 1000;
      setPeer(BEN, { cursor: { x: Math.round(6500 + 900 * Math.cos(u * 0.4)), y: Math.round(2600 + 1200 * Math.sin(u * 0.4)) } });
    } else if (!benSaid) {
      benSaid = true;
      say(BEN, "Looks good. I will add the kitchen window after lunch.");
    }
  };
  window.setInterval(tick, 50);

  // ------------------------------------------------ console and step files

  (window as unknown as { __liveDemo: unknown }).__liveDemo = {
    setMode: (m: DemoMode) => enter(m),
    pause: () => {
      paused = true;
    },
    resume: () => {
      paused = false;
    },
    move: (who: "ana" | "ben", x: number, y: number) => setPeer(who === "ana" ? ANA : BEN, { cursor: { x, y } }),
    offPlan: (who: "ana" | "ben") => setPeer(who === "ana" ? ANA : BEN, { cursor: null }),
    select: (who: "ana" | "ben", ids: string[]) => setPeer(who === "ana" ? ANA : BEN, { selection: ids }),
    type: (who: "ana" | "ben", text: string | null) => setPeer(who === "ana" ? ANA : BEN, { typing: text }),
    say: (who: "ana" | "ben", text: string, at: Point | null = null) => say(who === "ana" ? ANA : BEN, text, at),
    /** The host ends the session (as a guest sees it), or someone leaves. */
    end: () => {
      const wasGuest = mode === "joined" || mode === "reconnecting";
      mode = "off";
      participants = [];
      peers.clear();
      notice = "Ana Reyes ended the live session.";
      emitStatus();
      if (wasGuest) host.closeShared();
    },
    leave: (who: "ana" | "ben") => {
      const id = who === "ana" ? ANA : BEN;
      participants = participants.filter((p) => p.id !== id);
      peers.delete(id);
      host.emit({ type: "presence", participant_id: id, presence: null });
      emitStatus();
    },
    walls: host.wallIds,
    rooms: host.roomIds,
  };

  // Before the app loads: its first live_status and presence_list see the session.
  const start = (initial ?? "off") as DemoMode;
  if (start === "demo" || start === "guest" || start === "reconnecting") enter(start);
}
