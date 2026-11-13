// The stair tool (lane L2): what it places. The controller places a stair
// like a column (click, R rotates) and asks here for it: straight, L or U
// with the turn picked in the tool's flyout or with L and T over the plan
// (DECISIONS D54). Before the engine has worked out a stair that is not in
// the document (the placement ghost, a stair being resized), `stairGhost`
// asks it through a preview and keeps the answer, so the ghost shows the
// real flights and landing; the plan never does its own stair math.

import { create } from "zustand";
import type { StairGeometry, StairShape, Turn } from "../contract/bindings";
import { ipc } from "../contract/ipc";
import { useApp } from "../state/store";
import type { P } from "./geom";
import { moveStairGeometry } from "./roofsStairsGeom";
import type { StairEl } from "./slab";

/** About 180 mm risers, 250 mm treads, 1000 mm wide (DECISIONS D44). */
export const STAIR_RISER_MM = 180;
export const STAIR_TREAD_MM = 250;
export const STAIR_WIDTH_MM = 1000;
/** An L or U needs two flights of at least two risers. */
export const MIN_TURNING_RISERS = 4;
/** The gap between a new U stair's flights. */
export const U_WELL_MM = 100;

export interface StairToolState {
  shape: StairShape;
  /** Which way an L or U turns going up. */
  turn: Turn;
}

/** The shape and turn the next stair takes. Straight until changed. */
export const useStairTool = create<StairToolState>(() => ({ shape: "straight", turn: "left" }));

export function setStairShape(shape: StairShape): void {
  useStairTool.setState({ shape });
}

export function setStairTurn(turn: Turn): void {
  useStairTool.setState({ turn });
}

export const STAIR_SHAPES: ReadonlyArray<{ value: StairShape; label: string; hint: string }> = [
  { value: "straight", label: "Straight", hint: "one flight" },
  { value: "l", label: "L", hint: "quarter turn" },
  { value: "u", label: "U", hint: "half turn" },
];

const NEXT_SHAPE: Record<StairShape, StairShape> = { straight: "l", l: "u", u: "straight" };

/**
 * A key over the plan while the stair tool is on: L cycles straight, L, U;
 * T flips the turn of an L or U. True when it used the key.
 */
export function stairToolKey(key: string): boolean {
  const k = key.toLowerCase();
  const { shape, turn } = useStairTool.getState();
  if (k === "l") {
    setStairShape(NEXT_SHAPE[shape]);
    return true;
  }
  if (k === "t" && shape !== "straight") {
    setStairTurn(turn === "left" ? "right" : "left");
    return true;
  }
  return false;
}

/** The status bar hint while the stair tool is on. */
export function stairToolHint(state: StairToolState = useStairTool.getState()): string {
  if (state.shape === "straight") return "Click to place a straight stair. R rotates, L changes the shape";
  return `Click to place ${state.shape === "l" ? "an L" : "a U"} stair turning ${state.turn}. R rotates, L changes the shape, T the turn`;
}

/**
 * A stair up a level of `levelHeightMm` in the tool's shape: about 180 mm
 * risers (at least 3, 4 for an L or U), treads of about 250 mm (n - 1 of
 * them straight, n - 2 for an L or U, as each flight's top riser lands on
 * its landing or the floor), 1000 mm wide, with the 900 mm handrail new
 * stairs get (DECISIONS D44, D50, D54). `origin` is the center of the first
 * riser. The first flight takes half the risers and the landing is as deep
 * as the stair is wide until changed in the inspector.
 */
export function newStair(levelId: string, levelHeightMm: number | undefined, origin: P, rotationDeg: number, tool: StairToolState = useStairTool.getState()): StairEl {
  const turning = tool.shape !== "straight";
  const risers = Math.max(turning ? MIN_TURNING_RISERS : 3, Math.round((levelHeightMm ?? 3000) / STAIR_RISER_MM));
  const treads = risers - (turning ? 2 : 1);
  const el: StairEl = {
    kind: "stair",
    id: "",
    level_id: levelId,
    origin,
    rotation_deg: rotationDeg,
    width_mm: STAIR_WIDTH_MM,
    run_mm: treads * STAIR_TREAD_MM,
    riser_count: risers,
    // New stairs get a handrail (DECISIONS D50).
    railing: { height_mm: 900, style: "bars" },
  };
  // Straight leaves the new fields out, as the engine writes its defaults.
  if (!turning) return el;
  return { ...el, shape: tool.shape, turn: tool.turn, first_flight_risers: null, landing_mm: null, ...(tool.shape === "u" ? { well_mm: U_WELL_MM } : {}) };
}

// ---------------------------------------------------------------- the engine's ghost

interface Ghost {
  /** The document revision it was asked at. */
  revision: number;
  geometry: StairGeometry | null;
  /** Where the asked stair stood: the answer moves from here to each ghost. */
  at: Pick<StairEl, "origin" | "rotation_deg">;
}

/** Answers by everything but where a stair stands and which way it faces. A few shapes at a time. */
const ghosts = new Map<string, Ghost>();
const MAX_GHOSTS = 24;
let asking: string | null = null;
let next: { key: string; el: StairEl } | null = null;
const listeners = new Set<() => void>();

function ghostKey(el: StairEl): string {
  const { id: _id, origin: _origin, rotation_deg: _rotation, ...rest } = el;
  return JSON.stringify(rest);
}

function revision(): number | null {
  const doc = useApp.getState().doc;
  return doc ? doc.revision : null;
}

/** Calls `fn` each time an answer arrives, so the plan redraws the ghost. Returns the unsubscribe. */
export function onStairGhost(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

/**
 * The engine's geometry of a stair that is not in the document, at `el`'s
 * origin and rotation. Null until the first answer for its shape arrives
 * (draw the plain outline meanwhile); an answer from an older revision of
 * the document stands in while a fresh one is asked. One question at a
 * time: the latest shape wanted is asked next.
 */
export function stairGhost(el: StairEl): StairGeometry | null {
  const rev = revision();
  if (rev === null) return null;
  const key = ghostKey(el);
  const hit = ghosts.get(key);
  if (!hit || hit.revision !== rev) ask(key, el);
  return hit?.geometry ? moveStairGeometry(hit.geometry, hit.at, el) : null;
}

function ask(key: string, el: StairEl): void {
  if (asking !== null) {
    if (asking !== key) next = { key, el };
    return;
  }
  const rev = revision();
  if (rev === null) return;
  asking = key;
  const keep = (geometry: StairGeometry | null): void => {
    ghosts.delete(key);
    ghosts.set(key, { revision: rev, geometry, at: { origin: el.origin, rotation_deg: el.rotation_deg } });
    while (ghosts.size > MAX_GHOSTS) ghosts.delete(ghosts.keys().next().value as string);
  };
  ipc
    .docPreview({ type: "add_element", element: { ...el, id: "" } })
    .then(
      (r) => {
        const added = new Set(r.diff.added);
        keep(r.state.derived.stairs.find((g) => added.has(g.stair_id)) ?? null);
      },
      // Refused (an L with too few risers) or no engine: the plain outline.
      () => keep(null),
    )
    .finally(() => {
      asking = null;
      const want = next;
      next = null;
      if (want && want.key !== key && ghosts.get(want.key)?.revision !== revision()) ask(want.key, want.el);
      for (const fn of [...listeners]) fn();
    });
}

/** Forgets every answer (tests, another project). */
export function clearStairGhosts(): void {
  ghosts.clear();
  asking = null;
  next = null;
}
