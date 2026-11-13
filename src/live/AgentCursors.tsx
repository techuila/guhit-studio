// Agents on the plan (DECISIONS D47), with or without a live session: each
// agent's cursor (a diamond with the AI mark, not a person's arrow) at its
// work point, with its name and status line, and a faint dashed frame
// around a level it has claimed. Rendered by LiveLayer, which also outlines
// what each agent works on, what it just changed (its trail) and the
// elements it claimed (`useAgentOutlines`).
//
// An agent's point moves once per call, seconds apart, so its cursor eases
// over `--dur-scene` (agents.module.css) instead of the short linear glide
// of a person's. Positions go straight to CSS variables, never through React.
//
// The agent of a swarm part (D56) follows the swarm's reveal instead: its
// diamond goes to each element as the reveal draws it, each move as long as
// the reveal's segment (src/swarm/cursors.ts), and its line says what it draws.

import { useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { useShallow } from "zustand/react/shallow";
import type { DocState } from "../contract/bindings";
import { rectIsEmpty } from "../editor2d/geom";
import { boundsOfIds, buildIndex } from "../editor2d/model";
import { useApp } from "../state/store";
import { cursorIdsWithSwarm, followSwarmCursor, transitionOf } from "../swarm/cursors";
import { useRevealNow, useSwarm } from "../swarm/swarmStore";
import { cx } from "../ui/controls";
import { useLastTruthy } from "../ui/motionDom";
import { useListPresence } from "../ui/useListPresence";
import { AiMark } from "./AgentAvatar";
import { agentById, agentCursorIds, claimLine, statusText, type AgentView } from "./agents";
import { useAgents } from "./agentsStore";
import { peerVar } from "./format";
import type { CursorBoard } from "./LiveLayer";
import s from "./agents.module.css";

export function AgentCursors({ board }: { board: CursorBoard }) {
  const levelId = useApp((st) => st.activeLevelId);
  const presenceIds = useAgents(useShallow((st) => agentCursorIds(st.views, levelId, Date.now())));
  // Swarm parts show where their reveal cursor is, on this level.
  const swarm = useSwarm((st) => st.status);
  const tick = useRevealNow();
  const ids = useMemo(() => cursorIdsWithSwarm(presenceIds, swarm, levelId, Math.max(tick, performance.now())), [presenceIds, swarm, levelId, tick]);
  const rows = useListPresence(ids, (id) => id, "base");
  return (
    <>
      <AgentClaims />
      {rows.map((row) => (
        <AgentCursor key={row.key} id={row.key} board={board} entering={row.entering} leaving={row.leaving} />
      ))}
    </>
  );
}

function AgentCursor({ id, board, entering, leaving }: { id: string; board: CursorBoard; entering: boolean; leaving: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const name = useAgents((st) => agentById(st.views, id)?.name ?? null);
  const color = useAgents((st) => agentById(st.views, id)?.color ?? null);
  const activity = useAgents((st) => agentById(st.views, id)?.activity ?? null);
  const line = useAgents((st) => {
    const a = agentById(st.views, id);
    return a ? statusText(a) : null;
  });
  const shownName = useLastTruthy(name) ?? "";
  const shownColor = useLastTruthy(color) ?? 0;
  // A swarm part's line and work come from the reveal while it draws.
  const [swarmLine, setSwarmLine] = useState<string | null>(null);
  const [swarmMoving, setSwarmMoving] = useState(false);
  const shownLine = useLastTruthy(swarmLine ?? line);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    board.add(id, el);
    let first = true;
    let swarm = false;
    const apply = () => {
      if (swarm) return;
      // Leaving: the cursor fades where it was last seen.
      const p = agentById(useAgents.getState().views, id)?.presence.cursor;
      if (!p) return;
      board.move(id, p);
      if (first) {
        // Placed without a glide, then later moves glide: the style is read before the transition comes on.
        first = false;
        void el.offsetWidth;
        el.dataset.snap = "false";
      }
    };
    apply();
    const stop = useAgents.subscribe((st, prev) => {
      if (st.views !== prev.views) apply();
    });
    const stopSwarm = followSwarmCursor(id, (t) => {
      if (!t) {
        if (!swarm) return;
        swarm = false;
        el.style.transitionDuration = "";
        el.style.transitionTimingFunction = "";
        setSwarmLine(null);
        setSwarmMoving(false);
        apply();
        return;
      }
      swarm = true;
      // The CSS keeps which properties glide; the reveal sets how long and how.
      const tr = transitionOf(t);
      el.style.transitionDuration = first ? "0ms" : tr.duration;
      el.style.transitionTimingFunction = tr.easing;
      board.move(id, t.at);
      if (first) {
        first = false;
        void el.offsetWidth;
        el.dataset.snap = "false";
      }
      setSwarmLine(t.label || null);
      setSwarmMoving(t.ms > 0);
    });
    return () => {
      stop();
      stopSwarm();
      board.remove(id, el);
    };
  }, [board, id]);

  return (
    <div
      ref={ref}
      className={s.cursor}
      style={{ "--peer": peerVar(shownColor) } as CSSProperties}
      data-snap="true"
      data-activity={swarmMoving ? "working" : (activity ?? undefined)}
      data-testid={`agent-cursor-${id}`}
      aria-hidden
    >
      <div className={cx(s.cursorBody, entering && s.cursorEnter, leaving && s.cursorLeave)}>
        <span className={s.glyph}>
          <AiMark size={8} />
        </span>
        <div className={s.label}>
          <span className={s.labelName}>
            {shownName}
            <span className={s.labelTag}>AI</span>
          </span>
          {shownLine ? (
            <span key={shownLine} className={s.labelStatus}>
              {shownLine}
            </span>
          ) : null}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- claims

interface ClaimFrame {
  id: string;
  name: string;
  color: number;
  /** Plan mm: the top left corner, the size. */
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Room around a claimed level's work, mm. */
const CLAIM_PAD_MM = 600;

/** Agents that claimed the level on screen, with the frame around what is on it. */
function claimFrames(views: readonly AgentView[], doc: DocState | null, levelId: string | null): ClaimFrame[] {
  if (!doc || !levelId) return [];
  const claiming = views.filter((a) => a.claim?.level_ids.includes(levelId) && a.activity !== "stopped");
  if (claiming.length === 0) return [];
  const index = buildIndex(doc, levelId);
  // The building on that level: what an agent claims to work on, not the notes, cameras or underlays around it.
  const r = boundsOfIds(
    index,
    index.visible.filter((e) => e.kind === "wall" || e.kind === "room" || e.kind === "slab" || e.kind === "column" || e.kind === "stair").map((e) => e.id),
  );
  if (rectIsEmpty(r)) return [];
  // Several agents on one level: each frame sits a little further out, so all show.
  return claiming.map((a, i) => {
    const pad = CLAIM_PAD_MM + i * 250;
    return { id: a.id, name: a.name, color: a.color, x: r.minX - pad, y: r.maxY + pad, w: r.maxX - r.minX + 2 * pad, h: r.maxY - r.minY + 2 * pad };
  });
}

function sameFrames(a: readonly ClaimFrame[], b: readonly ClaimFrame[]): boolean {
  return a.length === b.length && a.every((x, i) => x.id === b[i].id && x.x === b[i].x && x.y === b[i].y && x.w === b[i].w && x.h === b[i].h && x.color === b[i].color && x.name === b[i].name);
}

function AgentClaims() {
  const levelId = useApp((st) => st.activeLevelId);
  const doc = useApp((st) => st.doc);
  // Only who claims this level, so a moving agent does not rebuild the frames.
  const claimers = useAgents(useShallow((st) => st.views.filter((a) => levelId && a.claim?.level_ids.includes(levelId) && a.activity !== "stopped").map((a) => `${a.id}|${a.name}|${a.color}`)));
  const lastFrames = useRef<ClaimFrame[]>([]);
  // `claimers` stands for the agents' claims: the frames are read from the store when it changes.
  const frames = useMemo(() => {
    const next = claimers.length > 0 ? claimFrames(useAgents.getState().views, doc, levelId) : [];
    if (sameFrames(next, lastFrames.current)) return lastFrames.current;
    lastFrames.current = next;
    return next;
  }, [claimers, doc, levelId]);
  const rows = useListPresence(frames, (f) => f.id, "base");
  return (
    <>
      {rows.map((row) => (
        <div
          key={row.key}
          className={cx(s.claim, row.leaving && s.claimLeave)}
          style={{ "--peer": peerVar(row.item.color), "--cx": row.item.x, "--cy": row.item.y, "--cw": row.item.w, "--ch": row.item.h } as CSSProperties}
          data-testid={`agent-claim-${row.key}`}
          aria-hidden
        >
          <span className={s.claimLabel}>{claimLine(row.item.name)}</span>
        </div>
      ))}
    </>
  );
}
