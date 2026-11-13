// Follow mode (DECISIONS D37). Clicking someone's avatar follows them: this
// window takes their level and view mode, then their plan area and 3D camera
// glide along. Both sides live here:
//   - the follower applies the followed person's presence (`plan_view`
//     through the plan controller LiveLayer hands to `attachPlan`, `camera`
//     through the 3D engine in `useLiveView`) and stops on its own input,
//   - the person followed samples the plan area and the camera for presence
//     (`localView`), only while someone follows them.
// No frame loop of its own: the plan eases in its draw pass, the camera
// through `ViewerEngine.schedule()`, and both are told by events.
//
// Stops (DECISIONS D37): Esc (shortcuts.tsx), the Stop button, the avatar
// again, `user_view` from either view, a fit or focus asked for here, a
// level or view mode change that is not the one followed, the person
// followed leaving and the session ending. Hover and pointer moves never stop it.

import type { LiveCamera, PlanView, Presence } from "../contract/bindings";
import type { PlanController } from "../editor2d/controller";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { dur } from "../ui/motion";
import type { ViewerEngine } from "../viewer3d/engine/ViewerEngine";
import { useLiveView } from "../viewer3d/render/liveView";
import { useViewer } from "../viewer3d/viewerStore";
import { breaksFollowLoop, canChat, canFollow, firstName, followLoops, followerIds, oldVersionLine, participant } from "./format";
import { useLive } from "./liveStore";

/**
 * Between two updates a followed view glides linearly over about one
 * update, like a remote cursor (`--dur-press` x 0.75, LiveLayer.module.css).
 * 0 with reduced motion: the view steps.
 */
export function glideMs(): number {
  return Math.round(dur("press") * 0.75);
}

/**
 * Why following stopped: this user, the person followed left, the session
 * ended, both followed each other (`breaksFollowLoop`), or their version
 * turned out to be one that cannot be followed (`canFollow`).
 */
export type StopReason = "user" | "left" | "ended" | "loop" | "old";

/** The presence of the person this window follows, null when it follows no one or it has not arrived. */
export function leaderPresence(): Presence | null {
  const st = useLive.getState();
  return st.following ? (st.peers[st.following]?.presence ?? null) : null;
}

/** Someone in the session follows this window: the plan area and camera go out. */
export function isFollowed(): boolean {
  const st = useLive.getState();
  return followerIds(st.status, st.peers, st.status.self_id).length > 0;
}

/**
 * Takes the followed person's level and view mode. Called on start and on
 * each of their presence updates; a level this copy does not have yet waits
 * for the next document.
 */
function syncPlace(): void {
  const pr = leaderPresence();
  if (!pr) return;
  const app = useApp.getState();
  if (pr.level_id && pr.level_id !== app.activeLevelId && app.doc?.project.levels.some((l) => l.id === pr.level_id)) app.setActiveLevel(pr.level_id);
  if (pr.view_mode && pr.view_mode !== app.viewMode) app.setViewMode(pr.view_mode);
}

/** Follows `id`, or switches to them from someone else. */
export function follow(id: string): void {
  const live = useLive.getState();
  const p = participant(live.status, id);
  if (!canChat(live.status.mode) || id === live.status.self_id || !p) return;
  if (live.following === id) return;
  // An older version never sends its views: say so rather than show a frame that follows nothing.
  if (!canFollow(live.peers, id)) {
    useApp.getState().toast("info", oldVersionLine(p.name));
    return;
  }
  // They see what this window sees already: following them back would loop.
  if (followLoops(live.status, live.peers, id, live.status.self_id)) {
    const direct = live.peers[id]?.presence.following === live.status.self_id;
    useApp.getState().toast("info", direct ? `${firstName(p.name)} is following you already.` : `${firstName(p.name)} already follows your view through someone else.`);
    return;
  }
  // The follower orbits: a walk or flight here ends first, before the stop rules watch.
  if (useViewer.getState().nav !== "orbit") useViewer.getState().setNav("orbit");
  live.setFollowing(id);
  syncPlace();
}

/** Stops following. A toast says why when it was not this user's doing. */
export function unfollow(reason: StopReason = "user"): void {
  const live = useLive.getState();
  const id = live.following;
  if (!id) return;
  const name = firstName(participant(live.status, id)?.name ?? names.get(id) ?? "them");
  live.setFollowing(null);
  if (reason === "left") useApp.getState().toast("info", `${name} left the live session. Following stopped.`);
  else if (reason === "ended") useApp.getState().toast("info", `Following ${name} stopped: the live session ended.`);
  else if (reason === "loop") useApp.getState().toast("info", `${name} started following you, so following ${name} stopped.`);
  else if (reason === "old") useApp.getState().toast("info", oldVersionLine(name));
}

/** The avatar in the top bar: follows that person, or stops when it is them already. */
export function toggleFollow(id: string): void {
  if (useLive.getState().following === id) unfollow();
  else follow(id);
}

/** Names of the people in the session, kept after they leave for the toast. */
const names = new Map<string, string>();

// ---------------------------------------------------------------- the plan

let planSource: PlanController | null = null;
let pushPresence: () => void = () => undefined;
let localCamera: LiveCamera | null = null;

/** What this window shows, for presence: the plan area and the 3D camera. */
export function localView(): { planView: PlanView | null; camera: LiveCamera | null } {
  return { planView: planSource?.planView() ?? null, camera: localCamera };
}

function samePlanView(a: PlanView, b: PlanView): boolean {
  return a.center.x === b.center.x && a.center.y === b.center.y && a.width_mm === b.width_mm && a.height_mm === b.height_mm;
}

/**
 * The plan on screen, from LiveLayer: sends its area while someone follows
 * this window, and shows the followed person's area while this window
 * follows someone. The first move of a follow eases (`--dur-scene`), the
 * ones after it glide linearly into each other. Returns the cleanup.
 */
export function attachPlan(controller: PlanController): () => void {
  planSource = controller;
  let started = false;
  let last: PlanView | null = null;
  let size = { width: 0, height: 0 };
  /** `resized`: the same area again, fitted to the canvas's new size (a view mode change animates the panes). */
  const apply = (resized = false) => {
    const area = leaderPresence()?.plan_view ?? null;
    if (!area || (!resized && started && last && samePlanView(last, area))) return;
    // No size yet (the pane is just mounting): the first view change retries.
    if (controller.planView() === null) return;
    controller.followView(area, started ? glideMs() : dur("scene"), started);
    started = true;
    last = area;
  };
  const offView = controller.onViewChange((_view, width, height) => {
    const resized = width !== size.width || height !== size.height;
    size = { width, height };
    if (width > 0 && useLive.getState().following && (!started || resized)) apply(resized);
    if (isFollowed()) pushPresence();
  });
  const offLive = useLive.subscribe((st, prev) => {
    if (st.following !== prev.following) {
      started = false;
      last = null;
    }
    if (st.following && (st.following !== prev.following || st.peers !== prev.peers)) apply();
  });
  apply();
  return () => {
    offView();
    offLive();
    if (planSource === controller) planSource = null;
    pushPresence();
  };
}

// ---------------------------------------------------------------- wiring

function sameCamera(a: LiveCamera, b: LiveCamera): boolean {
  const v = (p: LiveCamera["position"], q: LiveCamera["position"]) => p.x === q.x && p.y === q.y && p.z === q.z;
  return a.nav === b.nav && a.fov_deg === b.fov_deg && v(a.position, b.position) && v(a.target, b.target);
}

/**
 * Starts follow mode for the window, from wiring.ts with its presence push.
 * Returns the cleanup.
 */
export function startFollow(push: () => void): () => void {
  pushPresence = push;
  const stops: Array<() => void> = [];

  // ---- the person followed: the camera, sampled only while someone follows.
  let camOff: (() => void) | null = null;
  let camEngine: ViewerEngine | null = null;
  const syncCamera = () => {
    const engine = useLiveView.getState().engine;
    const want = engine !== null && isFollowed();
    if (want && camEngine === engine) return;
    camOff?.();
    camOff = null;
    camEngine = null;
    localCamera = null;
    if (!want || !engine) return;
    camEngine = engine;
    localCamera = engine.liveCamera();
    camOff = engine.onCameraChange((c) => {
      localCamera = c;
      push();
    });
    push();
  };
  stops.push(() => {
    camOff?.();
    camOff = null;
    camEngine = null;
    localCamera = null;
  });

  // ---- the follower: the 3D camera.
  let started3d: ViewerEngine | null = null;
  let lastCam: LiveCamera | null = null;
  const apply3d = () => {
    const engine = useLiveView.getState().engine;
    const cam = leaderPresence()?.camera ?? null;
    if (!engine || !cam) return;
    const first = started3d !== engine;
    if (!first && lastCam && sameCamera(lastCam, cam)) return;
    engine.follow(cam, first ? dur("scene") : glideMs(), !first);
    started3d = engine;
    lastCam = cam;
  };

  let followed = isFollowed();
  stops.push(
    useLive.subscribe((st, prev) => {
      if (st.status !== prev.status) for (const p of st.status.participants) names.set(p.id, p.name);
      const id = st.following;
      if (id !== prev.following) {
        // The camera gets its usual limits back (a new person is a new first move).
        if (started3d) started3d.endFollow();
        started3d = null;
        lastCam = null;
      }
      if (id) {
        // The session ended, or the person followed left (reconnecting keeps them).
        if (st.status.mode === "off") return unfollow("ended");
        if (st.status.mode !== "reconnecting" && !participant(st.status, id)) return unfollow("left");
        if (st.peers !== prev.peers && breaksFollowLoop(st.status, st.peers, id)) return unfollow("loop");
        // Their first presence came after the click, from a version that cannot be followed.
        if (st.peers[id] !== prev.peers[id] && !canFollow(st.peers, id)) return unfollow("old");
        if (id !== prev.following || st.peers[id] !== prev.peers[id]) {
          syncPlace();
          apply3d();
        }
      }
      if (st.peers !== prev.peers || st.status !== prev.status) {
        syncCamera();
        // Someone started or stopped following this window: the plan area goes out or stops.
        const now = isFollowed();
        if (now !== followed) {
          followed = now;
          push();
        }
      }
      if (id !== prev.following) push();
    }),
  );
  stops.push(
    useLiveView.subscribe((st, prev) => {
      if (st.engine !== prev.engine) {
        if (started3d === prev.engine) started3d = null;
        syncCamera();
        if (useLive.getState().following) apply3d();
      }
      if (st.pointer3d !== prev.pointer3d || st.engine !== prev.engine) push();
    }),
  );

  // ---- stop rules: this user's own view input.
  const stopOnInput = () => {
    if (useLive.getState().following) unfollow();
  };
  stops.push(bus.on("user_view", stopOnInput));
  stops.push(bus.on("zoom_to_fit", stopOnInput));
  stops.push(bus.on("focus_elements", stopOnInput));
  stops.push(bus.on("focus_point", stopOnInput));
  stops.push(bus.on("apply_camera", stopOnInput));
  stops.push(bus.on("walk_to", stopOnInput));
  stops.push(
    useApp.subscribe((s, p) => {
      if (!useLive.getState().following) return;
      const pr = leaderPresence();
      // Following itself takes their level and view mode; anything else is this user.
      if (s.activeLevelId !== p.activeLevelId && s.activeLevelId !== (pr?.level_id ?? null)) return unfollow();
      if (s.viewMode !== p.viewMode && s.viewMode !== (pr?.view_mode ?? null)) return unfollow();
      if (s.screen !== p.screen && s.screen !== "editor") return unfollow();
      if (s.doc !== p.doc) syncPlace();
    }),
  );
  stops.push(
    useViewer.subscribe((s, p) => {
      // Walking or flying starts here: that is this user moving.
      if (s.nav !== p.nav && s.nav !== "orbit" && useLive.getState().following) unfollow();
    }),
  );

  for (const p of useLive.getState().status.participants) names.set(p.id, p.name);
  syncCamera();
  return () => {
    for (const stop of stops) stop();
    pushPresence = () => undefined;
    if (useLive.getState().following) useLive.getState().setFollowing(null);
  };
}
