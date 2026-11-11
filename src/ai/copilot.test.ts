// The copilot dock's words and keys: Mod+Enter applies and Esc discards a
// pending proposal (D8 still holds: only Apply commits), the log following
// its newest line, readable tool and AI model names, and the Furnish
// button's label.
import { describe, expect, it } from "vitest";
import { logFollower, proposalKey, scrollToEnd, type ChatItem, type ProposalKeyEvent } from "./copilotStore";
import { modelLabel, toolLabel } from "./describe";
import { furnishLabel } from "./FurnishPanel";

const key = (k: string, mods: Partial<ProposalKeyEvent> = {}): ProposalKeyEvent => ({ key: k, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, ...mods });

describe("keys on a pending proposal", () => {
  const mac = { pending: true, draft: "", mac: true };
  const pc = { pending: true, draft: "", mac: false };

  it("Cmd+Enter applies on a Mac and Ctrl+Enter elsewhere", () => {
    expect(proposalKey(key("Enter", { metaKey: true }), mac)).toBe("apply");
    expect(proposalKey(key("Enter", { ctrlKey: true }), pc)).toBe("apply");
    // The other platform's modifier does nothing.
    expect(proposalKey(key("Enter", { ctrlKey: true }), mac)).toBeNull();
    expect(proposalKey(key("Enter", { metaKey: true }), pc)).toBeNull();
  });

  it("Esc discards", () => {
    expect(proposalKey(key("Escape"), mac)).toBe("discard");
    expect(proposalKey(key("Escape"), pc)).toBe("discard");
    expect(proposalKey(key("Escape", { shiftKey: true }), mac)).toBeNull();
  });

  it("plain Enter, Shift+Enter and other keys are left to the composer", () => {
    expect(proposalKey(key("Enter"), mac)).toBeNull();
    expect(proposalKey(key("Enter", { metaKey: true, shiftKey: true }), mac)).toBeNull();
    expect(proposalKey(key("a", { metaKey: true }), mac)).toBeNull();
  });

  it("does nothing while the composer has text, or with no pending proposal", () => {
    expect(proposalKey(key("Enter", { metaKey: true }), { ...mac, draft: "make it wider" })).toBeNull();
    expect(proposalKey(key("Escape"), { ...mac, draft: "x" })).toBeNull();
    // Whitespace alone is still an empty composer.
    expect(proposalKey(key("Escape"), { ...mac, draft: "  " })).toBe("discard");
    expect(proposalKey(key("Enter", { metaKey: true }), { ...mac, pending: false })).toBeNull();
    expect(proposalKey(key("Escape"), { ...mac, pending: false })).toBeNull();
  });
});

describe("the log follows its newest line", () => {
  const item = (id: number, more: Partial<ChatItem> = {}): ChatItem => ({ id, role: "assistant", text: "", toolsUsed: [], proposal: null, proposalStatus: null, proposalNote: null, tone: "normal", ...more });

  it("scrolls when the busy line mounts, a render after busy turns on, while cards wait below", () => {
    const follows = logFollower();
    const items = [item(1, { furnish: [] })];
    expect(follows(items, false)).toBe(true);
    // busy turned on: same items, the row not mounted yet. Nothing new to show.
    expect(follows(items, false)).toBe(false);
    // The next render mounts "Working on it": the log scrolls to it.
    expect(follows(items, true)).toBe(true);
    expect(follows(items, true)).toBe(false);
  });

  it("scrolls for a new message or a changed one, and when the busy line goes", () => {
    const follows = logFollower();
    const a = [item(1)];
    follows(a, true);
    const b = [...a, item(2)];
    expect(follows(b, true)).toBe(true);
    // A card's state changed (the store makes a new list).
    expect(follows(b.map((m) => (m.id === 2 ? { ...m, proposalStatus: "applied" as const } : m)), true)).toBe(true);
    expect(follows(b, false)).toBe(true);
  });

  it("scrolls to the end, smoothly unless motion is off", () => {
    const calls: ScrollToOptions[] = [];
    const el = { scrollHeight: 900, scrollTop: 0, scrollTo: (o: ScrollToOptions) => void calls.push(o) };
    scrollToEnd(el, true);
    scrollToEnd(el, false);
    expect(calls).toEqual([
      { top: 900, behavior: "smooth" },
      { top: 900, behavior: "auto" },
    ]);
    const bare = { scrollHeight: 640, scrollTop: 0 };
    scrollToEnd(bare, true);
    expect(bare.scrollTop).toBe(640);
    expect(() => scrollToEnd(null, true)).not.toThrow();
  });
});

describe("readable names", () => {
  it("names a tool without a label in words, not snake_case", () => {
    expect(toolLabel("add_wall")).toBe("Add wall");
    expect(toolLabel("get_plan")).toBe("Get plan");
    expect(toolLabel("furnish_rooms")).toBe("Furnish rooms");
    expect(toolLabel("list_rooms")).toBe("Read rooms");
  });

  it("names the AI model the way people read it", () => {
    expect(modelLabel("claude-opus-5-5")).toBe("Claude Opus 5.5");
    expect(modelLabel("claude-opus-5")).toBe("Claude Opus 5");
    expect(modelLabel("claude-sonnet-4-5-20250929")).toBe("Claude Sonnet 4.5");
    expect(modelLabel(" claude-haiku-4 ")).toBe("Claude Haiku 4");
    // Anything that is not a plain dashed id comes back as it is.
    expect(modelLabel("anthropic/claude:latest")).toBe("anthropic/claude:latest");
    expect(modelLabel("")).toBeNull();
    expect(modelLabel(null)).toBeNull();
  });
});

describe("the Furnish button", () => {
  const rooms = [
    { id: "r1", name: "Bedroom" },
    { id: "r2", name: "Living" },
  ];

  it("counts the rooms, names one selected room, and says when there is none", () => {
    expect(furnishLabel(rooms, [])).toBe("Furnish 2 rooms");
    expect(furnishLabel([rooms[0]], [])).toBe("Furnish 1 room");
    expect(furnishLabel([rooms[0]], ["r1"])).toBe("Furnish Bedroom");
    expect(furnishLabel(rooms, ["r1", "r2"])).toBe("Furnish 2 selected rooms");
    expect(furnishLabel([], [])).toBe("No closed rooms");
  });
});
