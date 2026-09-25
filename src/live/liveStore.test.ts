import { beforeEach, describe, expect, it } from "vitest";
import type { ChatMessage, LiveStatus, Presence } from "../contract/bindings";
import { LIVE_OFF, useLive } from "./liveStore";

const hosting: LiveStatus = {
  ...LIVE_OFF,
  mode: "hosting",
  self_id: "me",
  participants: [
    { id: "me", name: "Mara", color: 0, role: "host" },
    { id: "ana", name: "Ana", color: 1, role: "guest" },
    { id: "ben", name: "Ben", color: 2, role: "guest" },
  ],
  project_id: "p",
};

const pr = (over: Partial<Presence> = {}): Presence => ({ cursor: { x: 1, y: 2 }, level_id: "L", selection: [], typing: null, ai_scope: false, ...over });

function msg(id: string, author: string, sentAt: string): ChatMessage {
  return { id, author_id: author, author_name: author, color: 1, text: id, sent_at: sentAt, at: null, level_id: null, via_ai: false };
}

beforeEach(() => {
  useLive.setState({ status: LIVE_OFF, peers: {}, messages: [], said: {}, chat: { open: false, draft: "", sent: null, hint: 0 }, dialog: null, askEnd: false, askLeave: false, unread: 0 });
});

describe("presence", () => {
  it("keeps other people's presence, never this computer's", () => {
    const st = useLive.getState();
    st.setStatus(hosting);
    st.setPeer("ana", pr(), 10);
    st.setPeer("me", pr(), 10);
    expect(Object.keys(useLive.getState().peers)).toEqual(["ana"]);
    expect(useLive.getState().peers.ana.changedAt).toBe(10);
  });

  it("does not notify anyone when a presence did not change", () => {
    useLive.getState().setStatus(hosting);
    useLive.getState().setPeer("ana", pr(), 10);
    let calls = 0;
    const stop = useLive.subscribe(() => calls++);
    useLive.getState().setPeer("ana", pr(), 20);
    useLive.getState().setPeer("zed", null, 20);
    stop();
    expect(calls).toBe(0);
    expect(useLive.getState().peers.ana.changedAt).toBe(10);
  });

  it("forgets someone who left, and everyone when the session ends", () => {
    const st = useLive.getState();
    st.setStatus(hosting);
    st.setPeer("ana", pr(), 1);
    st.setPeer("ben", pr(), 1);
    st.setPeer("ana", null, 2);
    expect(Object.keys(useLive.getState().peers)).toEqual(["ben"]);
    st.setStatus({ ...hosting, participants: hosting.participants.filter((p) => p.id !== "ben") });
    expect(useLive.getState().peers).toEqual({});
    st.setPeer("ana", pr(), 3);
    st.setStatus(LIVE_OFF);
    expect(useLive.getState().peers).toEqual({});
  });
});

describe("cursor chat", () => {
  it("opens empty, sends, and clears only its own message", () => {
    const st = useLive.getState();
    st.setStatus(hosting);
    st.openCursorChat();
    st.setDraft("hello");
    expect(useLive.getState().chat).toMatchObject({ open: true, draft: "hello" });
    const first = st.markSent("hello");
    expect(useLive.getState().chat).toMatchObject({ open: false, draft: "", sent: { text: "hello" } });
    st.openCursorChat();
    const second = st.markSent("again");
    st.clearSent(first);
    expect(useLive.getState().chat.sent?.token).toBe(second);
    st.clearSent(second);
    expect(useLive.getState().chat.sent).toBeNull();
  });

  it("closes when the session stops", () => {
    const st = useLive.getState();
    st.setStatus(hosting);
    st.openCursorChat();
    st.setDraft("half a thought");
    st.setStatus({ ...hosting, mode: "reconnecting" });
    expect(useLive.getState().chat).toMatchObject({ open: false, draft: "" });
  });
});

describe("chat", () => {
  it("adds each message once and keeps the newest said per person", () => {
    const st = useLive.getState();
    st.setStatus(hosting);
    st.addMessage(msg("1", "ana", "2026-09-25T01:00:00Z"));
    st.addMessage(msg("1", "ana", "2026-09-25T01:00:00Z"));
    st.setMessages([msg("0", "ben", "2026-09-25T00:59:00Z"), ...useLive.getState().messages]);
    expect(useLive.getState().messages.map((m) => m.id)).toEqual(["0", "1"]);
    st.setSaid("ana", { id: "1", text: "one" });
    st.setSaid("ana", { id: "2", text: "two" });
    st.clearSaid("ana", "1");
    expect(useLive.getState().said.ana?.text).toBe("two");
    st.clearSaid("ana", "2");
    expect(useLive.getState().said.ana).toBeUndefined();
  });
});
