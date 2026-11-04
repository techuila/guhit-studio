// The glue of unread chat: focus, the unread count, the cards, the window's
// attention request and the Dock badge, with the desktop window API faked.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatMessage, LiveStatus } from "../contract/bindings";

const win = vi.hoisted(() => {
  const state = { focus: null as null | ((e: { payload: boolean }) => void) };
  return {
    state,
    onFocusChanged: vi.fn(async (fn: (e: { payload: boolean }) => void) => {
      state.focus = fn;
      return () => {
        state.focus = null;
      };
    }),
    isFocused: vi.fn(async () => true),
    requestUserAttention: vi.fn(async (_kind: number | null) => undefined),
    setBadgeCount: vi.fn(async (_count?: number) => undefined),
  };
});

vi.mock("../contract/ipc", async (orig) => ({ ...(await orig<typeof import("../contract/ipc")>()), isTauri: true }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => win,
  UserAttentionType: { Critical: 1, Informational: 2 },
}));

import { useShell } from "../shell/shellStore";
import { useApp } from "../state/store";
import { noteIncomingChat, startChatNotify, useNotify, dismissPeek } from "./chatNotify";
import { LIVE_OFF, useLive } from "./liveStore";

const status: LiveStatus = {
  ...LIVE_OFF,
  mode: "hosting",
  self_id: "me",
  participants: [
    { id: "me", name: "Mara", color: 0, role: "host", agent: null },
    { id: "ana", name: "Ana Reyes", color: 1, role: "guest", agent: null },
    { id: "ben", name: "Ben Cruz", color: 2, role: "guest", agent: null },
  ],
  project_id: "p",
};

let seq = 0;
function msg(author: string, text = "hello", over: Partial<ChatMessage> = {}): ChatMessage {
  seq += 1;
  return { id: `m${seq}`, author_id: author, author_name: author, color: 1, text, sent_at: new Date(seq).toISOString(), at: null, level_id: null, via_ai: false, ...over };
}

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

let stop: () => void;

beforeEach(async () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 9, 7, 12, 0, 0));
  vi.clearAllMocks();
  useLive.setState({ status, unread: 0, messages: [] });
  useApp.setState({ screen: "editor", viewMode: "2d", activeLevelId: "L1" });
  useShell.setState({ dockTab: "copilot", dockCollapsed: false });
  useNotify.setState({ focused: true, cards: [] });
  stop = startChatNotify();
  await vi.waitFor(() => expect(win.onFocusChanged).toHaveBeenCalled());
  await flush();
  vi.clearAllMocks();
});

afterEach(() => {
  stop();
  vi.useRealTimers();
});

const focus = async (focused: boolean) => {
  win.state.focus?.({ payload: focused });
  await flush();
};

describe("unread rule", () => {
  it("reads a message when the window is focused and Chat is on screen", () => {
    useShell.setState({ dockTab: "chat" });
    noteIncomingChat(msg("ana"));
    expect(useLive.getState().unread).toBe(0);
    expect(useNotify.getState().cards).toHaveLength(0);
  });

  it("counts it when Chat is not on screen, with a card and no attention request", () => {
    noteIncomingChat(msg("ana", "Door is too narrow"));
    expect(useLive.getState().unread).toBe(1);
    expect(useNotify.getState().cards).toMatchObject([{ authorId: "ana", count: 1, text: "Door is too narrow", role: "guest" }]);
    expect(win.requestUserAttention).not.toHaveBeenCalled();
  });

  it("counts it when the window is not focused, even with Chat open", async () => {
    useShell.setState({ dockTab: "chat" });
    await focus(false);
    noteIncomingChat(msg("ana"));
    expect(useLive.getState().unread).toBe(1);
    expect(useNotify.getState().cards).toHaveLength(1);
  });

  it("clears the count and the cards when the window is focused again with Chat open", async () => {
    useShell.setState({ dockTab: "chat" });
    await focus(false);
    noteIncomingChat(msg("ana"));
    noteIncomingChat(msg("ben"));
    expect(useLive.getState().unread).toBe(2);
    await focus(true);
    expect(useLive.getState().unread).toBe(0);
    expect(useNotify.getState().cards).toHaveLength(0);
    expect(win.setBadgeCount).toHaveBeenLastCalledWith(undefined);
  });

  it("keeps the count when the window is focused again with Chat elsewhere", async () => {
    await focus(false);
    noteIncomingChat(msg("ana"));
    await focus(true);
    expect(useLive.getState().unread).toBe(1);
    expect(useNotify.getState().cards).toHaveLength(1);
  });

  it("clears the cards when the count drops to zero some other way (Chat opened)", () => {
    noteIncomingChat(msg("ana"));
    expect(useNotify.getState().cards).toHaveLength(1);
    useLive.getState().setUnread(0);
    expect(useNotify.getState().cards).toHaveLength(0);
  });
});

describe("cards", () => {
  it("merges messages from one sender and stacks at most two", () => {
    noteIncomingChat(msg("ana", "one"));
    noteIncomingChat(msg("ana", "two"));
    noteIncomingChat(msg("ana", "three"));
    expect(useNotify.getState().cards).toMatchObject([{ authorId: "ana", count: 3, text: "three" }]);
    noteIncomingChat(msg("ben", "hi"));
    noteIncomingChat(msg("cy", "yo"));
    expect(useNotify.getState().cards.map((c) => c.authorId)).toEqual(["ben", "cy"]);
    expect(useLive.getState().unread).toBe(5);
  });

  it("takes the host role from the session", () => {
    noteIncomingChat(msg("me"));
    expect(useNotify.getState().cards[0].role).toBe("host");
  });

  it("dismisses one sender's card and keeps the count", () => {
    noteIncomingChat(msg("ana"));
    noteIncomingChat(msg("ben"));
    dismissPeek("ana");
    expect(useNotify.getState().cards.map((c) => c.authorId)).toEqual(["ben"]);
    expect(useLive.getState().unread).toBe(2);
  });

  it("skips the card for a cursor chat message the plan already shows, but still counts it", () => {
    noteIncomingChat(msg("ana", "here", { at: { x: 100, y: 200 }, level_id: "L1" }));
    expect(useNotify.getState().cards).toHaveLength(0);
    expect(useLive.getState().unread).toBe(1);
  });

  it("shows the card for a cursor chat message on another level, or with only the 3D view", () => {
    noteIncomingChat(msg("ana", "there", { at: { x: 1, y: 2 }, level_id: "L2" }));
    expect(useNotify.getState().cards).toHaveLength(1);
    useApp.setState({ viewMode: "3d" });
    noteIncomingChat(msg("ben", "here", { at: { x: 1, y: 2 }, level_id: "L1" }));
    expect(useNotify.getState().cards.map((c) => c.authorId)).toEqual(["ana", "ben"]);
  });

  it("shows the card for a cursor chat message when the window is not focused", async () => {
    await focus(false);
    noteIncomingChat(msg("ana", "here", { at: { x: 1, y: 2 }, level_id: "L1" }));
    expect(useNotify.getState().cards).toHaveLength(1);
  });
});

describe("attention and Dock badge", () => {
  it("asks for attention once per burst while unfocused", async () => {
    await focus(false);
    noteIncomingChat(msg("ana"));
    await vi.waitFor(() => expect(win.requestUserAttention).toHaveBeenCalledTimes(1));
    expect(win.requestUserAttention).toHaveBeenCalledWith(2);
    vi.setSystemTime(Date.now() + 4000);
    noteIncomingChat(msg("ben"));
    vi.setSystemTime(Date.now() + 4000);
    noteIncomingChat(msg("ana"));
    await flush();
    expect(win.requestUserAttention).toHaveBeenCalledTimes(1);
    // After a quiet gap the next message is a new burst.
    vi.setSystemTime(Date.now() + 60_000);
    noteIncomingChat(msg("ana"));
    await vi.waitFor(() => expect(win.requestUserAttention).toHaveBeenCalledTimes(2));
  });

  it("asks again in the next burst after the window was focused in between", async () => {
    await focus(false);
    noteIncomingChat(msg("ana"));
    await vi.waitFor(() => expect(win.requestUserAttention).toHaveBeenCalledTimes(1));
    await focus(true);
    await focus(false);
    vi.setSystemTime(Date.now() + 1000);
    noteIncomingChat(msg("ana"));
    await vi.waitFor(() => expect(win.requestUserAttention).toHaveBeenCalledTimes(2));
  });

  it("does not ask while the window is focused", async () => {
    noteIncomingChat(msg("ana"));
    await flush();
    expect(win.requestUserAttention).not.toHaveBeenCalled();
  });

  it("clears the Dock badge when the session ends, keeps the count, and stops asking", async () => {
    await focus(false);
    noteIncomingChat(msg("ana"));
    noteIncomingChat(msg("ben"));
    await vi.waitFor(() => expect(win.setBadgeCount).toHaveBeenLastCalledWith(2));
    await vi.waitFor(() => expect(win.requestUserAttention).toHaveBeenCalledTimes(1));

    useLive.setState({ status: LIVE_OFF });
    await vi.waitFor(() => expect(win.setBadgeCount).toHaveBeenLastCalledWith(undefined));
    // The Chat tab still counts what there is to read.
    expect(useLive.getState().unread).toBe(2);
    expect(useNotify.getState().cards).toHaveLength(0);

    // A late message of the ended session neither bounces the Dock nor brings the badge back.
    vi.setSystemTime(Date.now() + 120_000);
    win.setBadgeCount.mockClear();
    noteIncomingChat(msg("ana"));
    await flush();
    expect(win.requestUserAttention).toHaveBeenCalledTimes(1);
    expect(win.setBadgeCount).toHaveBeenLastCalledWith(undefined);
  });

  it("shows the count again when a session starts with messages still unread", async () => {
    noteIncomingChat(msg("ana"));
    await vi.waitFor(() => expect(win.setBadgeCount).toHaveBeenLastCalledWith(1));
    useLive.setState({ status: LIVE_OFF });
    await vi.waitFor(() => expect(win.setBadgeCount).toHaveBeenLastCalledWith(undefined));
    useLive.setState({ status });
    await vi.waitFor(() => expect(win.setBadgeCount).toHaveBeenLastCalledWith(1));
  });

  it("shows the unread count on the Dock icon and clears it at zero", async () => {
    noteIncomingChat(msg("ana"));
    noteIncomingChat(msg("ben"));
    await vi.waitFor(() => expect(win.setBadgeCount).toHaveBeenLastCalledWith(2));
    useLive.getState().setUnread(0);
    await vi.waitFor(() => expect(win.setBadgeCount).toHaveBeenLastCalledWith(undefined));
  });
});
