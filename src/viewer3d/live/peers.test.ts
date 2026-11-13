import { describe, expect, it } from "vitest";
import type { LiveStatus, Presence } from "../../contract/bindings";
import type { Peers } from "../../live/format";
import { historyTintOf, peerTints, pointerIds, sameTints } from "./peers";

const status = (mode: LiveStatus["mode"] = "hosting"): LiveStatus => ({
  mode,
  self_id: "me",
  participants: [
    { id: "me", name: "Mara", color: 0, role: "host" },
    { id: "ana", name: "Ana", color: 1, role: "guest" },
    { id: "ben", name: "Ben", color: 10, role: "guest" },
  ],
  invite: null,
  addresses: [],
  relay: "off",
  project_id: null,
  project_name: null,
  notice: null,
} as LiveStatus);

const presence = (over: Partial<Presence> = {}): Presence => ({
  cursor: null,
  level_id: null,
  selection: [],
  typing: null,
  ai_scope: false,
  following: null,
  view_mode: null,
  plan_view: null,
  camera: null,
  pointer_3d: null,
  ...over,
});

const colorOf = (i: number) => `#00000${i}`;

describe("pointerIds", () => {
  it("lists the others with a 3D pointer, in the host's order", () => {
    const peers: Peers = {
      ben: { presence: presence({ pointer_3d: { x: 1, y: 2, z: 3 } }), changedAt: 0 },
      ana: { presence: presence({ pointer_3d: { x: 4, y: 5, z: 6 } }), changedAt: 0 },
    };
    expect(pointerIds(status(), peers)).toEqual(["ana", "ben"]);
  });

  it("skips people off the model, this computer and a session that is off", () => {
    const peers: Peers = {
      me: { presence: presence({ pointer_3d: { x: 1, y: 2, z: 3 } }), changedAt: 0 },
      ana: { presence: presence(), changedAt: 0 },
      ben: { presence: presence({ pointer_3d: { x: 1, y: 2, z: 3 } }), changedAt: 0 },
    };
    expect(pointerIds(status(), peers)).toEqual(["ben"]);
    expect(pointerIds(status("off"), peers)).toEqual([]);
  });
});

describe("peerTints", () => {
  it("gives each selection its owner's color, wrapped into the eight tokens", () => {
    const peers: Peers = {
      ana: { presence: presence({ selection: ["w1", "w2"] }), changedAt: 0 },
      ben: { presence: presence({ selection: ["d1"] }), changedAt: 0 },
    };
    expect(peerTints(status(), peers, colorOf)).toEqual([
      { ids: ["w1", "w2"], color: "#000001" },
      { ids: ["d1"], color: "#000002" },
    ]);
  });

  it("leaves out empty selections", () => {
    const peers: Peers = { ana: { presence: presence(), changedAt: 0 } };
    expect(peerTints(status(), peers, colorOf)).toEqual([]);
  });
});

describe("historyTintOf", () => {
  it("uses the accent for this computer's own steps", () => {
    expect(historyTintOf({ ids: ["a"], color: null }, colorOf, "#0e8a8f")).toEqual({ ids: ["a"], color: "#0e8a8f" });
    expect(historyTintOf({ ids: ["a"], color: 3 }, colorOf, "#0e8a8f")).toEqual({ ids: ["a"], color: "#000003" });
    expect(historyTintOf({ ids: [], color: 3 }, colorOf, "#0e8a8f")).toBeNull();
    expect(historyTintOf(null, colorOf, "#0e8a8f")).toBeNull();
  });
});

describe("sameTints", () => {
  it("compares ids and colors in order", () => {
    const a = [{ ids: ["w1"], color: "#1" }];
    expect(sameTints(a, [{ ids: ["w1"], color: "#1" }])).toBe(true);
    expect(sameTints(a, [{ ids: ["w1"], color: "#2" }])).toBe(false);
    expect(sameTints(a, [{ ids: ["w2"], color: "#1" }])).toBe(false);
    expect(sameTints(a, [])).toBe(false);
  });
});
