// The Chat tab of the side dock: the live session's messages, who is in it,
// and a composer. Without a session it shows the project's past chat, read
// only, with a way to start or join one. Messages are saved with the project
// on the host (DECISIONS D29).
import { useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import type { ChatMessage } from "../contract/bindings";
import { ipc } from "../contract/ipc";
import { useShell } from "../shell/shellStore";
import { useApp } from "../state/store";
import { cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import { motionOK, usePresence } from "../ui/motion";
import { useLastTruthy } from "../ui/motionDom";
import { play } from "../ui/motionWaapi";
import { Avatar } from "./Avatar";
import { CHAT_MAX, avatarTip, canChat, firstName, fullTime, groupMessages, peerVar, timeLabel, type ChatGroup } from "./format";
import { useLive } from "./liveStore";
import { focusParticipant, focusPlanPoint, openJoin, openShare } from "./session";
import s from "./ChatPanel.module.css";

/** Within this many pixels of the bottom, the list follows new messages. */
const FOLLOW_PX = 48;

export function ChatPanel() {
  const mode = useLive((st) => st.status.mode);
  const messages = useLive((st) => st.messages);
  const selfId = useLive((st) => st.status.self_id);
  const loaded = useLive((st) => st.chatLoaded);
  const hasDoc = useApp((st) => st.doc !== null);
  const groups = useMemo(() => groupMessages(messages, selfId), [messages, selfId]);
  const listRef = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);
  const [behind, setBehind] = useState(0);

  // Messages there when the panel first had them do not rise in; later ones do.
  const known = useRef<Set<string> | null>(null);
  if (!loaded) known.current = null;
  else if (known.current === null) known.current = new Set(messages.map((m) => m.id));
  const isNew = (id: string) => known.current !== null && !known.current.has(id);

  // The list follows the newest message while you are at the bottom, or when
  // you sent it. Scrolled up reading, a pill says how many came in below.
  const lastId = messages[messages.length - 1]?.id ?? null;
  const lastMine = messages[messages.length - 1]?.author_id === selfId;
  const first = useRef(true);
  /** Until then the list scrolls itself: its scroll events are not the user leaving the bottom. */
  const following = useRef(0);
  const toBottom = (smooth: boolean) => {
    const el = listRef.current;
    if (!el) return;
    following.current = performance.now() + 700;
    el.scrollTo({ top: el.scrollHeight, behavior: smooth && motionOK() ? "smooth" : "auto" });
    atBottom.current = true;
    setBehind(0);
  };
  useLayoutEffect(() => {
    if (lastId === null) return;
    const jump = first.current;
    first.current = false;
    if (jump || atBottom.current || lastMine) toBottom(!jump);
    else setBehind((n) => n + 1);
  }, [lastId, lastMine]);

  // A hidden panel cannot scroll: coming back on screen, it catches up.
  const visible = useShell((st) => st.dockTab === "chat" && !st.dockCollapsed);
  useLayoutEffect(() => {
    const el = listRef.current;
    if (!visible || !el || !atBottom.current) return;
    el.scrollTop = el.scrollHeight;
    setBehind(0);
  }, [visible]);

  const onScroll = () => {
    const el = listRef.current;
    if (!el || performance.now() < following.current) return;
    atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < FOLLOW_PX;
    if (atBottom.current) setBehind(0);
  };

  const live = canChat(mode) || mode === "reconnecting";
  const behindP = usePresence(behind > 0, "hover");
  const shownBehind = useLastTruthy(behind > 0 ? behind : null) ?? 1;

  return (
    <div className={s.panel}>
      <People />
      <div className={s.listWrap}>
        <div className={s.list} ref={listRef} onScroll={onScroll} role="log" aria-live="polite" aria-label="Chat messages">
          {groups.length === 0 ? <Empty live={live} hasDoc={hasDoc} loaded={loaded} /> : null}
          {groups.map((g) => (
            <Group key={g.key} group={g} isNew={isNew} />
          ))}
        </div>
        {behindP.mounted ? (
          <button
            type="button"
            className={s.behind}
            data-stage={behindP.stage}
            onClick={() => toBottom(true)}
          >
            <Icon name="chevronDown" size={13} />
            {shownBehind === 1 ? "1 new message" : `${shownBehind} new messages`}
          </button>
        ) : null}
      </div>
      {live ? <Composer disabled={mode === "reconnecting"} /> : <OffBar hasDoc={hasDoc} />}
    </div>
  );
}

// ---------------------------------------------------------------- people

function People() {
  const mode = useLive((st) => st.status.mode);
  const participants = useLive((st) => st.status.participants);
  const selfId = useLive((st) => st.status.self_id);
  if (mode === "off") {
    return (
      <header className={s.head}>
        <span className={s.headTitle}>Chat</span>
        <span className={s.headMeta}>Not in a live session</span>
      </header>
    );
  }
  return (
    <header className={cx(s.head, s.headLive)}>
      <div className={s.strip} role="list" aria-label="People in the session">
        {participants.map((p) => (
          <button
            key={p.id}
            type="button"
            role="listitem"
            className={s.person}
            data-tip={p.id === selfId ? "You" : `${avatarTip(p, selfId)}. Show their pointer`}
            data-tip-side="bottom-start"
            disabled={p.id === selfId}
            onClick={() => focusParticipant(p.id)}
          >
            <Avatar p={p} size={18} />
            <span>{p.id === selfId ? "You" : firstName(p.name)}</span>
          </button>
        ))}
      </div>
      <span className={cx(s.headMeta, mode === "reconnecting" && s.headWarn)}>{mode === "reconnecting" ? "Reconnecting" : mode === "hosting" ? "Hosting" : "Joined"}</span>
    </header>
  );
}

// ---------------------------------------------------------------- messages

function Group({ group, isNew }: { group: ChatGroup; isNew: (id: string) => boolean }) {
  const style = { "--peer": peerVar(group.color) } as CSSProperties;
  return (
    <section className={cx(s.group, group.mine && s.groupMine)} style={style} aria-label={`${group.name}, ${timeLabel(group.sentAt)}`}>
      <header className={s.groupHead}>
        <span className={s.dot} aria-hidden />
        <span className={s.name}>{group.mine ? "You" : group.name}</span>
        {group.viaAi ? (
          <span className={s.aiTag} title="Sent by an AI client on their behalf">
            AI
          </span>
        ) : null}
        <time className={s.time} dateTime={group.sentAt} title={fullTime(group.sentAt)}>
          {timeLabel(group.sentAt)}
        </time>
      </header>
      {group.messages.map((m) => (
        <Message key={m.id} message={m} rise={isNew(m.id)} />
      ))}
    </section>
  );
}

function Message({ message, rise }: { message: ChatMessage; rise: boolean }) {
  const at = message.at;
  return (
    <div className={cx(s.message, rise && s.rise)}>
      <p className={s.text}>{message.text}</p>
      {at ? (
        <button
          type="button"
          className={s.showOnPlan}
          data-tip="Sent from cursor chat"
          data-tip-side="top-start"
          onClick={() => focusPlanPoint(at, message.level_id)}
        >
          <Icon name="pointer" size={12} />
          Show on plan
        </button>
      ) : null}
    </div>
  );
}

function Empty({ live, hasDoc, loaded }: { live: boolean; hasDoc: boolean; loaded: boolean }) {
  if (!loaded) return null;
  if (live) {
    return (
      <div className={s.empty}>
        <p className={s.emptyTitle}>No messages yet</p>
        <p className={s.emptyText}>Say hello here, or press / with the pointer on the plan to chat right where you point.</p>
      </div>
    );
  }
  return (
    <div className={s.empty}>
      <p className={s.emptyTitle}>{hasDoc ? "No chat in this project yet" : "No project open"}</p>
      <p className={s.emptyText}>Messages from live sessions are kept with the project, so the history shows here.</p>
    </div>
  );
}

// ---------------------------------------------------------------- composer

function Composer({ disabled }: { disabled: boolean }) {
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const iconRef = useRef<HTMLSpanElement>(null);
  const canSend = !disabled && !sending && draft.trim() !== "";

  const send = async () => {
    const text = draft.trim();
    if (!text || disabled || sending) return;
    // The arrow slides up and out, and comes back from below.
    play(
      iconRef.current,
      [
        { transform: "none", opacity: 1 },
        { transform: "translateY(-9px)", opacity: 0, offset: 0.45 },
        { transform: "translateY(7px)", opacity: 0, offset: 0.55 },
        { transform: "none", opacity: 1 },
      ],
      "base",
      "inOut",
    );
    setSending(true);
    setDraft("");
    try {
      useLive.getState().addMessage(await ipc.chatSend(text));
    } catch (e) {
      setDraft((d) => (d === "" ? text : d));
      useApp.getState().reportError(e);
    } finally {
      setSending(false);
      inputRef.current?.focus();
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    e.stopPropagation();
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void send();
    }
  };

  return (
    <>
      <div className={cx(s.composer, disabled && s.composerOff)}>
        <textarea
          ref={inputRef}
          className={s.input}
          rows={2}
          value={draft}
          maxLength={CHAT_MAX}
          placeholder={disabled ? "Waiting for the host to come back" : "Message everyone in the session"}
          disabled={disabled}
          aria-label="Chat message"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKeyDown}
        />
        <button type="button" className={s.send} disabled={!canSend} onClick={() => void send()} aria-label="Send">
          <span ref={iconRef} className={s.sendIcon}>
            <Icon name="send" size={16} />
          </span>
        </button>
      </div>
      <div className={s.foot}>
        <span>Enter to send, Shift+Enter for a new line</span>
      </div>
    </>
  );
}

function OffBar({ hasDoc }: { hasDoc: boolean }) {
  return (
    <div className={s.off}>
      <p>Chat works in a live session. Start one to invite people on your network, or join one with an invite.</p>
      <div className={s.offActions}>
        <button type="button" className={s.primary} disabled={!hasDoc} onClick={() => openShare()}>
          <Icon name="people" size={15} />
          Start live session
        </button>
        <button type="button" className={s.secondary} onClick={openJoin}>
          <Icon name="join" size={15} />
          Join one
        </button>
      </div>
    </div>
  );
}
