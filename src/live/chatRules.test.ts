import { describe, expect, it } from "vitest";
import type { ChatMessage } from "../contract/bindings";
import {
  BURST_GAP_MS,
  PEEK_CHARS,
  addPeek,
  badgeCount,
  chatButtonLabel,
  dismissLabel,
  dropPeek,
  isNewBurst,
  isUnread,
  newLabel,
  peekHeading,
  peekText,
  skipPeek,
  type PeekCard,
  type PeekView,
} from "./chatRules";

function msg(id: string, author: string, text = id, over: Partial<ChatMessage> = {}): ChatMessage {
  return { id, author_id: author, author_name: `${author} Reyes`, color: 1, text, sent_at: new Date(0).toISOString(), at: null, level_id: null, via_ai: false, ...over };
}

describe("isUnread", () => {
  it("counts a message as read only with the window focused and Chat on screen", () => {
    expect(isUnread(true, true)).toBe(false);
  });

  it("counts it as unread when Chat is not on screen", () => {
    expect(isUnread(true, false)).toBe(true);
  });

  it("counts it as unread when the window is not focused, even with Chat open", () => {
    expect(isUnread(false, true)).toBe(true);
    expect(isUnread(false, false)).toBe(true);
  });
});

describe("isNewBurst", () => {
  it("starts a burst with the first unread message", () => {
    expect(isNewBurst(null, 1000)).toBe(true);
  });

  it("keeps messages close together in one burst", () => {
    expect(isNewBurst(1000, 1000 + 5000)).toBe(false);
    expect(isNewBurst(1000, 1000 + BURST_GAP_MS)).toBe(false);
  });

  it("starts a new burst after a quiet gap", () => {
    expect(isNewBurst(1000, 1000 + BURST_GAP_MS + 1)).toBe(true);
    expect(isNewBurst(1000, 1000 + 2 * BURST_GAP_MS, 10_000)).toBe(true);
  });

  it("starts a new burst when the clock went back", () => {
    expect(isNewBurst(5000, 1000)).toBe(true);
  });

  it("asks once for a run of messages and again after the gap", () => {
    let last: number | null = null;
    let asks = 0;
    for (const t of [0, 2000, 4000, 9000, 20_000, 70_000, 71_000]) {
      if (isNewBurst(last, t)) asks += 1;
      last = t;
    }
    expect(asks).toBe(2);
  });
});

describe("badgeCount", () => {
  it("shows the count and clears at zero", () => {
    expect(badgeCount(3)).toBe(3);
    expect(badgeCount(1)).toBe(1);
    expect(badgeCount(0)).toBeUndefined();
    expect(badgeCount(-2)).toBeUndefined();
  });
});

describe("peekText", () => {
  it("keeps a short message whole", () => {
    expect(peekText("Door looks good")).toBe("Door looks good");
  });

  it("folds line breaks and runs of spaces", () => {
    expect(peekText("  one\n\n two\t three  ")).toBe("one two three");
  });

  it("cuts a long message at 80 characters with dots", () => {
    const out = peekText("x".repeat(200));
    expect(out).toBe(`${"x".repeat(PEEK_CHARS)}...`);
  });

  it("keeps a message of exactly 80 characters whole", () => {
    const text = "y".repeat(PEEK_CHARS);
    expect(peekText(text)).toBe(text);
  });

  it("counts emoji as one character and never cuts one in half", () => {
    const text = "😀".repeat(100);
    const out = peekText(text);
    expect([...out].length).toBe(PEEK_CHARS + 3);
    expect(out.endsWith("...")).toBe(true);
    expect(out.slice(0, -3)).toBe("😀".repeat(PEEK_CHARS));
  });

  it("does not leave a space before the dots", () => {
    const text = `${"a".repeat(PEEK_CHARS - 1)} ${"b".repeat(10)}`;
    expect(peekText(text)).toBe(`${"a".repeat(PEEK_CHARS - 1)}...`);
  });
});

describe("skipPeek", () => {
  const view: PeekView = { focused: true, planOnScreen: true, levelId: "L1" };

  it("skips a cursor chat message when the plan shows its level", () => {
    expect(skipPeek({ at: { x: 1, y: 2 }, level_id: "L1" }, view)).toBe(true);
  });

  it("shows a panel message", () => {
    expect(skipPeek({ at: null, level_id: null }, view)).toBe(false);
  });

  it("shows a cursor chat message from another level", () => {
    expect(skipPeek({ at: { x: 1, y: 2 }, level_id: "L2" }, view)).toBe(false);
  });

  it("shows a cursor chat message when the plan is not on screen (3D only, the hub)", () => {
    expect(skipPeek({ at: { x: 1, y: 2 }, level_id: "L1" }, { ...view, planOnScreen: false })).toBe(false);
  });

  it("shows it when the window is not focused: nobody sees the bubble", () => {
    expect(skipPeek({ at: { x: 1, y: 2 }, level_id: "L1" }, { ...view, focused: false })).toBe(false);
  });

  it("shows it when no level is on screen", () => {
    expect(skipPeek({ at: { x: 1, y: 2 }, level_id: null }, { ...view, levelId: null })).toBe(false);
  });
});

describe("addPeek", () => {
  it("adds the first message as a card with a count of one", () => {
    const cards = addPeek([], msg("m1", "ana", "Hello"), "guest");
    expect(cards).toHaveLength(1);
    expect(cards[0]).toMatchObject({ authorId: "ana", name: "ana Reyes", text: "Hello", count: 1, lastId: "m1", role: "guest", viaAi: false });
  });

  it("updates the same sender's card: count up, newest text, timer restarted", () => {
    let cards = addPeek([], msg("m1", "ana", "one"), "guest");
    const stamp = cards[0].stamp;
    cards = addPeek(cards, msg("m2", "ana", "two"), "guest");
    cards = addPeek(cards, msg("m3", "ana", "three"), "guest");
    expect(cards).toHaveLength(1);
    expect(cards[0]).toMatchObject({ count: 3, text: "three", lastId: "m3" });
    expect(cards[0].stamp).toBe(stamp + 2);
  });

  it("stacks another sender below", () => {
    let cards = addPeek([], msg("m1", "ana"), "guest");
    cards = addPeek(cards, msg("m2", "ben"), "guest");
    expect(cards.map((c) => c.authorId)).toEqual(["ana", "ben"]);
  });

  it("updates a card in place without moving it", () => {
    let cards = addPeek([], msg("m1", "ana"), "guest");
    cards = addPeek(cards, msg("m2", "ben"), "guest");
    cards = addPeek(cards, msg("m3", "ana", "again"), "guest");
    expect(cards.map((c) => c.authorId)).toEqual(["ana", "ben"]);
    expect(cards[0].count).toBe(2);
    expect(cards[1].count).toBe(1);
  });

  it("keeps at most two cards: a third sender pushes the oldest out", () => {
    let cards = addPeek([], msg("m1", "ana"), "guest");
    cards = addPeek(cards, msg("m2", "ben"), "guest");
    cards = addPeek(cards, msg("m3", "cy"), "guest");
    expect(cards.map((c) => c.authorId)).toEqual(["ben", "cy"]);
  });

  it("never changes the cards it was given", () => {
    const before = addPeek([], msg("m1", "ana"), "guest");
    const copy = JSON.parse(JSON.stringify(before)) as PeekCard[];
    addPeek(before, msg("m2", "ana"), "guest");
    addPeek(before, msg("m3", "ben"), "guest");
    expect(before).toEqual(copy);
  });

  it("carries the host role and the AI mark", () => {
    const cards = addPeek([], msg("m1", "ana", "x", { via_ai: true }), "host");
    expect(cards[0]).toMatchObject({ role: "host", viaAi: true });
  });

  it("cuts a long message to the start", () => {
    const cards = addPeek([], msg("m1", "ana", "z".repeat(300)), "guest");
    expect(cards[0].text).toBe(`${"z".repeat(PEEK_CHARS)}...`);
  });
});

describe("dropPeek", () => {
  it("removes one sender's card", () => {
    let cards = addPeek([], msg("m1", "ana"), "guest");
    cards = addPeek(cards, msg("m2", "ben"), "guest");
    expect(dropPeek(cards, "ana").map((c) => c.authorId)).toEqual(["ben"]);
  });

  it("returns the same array when there is no such card", () => {
    const cards = addPeek([], msg("m1", "ana"), "guest");
    expect(dropPeek(cards, "zed")).toBe(cards);
  });
});

describe("labels", () => {
  it("names the sender and how many are new", () => {
    expect(peekHeading({ name: "Ana Reyes", count: 1 })).toBe("Ana Reyes");
    expect(peekHeading({ name: "Ana Reyes", count: 3 })).toBe("Ana Reyes, 3 new");
  });

  it("shows the count pill from the second message", () => {
    expect(newLabel(1)).toBeNull();
    expect(newLabel(2)).toBe("2 new");
  });

  it("labels the Chat button with the count", () => {
    expect(chatButtonLabel(0)).toBe("Chat");
    expect(chatButtonLabel(1)).toBe("Chat, 1 unread message");
    expect(chatButtonLabel(5)).toBe("Chat, 5 unread messages");
  });

  it("labels the close button with the first name", () => {
    expect(dismissLabel({ name: "Ana Reyes" })).toBe("Dismiss the message from Ana");
  });
});
