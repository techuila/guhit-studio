// Copilot dock. A calm chat panel: the user asks, the copilot answers from
// model data or proposes typed edits. A proposal shows as a ghost in the 2D
// and 3D views (store.preview) and commits only when the user applies it.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import type { AiProposal } from "../contract/bindings";
import { ipc, toIpcError } from "../contract/ipc";
import { useShell } from "../shell/shellStore";
import { useApp } from "../state/store";
import { dur, ease, motionOK, tween, usePresence } from "../ui/motion";
import { collapseOut, growIn, play, settled } from "../ui/motionWaapi";
import { pendingItem, toHistory, useCopilot, type ChatItem } from "./copilotStore";
import { affectedRows, suggestionsFor, toolLabel, type Suggestion } from "./describe";
import { SettingsPopover } from "./SettingsPopover";
import { DrawnCheck, GearIcon, KindIcon, SendIcon, StopIcon } from "./icons";
import s from "./AiDock.module.css";

function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

const STALE_TEXT = "The plan changed after this proposal was made, so it was not applied. Ask again and I will work from the current plan.";

/** Drop a proposal on the backend. Failing here is harmless: a new chat turn replaces it anyway. */
function dropProposal(id: string) {
  void ipc.aiResolve(id, false).catch(() => undefined);
}

export function AiDock() {
  const doc = useApp((st) => st.doc);
  const selection = useApp((st) => st.selection);
  const items = useCopilot((st) => st.items);
  const busy = useCopilot((st) => st.busy);
  const settings = useCopilot((st) => st.settings);
  const settingsError = useCopilot((st) => st.settingsError);

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

  // A different project (or none) starts a clean conversation and never
  // inherits a ghost preview.
  useEffect(() => {
    const st = useCopilot.getState();
    if (st.projectId === projectId) return;
    const open = pendingItem(st.items);
    if (open?.proposal) dropProposal(open.proposal.id);
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
      const open = pendingItem(st.items);
      if (!open?.proposal) return;
      useApp.getState().setPreview(null);
      useApp.getState().setHover(null);
      st.patch(open.id, { proposalStatus: "discarded", proposalNote: "Discarded when the copilot closed." });
      dropProposal(open.proposal.id);
    };
  }, []);

  // The list follows the newest message instead of jumping to it.
  useLayoutEffect(() => {
    const el = listRef.current;
    if (!el) return;
    if (typeof el.scrollTo === "function") el.scrollTo({ top: el.scrollHeight, behavior: motionOK() ? "smooth" : "auto" });
    else el.scrollTop = el.scrollHeight;
  }, [items, busy]);

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
      if (!message || copilot.busy) return;
      const app = useApp.getState();
      if (!app.doc) return;

      // A new turn replaces the old proposal. The backend drops it too.
      retirePending("Replaced by a newer request.");
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

  const stop = useCallback(() => {
    const copilot = useCopilot.getState();
    copilot.nextRequest();
    copilot.setBusy(false);
    copilot.push({ role: "note", text: "Stopped. Nothing was changed." });
    inputRef.current?.focus();
  }, []);

  const apply = useCallback(async (item: ChatItem, proposal: AiProposal) => {
    const copilot = useCopilot.getState();
    copilot.patch(item.id, { proposalStatus: "applying" });
    try {
      const result = await ipc.aiResolve(proposal.id, true);
      const app = useApp.getState();
      app.setPreview(null);
      app.setHover(null);
      if (result.applied) {
        app.setDoc(result.applied.state);
        app.toast("success", "Applied. Undo reverts it in one step.");
        copilot.patch(item.id, { proposalStatus: "applied", proposalNote: "Applied. Undo reverts it in one step." });
      } else {
        copilot.patch(item.id, { proposalStatus: "discarded", proposalNote: "Nothing was applied." });
      }
    } catch (e) {
      const err = toIpcError(e);
      const app = useApp.getState();
      app.setPreview(null);
      app.setHover(null);
      const stale = err.code === "stale" || err.code === "not_found";
      copilot.patch(item.id, { proposalStatus: "stale", proposalNote: stale ? STALE_TEXT : `Not applied: ${err.message}` });
      // Make sure the mirror matches the engine before the user asks again.
      try {
        const state = await ipc.docState();
        if (state) app.setDoc(state);
      } catch {
        // The next action will surface a bridge problem.
      }
    }
  }, []);

  const discard = useCallback((item: ChatItem, proposal: AiProposal) => {
    const app = useApp.getState();
    app.setPreview(null);
    app.setHover(null);
    useCopilot.getState().patch(item.id, { proposalStatus: "discarded", proposalNote: "Discarded. The plan was not changed." });
    dropProposal(proposal.id);
  }, []);

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      submitRef.current(draft);
    }
  };

  const noKey = settings !== null && !settings.has_api_key;
  const suggestions = suggestionsFor(doc, selection);
  const canSend = !!doc && !busy && !noKey && draft.trim() !== "";

  // Exits animate: the popover, the busy line and the chip row all stay
  // mounted long enough to leave.
  const settings0 = usePresence(settingsOpen, "base");
  const working = usePresence(busy, "hover");
  const chipsOpen = !noKey && items.length > 0 && !busy && !pending && draft === "";
  const chipRow = usePresence(chipsOpen, "hover");

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
      <header className={s.head}>
        <span className={s.aiTag}>AI</span>
        <span className={s.headTitle} title="Model used by the copilot">
          {settings?.model ?? "Copilot"}
        </span>
        <span className={s.headMeta}>
          {selection.length === 0 ? "No selection" : `${selection.length} selected`}
        </span>
        <button
          type="button"
          className={cx(s.iconBtn, settingsOpen && s.iconBtnOn)}
          aria-label="Copilot settings"
          aria-expanded={settingsOpen}
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

      <div className={s.list} ref={listRef} role="log" aria-live="polite">
        {noKey ? (
          <NoKeyCard onOpenSettings={() => setSettingsOpen(true)} />
        ) : items.length === 0 ? (
          <EmptyState suggestions={suggestions} onPick={submit} disabled={!doc || busy} hasSelection={selection.length > 0} />
        ) : null}

        {items.map((m) => (
          <Message key={m.id} item={m} onApply={apply} onDiscard={discard} />
        ))}

        {working.mounted ? (
          <div className={s.working} data-stage={working.stage} data-testid="ai-busy" role="status">
            <span className={s.aiTag}>AI</span>
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
            <Chip key={sg.text} suggestion={sg} compact onPick={submit} />
          ))}
        </div>
      ) : null}

      <div className={s.composer}>
        <textarea
          ref={inputRef}
          className={s.input}
          rows={2}
          value={draft}
          placeholder={noKey ? "Add an API key to start" : "Ask about the plan or describe a change"}
          disabled={!doc || noKey}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKeyDown}
          aria-label="Message to the copilot"
        />
        {busy ? (
          <button type="button" className={cx(s.sendBtn, s.stopBtn)} onClick={stop} aria-label="Stop waiting">
            <StopIcon />
          </button>
        ) : (
          <button type="button" className={s.sendBtn} disabled={!canSend} onClick={() => submit(draft)} aria-label="Send">
            <span className={s.sendIcon} ref={sendIconRef}>
              <SendIcon />
            </span>
          </button>
        )}
      </div>
      <div className={s.foot}>
        <span>Enter to send, Shift+Enter for a new line</span>
      </div>
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
        Answers come from the model data. Edits show as a preview first and are applied only when you approve.
        English, Filipino and Taglish all work.
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
        The key is kept in the app's data folder, readable only by your user account, never in the project, and
        the app never shows it again. Drawing, 3D and exports work without it.
      </p>
      <button type="button" className={s.primaryBtn} onClick={onOpenSettings}>
        Add API key
      </button>
    </div>
  );
}

function Message({ item, onApply, onDiscard }: { item: ChatItem; onApply: (item: ChatItem, p: AiProposal) => void; onDiscard: (item: ChatItem, p: AiProposal) => void }) {
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
  const readTools = item.proposal ? [] : item.toolsUsed;
  return (
    <div className={cx(s.msg, s.msgAi)}>
      <div className={s.msgHead}>
        <span className={s.aiTag}>AI</span>
        {readTools.length > 0 ? <span className={s.grounded}>From model data: {readTools.map(toolLabel).join(", ")}</span> : null}
      </div>
      <div className={s.bubbleAi}>{item.text}</div>
      {item.proposal ? <ProposalCard item={item} proposal={item.proposal} onApply={onApply} onDiscard={onDiscard} /> : null}
    </div>
  );
}

const CHANGE_LABEL = { added: "Added", changed: "Changed", removed: "Removed" } as const;

function ProposalCard({ item, proposal, onApply, onDiscard }: { item: ChatItem; proposal: AiProposal; onApply: (item: ChatItem, p: AiProposal) => void; onDiscard: (item: ChatItem, p: AiProposal) => void }) {
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

  return (
    <section className={cx(s.card, !live && s.cardDone)} ref={cardRef} aria-label="Proposed change">
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

      <div className={s.counts}>
        <span className={s.count} data-kind="added">
          <Count value={diff.added.length} /> added
        </span>
        <span className={s.count} data-kind="changed">
          <Count value={diff.modified.length} /> changed
        </span>
        <span className={s.count} data-kind="removed">
          <Count value={diff.removed.length} /> removed
        </span>
      </div>

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
        {steps} staged {steps === 1 ? "edit" : "edits"}. Tools: {item.toolsUsed.map(toolLabel).join(", ")}
      </p>

      {!resolved ? (
        <div className={s.cardActions} ref={actionsRef}>
          <button
            type="button"
            className={cx(s.applyBtn, applied && s.applyBtnDone)}
            data-testid="proposal-apply"
            disabled={applying || applied}
            onClick={() => onApply(item, proposal)}
          >
            {applied ? <DrawnCheck className={s.checkPath} /> : applying ? "Applying" : "Apply"}
          </button>
          <button type="button" className={s.discardBtn} disabled={applying || applied} onClick={() => onDiscard(item, proposal)}>
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
