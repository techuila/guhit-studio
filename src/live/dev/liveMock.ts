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
// setMode, setRelay, pause, resume, move, select, type, say, leave, and for
// follow mode (DECISIONS D37): view, walk, follow. Ana works in split view
// and Ben on the plan; like the engine's peers, they send their plan area and
// camera only while this window follows them.

import type { AppEvent, ChatMessage, DocState, IpcError, LiveCamera, LiveMode, LiveRelay, LiveStatus, NavMode, Participant, PlanView, Point, Presence, ProjectMeta, Vec3, ViewMode } from "../../contract/bindings";

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
  /** The hosted session's registration with the relay (DECISIONS D32). */
  let relay: LiveRelay = "off";
  let participants: Participant[] = [];
  let notice: string | null = null;
  const peers = new Map<string, Presence>();
  /** What each peer would send while followed: their plan area and camera. */
  const views = new Map<string, { plan: PlanView | null; camera: LiveCamera | null }>();
  /** Who this window follows, from its last `presence_set`. */
  let selfFollowing: string | null = null;
  let paused = false;
  let seq = 1;
  const t0 = Date.now();
  const iso = (agoMs: number) => new Date(Date.now() - agoMs).toISOString();

  const invite = `guhit-live:${b64url(JSON.stringify({ v: 2, secret: "q7Zr0cXlUz3k9vJ2m4NwPg", pin: "3uJ0n8x2vVqk1c5Y7mR9sT4wZ6aB0dE2fG4hJ6kL8nP", addrs: ["192.168.1.20:1460", "100.101.7.12:1460", "127.0.0.1:1460"], project: host.projectName, relay: { url: "wss://relay.example.com", room: "Xo3Wm9qTz1Lr8Vb2Nc5Ydg" } }))}`;

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
    addresses: mode === "hosting" ? ["192.168.1.20:1460", "100.101.7.12:1460", "127.0.0.1:1460"] : [],
    relay: mode === "hosting" ? relay : "off",
    project_id: mode === "off" ? null : host.projectId,
    project_name: mode === "off" ? null : host.projectName,
    notice,
  });

  const emitStatus = () => host.emit({ type: "live", status: status() });
  const presence = (over: Partial<Presence>): Presence => ({
    cursor: null,
    level_id: host.levelId,
    selection: [],
    typing: null,
    ai_scope: false,
    following: null,
    view_mode: "2d",
    plan_view: null,
    camera: null,
    pointer_3d: null,
    ...over,
  });
  /** A peer's plan area and camera ride along only while this window follows them. */
  const withViews = (id: string, p: Presence): Presence => {
    const v = selfFollowing === id ? views.get(id) : undefined;
    const plan = p.view_mode !== "3d" ? (v?.plan ?? null) : null;
    const camera = p.view_mode !== "2d" ? (v?.camera ?? null) : null;
    return { ...p, plan_view: plan, camera };
  };
  const setPeer = (id: string, over: Partial<Presence>) => {
    if (mode === "off" || !participants.some((p) => p.id === id)) return;
    const next = withViews(id, { ...(peers.get(id) ?? presence({})), ...over });
    const before = peers.get(id);
    peers.set(id, next);
    if (before && JSON.stringify(before) === JSON.stringify(next)) return;
    host.emit({ type: "presence", participant_id: id, presence: next });
  };
  /** The area around a pointer, 9 by 6 m, as a peer's screen would show it. */
  const areaAt = (c: Point): PlanView => ({ center: { x: c.x, y: c.y }, width_mm: 9000, height_mm: 6000 });
  /** An orbit camera circling the plan at `u` radians, looking at the middle of the house. */
  const orbitAt = (u: number, nav: NavMode = "orbit"): LiveCamera => {
    const target: Vec3 = { x: 4500, y: 3500, z: 1200 };
    if (nav !== "orbit") return { position: { x: Math.round(2500 + 600 * Math.cos(u)), y: Math.round(3000 + 600 * Math.sin(u)), z: 1600 }, target: { x: Math.round(2500 + 3000 * Math.cos(u + 0.6)), y: Math.round(3000 + 3000 * Math.sin(u + 0.6)), z: 1600 }, fov_deg: 60, nav };
    return { position: { x: Math.round(target.x + 14000 * Math.cos(u)), y: Math.round(target.y + 14000 * Math.sin(u)), z: 9000 }, target, fov_deg: 45, nav };
  };
  /** A peer's plan area and camera, sent when followed. */
  const setView = (id: string, plan: PlanView | null, camera: LiveCamera | null) => {
    views.set(id, { plan, camera });
    setPeer(id, {});
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
    views.clear();
    relay = next === "demo" ? "ready" : "off";
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
      views.set(ANA, { plan: areaAt({ x: 2200, y: 3600 }), camera: orbitAt(0.6) });
      views.set(BEN, { plan: areaAt({ x: 6600, y: 1800 }), camera: null });
      setPeer(ANA, { cursor: { x: 2200, y: 3600 }, view_mode: "split", pointer_3d: { x: 3000, y: 2500, z: 1200 } });
      setPeer(BEN, { cursor: { x: 6600, y: 1800 }, view_mode: "2d" });
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
    presence_set: (a: Record<string, unknown>) => {
      const next = ((a.presence as Presence | undefined)?.following ?? null) as string | null;
      if (next !== selfFollowing) {
        selfFollowing = next;
        // Followed now, or no longer: the peers start or stop sending their views.
        for (const id of [...peers.keys()]) setPeer(id, {});
      }
      return null;
    },
    presence_list: () => (mode === "off" ? [] : [...peers].map(([participant_id, presence]) => ({ participant_id, presence }))),
    live_status: () => status(),
    live_host: async () => {
      if (!profile) fail("bad_args", "Pick a name first. Others see it next to your pointer.");
      await new Promise((r) => setTimeout(r, 450));
      mode = "hosting";
      notice = null;
      relay = "connecting";
      participants = [{ id: SELF, name: profile, color: 0, role: "host" }];
      emitStatus();
      // The relay takes the session a moment later.
      setTimeout(() => {
        if (mode !== "hosting" || relay !== "connecting") return;
        relay = "ready";
        emitStatus();
      }, 1200);
      // Someone joins a moment later.
      setTimeout(() => {
        if (mode !== "hosting" || participants.some((p) => p.id === ANA)) return;
        participants = [...participants, { id: ANA, name: "Ana Reyes", color: 1, role: "guest" }];
        emitStatus();
        views.set(ANA, { plan: areaAt({ x: 2200, y: 3600 }), camera: orbitAt(0.6) });
        setPeer(ANA, { cursor: { x: 2200, y: 3600 }, view_mode: "split" });
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
      if (/unreachable/i.test(project)) fail("live_unreachable", "Could not reach the host directly or through the relay. The host is not online, so the session may have ended.");
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
  let anaNav: NavMode = "orbit";

  const tick = () => {
    if (paused || (mode !== "hosting" && mode !== "joined")) return;
    const t = ((Date.now() - t0) / 1000) % 30;
    // Ana: wanders the living room, stops at the partition, asks, moves on.
    if (t < 6 || t >= 12) {
      const u = (Date.now() - t0) / 1000;
      const cursor = { x: Math.round(2500 + 1700 * Math.sin(u * 0.55)), y: Math.round(3000 + 1600 * Math.sin(u * 0.83 + 1)) };
      // Her view drifts behind her pointer; her camera circles slowly.
      views.set(ANA, { plan: areaAt({ x: Math.round(2500 + 800 * Math.sin(u * 0.55)), y: Math.round(3000 + 700 * Math.sin(u * 0.83 + 1)) }), camera: orbitAt(0.6 + u * 0.05, anaNav) });
      setPeer(ANA, { cursor, pointer_3d: { x: cursor.x, y: cursor.y, z: 900 } });
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
      const cursor = { x: Math.round(6500 + 900 * Math.cos(u * 0.4)), y: Math.round(2600 + 1200 * Math.sin(u * 0.4)) };
      views.set(BEN, { plan: areaAt({ x: 6500, y: 2600 }), camera: views.get(BEN)?.camera ?? null });
      setPeer(BEN, { cursor });
    } else if (!benSaid) {
      benSaid = true;
      say(BEN, "Looks good. I will add the kitchen window after lunch.");
    }
  };
  window.setInterval(tick, 50);

  // ------------------------------------------------ console and step files

  (window as unknown as { __liveDemo: unknown }).__liveDemo = {
    setMode: (m: DemoMode) => enter(m),
    /** The hosted session's relay state, for the Share dialog's reach line. */
    setRelay: (r: LiveRelay) => {
      relay = r;
      emitStatus();
    },
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
    /** Their view mode, plan area (null keeps it) and camera. */
    view: (who: "ana" | "ben", viewMode: ViewMode, center: Point | null = null) => {
      const id = who === "ana" ? ANA : BEN;
      const v = views.get(id);
      views.set(id, { plan: center ? areaAt(center) : (v?.plan ?? null), camera: viewMode === "2d" ? null : (v?.camera ?? orbitAt(0.6, anaNav)) });
      setPeer(id, { view_mode: viewMode });
    },
    /** Ana walks, flies or orbits in 3D: a follower sees "Ana is walking". */
    walk: (nav: NavMode) => {
      anaNav = nav;
      setView(ANA, views.get(ANA)?.plan ?? null, orbitAt(0.6, nav));
    },
    /** Someone follows someone: "self" is this window. */
    follow: (who: "ana" | "ben", target: "ana" | "ben" | "self" | null) => setPeer(who === "ana" ? ANA : BEN, { following: target === null ? null : target === "self" ? SELF : target === "ana" ? ANA : BEN }),
    walls: host.wallIds,
    rooms: host.roomIds,
  };

  // Before the app loads: its first live_status and presence_list see the session.
  const start = (initial ?? "off") as DemoMode;
  if (start === "demo" || start === "guest" || start === "reconnecting") enter(start);
}
