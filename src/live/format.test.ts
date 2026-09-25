import { describe, expect, it } from "vitest";
import type { ChatMessage, LiveStatus, Participant, Presence } from "../contract/bindings";
import {
  avatarTip,
  cleanName,
  cursorIds,
  firstName,
  fullTime,
  groupMessages,
  initials,
  mergeMessages,
  parseInvite,
  peerIndex,
  peerSelections,
  peerVar,
  possessive,
  sameSelections,
  stepAuthor,
  timeLabel,
  type Peers,
} from "./format";

const ana: Participant = { id: "a", name: "Ana Reyes", color: 0, role: "host" };
const ben: Participant = { id: "b", name: "Ben", color: 1, role: "guest" };
const cy: Participant = { id: "c", name: "Cy Tan", color: 2, role: "guest" };

function status(over: Partial<LiveStatus> = {}): LiveStatus {
  return { mode: "hosting", self_id: "a", participants: [ana, ben, cy], invite: null, addresses: [], project_id: "p", project_name: "Bungalow", notice: null, ...over };
}

function presence(over: Partial<Presence> = {}): Presence {
  return { cursor: null, level_id: "L1", selection: [], typing: null, ai_scope: false, ...over };
}

/** Local time, so labels do not depend on the machine's time zone. */
function at(y: number, mo: number, d: number, h: number, mi: number): string {
  return new Date(y, mo - 1, d, h, mi).toISOString();
}

function msg(id: string, author: Participant, sentAt: string, over: Partial<ChatMessage> = {}): ChatMessage {
  return { id, author_id: author.id, author_name: author.name, color: author.color, text: id, sent_at: sentAt, at: null, level_id: null, via_ai: false, ...over };
}

describe("names and colors", () => {
  it("makes initials from the first and last word", () => {
    expect(initials("Ana Reyes")).toBe("AR");
    expect(initials("ana maria de la cruz")).toBe("AC");
    expect(initials("ben")).toBe("B");
    expect(initials("   ")).toBe("?");
    expect(initials("Élise Ñoño")).toBe("ÉÑ");
  });

  it("finds the first name", () => {
    expect(firstName("Ana Reyes")).toBe("Ana");
    expect(firstName("  Ben ")).toBe("Ben");
    expect(possessive("Ana Reyes")).toBe("Ana's");
    expect(possessive("Jules")).toBe("Jules'");
  });

  it("cleans a name like the engine", () => {
    expect(cleanName("  Ana \t Reyes\n")).toBe("Ana Reyes");
    expect(cleanName("x".repeat(60))).toHaveLength(40);
    expect(cleanName("\u0007")).toBe("");
  });

  it("wraps color indexes into the eight tokens", () => {
    expect(peerVar(3)).toBe("var(--peer-3)");
    expect(peerIndex(9)).toBe(1);
    expect(peerIndex(-1)).toBe(7);
    expect(peerIndex(Number.NaN)).toBe(0);
  });

  it("says who a person is in the avatar tooltip", () => {
    expect(avatarTip(ana, "b")).toBe("Ana Reyes, host");
    expect(avatarTip(ana, "a")).toBe("Ana Reyes, host, you");
    expect(avatarTip(ben, "a")).toBe("Ben");
  });
});

describe("stepAuthor", () => {
  it("names someone else in the session", () => {
    expect(stepAuthor(status(), "b", [])).toBe("Ben");
  });

  it("says nothing for your own step or outside a session", () => {
    expect(stepAuthor(status(), "a", [])).toBeNull();
    expect(stepAuthor(status(), null, [])).toBeNull();
    expect(stepAuthor(status({ mode: "off" }), "b", [])).toBeNull();
  });

  it("finds a person who left from their messages", () => {
    const gone = msg("m1", { id: "z", name: "Zed", color: 4, role: "guest" }, at(2026, 9, 25, 9, 0));
    expect(stepAuthor(status(), "z", [gone])).toBe("Zed");
    expect(stepAuthor(status(), "q", [gone])).toBe("someone else");
  });
});

describe("cursorIds", () => {
  const peers: Peers = {
    b: { presence: presence({ cursor: { x: 1, y: 2 } }), changedAt: 0 },
    c: { presence: presence({ cursor: { x: 1, y: 2 }, level_id: "L2" }), changedAt: 0 },
    a: { presence: presence({ cursor: { x: 1, y: 2 } }), changedAt: 0 },
  };

  it("shows others on the level on screen only", () => {
    expect(cursorIds(status(), peers, "L1")).toEqual(["b"]);
    expect(cursorIds(status(), peers, "L2")).toEqual(["c"]);
  });

  it("hides a pointer that is off the plan", () => {
    expect(cursorIds(status(), { b: { presence: presence(), changedAt: 0 } }, "L1")).toEqual([]);
  });

  it("shows nothing outside a session or without a level", () => {
    expect(cursorIds(status({ mode: "off" }), peers, "L1")).toEqual([]);
    expect(cursorIds(status(), peers, null)).toEqual([]);
  });

  it("ignores presence of someone not in the session", () => {
    expect(cursorIds(status({ participants: [ana, cy] }), peers, "L1")).toEqual([]);
  });
});

describe("peerSelections", () => {
  it("lists what others selected, in their color", () => {
    const peers: Peers = {
      b: { presence: presence({ selection: ["w1", "w2"] }), changedAt: 0 },
      c: { presence: presence(), changedAt: 0 },
      a: { presence: presence({ selection: ["mine"] }), changedAt: 0 },
    };
    const list = peerSelections(status(), peers);
    expect(list).toEqual([{ id: "b", color: 1, ids: ["w1", "w2"] }]);
    expect(sameSelections(list, [{ id: "b", color: 1, ids: ["w1", "w2"] }])).toBe(true);
    expect(sameSelections(list, [{ id: "b", color: 1, ids: ["w1"] }])).toBe(false);
    expect(sameSelections(list, [{ id: "b", color: 2, ids: ["w1", "w2"] }])).toBe(false);
    expect(sameSelections(list, [])).toBe(false);
  });
});

describe("groupMessages", () => {
  it("groups a run from one person under one header", () => {
    const list = [
      msg("1", ana, at(2026, 9, 25, 9, 0)),
      msg("2", ana, at(2026, 9, 25, 9, 2)),
      msg("3", ben, at(2026, 9, 25, 9, 3)),
      msg("4", ana, at(2026, 9, 25, 9, 4)),
    ];
    const groups = groupMessages(list, "a");
    expect(groups.map((g) => g.messages.map((m) => m.id))).toEqual([["1", "2"], ["3"], ["4"]]);
    expect(groups[0].mine).toBe(true);
    expect(groups[1].mine).toBe(false);
    expect(groups[0].sentAt).toBe(list[0].sent_at);
  });

  it("breaks a run after a pause, on a new day and for AI messages", () => {
    const list = [
      msg("1", ana, at(2026, 9, 25, 9, 0)),
      msg("2", ana, at(2026, 9, 25, 9, 20)),
      msg("3", ana, at(2026, 9, 25, 9, 21), { via_ai: true }),
      msg("4", ana, at(2026, 9, 25, 23, 59)),
      msg("5", ana, at(2026, 9, 26, 0, 1)),
    ];
    expect(groupMessages(list, null).map((g) => g.key)).toEqual(["1", "2", "3", "4", "5"]);
    expect(groupMessages(list, null)[2].viaAi).toBe(true);
  });
});

describe("mergeMessages", () => {
  it("adds new messages once, in the order they were sent", () => {
    const a = msg("1", ana, at(2026, 9, 25, 9, 0));
    const b = msg("2", ben, at(2026, 9, 25, 9, 1));
    const c = msg("3", ben, at(2026, 9, 25, 9, 2));
    const cur = [a, c];
    expect(mergeMessages(cur, [b, c]).map((m) => m.id)).toEqual(["1", "2", "3"]);
    expect(mergeMessages(cur, [a])).toBe(cur);
  });

  it("keeps one copy of a message that comes twice in one load", () => {
    const a = msg("1", ana, at(2026, 9, 25, 9, 0));
    const b = msg("2", ben, at(2026, 9, 25, 9, 1));
    expect(mergeMessages([], [a, b, a, b]).map((m) => m.id)).toEqual(["1", "2"]);
  });

  it("keeps the newest when over the limit", () => {
    const list = Array.from({ length: 6 }, (_, i) => msg(`${i}`, ana, at(2026, 9, 25, 9, i)));
    expect(mergeMessages([], list, 4).map((m) => m.id)).toEqual(["2", "3", "4", "5"]);
  });
});

describe("timeLabel", () => {
  const now = new Date(2026, 8, 25, 15, 0);
  it("shows the clock for today", () => {
    expect(timeLabel(at(2026, 9, 25, 9, 41), now)).toBe("9:41 AM");
    expect(timeLabel(at(2026, 9, 25, 13, 5), now)).toBe("1:05 PM");
  });

  it("says yesterday, then the date", () => {
    expect(timeLabel(at(2026, 9, 24, 21, 30), now)).toBe("Yesterday 9:30 PM");
    expect(timeLabel(at(2026, 9, 20, 8, 0), now)).toBe("Sep 20, 8:00 AM");
    expect(timeLabel(at(2025, 12, 31, 8, 0), now)).toBe("Dec 31, 2025");
  });

  it("is empty for a stamp it cannot read", () => {
    expect(timeLabel("not a time", now)).toBe("");
    expect(fullTime("nope")).toBe("");
    expect(fullTime(at(2026, 9, 25, 9, 41))).toBe("Sep 25, 2026, 9:41 AM");
  });
});

describe("parseInvite", () => {
  const encode = (v: unknown) => {
    const bytes = new TextEncoder().encode(JSON.stringify(v));
    const b64 = btoa(String.fromCharCode(...bytes));
    return `guhit-live:${b64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "")}`;
  };

  it("reads the project and the host's addresses", () => {
    const invite = encode({ v: 1, secret: "s", pin: "p", addrs: ["192.168.1.20:1460", "127.0.0.1:1460"], project: "Bahay ni Lola ñ" });
    expect(parseInvite(`  ${invite}\n`)).toEqual({ project: "Bahay ni Lola ñ", addrs: ["192.168.1.20:1460", "127.0.0.1:1460"] });
  });

  it("refuses anything else", () => {
    expect(parseInvite("hello")).toBeNull();
    expect(parseInvite("guhit-live:")).toBeNull();
    expect(parseInvite("guhit-live:!!!")).toBeNull();
    expect(parseInvite(encode({ v: 1, addrs: [] }))).toBeNull();
    expect(parseInvite(`guhit-live:${btoa("not json")}`)).toBeNull();
  });

  it("allows an invite without a project name", () => {
    expect(parseInvite(encode({ v: 1, secret: "s", pin: "p", addrs: [] }))).toEqual({ project: null, addrs: [] });
  });
});
