// Copilot dock. A calm chat panel: the user asks, the copilot answers from
// the plan or proposes typed edits. A proposal shows as a ghost in the 2D
// and 3D views (store.preview) and commits only when the user applies it
// (D8). The Ask | Swarm switch shows the swarm panel instead (src/swarm,
// D56); the conversation keeps its state while the swarm shows.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import type { AiProposal } from "../contract/bindings";
import { ipc, toIpcError } from "../contract/ipc";
import { MOD, SHIFT } from "../shell/actions";
import { ConnectLink } from "../shell/connect/ConnectButton";
import { useShell } from "../shell/shellStore";
import { useApp } from "../state/store";
import { Segmented, Switch, cx } from "../ui/controls";
import { SwarmPanel } from "../swarm/SwarmPanel";
import { swarmLocked, useSwarm, useSwarmLocked, type DockMode } from "../swarm/swarmStore";
import { startSwarmWiring } from "../swarm/wiring";
import { LOCK_HINT, workersOf } from "../swarm/view";
import { Icon } from "../ui/icons";
import { dur, ease, motionOK, tween, usePresence } from "../ui/motion";
import { collapseOut, growIn, play, settled } from "../ui/motionWaapi";
import { STALE_TEXT, applyProposal } from "./apply";
import { composerGate, logFollower, pendingItem, proposalKey, scrollToEnd, toHistory, useCopilot, type ChatItem } from "./copilotStore";
import { affectedRows, modelLabel, selectionLabel, suggestionsFor, toolLabel, type Suggestion } from "./describe";
import { requestFurnish, retireFurnish, roomsToFurnish, turnRooms } from "./furnish";
import { FurnishBar, FurnishCards } from "./FurnishPanel";
import { SettingsPopover } from "./SettingsPopover";
import { DrawnCheck, GearIcon, KindIcon, SendIcon, StopIcon } from "./icons";
import s from "./AiDock.module.css";

const MAC = MOD === "⌘";

/** Drop a proposal on the backend. Failing here is harmless: a new chat turn replaces it anyway. */
function dropProposal(id: string) {
  void ipc.aiResolve(id, false).catch(() => undefined);
}

export function AiDock() {
  const doc = useApp((st) => st.doc);
  const selection = useApp((st) => st.selection);
  const aiScope = useApp((st) => st.aiScope);
  const setAiScope = useApp((st) => st.setAiScope);
  const items = useCopilot((st) => st.items);
  const busy = useCopilot((st) => st.busy);
  const settings = useCopilot((st) => st.settings);
  const settingsError = useCopilot((st) => st.settingsError);
  const mode = useSwarm((st) => st.mode);
  const swarmParts = useSwarm((st) => workersOf(st.status).length);
  // A swarm draft locks the model (D57): the engine refuses chat and furnish then.
  const locked = useSwarmLocked();

  const [draft, setDraft] = useState("");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const projectId = doc?.project.id ?? null;
  const revision = doc?.revision ?? null;
  const pending = pendingItem(items);

  const loadSettings = useCallback(async () => {
    try {
      useCopilot.getState().setSettings(await ipc.aiSettingsGet());
    } catch (e) {
      useCopilot.getState().setSettings(null, toIpcError(e).message);
    }
  }, []);

  useEffect(() => {
    void loadSettings();
  }, [loadSettings]);

  // The swarm's status and draft follow the backend while the editor is open.
  useEffect(() => startSwarmWiring(), []);

  // A different project (or none) starts a clean conversation and never
  // inherits a ghost preview.
  useEffect(() => {
    const st = useCopilot.getState();
    if (st.projectId === projectId) return;
    const open = pendingItem(st.items);
    if (open?.proposal) dropProposal(open.proposal.id);
    retireFurnish("The project changed.");
    useApp.getState().setPreview(null);
    st.reset(projectId);
  }, [projectId]);

  // The plan moved under a pending proposal (user edit, undo, redo): the
  // preview no longer describes what Apply would do. Retire it right away.
  useEffect(() => {
    if (!pending?.proposal || pending.proposalStatus !== "pending" || revision === null) return;
    if (pending.proposal.base_revision === revision) return;
    useApp.getState().setPreview(null);
    useApp.getState().setHover(null);
    useCopilot.getState().patch(pending.id, { proposalStatus: "stale", proposalNote: STALE_TEXT });
    dropProposal(pending.proposal.id);
  }, [pending, revision]);

  // Never leave a ghost behind when the dock goes away.
  useEffect(() => {
    return () => {
      const st = useCopilot.getState();
      if (st.items.some((m) => m.furnish?.some((r) => r.status === "pending"))) {
        retireFurnish("Discarded when the copilot closed.");
        useApp.getState().setPreview(null);
      }
      const open = pendingItem(st.items);
      if (!open?.proposal) return;
      useApp.getState().setPreview(null);
      useApp.getState().setHover(null);
      st.patch(open.id, { proposalStatus: "discarded", proposalNote: "Discarded when the copilot closed." });
      dropProposal(open.proposal.id);
    };
  }, []);

  const retirePending = useCallback((note: string) => {
    const st = useCopilot.getState();
    const open = pendingItem(st.items);
    if (!open?.proposal) return;
    useApp.getState().setPreview(null);
    useApp.getState().setHover(null);
    st.patch(open.id, { proposalStatus: "discarded", proposalNote: note });
  }, []);

  const send = useCallback(
    async (text: string) => {
      const message = text.trim();
      const copilot = useCopilot.getState();
      if (!message || copilot.busy || swarmLocked()) return;
      const app = useApp.getState();
      if (!app.doc) return;

      // A new turn replaces the old proposal. The backend drops it too.
      retirePending("Replaced by a newer request.");
      retireFurnish("Replaced by a newer request.");
      const history = toHistory(useCopilot.getState().items);
      copilot.push({ role: "user", text: message });
      copilot.setBusy(true);
      setDraft("");
      const request = copilot.nextRequest();
      const isCurrent = () => useCopilot.getState().requestSeq === request;

      try {
        // "Only the selection" limits every edit of this turn to it (DECISIONS D30).
        const scope = app.aiScope && app.selection.length > 0 ? { ids: app.selection } : null;
        const turn = await ipc.aiChat({ message, selection_ids: app.selection, active_level_id: app.activeLevelId, history, scope });
        if (!isCurrent()) {
          // The user stopped waiting. Do not show a late ghost.
          if (turn.proposal) dropProposal(turn.proposal.id);
          return;
        }
        const current = useApp.getState().doc;
        const fresh = !!turn.proposal && current?.revision === turn.proposal.base_revision;
        useCopilot.getState().push({
          role: "assistant",
          text: turn.reply,
          toolsUsed: turn.tools_used,
          proposal: turn.proposal,
          proposalStatus: turn.proposal ? (fresh ? "pending" : "stale") : null,
          proposalNote: turn.proposal && !fresh ? STALE_TEXT : null,
        });
        if (turn.proposal) {
          if (fresh) {
            useApp.getState().setPreview(turn.proposal.preview);
            // The user must never have to hunt for the approve button: a
            // collapsed dock opens the moment a proposal is ready to review.
            useShell.getState().setDockCollapsed(false);
          } else dropProposal(turn.proposal.id);
        }
      } catch (e) {
        if (!isCurrent()) return;
        const err = toIpcError(e);
        if (err.code === "ai_not_configured") void loadSettings();
        useCopilot.getState().push({ role: "note", tone: "error", text: err.message });
      } finally {
        if (isCurrent()) useCopilot.getState().setBusy(false);
      }
    },
    [loadSettings, retirePending],
  );

  // "Furnish rooms" (lane L5): one proposal per room, each applied on its own (D8).
  const furnish = useCallback(
    async (style: string) => {
      const copilot = useCopilot.getState();
      if (copilot.busy || swarmLocked()) return;
      const app = useApp.getState();
      const rooms = roomsToFurnish(app.doc, app.selection, app.activeLevelId);
      if (rooms.length === 0) return;
      retirePending("Replaced by a newer request.");
      retireFurnish("Replaced by a newer request.");
      app.setPreview(null);
      copilot.push({ role: "user", text: `Furnish ${rooms.length === 1 ? rooms[0].name : `${rooms.length} rooms`}, ${style} style` });
      copilot.setBusy(true);
      const request = copilot.nextRequest();
      const isCurrent = () => useCopilot.getState().requestSeq === request;
      try {
        const turn = await requestFurnish(rooms, style);
        if (!isCurrent()) {
          for (const p of turn.proposals ?? []) dropProposal(p.id);
          return;
        }
        const cards = turnRooms(turn, rooms);
        useCopilot.getState().push({ role: "assistant", text: turn.reply, toolsUsed: turn.tools_used, furnish: cards, furnishStyle: style });
        if (cards[0]) {
          useApp.getState().setPreview(cards[0].proposal.preview);
          useShell.getState().setDockCollapsed(false);
        }
      } catch (e) {
        if (!isCurrent()) return;
        const err = toIpcError(e);
        if (err.code === "ai_not_configured") void loadSettings();
        useCopilot.getState().push({ role: "note", tone: "error", text: err.message });
      } finally {
        if (isCurrent()) useCopilot.getState().setBusy(false);
      }
    },
    [loadSettings, retirePending],
  );

  const stop = useCallback(() => {
    const copilot = useCopilot.getState();
    copilot.nextRequest();
    copilot.setBusy(false);
    copilot.push({ role: "note", text: "Stopped. Nothing was changed." });
    inputRef.current?.focus();
  }, []);

  const apply = useCallback((item: ChatItem, proposal: AiProposal) => void applyProposal(item.id, proposal), []);

  const discard = useCallback((item: ChatItem, proposal: AiProposal) => {
    const app = useApp.getState();
    app.setPreview(null);
    app.setHover(null);
    useCopilot.getState().patch(item.id, { proposalStatus: "discarded", proposalNote: "Discarded. The plan was not changed." });
    dropProposal(proposal.id);
  }, []);

  // Mod+Enter applies and Esc discards the pending proposal, from the empty
  // composer or from the card (U19). Only Apply still commits (D8).
  const onProposalKey = (e: KeyboardEvent<HTMLElement>, text: string): boolean => {
    if (e.nativeEvent.isComposing) return false;
    const open = pendingItem(useCopilot.getState().items);
    const what = proposalKey(e, { pending: open?.proposalStatus === "pending" && !!open.proposal, draft: text, mac: MAC });
    if (!what || !open?.proposal) return false;
    e.preventDefault();
    e.stopPropagation();
    if (what === "apply") apply(open, open.proposal);
    else discard(open, open.proposal);
    return true;
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (onProposalKey(e, draft)) return;
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      if (!locked) submitRef.current(draft);
    }
  };

  const noKey = settings !== null && !settings.has_api_key;
  const suggestions = suggestionsFor(doc, selection);
  const gate = composerGate({ hasDoc: !!doc, busy, noKey, locked, text: draft });
  const canSend = gate.canSend;
  const scoped = aiScope && selection.length > 0;

  // "Only the selection" (DECISIONS D30) shows while something is selected,
  // and keeps its last words while it leaves.
  const scopeRow = usePresence(selection.length > 0 && !noKey, "hover");
  const scopeText = useRef("");
  if (selection.length > 0) scopeText.current = selectionLabel(doc, selection);

  // Exits animate: the popover, the busy line and the chip row all stay
  // mounted long enough to leave.
  const settings0 = usePresence(settingsOpen, "base");
  const working = usePresence(busy, "hover");

  // The list follows its newest line instead of jumping to it: a new or
  // changed message, and the busy line, which mounts a render after `busy`
  // turns on (keyed on `busy` alone, it stayed below the fold while cards
  // waited for Apply).
  const [follows] = useState(logFollower);
  useLayoutEffect(() => {
    if (follows(items, working.mounted)) scrollToEnd(listRef.current, motionOK());
  });

  // The chips give way to a proposal or Furnish cards that wait for Apply, so the card has the room.
  const furnishWaiting = items.some((m) => m.furnish?.some((r) => r.status === "pending" || r.status === "applying"));
  const chipsOpen = !noKey && items.length > 0 && !busy && !pending && !furnishWaiting && draft === "";
  const chipRow = usePresence(chipsOpen, "hover");

  // The composer is one line until it is used.
  const [composing, setComposing] = useState(false);
  const model = modelLabel(settings?.model);
  const headMeta =
    mode === "swarm"
      ? swarmParts > 0
        ? `${swarmParts} ${swarmParts === 1 ? "agent" : "agents"}`
        : null
      : selection.length === 0
        ? null
        : scoped
          ? `Limited to ${selection.length} selected`
          : `${selection.length} selected`;

  // The arrow slides out of the send button when a message goes.
  const sendIconRef = useRef<HTMLSpanElement>(null);
  const submit = useCallback(
    (text: string) => {
      play(
        sendIconRef.current,
        [
          { transform: "none", opacity: 1 },
          { transform: "translateY(-9px)", opacity: 0, offset: 0.45 },
          { transform: "translateY(7px)", opacity: 0, offset: 0.55 },
          { transform: "none", opacity: 1 },
        ],
        "base",
        "inOut",
      );
      void send(text);
    },
    [send],
  );
  // Enter sends too, and it gets the same arrow slide.
  const submitRef = useRef(submit);
  submitRef.current = submit;

  return (
    <div className={s.dock}>
      {/* The Copilot tab already says this is the AI: no tag here, and the AI model's name lives in the gear's tip. */}
      <header className={s.head}>
        <Segmented<DockMode>
          label="Ask the copilot or run a swarm"
          value={mode}
          onChange={(m) => useSwarm.getState().setMode(m)}
          options={[
            { value: "chat", label: "Ask", tip: "Ask the copilot. Edits wait for Apply" },
            { value: "swarm", label: "Swarm", tip: "Several agents draw at once, into a draft you accept" },
          ]}
        />
        <span className={s.headMeta} data-testid="ai-head-meta">
          {headMeta}
        </span>
        <button
          type="button"
          className={cx(s.iconBtn, settingsOpen && s.iconBtnOn)}
          aria-label="Copilot settings"
          aria-expanded={settingsOpen}
          data-tip={noKey ? "Copilot settings: add an API key" : model ? `Copilot settings. AI model: ${model}` : "Copilot settings"}
          data-tip-side="bottom-end"
          // The tip would draw over the open popover.
          data-tip-off={settingsOpen ? "true" : undefined}
          onClick={() => setSettingsOpen((v) => !v)}
        >
          <GearIcon />
          {noKey ? <span className={s.dotWarn} aria-hidden /> : null}
        </button>
      </header>

      {settings0.mounted ? (
        <SettingsPopover
          stage={settings0.stage}
          settings={settings}
          loadError={settingsError}
          onClose={() => setSettingsOpen(false)}
          onSaved={(next) => useCopilot.getState().setSettings(next)}
        />
      ) : null}

      {mode === "swarm" ? <SwarmPanel hasKey={!noKey} hasDoc={!!doc} /> : null}

      {mode === "chat" ? (
        <>
          <div className={s.list} ref={listRef} role="log" aria-live="polite">
            {noKey ? (
              <NoKeyCard onOpenSettings={() => setSettingsOpen(true)} />
            ) : items.length === 0 ? (
              <EmptyState suggestions={suggestions} onPick={submit} disabled={!doc || busy || locked} hasSelection={selection.length > 0} />
            ) : null}

            {items.map((m) => (
              <Message key={m.id} item={m} onApply={apply} onDiscard={discard} onKey={(e) => onProposalKey(e, "")} />
            ))}

            {working.mounted ? (
              <div className={s.working} data-stage={working.stage} data-testid="ai-busy" role="status">
                <span className={s.workingText}>Working on it</span>
                <span className={s.dots} aria-hidden>
                  <i />
                  <i />
                  <i />
                </span>
              </div>
            ) : null}
          </div>

          {chipRow.mounted ? (
            <div className={s.chipRow} data-stage={chipRow.stage} data-testid="chip-row">
              {suggestions.slice(0, 3).map((sg) => (
                <Chip key={sg.text} suggestion={sg} compact onPick={submit} disabled={locked} />
              ))}
            </div>
          ) : null}

          {scopeRow.mounted ? (
            <div className={cx(s.scopeRow, aiScope && s.scopeRowOn)} data-stage={scopeRow.stage} data-testid="ai-scope">
              <span className={s.scopeWhat}>
                <Icon name="select" size={12} />
                <b>{scopeText.current}</b>
              </span>
              <span
                className={s.scopeLabel}
                data-tip="The copilot and your agents change only the selection and what stands in a selected room"
                data-tip-side="top-end"
                onClick={() => setAiScope(!aiScope)}
              >
                Only the selection
              </span>
              <Switch checked={aiScope} onChange={setAiScope} label="AI changes only the selection" />
            </div>
          ) : null}

          {doc && !noKey ? <FurnishBar disabled={!gate.canFurnish} tip={gate.tip} onFurnish={(style) => void furnish(style)} /> : null}

          <div className={s.composer}>
            <textarea
              ref={inputRef}
              className={s.input}
              rows={composing || draft !== "" || locked ? 2 : 1}
              value={draft}
              placeholder={noKey ? "Add an API key to start" : locked ? LOCK_HINT : scoped ? (composing ? "Describe a change to the selection" : "Describe a change") : composing ? "Ask about the plan or describe a change" : "Ask or describe a change"}
              disabled={!doc || noKey}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={onKeyDown}
              onFocus={() => setComposing(true)}
              onBlur={() => setComposing(false)}
              aria-label="Message to the copilot"
            />
            {busy ? (
              <button type="button" className={cx(s.sendBtn, s.stopBtn)} onClick={stop} aria-label="Stop waiting" data-tip="Stop waiting" data-tip-side="top-end">
                <StopIcon />
              </button>
            ) : (
              <button
                type="button"
                className={s.sendBtn}
                disabled={!canSend}
                onClick={() => submit(draft)}
                aria-label="Send"
                data-tip={gate.tip ?? `Enter to send, ${SHIFT}Enter for a new line`}
                data-tip-side="top-end"
              >
                <span className={s.sendIcon} ref={sendIconRef}>
                  <SendIcon />
                </span>
              </button>
            )}
          </div>
        </>
      ) : null}
    </div>
  );
}

function Chip({ suggestion, onPick, compact, disabled }: { suggestion: Suggestion; onPick: (text: string) => void; compact?: boolean; disabled?: boolean }) {
  const [used, setUsed] = useState(false);
  return (
    <button
      type="button"
      className={cx(s.chip, compact && s.chipCompact)}
      data-used={used}
      disabled={disabled}
      onClick={() => {
        setUsed(true);
        onPick(suggestion.text);
      }}
    >
      <span>{suggestion.text}</span>
      {suggestion.tag && !compact ? <em className={s.chipTag}>{suggestion.tag}</em> : null}
    </button>
  );
}

/** Counts up from 0 when a proposal card arrives. Jumps under reduced motion. */
function Count({ value }: { value: number }) {
  const [shown, setShown] = useState(0);
  useEffect(() => {
    const t = tween(dur("base"), (p) => setShown(Math.round(value * p)), { easing: ease.out });
    return () => t.cancel();
  }, [value]);
  return <b>{shown}</b>;
}

function EmptyState({ suggestions, onPick, disabled, hasSelection }: { suggestions: Suggestion[]; onPick: (text: string) => void; disabled: boolean; hasSelection: boolean }) {
  return (
    <div className={s.empty}>
      <p className={s.emptyTitle}>Ask about the plan, or describe a change.</p>
      <p className={s.emptyText}>
        Answers are read from the plan, not guessed. Edits show as a preview first and change nothing until you
        click Apply. English, Filipino and Taglish all work.
      </p>
      <p className={s.emptyLabel}>{hasSelection ? "For the selection" : "Try"}</p>
      <div className={s.chips}>
        {suggestions.map((sg) => (
          <Chip key={sg.text} suggestion={sg} onPick={onPick} disabled={disabled} />
        ))}
      </div>
    </div>
  );
}

function NoKeyCard({ onOpenSettings }: { onOpenSettings: () => void }) {
  return (
    <div className={s.noKey}>
      <p className={s.emptyTitle}>The copilot needs a Claude API key</p>
      <ol className={s.steps}>
        <li>Create an API key in the Claude Console at platform.claude.com and add credits there.</li>
        <li>Open the copilot settings and paste it. Subscription and Claude Code tokens do not work.</li>
      </ol>
      <p className={s.emptyText}>
        <ConnectLink />
      </p>
      <p className={s.emptyText}>
        The key is kept in the app's data folder, readable only by your user account, never in the project, and
        the app never shows it again. Drawing, 3D and exports work without it.
      </p>
      <button type="button" className={s.primaryBtn} onClick={onOpenSettings}>
        Add API key
      </button>
    </div>
  );
}

interface MessageProps {
  item: ChatItem;
  onApply: (item: ChatItem, p: AiProposal) => void;
  onDiscard: (item: ChatItem, p: AiProposal) => void;
  /** Mod+Enter and Esc on a pending card. */
  onKey: (e: KeyboardEvent<HTMLElement>) => void;
}

function Message({ item, onApply, onDiscard, onKey }: MessageProps) {
  if (item.role === "user") {
    return (
      <div className={cx(s.msg, s.msgUser)}>
        <div className={s.bubbleUser}>{item.text}</div>
      </div>
    );
  }
  if (item.role === "note") {
    return <div className={cx(s.note, item.tone === "error" && s.noteError)}>{item.text}</div>;
  }
  // Every answer here is the copilot's: no "AI" tag. What it read says where the answer came from.
  const readTools = item.proposal ? "" : item.toolsUsed.map(toolLabel).join(", ");
  return (
    <div className={cx(s.msg, s.msgAi)}>
      {readTools ? (
        <div className={s.msgHead}>
          <span className={s.grounded}>Read from the plan: {readTools}</span>
        </div>
      ) : null}
      <div className={s.bubbleAi}>{item.text}</div>
      {item.proposal ? <ProposalCard item={item} proposal={item.proposal} onApply={onApply} onDiscard={onDiscard} onKey={onKey} /> : null}
      {item.furnish && item.furnish.length > 0 ? <FurnishCards item={item} /> : null}
    </div>
  );
}

const CHANGE_LABEL = { added: "Added", changed: "Changed", removed: "Removed" } as const;

function ProposalCard({ item, proposal, onApply, onDiscard, onKey }: Omit<MessageProps, "item"> & { item: ChatItem; proposal: AiProposal }) {
  const doc = useApp((st) => st.doc);
  const hoverId = useApp((st) => st.hoverId);
  const setHover = useApp((st) => st.setHover);
  const diff = proposal.preview.diff;
  const rows = affectedRows(proposal, doc);
  const live = item.proposalStatus === "pending" || item.proposalStatus === "applying";
  const applying = item.proposalStatus === "applying";
  const steps = proposal.command.type === "batch" ? proposal.command.commands.length : 1;
  const cardRef = useRef<HTMLElement>(null);
  const actionsRef = useRef<HTMLDivElement>(null);
  const [resolved, setResolved] = useState(!live);
  const applied = item.proposalStatus === "applied";

  // The card expands in when the proposal arrives, then the message list
  // scrolls so the whole card - header down to Apply/Discard - is in view.
  // Waiting for the grow-in to settle (rather than scrolling right away)
  // matters: scrolling too early measures the card mid-animation, while its
  // height is still small, and the scroll never catches up once it finishes
  // growing. A card taller than the list itself still shows its actions,
  // pinned as a sticky footer (see .cardActions).
  useLayoutEffect(() => {
    const el = cardRef.current;
    if (!el) return;
    const reveal = () => el.scrollIntoView({ block: "start", behavior: motionOK() ? "smooth" : "auto" });
    const anim = growIn(el, "panel");
    if (!anim) {
      reveal();
      return;
    }
    let cancelled = false;
    void settled(anim).then(() => {
      if (!cancelled) reveal();
    });
    return () => {
      cancelled = true;
    };
  }, []);

  // Apply draws a check mark in the button, then the action row collapses
  // away and the note takes its place. Discard collapses it right away.
  useEffect(() => {
    if (live || resolved) return;
    let cancelled = false;
    const run = async () => {
      if (applied) await new Promise((r) => setTimeout(r, dur("base")));
      if (cancelled) return;
      await collapseOut(actionsRef.current, "base");
      if (!cancelled) setResolved(true);
    };
    void run();
    return () => {
      cancelled = true;
    };
  }, [live, resolved, applied]);

  // Only the counts that are not zero, in ink: "2 added", "1 changed".
  const counts = (
    [
      ["added", diff.added.length],
      ["changed", diff.modified.length],
      ["removed", diff.removed.length],
    ] as const
  ).filter(([, n]) => n > 0);
  const tools = item.toolsUsed.map(toolLabel).join(", ");

  return (
    <section className={cx(s.card, !live && s.cardDone)} ref={cardRef} aria-label="Proposed change" onKeyDown={live ? onKey : undefined}>
      <header className={s.cardHead}>
        <span className={s.cardTitle}>{live ? "Proposed change" : "Proposal"}</span>
        <span key={item.proposalStatus ?? "none"} className={s.cardState} data-state={item.proposalStatus ?? undefined}>
          {item.proposalStatus === "pending" && "Preview on the plan"}
          {item.proposalStatus === "applying" && "Applying"}
          {item.proposalStatus === "applied" && "Applied"}
          {item.proposalStatus === "discarded" && "Discarded"}
          {item.proposalStatus === "stale" && "Out of date"}
        </span>
      </header>

      <p className={s.cardSummary}>{diff.summary}</p>

      {counts.length > 0 ? (
        <p className={s.counts}>
          {counts.map(([kind, n]) => (
            <span key={kind} className={s.count} data-kind={kind}>
              <Count value={n} /> {kind}
            </span>
          ))}
        </p>
      ) : null}

      {rows.length > 0 ? (
        <ul className={s.rows} onMouseLeave={() => live && setHover(null)}>
          {rows.map((row) => (
            <li
              key={`${row.change}-${row.id}`}
              className={cx(s.row, hoverId === row.id && s.rowHover)}
              onMouseEnter={() => live && setHover(row.id)}
            >
              <KindIcon kind={row.kind} />
              <span className={s.rowName}>{row.name}</span>
              <span className={s.rowChange} data-kind={row.change}>
                {CHANGE_LABEL[row.change]}
              </span>
            </li>
          ))}
        </ul>
      ) : (
        <p className={s.cardEmpty}>No elements change. Project settings or the roof may.</p>
      )}

      <p className={s.cardTools}>
        {steps} {steps === 1 ? "edit" : "edits"}
        {tools ? `: ${tools}` : ""}
      </p>

      {!resolved ? (
        <div className={s.cardActions} ref={actionsRef}>
          {/* The word and the check share one grid cell, so the button keeps its width as it turns
              into a check. While it applies, the card's state says "Applying". */}
          <button
            type="button"
            className={cx(s.applyBtn, applied && s.applyBtnDone)}
            data-testid="proposal-apply"
            disabled={applying || applied}
            onClick={() => onApply(item, proposal)}
            aria-label={applied ? "Applied" : undefined}
            data-tip={live && !applying ? `Apply (${MOD}Enter)` : undefined}
            data-tip-side="top-start"
          >
            <span className={s.applyFace} data-on={!applied} aria-hidden={applied}>
              Apply
            </span>
            {applied ? <DrawnCheck className={s.checkPath} /> : null}
          </button>
          <button type="button" className={s.discardBtn} disabled={applying || applied} onClick={() => onDiscard(item, proposal)} data-tip={live && !applying ? "Discard (Esc)" : undefined} data-tip-side="top">
            Discard
          </button>
          <span className={s.cardHint}>One undo reverts it</span>
        </div>
      ) : (
        <p className={s.cardNote} data-state={item.proposalStatus ?? undefined}>
          {item.proposalNote}
        </p>
      )}
    </section>
  );
}
