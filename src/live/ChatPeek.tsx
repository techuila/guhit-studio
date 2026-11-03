// The card that slides in when a chat message arrives while Chat is not on
// screen (or the window is not focused): sender, the start of the message,
// Reply (DECISIONS D41). Mounted once by LiveRoot. At most two cards, one per
// sender; more messages from a sender update their card ("Ana, 3 new"). A
// card leaves by itself after a few seconds, waits while it is hovered or
// holds focus or the window is not in front, and closes with its button or
// Escape. Enter and exit: docs/MOTION.md, "Chat card".
import { useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import { useListPresence } from "../ui/useListPresence";
import { flip } from "../ui/motionWaapi";
import { Avatar, peerStyle } from "./Avatar";
import { PEEK_MS, dismissLabel, newLabel, peekHeading, type PeekCard } from "./chatRules";
import { dismissPeek, replyFromPeek, useNotify } from "./chatNotify";
import s from "./ChatPeek.module.css";

/**
 * Calls `onDone` once the card has been left alone for `ms`. The clock stops
 * while `running` is false and picks up where it was; a new `resetKey` (the
 * card was updated) starts it over.
 */
function useDwell(running: boolean, ms: number, resetKey: number, onDone: () => void): void {
  const left = useRef(ms);
  const key = useRef(resetKey);
  const done = useRef(onDone);
  done.current = onDone;
  useLayoutEffect(() => {
    if (key.current !== resetKey) {
      key.current = resetKey;
      left.current = ms;
    }
    if (!running) return;
    const t0 = performance.now();
    const id = window.setTimeout(() => done.current(), left.current);
    return () => {
      window.clearTimeout(id);
      left.current = Math.max(0, left.current - (performance.now() - t0));
    };
  }, [running, resetKey, ms]);
}

function Card({ card, entering, leaving }: { card: PeekCard; entering: boolean; leaving: boolean }) {
  const windowFocused = useNotify((st) => st.focused);
  const [held, setHeld] = useState(false);
  useDwell(!leaving && !held && windowFocused, PEEK_MS, card.stamp, () => dismissPeek(card.authorId));
  const pill = newLabel(card.count);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    dismissPeek(card.authorId);
  };

  return (
    <div
      className={cx(s.slot, entering && s.enter, leaving && s.leave)}
      style={peerStyle(card.color) as CSSProperties}
      data-peek={card.authorId}
      aria-hidden={leaving || undefined}
      onPointerEnter={() => setHeld(true)}
      onPointerLeave={() => setHeld(false)}
      onFocus={() => setHeld(true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setHeld(false);
      }}
      onKeyDown={onKeyDown}
    >
      <section className={s.card} aria-label={`New message from ${peekHeading(card)}`}>
        <Avatar p={card} size={28} />
        <div className={s.body}>
          <div className={s.head}>
            <span className={s.name}>{card.name}</span>
            {card.viaAi ? (
              <span className={s.aiTag} title="Sent by an AI client on their behalf">
                AI
              </span>
            ) : null}
            {pill ? (
              // Keyed by the count, so each new message pops it once.
              <span key={card.count} className={s.count}>
                {pill}
              </span>
            ) : null}
          </div>
          {/* Keyed by the message, so a newer one fades in. */}
          <p key={card.lastId} className={s.text}>
            {card.text}
          </p>
          <button type="button" className={s.reply} onClick={() => replyFromPeek(card.authorId)}>
            <Icon name="undo" size={12} />
            Reply
          </button>
        </div>
        <button type="button" className={s.close} aria-label={dismissLabel(card)} data-tip="Dismiss" data-tip-side="bottom-end" onClick={() => dismissPeek(card.authorId)}>
          <Icon name="close" size={13} />
        </button>
      </section>
    </div>
  );
}

export function ChatPeek() {
  const cards = useNotify((st) => st.cards);
  const rows = useListPresence(cards, (c) => c.authorId, "base");
  const stack = useRef<HTMLDivElement>(null);
  const rects = useRef(new Map<string, DOMRect>());

  // When a card leaves, the one below it slides up instead of jumping.
  useLayoutEffect(() => {
    const els = stack.current ? Array.from(stack.current.querySelectorAll<HTMLElement>("[data-peek]")) : [];
    rects.current = flip(els, rects.current, (el) => el.dataset.peek);
  });

  return (
    <div ref={stack} className={s.stack} role="status" aria-live="polite" aria-atomic="false" aria-relevant="additions text">
      {rows.map((row) => (
        <Card key={row.key} card={row.item} entering={row.entering} leaving={row.leaving} />
      ))}
    </div>
  );
}
