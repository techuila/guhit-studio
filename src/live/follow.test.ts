import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DocState, LiveCamera, LiveStatus, PlanView, Presence } from "../contract/bindings";
import type { PlanController } from "../editor2d/controller";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { DUR } from "../ui/motion";
import type { ViewerEngine } from "../viewer3d/engine/ViewerEngine";
import { useLiveView } from "../viewer3d/render/liveView";
import { useViewer } from "../viewer3d/viewerStore";
import type { AgentView } from "./agents";
import { useAgents } from "./agentsStore";
import { attachPlan, follow, glideMs, isFollowed, localView, startFollow, toggleFollow, unfollow } from "./follow";
import { breaksFollowLoop, followLoops, followerIds, type Peers } from "./format";
import { LIVE_OFF, useLive } from "./liveStore";

const hosting: LiveStatus = {
  ...LIVE_OFF,
  mode: "hosting",
  self_id: "me",
  participants: [
    { id: "me", name: "Mara", color: 0, role: "host", agent: null },
    { id: "ana", name: "Ana Reyes", color: 1, role: "guest", agent: null },
    { id: "ben", name: "Ben", color: 2, role: "guest", agent: null },
  ],
  project_id: "p",
};

const area: PlanView = { center: { x: 4000, y: 3000 }, width_mm: 9000, height_mm: 6000 };
const cam: LiveCamera = { position: { x: 0, y: -12000, z: 9000 }, target: { x: 4000, y: 3000, z: 1200 }, fov_deg: 45, nav: "orbit" };

const pr = (over: Partial<Presence> = {}): Presence => ({
  cursor: null,
  level_id: "L1",
  selection: [],
  typing: null,
  ai_scope: false,
  following: null,
  view_mode: "2d",
  plan_view: null,
  camera: null,
  pointer_3d: null, status: null,
  ...over,
});

const doc = { project: { id: "p", levels: [{ id: "L1" }, { id: "L2" }], elements: [] } } as unknown as DocState;

function fakeController() {
  const listeners = new Set<(v: unknown, w: number, h: number) => void>();
  let width = 800;
  const c = {
    followView: vi.fn(),
    planView: vi.fn(() => (width > 0 ? { center: { x: 1, y: 2 }, width_mm: 100, height_mm: 50 } : null)),
    onViewChange: (fn: (v: unknown, w: number, h: number) => void) => {
      listeners.add(fn);
      fn({}, width, 600);
      return () => listeners.delete(fn);
    },
  };
  return {
    controller: c as unknown as PlanController,
    followView: c.followView,
    resize: (w: number) => {
      width = w;
      for (const fn of listeners) fn({}, w, 600);
    },
  };
}

function fakeEngine() {
  const off = vi.fn();
  const e = {
    follow: vi.fn(),
    endFollow: vi.fn(),
    onCameraChange: vi.fn(() => off),
    liveCamera: (): LiveCamera => ({ position: { x: 1, y: 2, z: 3 }, target: { x: 4, y: 5, z: 6 }, fov_deg: 50, nav: "orbit" }),
  };
  return { engine: e as unknown as ViewerEngine, follow: e.follow, endFollow: e.endFollow, onCameraChange: e.onCameraChange, off };
}

let stop: () => void = () => undefined;
const push = vi.fn();

beforeEach(() => {
  useLive.setState({ status: hosting, peers: {}, following: null });
  useApp.setState({ doc, activeLevelId: "L1", viewMode: "2d", screen: "editor", toasts: [], selection: [] });
  useViewer.setState({ nav: "orbit" });
  useLiveView.setState({ engine: null, pointer3d: null });
  push.mockClear();
  stop = startFollow(push);
});

afterEach(() => {
  stop();
});

const setPeer = (id: string, over: Partial<Presence>) => useLive.getState().setPeer(id, pr(over), Date.now());
const toasts = () => useApp.getState().toasts.map((t) => t.message);

describe("starting and stopping", () => {
  it("follows someone, and takes their level and view mode", () => {
    setPeer("ana", { level_id: "L2", view_mode: "split" });
    follow("ana");
    expect(useLive.getState().following).toBe("ana");
    expect(useApp.getState().activeLevelId).toBe("L2");
    expect(useApp.getState().viewMode).toBe("split");
  });

  it("goes along when they change level or view mode, and keeps following", () => {
    setPeer("ana", {});
    follow("ana");
    setPeer("ana", { level_id: "L2", view_mode: "3d" });
    expect(useApp.getState().activeLevelId).toBe("L2");
    expect(useApp.getState().viewMode).toBe("3d");
    expect(useLive.getState().following).toBe("ana");
  });

  it("stops on clicking the avatar again, and switches on another one", () => {
    toggleFollow("ana");
    expect(useLive.getState().following).toBe("ana");
    toggleFollow("ben");
    expect(useLive.getState().following).toBe("ben");
    toggleFollow("ben");
    expect(useLive.getState().following).toBeNull();
  });

  it("never follows this window, someone not here, or outside a session", () => {
    follow("me");
    follow("zed");
    expect(useLive.getState().following).toBeNull();
    useLive.setState({ status: { ...hosting, mode: "reconnecting" } });
    follow("ana");
    expect(useLive.getState().following).toBeNull();
  });

  it("ends a walk here first: the follower orbits", () => {
    useViewer.setState({ nav: "walk" });
    follow("ana");
    expect(useViewer.getState().nav).toBe("orbit");
    expect(useLive.getState().following).toBe("ana");
  });

  it("refuses to follow someone who follows this window, which would loop", () => {
    setPeer("ana", { following: "me" });
    follow("ana");
    expect(useLive.getState().following).toBeNull();
    expect(toasts()).toEqual(["Ana is following you already."]);
  });

  it("refuses someone on a version that cannot be followed, and says so", () => {
    setPeer("ana", { view_mode: null });
    follow("ana");
    expect(useLive.getState().following).toBeNull();
    expect(toasts()).toEqual(["Ana's version of Guhit cannot be followed. Ask Ana to update."]);
  });

  it("stops with that line when their first presence comes after the click, from such a version", () => {
    follow("ana");
    expect(useLive.getState().following).toBe("ana");
    setPeer("ana", { view_mode: null });
    expect(useLive.getState().following).toBeNull();
    expect(toasts()).toEqual(["Ana's version of Guhit cannot be followed. Ask Ana to update."]);
  });

  it("tells who follows whom", () => {
    useLive.getState().setPeer("ana", pr({ following: "me" }), 1);
    expect(isFollowed()).toBe(true);
    const st = useLive.getState();
    expect(followerIds(st.status, st.peers, "me")).toEqual(["ana"]);
  });
});

describe("stop rules", () => {
  beforeEach(() => {
    setPeer("ana", { level_id: "L1", view_mode: "2d" });
    follow("ana");
  });
  const following = () => useLive.getState().following;

  it("stops on this user's own view input, from either view", () => {
    bus.emit("user_view", { view: "plan" });
    expect(following()).toBeNull();
    follow("ana");
    bus.emit("user_view", { view: "3d" });
    expect(following()).toBeNull();
  });

  it("stops on a fit or focus asked for here", () => {
    for (const ask of [() => bus.emit("zoom_to_fit"), () => bus.emit("focus_elements", ["w1"]), () => bus.emit("focus_point", { point: { x: 0, y: 0 }, level_id: null }), () => bus.emit("walk_to", { ids: [], location: null })]) {
      follow("ana");
      ask();
      expect(following()).toBeNull();
    }
  });

  it("stops when this user changes level or view mode", () => {
    useApp.getState().setActiveLevel("L2");
    expect(following()).toBeNull();
    follow("ana");
    expect(useApp.getState().activeLevelId).toBe("L1");
    useApp.getState().setViewMode("3d");
    expect(following()).toBeNull();
  });

  it("stops when this user starts walking", () => {
    useViewer.getState().setNav("walk");
    expect(following()).toBeNull();
  });

  it("never stops on hover, pointer moves or a selection", () => {
    useApp.getState().setHover("w1");
    useApp.setState({ cursor: { x: 10, y: 20 } });
    useApp.getState().select(["w1"]);
    useLiveView.setState({ pointer3d: { x: 1, y: 2, z: 3 } });
    expect(following()).toBe("ana");
  });

  it("stops with a toast when the person followed leaves", () => {
    useLive.getState().setStatus({ ...hosting, participants: hosting.participants.filter((p) => p.id !== "ana") });
    expect(following()).toBeNull();
    expect(toasts()).toEqual(["Ana left the live session. Following stopped."]);
  });

  it("stops with a toast when the session ends, and keeps going while reconnecting", () => {
    useLive.getState().setStatus({ ...hosting, mode: "reconnecting" });
    expect(following()).toBe("ana");
    useLive.getState().setStatus(LIVE_OFF);
    expect(following()).toBeNull();
    expect(toasts()).toEqual(["Following Ana stopped: the live session ended."]);
  });

  it("breaks a loop of two people who clicked each other at once: the lowest id stops", () => {
    // "ana" sorts before "me": Ana's window is the one that stops.
    setPeer("ana", { following: "me" });
    expect(following()).toBe("ana");
    setPeer("ana", {});
    // As "aa", the lowest id of the loop, this window stops.
    useLive.getState().setStatus({ ...hosting, self_id: "aa", participants: hosting.participants.map((p) => (p.id === "me" ? { ...p, id: "aa" } : p)) });
    setPeer("ana", { following: "aa" });
    expect(following()).toBeNull();
    expect(toasts()).toEqual(["Ana started following you, so following Ana stopped."]);
  });

  it("stops with no toast on this user's own doing", () => {
    unfollow();
    expect(following()).toBeNull();
    expect(toasts()).toEqual([]);
  });
});

describe("applying their views", () => {
  it("eases the plan to their area first, then glides linearly", () => {
    const f = fakeController();
    const detach = attachPlan(f.controller);
    setPeer("ana", { plan_view: area });
    follow("ana");
    expect(f.followView).toHaveBeenLastCalledWith(area, DUR.scene, false);
    const next = { ...area, center: { x: 4100, y: 3000 } };
    setPeer("ana", { plan_view: next });
    expect(f.followView).toHaveBeenLastCalledWith(next, glideMs(), true);
    // The same area again does nothing; a resized canvas fits it again.
    const calls = f.followView.mock.calls.length;
    setPeer("ana", { plan_view: next, cursor: { x: 1, y: 1 } });
    expect(f.followView.mock.calls.length).toBe(calls);
    f.resize(500);
    expect(f.followView).toHaveBeenLastCalledWith(next, glideMs(), true);
    detach();
  });

  it("waits for a plan that has no size yet", () => {
    const f = fakeController();
    f.resize(0);
    const detach = attachPlan(f.controller);
    setPeer("ana", { plan_view: area });
    follow("ana");
    expect(f.followView).not.toHaveBeenCalled();
    f.resize(640);
    expect(f.followView).toHaveBeenLastCalledWith(area, DUR.scene, false);
    detach();
  });

  it("eases the 3D camera first, then glides linearly, and again on a new 3D view", () => {
    const e = fakeEngine();
    useLiveView.setState({ engine: e.engine });
    setPeer("ana", { view_mode: "3d", camera: cam });
    follow("ana");
    expect(e.follow).toHaveBeenLastCalledWith(cam, DUR.scene, false);
    const next = { ...cam, position: { x: 500, y: -12000, z: 9000 } };
    setPeer("ana", { view_mode: "3d", camera: next });
    expect(e.follow).toHaveBeenLastCalledWith(next, glideMs(), true);
    const other = fakeEngine();
    useLiveView.setState({ engine: other.engine });
    expect(other.follow).toHaveBeenLastCalledWith(next, DUR.scene, false);
    // Stopping gives the camera its usual limits back.
    unfollow();
    expect(other.endFollow).toHaveBeenCalledTimes(1);
  });
});

describe("being followed", () => {
  it("samples the camera only while someone follows this window", () => {
    const e = fakeEngine();
    useLiveView.setState({ engine: e.engine });
    expect(e.onCameraChange).not.toHaveBeenCalled();
    expect(localView().camera).toBeNull();
    setPeer("ana", { following: "me" });
    expect(e.onCameraChange).toHaveBeenCalledTimes(1);
    expect(localView().camera).toEqual({ position: { x: 1, y: 2, z: 3 }, target: { x: 4, y: 5, z: 6 }, fov_deg: 50, nav: "orbit" });
    expect(push).toHaveBeenCalled();
    setPeer("ana", { following: null });
    expect(e.off).toHaveBeenCalledTimes(1);
    expect(localView().camera).toBeNull();
  });

  it("reads the plan area from the plan on screen, and pushes it as the view moves while followed", () => {
    const f = fakeController();
    const detach = attachPlan(f.controller);
    expect(localView().planView).toEqual({ center: { x: 1, y: 2 }, width_mm: 100, height_mm: 50 });
    push.mockClear();
    f.resize(700);
    expect(push).not.toHaveBeenCalled();
    setPeer("ben", { following: "me" });
    push.mockClear();
    f.resize(720);
    expect(push).toHaveBeenCalled();
    detach();
    expect(localView().planView).toBeNull();
  });
});

describe("follow chains", () => {
  const status = hosting;
  const peers = (links: Record<string, string | null>): Peers => Object.fromEntries(Object.entries(links).map(([id, f]) => [id, { presence: pr({ following: f }), changedAt: 0 }]));

  it("sees a chain that comes back to this window", () => {
    expect(followLoops(status, peers({ ana: "me" }), "ana", "me")).toBe(true);
    expect(followLoops(status, peers({ ana: "ben", ben: "me" }), "ana", "me")).toBe(true);
    expect(followLoops(status, peers({ ana: "ben", ben: null }), "ana", "me")).toBe(false);
    expect(followLoops(status, peers({ ana: "ben", ben: "ana" }), "ana", "me")).toBe(false);
  });

  it("lets exactly one of a loop stop: the lowest id", () => {
    expect(breaksFollowLoop({ ...status, self_id: "aa" }, peers({ ana: "aa" }), "ana")).toBe(true);
    expect(breaksFollowLoop(status, peers({ ana: "me" }), "ana")).toBe(false);
    expect(breaksFollowLoop({ ...status, self_id: "aa" }, peers({ ana: "ben", ben: "aa" }), "ana")).toBe(true);
    expect(breaksFollowLoop(status, peers({ ana: null }), "ana")).toBe(false);
  });
});

describe("following an agent (DECISIONS D47)", () => {
  const agent = (over: Partial<AgentView> = {}): AgentView => ({
    id: "agent-ground",
    name: "Ground floor",
    color: 3,
    client: "claude-code",
    clientVersion: "2",
    ownerId: null,
    ownerName: "Mara",
    presence: pr({ level_id: "L2", view_mode: null, cursor: { x: 4000, y: 3000 }, plan_view: area, pointer_3d: { x: 4000, y: 3000, z: 1200 } }),
    activity: "working",
    limit: null,
    claim: null,
    steps: 3,
    lastCallAt: new Date().toISOString(),
    local: true,
    ...over,
  });

  beforeEach(() => {
    // No live session: agents are followed all the same.
    useLive.setState({ status: LIVE_OFF, peers: {}, following: null });
    useAgents.setState({ views: [agent()] });
  });

  afterEach(() => {
    useAgents.setState({ views: [], trails: {} });
  });

  it("follows without a session: takes its level and keeps this window's view mode", () => {
    useApp.setState({ viewMode: "split" });
    follow("agent-ground");
    expect(useLive.getState().following).toBe("agent-ground");
    expect(useApp.getState().activeLevelId).toBe("L2");
    expect(useApp.getState().viewMode).toBe("split");
  });

  it("eases the plan to each new work area over --dur-scene, never the short linear glide", () => {
    const f = fakeController();
    const detach = attachPlan(f.controller);
    follow("agent-ground");
    expect(f.followView).toHaveBeenLastCalledWith(area, DUR.scene, false);
    const next = { center: { x: 9000, y: 3000 }, width_mm: 9000, height_mm: 6000 };
    useAgents.setState({ views: [agent({ presence: pr({ level_id: "L2", view_mode: null, plan_view: next }) })] });
    expect(f.followView).toHaveBeenLastCalledWith(next, DUR.scene, false);
    detach();
  });

  it("pans the 3D target to its point, keeping this camera's direction", () => {
    const e = fakeEngine();
    useLiveView.setState({ engine: e.engine });
    follow("agent-ground");
    // The fake camera looks from (1, 2, 3) at (4, 5, 6): the same offset, now at the agent's point.
    expect(e.follow).toHaveBeenLastCalledWith({ position: { x: 3997, y: 2997, z: 1197 }, target: { x: 4000, y: 3000, z: 1200 }, fov_deg: 50, nav: "orbit" }, DUR.scene, false);
    useAgents.setState({ views: [agent({ presence: pr({ level_id: "L2", pointer_3d: { x: 5000, y: 3000, z: 1200 } }) })] });
    expect(e.follow).toHaveBeenLastCalledWith(expect.objectContaining({ target: { x: 5000, y: 3000, z: 1200 }, position: { x: 4997, y: 2997, z: 1197 } }), DUR.scene, false);
  });

  it("stops on this user's own view input, and when the agent is gone", () => {
    follow("agent-ground");
    bus.emit("user_view", { view: "plan" });
    expect(useLive.getState().following).toBeNull();
    follow("agent-ground");
    useAgents.setState({ views: [] });
    expect(useLive.getState().following).toBeNull();
    const all = toasts();
    expect(all[all.length - 1]).toBe("Ground floor is no longer working here. Following stopped.");
  });
});
