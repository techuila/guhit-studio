import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AiProposal, AiTurn, DocState } from "../contract/bindings";

const calls: { chat: unknown[]; resolve: [string, boolean][]; preview: unknown[] } = { chat: [], resolve: [], preview: [] };
let nextTurn: AiTurn | null = null;
/** Proposals the engine no longer holds: resolving them answers not_found. */
const gone = new Set<string>();
/** While true the engine refuses every commit `swarm_running`, as with a swarm draft open (D57). */
let swarmOpen = false;

vi.mock("../contract/ipc", async (orig) => ({
  ...(await orig<typeof import("../contract/ipc")>()),
  ipc: {
    aiChat: vi.fn(async (req: unknown) => {
      calls.chat.push(req);
      return nextTurn;
    }),
    aiResolve: vi.fn(async (id: string, accept: boolean) => {
      calls.resolve.push([id, accept]);
      if (accept && swarmOpen) throw { code: "swarm_running", message: "A swarm draft is open." };
      if (gone.has(id)) throw { code: "not_found", message: "That proposal is no longer pending." };
      return { applied: accept ? { state: doc(9) } : null };
    }),
    // The bedroom's batch run again on the plan after the living room: the same object.
    docPreview: vi.fn(async (command: unknown) => {
      calls.preview.push(command);
      return { state: doc(10, [{ kind: "asset", id: "b1", name: "Object b1" }]), diff: { added: ["b1"], modified: [], removed: [], summary: "" } };
    }),
    docState: vi.fn(async () => doc(9)),
  },
}));

import { useApp } from "../state/store";
import { useCopilot } from "./copilotStore";
import { LOCKED_APPLY_TEXT } from "./apply";
import { applyRoom, discardRoom, previewRoom, resetFurnishState, roomsToFurnish, turnRooms } from "./furnish";

function doc(revision: number, extra: unknown[] = []): DocState {
  return {
    revision,
    project: {
      id: "p1",
      levels: [{ id: "L1", name: "Ground", elevation_mm: 0, height_mm: 3000 }, { id: "L2", name: "Second", elevation_mm: 3000, height_mm: 3000 }],
      elements: [
        { kind: "room", id: "living", level_id: "L1", name: "Living", usage: "living" },
        { kind: "room", id: "bed", level_id: "L1", name: "Bedroom", usage: "bedroom" },
        { kind: "room", id: "open", level_id: "L1", name: "Open", usage: "other" },
        { kind: "room", id: "up", level_id: "L2", name: "Hall", usage: "hallway" },
        ...extra,
      ],
    },
    derived: { rooms: [{ room_id: "living" }, { room_id: "bed" }, { room_id: "up" }] },
  } as unknown as DocState;
}

function proposal(id: string, room: string, revision: number, assetIds: string[]): AiProposal {
  const assets = assetIds.map((a) => ({ kind: "asset", id: a, name: `Object ${a}` }));
  return {
    id,
    command: { type: "batch", label: `AI: Furnish ${room}`, commands: [] },
    preview: { state: doc(revision + 1, assets), diff: { added: assetIds, modified: [], removed: [], summary: "" } },
    base_revision: revision,
  } as unknown as AiProposal;
}

const ROOMS = [
  { id: "living", name: "Living" },
  { id: "bed", name: "Bedroom" },
];

function setup(): number {
  calls.chat = [];
  calls.resolve = [];
  calls.preview = [];
  gone.clear();
  swarmOpen = false;
  resetFurnishState();
  useCopilot.getState().reset("p1");
  useApp.setState({ doc: doc(5), selection: [], activeLevelId: "L1" });
  const turn = { reply: "ok", proposal: null, tools_used: [], proposals: [proposal("pa", "Living", 5, ["a1", "a2"]), proposal("pb", "Bedroom", 5, ["b1"])] } as AiTurn;
  return useCopilot.getState().push({ role: "assistant", text: "ok", furnish: turnRooms(turn, ROOMS), furnishStyle: "modern" });
}

const roomsOf = (itemId: number) => useCopilot.getState().items.find((m) => m.id === itemId)!.furnish!;

describe("furnish rooms", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("furnishes the selected closed rooms, or every closed room of the active level", () => {
    expect(roomsToFurnish(doc(1), [], "L1").map((r) => r.id)).toEqual(["living", "bed"]);
    expect(roomsToFurnish(doc(1), ["bed", "open"], "L1").map((r) => r.id)).toEqual(["bed"]);
    expect(roomsToFurnish(doc(1), [], "L2").map((r) => r.id)).toEqual(["up"]);
    expect(roomsToFurnish(null, [], null)).toEqual([]);
  });

  it("applies each room directly from the engine's queue, in any order", async () => {
    const item = setup();
    await applyRoom(item, "bed", "modern");
    expect(calls.chat).toHaveLength(0);
    expect(calls.resolve).toEqual([["pb", true]]);
    expect(roomsOf(item)[1].status).toBe("applied");
    // The living room waits; its preview was made again on the plan as it is now.
    expect(roomsOf(item)[0].status).toBe("pending");
    expect(calls.preview).toHaveLength(1);
    await applyRoom(item, "living", "modern");
    expect(calls.resolve[1]).toEqual(["pa", true]);
    expect(roomsOf(item)[0].status).toBe("applied");
    expect(calls.chat).toHaveLength(0);
  });

  it("asks for a room alone, without the model, when the engine no longer holds it", async () => {
    const item = setup();
    gone.add("pb");
    nextTurn = { reply: "", proposal: null, tools_used: [], proposals: [proposal("pb2", "Bedroom", 5, ["b1"])] } as AiTurn;
    await applyRoom(item, "bed", "modern");
    expect(calls.chat).toHaveLength(1);
    const req = calls.chat[0] as { task: { room_ids: string[]; style: string } };
    expect(req.task).toEqual({ type: "furnish", room_ids: ["bed"], style: "modern" });
    expect(calls.resolve).toEqual([["pb", true], ["pb2", true]]);
    expect(roomsOf(item)[1].status).toBe("applied");
  });

  it("shows a room again for review when planning it again gives other objects", async () => {
    const item = setup();
    gone.add("pb");
    nextTurn = { reply: "", proposal: null, tools_used: [], proposals: [proposal("pb2", "Bedroom", 5, ["b9"])] } as AiTurn;
    await applyRoom(item, "bed", "modern");
    expect(calls.resolve).toEqual([["pb", true]]);
    const bed = roomsOf(item)[1];
    expect(bed.status).toBe("pending");
    expect(bed.proposal.id).toBe("pb2");
    expect(bed.note).toMatch(/planned again/);
  });

  describe("two rooms named Bedroom (GR3 P1-3)", () => {
    // Bedroom A west of x = 4000, Bedroom B east of it, both on L1; the
    // upstairs Hall sits over Bedroom A.
    const square = (x0: number, x1: number) => [
      { x: x0, y: 0 },
      { x: x1, y: 0 },
      { x: x1, y: 3000 },
      { x: x0, y: 3000 },
    ];
    const roomsDoc = (assets: unknown[] = []): DocState =>
      ({
        revision: 5,
        project: {
          id: "p1",
          levels: [{ id: "L1", name: "Ground", elevation_mm: 0, height_mm: 3000 }, { id: "L2", name: "Second", elevation_mm: 3000, height_mm: 3000 }],
          elements: [
            { kind: "room", id: "room-A", level_id: "L1", name: "Bedroom", usage: "bedroom" },
            { kind: "room", id: "room-B", level_id: "L1", name: "Bedroom", usage: "bedroom" },
            { kind: "room", id: "hall", level_id: "L2", name: "Hall", usage: "hallway" },
            ...assets,
          ],
        },
        derived: {
          rooms: [
            { room_id: "room-A", polygon: square(0, 4000) },
            { room_id: "room-B", polygon: square(4000, 8000) },
            { room_id: "hall", polygon: square(0, 4000) },
          ],
        },
      }) as unknown as DocState;
    /** A proposal whose objects stand at `at` on `level`, labelled by the room's name only. */
    const placed = (id: string, name: string, at: Array<{ x: number; y: number }>, level = "L1"): AiProposal => {
      const assets = at.map((p, i) => ({ kind: "asset", id: `${id}-${i}`, level_id: level, name: "Bed", position: p }));
      return {
        id,
        command: { type: "batch", label: `AI: Furnish ${name}`, commands: [] },
        preview: { state: roomsDoc(assets), diff: { added: assets.map((a) => a.id), modified: [], removed: [], summary: "" } },
        base_revision: 5,
      } as unknown as AiProposal;
    };
    const BEDROOMS = [
      { id: "room-A", name: "Bedroom" },
      { id: "room-B", name: "Bedroom" },
    ];
    const turnOf = (...proposals: AiProposal[]) => ({ reply: "ok", proposal: null, tools_used: [], proposals }) as unknown as AiTurn;

    it("pairs each proposal with the room its objects stand in, not the first room of that name", () => {
      const cards = turnRooms(turnOf(placed("pa", "Bedroom", [{ x: 1000, y: 1500 }]), placed("pb", "Bedroom", [{ x: 6000, y: 1500 }, { x: 7000, y: 500 }])), BEDROOMS);
      expect(cards.map((c) => [c.proposal.id, c.roomId])).toEqual([
        ["pa", "room-A"],
        ["pb", "room-B"],
      ]);
      // In the other order too.
      const swapped = turnRooms(turnOf(placed("pb", "Bedroom", [{ x: 6000, y: 1500 }]), placed("pa", "Bedroom", [{ x: 1000, y: 1500 }])), BEDROOMS);
      expect(swapped.map((c) => [c.proposal.id, c.roomId])).toEqual([
        ["pb", "room-B"],
        ["pa", "room-A"],
      ]);
    });

    it("pairs the second bedroom's proposal with it when nothing fit the first", () => {
      const cards = turnRooms(turnOf(placed("pb", "Bedroom", [{ x: 6000, y: 1500 }])), BEDROOMS);
      expect(cards.map((c) => c.roomId)).toEqual(["room-B"]);
    });

    it("ignores a room on another level that lies under the objects", () => {
      const cards = turnRooms(turnOf(placed("ph", "Hall", [{ x: 1000, y: 1500 }], "L2")), [...BEDROOMS, { id: "hall", name: "Hall" }]);
      expect(cards.map((c) => c.roomId)).toEqual(["hall"]);
    });

    it("applies the second bedroom's own proposal, and leaves the first waiting", async () => {
      calls.resolve = [];
      calls.chat = [];
      gone.clear();
      resetFurnishState();
      useCopilot.getState().reset("p1");
      useApp.setState({ doc: roomsDoc(), selection: [], activeLevelId: "L1" });
      const cards = turnRooms(turnOf(placed("pa", "Bedroom", [{ x: 1000, y: 1500 }]), placed("pb", "Bedroom", [{ x: 6000, y: 1500 }])), BEDROOMS);
      const item = useCopilot.getState().push({ role: "assistant", text: "ok", furnish: cards, furnishStyle: "modern" });
      await applyRoom(item, "room-B", "modern");
      expect(calls.resolve).toEqual([["pb", true]]);
      expect(roomsOf(item).map((r) => [r.roomId, r.status])).toEqual([
        ["room-A", "pending"],
        ["room-B", "applied"],
      ]);
      await applyRoom(item, "room-A", "modern");
      expect(calls.resolve[1]).toEqual(["pa", true]);
      expect(calls.chat).toHaveLength(0);
    });
  });

  // Gate P1-5: Apply during a swarm draft must not lose the room's proposal.
  it("a room refused swarm_running stays pending with its preview, and applies after the draft", async () => {
    const item = setup();
    const pa = roomsOf(item)[0];
    previewRoom(pa);
    swarmOpen = true;
    await applyRoom(item, "living", "modern");
    expect(roomsOf(item)[0].status).toBe("pending");
    expect(roomsOf(item)[0].note).toBe(LOCKED_APPLY_TEXT);
    expect(roomsOf(item)[0].proposal.id).toBe("pa");
    expect(useApp.getState().preview).toBe(pa.proposal.preview);
    // Nothing asked the model again.
    expect(calls.chat).toHaveLength(0);
    // The draft is discarded: the same proposal applies.
    swarmOpen = false;
    await applyRoom(item, "living", "modern");
    expect(calls.resolve).toEqual([["pa", true], ["pa", true]]);
    expect(roomsOf(item)[0].status).toBe("applied");
  });

  it("discards a room, telling the engine", () => {
    const item = setup();
    discardRoom(item, "living");
    expect(calls.resolve).toEqual([["pa", false]]);
    discardRoom(item, "bed");
    expect(calls.resolve).toEqual([["pa", false], ["pb", false]]);
    expect(roomsOf(item).map((r) => r.status)).toEqual(["discarded", "discarded"]);
  });
});
