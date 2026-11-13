// "Furnish rooms" in the copilot dock (lane L5, DECISIONS D8, D54): one
// proposal per room, each previewed on the plan and applied or discarded
// on its own, each one undo step.
//
// Every room's proposal waits in the engine's furnishing queue
// (crates/guhit-app/src/ai/furnish.rs), so Apply resolves it directly; one
// made before another room went in is checked again there. When the engine
// no longer holds it, the dock asks for that room alone: the model's picks
// are kept per room, so this makes no model call and gives the same objects
// with the same ids on the plan as it is now. If the objects come out
// different (the plan changed around them), the room is shown again for
// review instead of applied.

import type { AiProposal, AiTurn, DocState, Element } from "../contract/bindings";
import { ipc, toIpcError } from "../contract/ipc";
import { pointInPolygon } from "../editor2d/geom";
import { useApp } from "../state/store";
import { LOCKED_APPLY_TEXT } from "./apply";
import { useCopilot, type FurnishRoom } from "./copilotStore";

/** The styles offered, as the engine reads them (`placement::style_of`). */
export const FURNISH_STYLES = [
  { key: "modern", label: "Modern" },
  { key: "tropical", label: "Tropical" },
  { key: "classic", label: "Classic" },
  { key: "industrial", label: "Industrial" },
] as const;

/** The rooms to furnish: the selected rooms, or every room of the active level. */
export function roomsToFurnish(doc: DocState | null, selection: string[], activeLevelId: string | null): { id: string; name: string }[] {
  if (!doc) return [];
  const rooms = doc.project.elements.filter((e): e is Extract<Element, { kind: "room" }> => e.kind === "room");
  const closed = new Set((doc.derived.rooms ?? []).map((r) => r.room_id));
  const picked = rooms.filter((r) => selection.includes(r.id) && closed.has(r.id));
  if (picked.length > 0) return picked.map((r) => ({ id: r.id, name: r.name }));
  const level = activeLevelId ?? doc.project.levels[0]?.id ?? null;
  return rooms.filter((r) => r.level_id === level && closed.has(r.id)).map((r) => ({ id: r.id, name: r.name }));
}

/**
 * The room a proposal furnishes, by id: of the asked rooms not paired yet
 * (`taken`), the one its new objects stand in, on that room's level, in the
 * proposal's own dry run. Two rooms often share a name ("Bedroom", "T&B"),
 * and a room where nothing fits gets no proposal, so neither the name in the
 * batch label ("AI: Furnish <room>") nor the order says which room it is.
 * The name only breaks a tie, or answers when the dry run has no outlines.
 */
function roomOf(p: AiProposal, rooms: { id: string; name: string }[], taken: ReadonlySet<string>): { id: string; name: string } {
  const label = p.command.type === "batch" ? p.command.label : "";
  const name = label.replace(/^AI: Furnish /, "");
  const free = rooms.filter((r) => !taken.has(r.id));
  const { project, derived } = p.preview.state;
  const added = new Set(p.preview.diff.added);
  const objects = project.elements.filter((e): e is Extract<Element, { kind: "asset" }> => e.kind === "asset" && added.has(e.id));
  let best: { id: string; name: string } | null = null;
  let bestScore = 0;
  for (const r of free) {
    const room = project.elements.find((e) => e.kind === "room" && e.id === r.id);
    const outline = derived.rooms?.find((g) => g.room_id === r.id)?.polygon;
    if (!room || room.kind !== "room" || !outline || outline.length < 3) continue;
    const inside = objects.filter((o) => o.level_id === room.level_id && pointInPolygon(o.position, outline)).length;
    const score = inside > 0 ? inside * 2 + (r.name === name ? 1 : 0) : 0;
    if (score > bestScore) {
      best = r;
      bestScore = score;
    }
  }
  return best ?? free.find((r) => r.name === name) ?? { id: "", name };
}

/** The objects a proposal adds, by id, for comparing two plans of a room. */
function addedOf(p: AiProposal): string {
  const ids = new Set(p.preview.diff.added);
  const els = p.preview.state.project.elements.filter((e) => ids.has(e.id));
  return JSON.stringify(els);
}

/** The backend's pending proposal, as far as the dock knows. */
let backendPending: string | null = null;

export function turnRooms(turn: AiTurn, rooms: { id: string; name: string }[]): FurnishRoom[] {
  const list = turn.proposals ?? [];
  backendPending = list[0]?.id ?? null;
  const taken = new Set<string>();
  return list.map((p) => {
    const r = roomOf(p, rooms, taken);
    if (r.id) taken.add(r.id);
    return { roomId: r.id, roomName: r.name, proposal: p, status: "pending", note: null };
  });
}

/** Asks for the furnishing of `rooms` in `style`. */
export async function requestFurnish(rooms: { id: string; name: string }[], style: string): Promise<AiTurn> {
  const app = useApp.getState();
  const scope = app.aiScope && app.selection.length > 0 ? { ids: app.selection } : null;
  return ipc.aiChat({
    message: `Furnish ${rooms.length === 1 ? rooms[0].name : `${rooms.length} rooms`}, ${style}`,
    selection_ids: app.selection,
    active_level_id: app.activeLevelId,
    history: [],
    scope,
    task: { type: "furnish", room_ids: rooms.map((r) => r.id), style },
  });
}

function patchRoom(itemId: number, roomId: string, patch: Partial<FurnishRoom>): void {
  const st = useCopilot.getState();
  const item = st.items.find((m) => m.id === itemId);
  if (!item?.furnish) return;
  st.patch(itemId, { furnish: item.furnish.map((r) => (r.roomId === roomId ? { ...r, ...patch } : r)) });
}

function roomFor(itemId: number, roomId: string): FurnishRoom | null {
  return useCopilot.getState().items.find((m) => m.id === itemId)?.furnish?.find((r) => r.roomId === roomId) ?? null;
}

/** Shows one room's proposal on the plan. */
export function previewRoom(room: FurnishRoom | null): void {
  useApp.getState().setPreview(room && room.status === "pending" ? room.proposal.preview : null);
}

/**
 * The rooms still waiting get a dry run on the plan as it is now, so their
 * previews show what is already applied too.
 */
async function refreshPending(itemId: number): Promise<void> {
  const item = useCopilot.getState().items.find((m) => m.id === itemId);
  for (const r of item?.furnish ?? []) {
    if (r.status !== "pending") continue;
    try {
      const preview = await ipc.docPreview(r.proposal.command);
      const revision = useApp.getState().doc?.revision ?? r.proposal.base_revision;
      patchRoom(itemId, r.roomId, { proposal: { ...r.proposal, preview, base_revision: revision } });
    } catch {
      patchRoom(itemId, r.roomId, { status: "stale", note: "This room no longer fits the plan. Ask again to furnish it." });
    }
  }
}

/** After a room's resolve: the plan, the card, a toast, and fresh previews for the rest. */
async function landed(itemId: number, room: FurnishRoom, state: DocState | null): Promise<void> {
  const st = useApp.getState();
  st.setPreview(null);
  st.setHover(null);
  if (!state) {
    patchRoom(itemId, room.roomId, { status: "discarded", note: "Nothing was applied." });
    return;
  }
  st.setDoc(state);
  patchRoom(itemId, room.roomId, { status: "applied", note: "Applied. Undo reverts this room in one step." });
  st.toast("success", `${room.roomName} furnished. Undo reverts it in one step.`);
  await refreshPending(itemId);
}

/** Applies one room's proposal (D8: the user approves each room). */
export async function applyRoom(itemId: number, roomId: string, style: string): Promise<void> {
  const room = roomFor(itemId, roomId);
  if (!room || room.status !== "pending") return;
  patchRoom(itemId, roomId, { status: "applying" });
  try {
    // Every room waits in the engine's furnishing queue: apply it directly,
    // checked again there when another room went in first.
    const direct = await ipc.aiResolve(room.proposal.id, true).catch((e) => {
      const err = toIpcError(e);
      if (err.code === "stale" || err.code === "not_found") return null;
      throw e;
    });
    if (direct) {
      if (backendPending === room.proposal.id) backendPending = null;
      await landed(itemId, room, direct.applied?.state ?? null);
      return;
    }
    // Gone from the queue (an older engine, or the plan moved under it): plan
    // this room again on the plan as it is now. No model call is made.
    const again = await requestFurnish([{ id: roomId, name: room.roomName }], style);
    const next = again.proposals?.[0];
    backendPending = next?.id ?? null;
    if (!next) {
      patchRoom(itemId, roomId, { status: "stale", note: "Nothing fits this room any more." });
      return;
    }
    if (addedOf(next) !== addedOf(room.proposal)) {
      // Not what the user saw: show it for review instead of applying it (D8).
      patchRoom(itemId, roomId, { status: "pending", proposal: next, note: "The plan changed, so this room was planned again. Review it on the plan, then apply." });
      previewRoom({ ...room, proposal: next, status: "pending" });
      return;
    }
    const result = await ipc.aiResolve(next.id, true);
    backendPending = null;
    await landed(itemId, room, result.applied?.state ?? null);
  } catch (e) {
    const err = toIpcError(e);
    if (err.code === "swarm_running") {
      // A swarm draft locks the model (D57). The engine keeps the room's
      // proposal, so the card and its preview stay for after the draft.
      patchRoom(itemId, roomId, { status: "pending", note: LOCKED_APPLY_TEXT });
      return;
    }
    patchRoom(itemId, roomId, { status: "stale", note: err.code === "stale" || err.code === "not_found" ? "The plan changed, so this room was not applied. Ask again." : `Not applied: ${err.message}` });
    useApp.getState().setPreview(null);
    try {
      const state = await ipc.docState();
      if (state) useApp.getState().setDoc(state);
    } catch {
      // The next action will show a bridge problem.
    }
  }
}

/** Discards one room's proposal. */
export function discardRoom(itemId: number, roomId: string): void {
  const room = roomFor(itemId, roomId);
  if (!room || room.status !== "pending") return;
  if (backendPending === room.proposal.id) backendPending = null;
  // The engine drops it from its queue; one it no longer holds is harmless.
  void ipc.aiResolve(room.proposal.id, false).catch(() => undefined);
  patchRoom(itemId, roomId, { status: "discarded", note: "Discarded. The room was not changed." });
  useApp.getState().setPreview(null);
}

/** Every room still waiting in the conversation is retired: a new turn replaces it. */
export function retireFurnish(note: string): void {
  const st = useCopilot.getState();
  for (const m of st.items) {
    if (!m.furnish?.some((r) => r.status === "pending")) continue;
    st.patch(m.id, { furnish: m.furnish.map((r) => (r.status === "pending" ? { ...r, status: "discarded", note } : r)) });
  }
  backendPending = null;
}

/** For tests: forget what the backend holds. */
export function resetFurnishState(): void {
  backendPending = null;
}
