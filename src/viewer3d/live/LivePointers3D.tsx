// Other people's pointers over the 3D view (DECISIONS D37): a dot in their
// color with their name, on the model point under their pointer
// (`Presence::pointer_3d`). Mounted by Viewer3D over the canvas.
//
// Cost rules, as on the plan (src/live/LiveLayer.tsx). A pointer update never
// re-renders React: the point goes through the camera as last drawn and
// straight into the marker's transform, which glides with a short linear
// transition. A camera move re-places every marker from the engine's drawn
// frame (`onProjectionChange`) with no transition, so markers stay on the
// model. Nothing here runs a frame loop. A marker behind something solid
// dims: one raycast per marker on a pointer update and once the camera
// rests, never per frame.

import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { useShallow } from "zustand/react/shallow";
import type { Vec3 } from "../../contract/bindings";
import { participant, peerVar } from "../../live/format";
import { useLive } from "../../live/liveStore";
import { labelFlip } from "../../live/placement";
import { cx } from "../../ui/controls";
import { useLastTruthy } from "../../ui/motionDom";
import { useListPresence } from "../../ui/useListPresence";
import type { ViewerEngine } from "../engine/ViewerEngine";
import { pointerIds } from "./peers";
import { onView, samePoint } from "./project";
import s from "./LivePointers3D.module.css";

/** A pointer update this soon after the picture moved jumps instead of gliding: the camera is still going. */
const CAMERA_REST_MS = 120;

/** Room around the canvas where a marker still shows, CSS pixels. */
const EDGE_PX = 24;

interface Item {
  el: HTMLElement;
  p: Vec3 | null;
  snap: boolean;
  off: boolean;
  hidden: boolean;
  fx: boolean;
  fy: boolean;
}

/** Where every marker is, re-placed when the picture moves. */
class PointerBoard {
  private items = new Map<string, Item>();
  private movedAt = Number.NEGATIVE_INFINITY;
  private restTimer = 0;

  constructor(private readonly engine: ViewerEngine) {}

  add(id: string, el: HTMLElement): void {
    this.items.set(id, { el, p: null, snap: true, off: true, hidden: false, fx: false, fy: false });
  }

  remove(id: string, el: HTMLElement): void {
    if (this.items.get(id)?.el === el) this.items.delete(id);
  }

  /**
   * A new pointer from presence. It glides there unless the camera is moving.
   * Presence that changed something else (the plan cursor, the selection)
   * leaves the marker alone: no placing, no raycast.
   */
  move(id: string, p: Vec3): void {
    const item = this.items.get(id);
    if (!item || samePoint(item.p, p)) return;
    item.p = p;
    this.place(item, performance.now() - this.movedAt > CAMERA_REST_MS);
    this.checkHidden(item);
  }

  /** The engine drew a frame that moved the picture: every marker follows the model at once. */
  reproject(): void {
    if (this.items.size === 0) return;
    this.movedAt = performance.now();
    for (const item of this.items.values()) this.place(item, false);
    // Whether a marker is behind something is asked once the camera rests.
    window.clearTimeout(this.restTimer);
    this.restTimer = window.setTimeout(() => {
      for (const item of this.items.values()) this.checkHidden(item);
    }, CAMERA_REST_MS);
  }

  dispose(): void {
    window.clearTimeout(this.restTimer);
    this.items.clear();
  }

  private place(item: Item, glide: boolean): void {
    if (!item.p) return;
    const { width, height } = this.engine.viewSize();
    const at = this.engine.projectToCanvas(item.p);
    const off = !onView(at, width, height, EDGE_PX);
    const el = item.el;
    // Coming back onto the view, a marker appears where it is, not sliding in from the edge.
    const snap = !glide || item.off || off;
    if (snap !== item.snap) el.dataset.snap = String((item.snap = snap));
    if (off !== item.off) el.dataset.off = String((item.off = off));
    if (off) return;
    el.style.transform = `translate3d(${at.x.toFixed(1)}px, ${at.y.toFixed(1)}px, 0)`;
    const f = labelFlip(at, width, height);
    if (f.x !== item.fx) el.dataset.flipX = String((item.fx = f.x));
    if (f.y !== item.fy) el.dataset.flipY = String((item.fy = f.y));
  }

  private checkHidden(item: Item): void {
    const hidden = item.p !== null && !item.off && this.engine.pointHidden(item.p);
    if (hidden !== item.hidden) item.el.dataset.hidden = String((item.hidden = hidden));
  }
}

export function LivePointers3D({ engine }: { engine: ViewerEngine }) {
  const [board, setBoard] = useState<PointerBoard | null>(null);

  useLayoutEffect(() => {
    const b = new PointerBoard(engine);
    setBoard(b);
    const stop = engine.onProjectionChange(() => b.reproject());
    return () => {
      stop();
      b.dispose();
      setBoard(null);
    };
  }, [engine]);

  const ids = useLive(useShallow((st) => pointerIds(st.status, st.peers)));
  const rows = useListPresence(ids, (id) => id, "base");
  // Reconnecting: the markers show where people were, not where they are.
  const stale = useLive((st) => st.status.mode === "reconnecting");
  if (!board || rows.length === 0) return null;
  return (
    <div className={s.layer} data-stale={stale || undefined} data-testid="live-pointers-3d" aria-hidden>
      {rows.map((row) => (
        <Marker key={row.key} id={row.key} board={board} entering={row.entering} leaving={row.leaving} />
      ))}
    </div>
  );
}

function Marker({ id, board, entering, leaving }: { id: string; board: PointerBoard; entering: boolean; leaving: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const name = useLive((st) => participant(st.status, id)?.name ?? null);
  const color = useLive((st) => participant(st.status, id)?.color ?? null);
  const shownName = useLastTruthy(name) ?? "";
  const shownColor = useLastTruthy(color) ?? 0;

  // Position: straight to the element, never through React.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    board.add(id, el);
    const apply = () => {
      // Leaving: the marker fades where it was last seen.
      const p = useLive.getState().peers[id]?.presence.pointer_3d;
      if (p) board.move(id, p);
    };
    apply();
    const stop = useLive.subscribe((st, prev) => {
      if (st.peers[id] !== prev.peers[id]) apply();
    });
    return () => {
      stop();
      board.remove(id, el);
    };
  }, [board, id]);

  return (
    <div
      ref={ref}
      className={s.marker}
      style={{ "--peer": peerVar(shownColor) } as CSSProperties}
      data-snap="true"
      data-off="true"
      data-hidden="false"
      data-testid={`live-pointer-3d-${id}`}
    >
      <div className={cx(s.body, entering && s.enter, leaving && s.leave)}>
        <span className={s.dot} />
        <span className={s.label}>{shownName}</span>
      </div>
    </div>
  );
}
