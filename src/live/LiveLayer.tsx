// The live session over the plan (DECISIONS D29): everyone else's cursor in
// their color with their name, what they type in cursor chat, what they
// selected, and this window's own cursor chat bubble. Mounted by PlanCanvas.
//
// Cost rules. A remote pointer moving never redraws the canvas and never
// re-renders React: its position goes straight to two CSS variables
// (`--live-x`, `--live-y`, plan mm) that glide with a short linear
// transition. The view transform lives in three more on the layer (`--ox`,
// `--oy`, `--s`), written when the plan tells us the view changed (from its
// own draw pass), so a pan or zoom moves every cursor at once with no
// transition. Nothing here runs a frame loop. Only a changed selection
// redraws the canvas.

import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { useShallow } from "zustand/react/shallow";
import type { Point } from "../contract/bindings";
import { ipc } from "../contract/ipc";
import type { PlanController } from "../editor2d/controller";
import type { View } from "../editor2d/view";
import { useApp } from "../state/store";
import { isModalOpen } from "../ui/Dialog";
import { cx } from "../ui/controls";
import { usePresence } from "../ui/motion";
import { useLastTruthy } from "../ui/motionDom";
import { play } from "../ui/motionWaapi";
import { useListPresence } from "../ui/useListPresence";
import { useViewer } from "../viewer3d/viewerStore";
import { CURSOR_CHAT_MAX, canChat, cursorIds, participant, peerIndex, peerSelections, peerVar } from "./format";
import { SENT_MS, useLive } from "./liveStore";
import { bubbleAt, labelFlip, planToCanvas } from "./placement";
import s from "./LiveLayer.module.css";

/** A cursor untouched this long dims. */
export const IDLE_MS = 20_000;

/** How long "/" outside a session shows its hint. */
const HINT_MS = 2600;

/** After typing stops without a message, the text lingers this long: the message may still be on its way. */
const TYPING_GRACE_MS = 600;

let glideRegistered = false;

/**
 * Registers the cursor position variables as numbers, so they can glide
 * with a CSS transition. Where that is not supported, cursors step at the
 * presence rate instead: still right, just less smooth.
 */
function registerGlide(): void {
  if (glideRegistered) return;
  glideRegistered = true;
  if (typeof CSS === "undefined" || typeof CSS.registerProperty !== "function") return;
  for (const name of ["--live-x", "--live-y"]) {
    try {
      CSS.registerProperty({ name, syntax: "<number>", inherits: false, initialValue: "0" });
    } catch {
      // Already registered (a hot reload).
    }
  }
}

/** Where every cursor is, so a view change can flip labels at the canvas edge. */
class CursorBoard {
  view: View | null = null;
  width = 0;
  height = 0;
  private items = new Map<string, { el: HTMLElement; p: Point | null; fx: boolean; fy: boolean }>();

  constructor(private readonly layer: HTMLElement) {}

  setView(view: View, width: number, height: number): void {
    this.view = view;
    this.width = width;
    this.height = height;
    const st = this.layer.style;
    st.setProperty("--ox", String(view.ox));
    st.setProperty("--oy", String(view.oy));
    st.setProperty("--s", String(view.scale));
    for (const item of this.items.values()) this.flip(item);
  }

  add(id: string, el: HTMLElement): void {
    this.items.set(id, { el, p: null, fx: false, fy: false });
  }

  remove(id: string, el: HTMLElement): void {
    if (this.items.get(id)?.el === el) this.items.delete(id);
  }

  move(id: string, p: Point): void {
    const item = this.items.get(id);
    if (!item) return;
    item.p = p;
    item.el.style.setProperty("--live-x", String(p.x));
    item.el.style.setProperty("--live-y", String(p.y));
    this.flip(item);
  }

  private flip(item: { el: HTMLElement; p: Point | null; fx: boolean; fy: boolean }): void {
    if (!this.view || !item.p) return;
    const f = labelFlip(planToCanvas(this.view, item.p), this.width, this.height);
    if (f.x !== item.fx) item.el.dataset.flipX = String((item.fx = f.x));
    if (f.y !== item.fy) item.el.dataset.flipY = String((item.fy = f.y));
  }
}

export function LiveLayer({ controller, rootRef }: { controller: PlanController; rootRef: RefObject<HTMLDivElement | null> }) {
  const layerRef = useRef<HTMLDivElement>(null);
  const [board, setBoard] = useState<CursorBoard | null>(null);

  useLayoutEffect(() => {
    const layer = layerRef.current;
    if (!layer) return;
    registerGlide();
    const b = new CursorBoard(layer);
    setBoard(b);
    const stop = controller.onViewChange((view, width, height) => b.setView(view, width, height));
    return () => {
      stop();
      setBoard(null);
    };
  }, [controller]);

  usePeerSelections(controller, rootRef);
  useSlashKey(controller);
  // Reconnecting: the cursors on screen are where people were, not where they are.
  const stale = useLive((st) => st.status.mode === "reconnecting");

  return (
    <div ref={layerRef} className={s.layer} data-stale={stale || undefined}>
      {board ? <RemoteCursors board={board} /> : null}
      {board ? <CursorChat controller={controller} board={board} rootRef={rootRef} /> : null}
    </div>
  );
}

// ---------------------------------------------------------------- selections

/** Pushes other people's selections to the plan, in their resolved colors. */
function usePeerSelections(controller: PlanController, rootRef: RefObject<HTMLDivElement | null>): void {
  useEffect(() => {
    const colors = new Map<number, string>();
    const colorOf = (index: number): string => {
      const i = peerIndex(index);
      let c = colors.get(i);
      if (!c) {
        const el = rootRef.current ?? document.documentElement;
        c = getComputedStyle(el).getPropertyValue(`--peer-${i}`).trim() || "#495057";
        colors.set(i, c);
      }
      return c;
    };
    const push = () => {
      const st = useLive.getState();
      controller.setPeerSelections(peerSelections(st.status, st.peers).map((p) => ({ id: p.id, color: colorOf(p.color), ids: p.ids })));
    };
    push();
    // The plan compares, so a presence event that only moved a pointer costs nothing.
    return useLive.subscribe((st, prev) => {
      if (st.peers !== prev.peers || st.status !== prev.status) push();
    });
  }, [controller, rootRef]);
}

// ---------------------------------------------------------------- "/"

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || !el.tagName) return false;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable;
}

/**
 * "/" with the pointer on the plan opens cursor chat. Not while typing in a
 * field, not in the middle of a canvas operation (the plan swallows keys
 * then anyway), not while walking or flying in 3D, not under a dialog.
 * Outside a live session it shows a short hint instead.
 */
function useSlashKey(controller: PlanController): void {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "/" || e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey || e.repeat) return;
      if (isTyping(e.target) || isModalOpen()) return;
      if (useViewer.getState().nav !== "orbit") return;
      if (!controller.pointerInside || !controller.isIdle() || !controller.cursorScreen) return;
      e.preventDefault();
      const live = useLive.getState();
      if (canChat(live.status.mode)) live.openCursorChat();
      else live.showHint();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [controller]);
}

// ---------------------------------------------------------------- remote cursors

function RemoteCursors({ board }: { board: CursorBoard }) {
  const levelId = useApp((st) => st.activeLevelId);
  const ids = useLive(useShallow((st) => cursorIds(st.status, st.peers, levelId)));
  const rows = useListPresence(ids, (id) => id, "base");
  return (
    <>
      {rows.map((row) => (
        <RemoteCursor key={row.key} id={row.key} board={board} entering={row.entering} leaving={row.leaving} />
      ))}
    </>
  );
}

function RemoteCursor({ id, board, entering, leaving }: { id: string; board: CursorBoard; entering: boolean; leaving: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const name = useLive((st) => participant(st.status, id)?.name ?? null);
  const color = useLive((st) => participant(st.status, id)?.color ?? null);
  const typing = useLive((st) => st.peers[id]?.presence.typing ?? null);
  const said = useLive((st) => st.said[id]?.text ?? null);
  const shownName = useLastTruthy(name) ?? "";
  const shownColor = useLastTruthy(color) ?? 0;
  const grace = useTypingGrace(typing);
  const text = typing ?? said ?? grace;

  // Position and idleness: straight to the element, never through React.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    board.add(id, el);
    let timer = 0;
    const idleAfter = (ms: number) => {
      window.clearTimeout(timer);
      el.dataset.idle = ms <= 0 ? "true" : "false";
      if (ms > 0) timer = window.setTimeout(() => (el.dataset.idle = "true"), ms);
    };
    const apply = (first: boolean) => {
      const peer = useLive.getState().peers[id];
      if (!peer) return;
      // Leaving: the cursor fades where it was last seen.
      if (peer.presence.cursor) board.move(id, peer.presence.cursor);
      idleAfter(first ? IDLE_MS - (Date.now() - peer.changedAt) : IDLE_MS);
    };
    apply(true);
    const stop = useLive.subscribe((st, prev) => {
      if (st.peers[id] !== prev.peers[id]) apply(false);
    });
    return () => {
      stop();
      window.clearTimeout(timer);
      board.remove(id, el);
    };
  }, [board, id]);

  // The name grows into a bubble when there is something to read.
  const labelRef = useRef<HTMLDivElement>(null);
  const bubble = text !== null;
  const wasBubble = useRef(bubble);
  useEffect(() => {
    if (bubble && !wasBubble.current) {
      play(labelRef.current, [{ transform: "scale(0.9)", opacity: 0.6 }, { transform: "none", opacity: 1 }], "base", "out");
    }
    wasBubble.current = bubble;
  }, [bubble]);

  return (
    <div ref={ref} className={s.cursor} style={{ "--peer": peerVar(shownColor) } as CSSProperties} data-idle="false" aria-hidden>
      <div className={cx(s.cursorBody, entering && s.enter, leaving && s.leave)}>
        <svg className={s.arrow} width="18" height="20" viewBox="0 0 18 20" aria-hidden>
          <path d="M2 1.8v14.1l3.9-3.6 2.6 5.8 2.7-1.2-2.6-5.7 5.3-.3z" />
        </svg>
        <div ref={labelRef} className={cx(s.label, bubble && s.labelBubble)}>
          <span className={s.labelName}>{shownName}</span>
          {bubble ? <span className={cx(s.labelText, text === "" && s.labelTyping)}>{text === "" ? "typing" : text}</span> : null}
        </div>
      </div>
    </div>
  );
}

/** Keeps the last typed text a moment after typing stops, so a message on its way does not flicker the bubble. */
function useTypingGrace(typing: string | null): string | null {
  const [grace, setGrace] = useState<string | null>(null);
  const prev = useRef(typing);
  useEffect(() => {
    const before = prev.current;
    prev.current = typing;
    if (typing !== null) {
      setGrace(null);
      return;
    }
    if (!before) return;
    setGrace(before);
    const t = window.setTimeout(() => setGrace(null), TYPING_GRACE_MS);
    return () => window.clearTimeout(t);
  }, [typing]);
  return grace;
}

// ---------------------------------------------------------------- cursor chat

/**
 * This window's cursor chat, FigJam style: a bubble at the pointer in your
 * color. It tracks the pointer 1:1. Enter sends, Escape or an empty Enter
 * closes, clicking elsewhere closes. What you type shows to the others as
 * you type (presence `typing`). A sent message stays a few seconds.
 */
function CursorChat({ controller, board, rootRef }: { controller: PlanController; board: CursorBoard; rootRef: RefObject<HTMLDivElement | null> }) {
  const open = useLive((st) => st.chat.open);
  const draft = useLive((st) => st.chat.draft);
  const sent = useLive((st) => st.chat.sent);
  const hintSeq = useLive((st) => st.chat.hint);
  const color = useLive((st) => participant(st.status, st.status.self_id)?.color ?? 0);
  const [hint, setHint] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const size = useRef({ w: 200, h: 36 });

  // What the bubble holds: the text field, your sent message, or the hint.
  // While it fades out it keeps showing what it showed last.
  const now = open ? "input" : sent ? "sent" : hint ? "hint" : null;
  const last = useRef<{ mode: "input" | "sent" | "hint"; text: string }>({ mode: "hint", text: "" });
  if (now) last.current = { mode: now, text: now === "input" ? draft : now === "sent" ? (sent?.text ?? "") : "" };
  const p = usePresence(now !== null, "base");

  // "/" outside a session: the hint shows a moment. Only a new press, not a remount.
  const seenHint = useRef(hintSeq);
  useEffect(() => {
    if (hintSeq === seenHint.current) return;
    seenHint.current = hintSeq;
    setHint(true);
    const t = window.setTimeout(() => setHint(false), HINT_MS);
    return () => window.clearTimeout(t);
  }, [hintSeq]);

  // A sent message stays a few seconds, then fades.
  useEffect(() => {
    if (!sent) return;
    const t = window.setTimeout(() => useLive.getState().clearSent(sent.token), SENT_MS);
    return () => window.clearTimeout(t);
  }, [sent]);

  // Follows the pointer 1:1: written straight to the element on each move.
  const place = useRef(() => {});
  place.current = () => {
    const el = wrapRef.current;
    const at = controller.cursorScreen;
    if (!el || !at) return;
    const root = rootRef.current;
    const pos = bubbleAt(at, size.current, board.width || root?.clientWidth || 0, board.height || root?.clientHeight || 0);
    el.style.transform = `translate(${Math.round(pos.x)}px, ${Math.round(pos.y)}px)`;
  };
  const shown = p.mounted || now !== null;
  useLayoutEffect(() => {
    if (!shown) return;
    const root = rootRef.current;
    const move = () => place.current();
    move();
    root?.addEventListener("pointermove", move);
    return () => root?.removeEventListener("pointermove", move);
  }, [shown, rootRef]);

  // The text changes the bubble's size: measure it and keep it on the canvas.
  useLayoutEffect(() => {
    const el = wrapRef.current?.firstElementChild as HTMLElement | null;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    if (w === size.current.w && h === size.current.h) return;
    size.current = { w, h };
    place.current();
  });

  // The field renders in the same commit that opens the bubble, so it has the
  // focus before the next key arrives (the first letter is never lost); the
  // presence hook catches up one effect later and plays the enter.
  if (!p.mounted && now === null) return null;
  const stage = p.mounted ? p.stage : "enter";
  const { mode, text } = last.current;
  return (
    <div ref={wrapRef} className={s.chatWrap}>
      <div className={cx(s.chatBubble, mode === "hint" && s.chatHint)} data-stage={stage} style={{ "--peer": peerVar(color) } as CSSProperties}>
        {mode === "input" && open ? (
          <ChatInput controller={controller} />
        ) : mode === "hint" ? (
          <span className={s.chatHintText}>Cursor chat works in a live session. Start one with Share.</span>
        ) : (
          // A sent message, or a closed field fading out as a plain copy.
          <span className={cx(s.chatSent, text === "" && s.chatEmpty)}>{text || "Say something"}</span>
        )}
      </div>
    </div>
  );
}

function ChatInput({ controller }: { controller: PlanController }) {
  const draft = useLive((st) => st.chat.draft);
  const ref = useRef<HTMLTextAreaElement>(null);
  const sending = useRef(false);

  useLayoutEffect(() => {
    ref.current?.focus({ preventScroll: true });
  }, []);

  const send = () => {
    const live = useLive.getState();
    const text = live.chat.draft.trim();
    if (text === "") {
      live.closeCursorChat();
      return;
    }
    const app = useApp.getState();
    const at = app.cursor ?? (controller.cursorWorld ? { x: controller.cursorWorld.x, y: controller.cursorWorld.y } : null);
    sending.current = true;
    const token = live.markSent(text);
    void ipc
      .chatSend(text, at, app.activeLevelId)
      .then((message) => useLive.getState().addMessage(message))
      .catch((e) => {
        useLive.getState().clearSent(token);
        useApp.getState().reportError(e);
      });
  };

  const remaining = CURSOR_CHAT_MAX - [...draft].length;
  return (
    <span className={s.chatField} data-value={draft}>
      <textarea
        ref={ref}
        className={s.chatInput}
        rows={1}
        value={draft}
        maxLength={CURSOR_CHAT_MAX}
        placeholder="Say something"
        aria-label="Cursor chat message"
        spellCheck
        onChange={(e) => useLive.getState().setDraft(e.target.value.replace(/\n/g, " "))}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter" && !e.nativeEvent.isComposing) {
            e.preventDefault();
            send();
          } else if (e.key === "Escape") {
            e.preventDefault();
            useLive.getState().closeCursorChat();
          }
        }}
        onBlur={() => {
          if (!sending.current) useLive.getState().closeCursorChat();
        }}
      />
      {remaining <= 20 ? <span className={s.chatCount}>{remaining}</span> : null}
    </span>
  );
}
