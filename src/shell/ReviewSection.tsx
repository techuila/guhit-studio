// The review list in the project inspector: open items grouped by level and
// room (warnings first), each with Show, Walk to it and Set aside; the items
// set aside with their notes and Reopen; and the set-aside items the checks
// no longer find (resolved). Review items are suggestions: set aside, reopen
// and resolved are the only states, never "approved" (DECISIONS D24).
//
// Triage keys while the list has focus: up and down move, S sets the item
// aside (the note field takes focus), O reopens, Enter shows it.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import type { Command, DocState, Element, Issue, ReviewMark } from "../contract/bindings";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { Button, Section, cx } from "../ui/controls";
import { Icon, type IconName } from "../ui/icons";
import { useListPresence } from "../ui/useListPresence";
import { play } from "../ui/motionWaapi";
import { walkTo } from "./actions";
import { elementTitle } from "./ElementFields";
import { useReveal } from "./PipeSections";
import { orderIssues } from "./pipes";
import {
  asideRows,
  checkLabel,
  groupReview,
  markCovers,
  neighbourAfterRemoval,
  rememberIssues,
  resolvedTitle,
  targetFor,
  targetKey,
  triageKey,
  type AsideRow,
  type AsideScope,
  type TriageRow,
} from "./review";
import { useSection, useShell } from "./shellStore";
import s from "./Review.module.css";

const dispatch = (command: Command) => useApp.getState().dispatch(command);

const SEVERITY_ICON: Record<Issue["severity"], IconName> = { info: "info", warning: "warning", error: "warning" };

const SCOPES: Array<{ value: AsideScope; label: string }> = [
  { value: "issue", label: "This item" },
  { value: "check", label: "This check everywhere" },
  { value: "element", label: "This check on this object" },
];

/** A row of the open list: a level heading, a room heading or an item. */
type OpenRow =
  | { type: "level"; key: string; label: string; count: number }
  | { type: "room"; key: string; label: string; count: number; worst: Issue["severity"] }
  | { type: "item"; key: string; issue: Issue };

const rowKey = (r: OpenRow) => r.key;
const asideKey = (r: AsideRow) => `aside:${r.key}`;
const resolvedKey = (m: ReviewMark) => `resolved:${targetKey(m.target)}`;
/** A DOM id for a triage row, to focus and scroll to it. */
const domId = (key: string) => `review-${key.replace(/[^a-zA-Z0-9_-]/g, "_")}`;

/** How an element is called in "on ..." phrases. */
function nameOf(el: Element, doc: DocState): string {
  const t = elementTitle(el, doc);
  if (el.kind === "room" || el.kind === "asset" || el.kind === "camera") return t.subtitle || t.title;
  if (el.kind === "pipe" && el.name) return el.name;
  return `the ${t.title.toLowerCase()}`;
}

interface Editing {
  issueId: string;
  note: string;
  scope: AsideScope;
  invalid: number;
}

export function ReviewSection({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("review", true);
  const sectionRef = useRef<HTMLDivElement>(null);
  const flash = useReveal("review", sectionRef);
  const select = useApp((st) => st.select);

  const issues = doc.derived.issues;
  // So a resolved item can still say what it was.
  useEffect(() => rememberIssues(issues), [issues]);

  // Open items only: an item a mark covers is listed under Set aside instead.
  const { items, notes } = useMemo(() => orderIssues(issues.filter((i) => i.status !== "ignored")), [issues]);
  const groups = useMemo(() => groupReview(doc, items), [doc, items]);
  const openRows = useMemo<OpenRow[]>(
    () =>
      groups.flatMap((level) => [
        { type: "level" as const, key: `level:${level.key}`, label: level.label, count: level.count },
        ...level.rooms.flatMap((room) => [
          { type: "room" as const, key: `room:${room.key}`, label: room.label, count: room.items.length, worst: room.worst },
          ...room.items.map((issue) => ({ type: "item" as const, key: `open:${issue.id}`, issue })),
        ]),
      ]),
    [groups],
  );
  const byId = useMemo(() => new Map(doc.project.elements.map((e) => [e.id, e])), [doc.project.elements]);
  const aside = useMemo(() => asideRows(doc, (id) => (byId.has(id) ? nameOf(byId.get(id)!, doc) : null)), [doc, byId]);
  const resolved = doc.derived.review_resolved;

  const presentOpen = useListPresence(openRows, rowKey);
  const presentNotes = useListPresence(notes, (i: Issue) => i.id);
  const presentAside = useListPresence(aside, asideKey);
  const presentResolved = useListPresence(resolved, resolvedKey);

  const triage = useMemo<TriageRow[]>(
    () => [
      ...openRows.filter((r) => r.type === "item").map((r) => ({ key: r.key, kind: "open" as const })),
      ...aside.map((r) => ({ key: asideKey(r), kind: "aside" as const })),
    ],
    [openRows, aside],
  );

  const [active, setActive] = useState<string | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [busy, setBusy] = useState(false);

  // The active row went away (fixed, set aside, reopened by an undo): let go of it.
  useEffect(() => {
    if (active && !triage.some((r) => r.key === active)) setActive(null);
    if (editing && !items.some((i) => i.id === editing.issueId)) setEditing(null);
  }, [triage, items, active, editing]);

  /** Roving focus: the active row holds the list's one tab stop. */
  const tabStop = active && triage.some((r) => r.key === active) ? active : (triage[0]?.key ?? null);
  const focusRow = (key: string | null) => {
    if (!key) return;
    // After the render that makes the row active (and after a row leaves).
    requestAnimationFrame(() => {
      const el = document.getElementById(domId(key));
      el?.focus({ preventScroll: true });
      el?.scrollIntoView({ block: "nearest" });
    });
  };

  const known = (ids: string[]) => ids.filter((id) => byId.has(id));
  const show = (ids: string[]) => {
    const found = known(ids);
    if (found.length === 0) return;
    // The inspector keeps this list up while these are the selection.
    useShell.getState().setReviewShown(found);
    select(found);
    bus.emit("focus_elements", found);
  };
  const walk = (issue: Issue) => void walkTo(known(issue.element_ids), issue.location);
  const objectOf = (issue: Issue) => {
    const el = byId.get(issue.element_ids[0] ?? "");
    return el ? nameOf(el, doc) : null;
  };

  const startAside = (issue: Issue) => {
    setActive(`open:${issue.id}`);
    setEditing({ issueId: issue.id, note: "", scope: "issue", invalid: 0 });
  };

  const commitAside = async (issue: Issue, e: Editing) => {
    const note = e.note.trim();
    if (!note) {
      setEditing({ ...e, invalid: e.invalid + 1 });
      return;
    }
    const target = targetFor(e.scope, issue);
    if (!target) return;
    // Land on the nearest open item the new mark leaves open.
    const stays = triage.filter((r) => {
      if (r.kind !== "open") return false;
      const other = items.find((i) => `open:${i.id}` === r.key);
      return r.key === `open:${issue.id}` || (other !== undefined && !markCovers(target, other));
    });
    const next = neighbourAfterRemoval(stays, `open:${issue.id}`);
    setBusy(true);
    const result = await dispatch({ type: "set_review_mark", target, note });
    setBusy(false);
    if (!result) return;
    setEditing(null);
    setActive(next);
    focusRow(next);
  };

  const reopen = async (row: AsideRow) => {
    const next = neighbourAfterRemoval(triage, asideKey(row));
    const result = await dispatch({ type: "set_review_mark", target: row.mark.target, note: null });
    if (!result) return;
    setActive(next);
    focusRow(next);
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    // Only keys on a row itself: the note field and a row's buttons keep theirs.
    const rowKey = (e.target as HTMLElement).dataset?.triage;
    if (!rowKey) return;
    // Show selects the item's objects: on a row, these keys must not delete
    // or nudge them behind the user's back.
    if (!e.metaKey && !e.ctrlKey && ["Delete", "Backspace", "ArrowLeft", "ArrowRight"].includes(e.key)) {
      e.preventDefault();
      e.stopPropagation();
      return;
    }
    const action = triageKey(e, triage, rowKey);
    if (!action) return;
    e.preventDefault();
    e.stopPropagation();
    if (action.type === "move") {
      setActive(action.key);
      focusRow(action.key);
      return;
    }
    const issue = items.find((i) => `open:${i.id}` === action.key);
    const row = aside.find((r) => asideKey(r) === action.key);
    if (action.type === "set_aside" && issue) startAside(issue);
    else if (action.type === "reopen" && row) void reopen(row);
    else if (action.type === "show") show(issue?.element_ids ?? row?.elementIds ?? []);
  };

  const clickRow = (key: string, ids: string[], target: EventTarget | null) => {
    if ((target as HTMLElement | null)?.closest("button, input, a")) return;
    setActive(key);
    show(ids);
  };

  const count = items.length;
  const empty = count === 0 && notes.length === 0;

  return (
    <div ref={sectionRef} className={cx(s.reveal, flash && s.revealFlash)}>
      <Section
        title="Review"
        icon="check"
        count={count}
        open={open}
        onToggle={toggle}
        aside={aside.length > 0 ? <span className={s.sectionAside}>{aside.length} set aside</span> : null}
      >
        {empty ? <p className={s.empty}>Nothing to flag right now.</p> : null}
        <div className={s.list} role="list" aria-label="Review items. Up and down move, S sets aside, O reopens, Enter shows." onKeyDown={onKeyDown}>
          {presentOpen.map(({ key, item: row, entering, leaving }) => (
            <div key={key} className={cx(s.row, entering && s.rowIn, leaving && s.rowOut)} inert={leaving}>
              <div className={s.rowInner}>
                {row.type === "level" ? (
                  <div className={s.levelHead}>
                    <span>{row.label}</span>
                    <span className={s.levelCount}>{row.count}</span>
                  </div>
                ) : row.type === "room" ? (
                  <div className={s.roomHead}>
                    <Icon name={row.label === "Whole project" ? "folder" : "room"} size={13} />
                    <span className={s.roomName}>{row.label}</span>
                    <span className={cx(s.roomCount, row.worst !== "info" && s.roomCountWarn)}>{row.count}</span>
                  </div>
                ) : (
                  <OpenItem
                    issue={row.issue}
                    rowKey={key}
                    active={active === key}
                    tabStop={tabStop === key}
                    onFocus={() => setActive(key)}
                    editing={editing?.issueId === row.issue.id ? editing : null}
                    busy={busy}
                    canShow={known(row.issue.element_ids).length > 0}
                    objectName={objectOf(row.issue)}
                    onClick={(target) => clickRow(key, row.issue.element_ids, target)}
                    onShow={() => show(row.issue.element_ids)}
                    onWalk={() => walk(row.issue)}
                    onStartAside={() => startAside(row.issue)}
                    onEdit={setEditing}
                    onCommit={(ed) => void commitAside(row.issue, ed)}
                    onCancel={() => {
                      setEditing(null);
                      focusRow(key);
                    }}
                  />
                )}
              </div>
            </div>
          ))}

          {presentNotes.map(({ key, item: note, entering, leaving }) => (
            <div key={key} className={cx(s.row, entering && s.rowIn, leaving && s.rowOut)} inert={leaving}>
              <div className={s.rowInner}>
                <div className={s.note}>
                  <Icon name="info" size={15} className={s.icon} />
                  <div className={s.body}>
                    <span>{note.message}</span>
                    <div className={s.actions}>
                      <Button size="sm" icon="fit" onClick={() => show(note.element_ids)} disabled={known(note.element_ids).length === 0}>
                        Show
                      </Button>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          ))}

          {presentAside.length > 0 ? (
            <div className={s.subhead}>
              <Icon name="aside" size={13} />
              <span>Set aside</span>
              <span className={s.levelCount}>{aside.length}</span>
            </div>
          ) : null}
          {presentAside.map(({ key, item: row, entering, leaving }) => (
            <div key={key} className={cx(s.row, entering && s.rowIn, leaving && s.rowOut)} inert={leaving}>
              <div className={s.rowInner}>
                <div
                  id={domId(key)}
                  role="listitem"
                  tabIndex={tabStop === key ? 0 : -1}
                  data-triage={key}
                  aria-current={active === key ? "true" : undefined}
                  className={cx(s.item, s.asideItem, active === key && s.itemActive)}
                  onFocus={(e) => {
                    if (e.target === e.currentTarget) setActive(key);
                  }}
                  onClick={(e) => clickRow(key, row.elementIds, e.target)}
                >
                  <Icon name="aside" size={15} className={s.icon} />
                  <div className={s.body}>
                    <span className={s.message}>{row.title}</span>
                    {row.mark.target.kind !== "issue" ? (
                      <span className={s.meta}>
                        {row.issues.length === 0 ? "No items right now" : `${row.issues.length} ${row.issues.length === 1 ? "item" : "items"}`}
                      </span>
                    ) : null}
                    <q className={s.quote}>{row.mark.note}</q>
                    <Reveal open={active === key}>
                      <div className={s.actions}>
                        <Button size="sm" icon="fit" onClick={() => show(row.elementIds)} disabled={known(row.elementIds).length === 0}>
                          Show
                        </Button>
                        <Button size="sm" icon="reopen" onClick={() => void reopen(row)}>
                          Reopen
                        </Button>
                      </div>
                    </Reveal>
                  </div>
                </div>
              </div>
            </div>
          ))}
        </div>

        {presentResolved.length > 0 ? (
          <div className={s.subhead}>
            <Icon name="resolved" size={13} />
            <span>Resolved</span>
            <span className={s.levelCount}>{resolved.length}</span>
          </div>
        ) : null}
        {presentResolved.map(({ key, item: mark, entering, leaving }) => (
          <div key={key} className={cx(s.row, entering && s.rowIn, leaving && s.rowOut)} inert={leaving}>
            <div className={s.rowInner}>
              <div className={cx(s.item, s.resolvedItem)}>
                <Icon name="resolved" size={15} className={s.icon} />
                <div className={s.body}>
                  <span className={s.message}>{mark.target.kind === "issue" ? resolvedTitle(mark.target.id) : checkLabel(mark.target.code)}</span>
                  <span className={s.meta}>The checks no longer find it.</span>
                  {mark.note ? <q className={s.quote}>{mark.note}</q> : null}
                </div>
                <Button
                  size="sm"
                  variant="ghost"
                  className={s.clear}
                  aria-label="Clear this resolved item"
                  data-tip="Forget it. If it comes back, it is open"
                  data-tip-side="top-end"
                  onClick={() => void dispatch({ type: "set_review_mark", target: mark.target, note: null })}
                >
                  Clear
                </Button>
              </div>
            </div>
          </div>
        ))}

        {triage.length > 0 ? (
          <p className={s.keys}>
            With the list focused: <kbd>↑</kbd> <kbd>↓</kbd> move, <kbd>S</kbd> set aside, <kbd>O</kbd> reopen, <kbd>Enter</kbd> show.
          </p>
        ) : null}
        <p className={s.disclaimer}>
          Suggestions from a design check, for you to judge. This is not a permit, structural, plumbing, electrical or building code review.
        </p>
      </Section>
    </div>
  );
}

/** Height reveal for a row's actions (MOTION.md rule 5). Collapsed actions are inert. */
function Reveal({ open, children }: { open: boolean; children: ReactNode }) {
  return (
    <div className={cx(s.actionsReveal, open && s.actionsRevealOpen)} inert={!open}>
      <div className={s.actionsRevealInner}>{children}</div>
    </div>
  );
}

interface OpenItemProps {
  issue: Issue;
  rowKey: string;
  active: boolean;
  /** Holds the list's one tab stop. */
  tabStop: boolean;
  onFocus: () => void;
  editing: Editing | null;
  busy: boolean;
  canShow: boolean;
  /** "the Kitchen", for "This check on this object". */
  objectName: string | null;
  onClick: (target: EventTarget | null) => void;
  onShow: () => void;
  onWalk: () => void;
  onStartAside: () => void;
  onEdit: (e: Editing) => void;
  onCommit: (e: Editing) => void;
  onCancel: () => void;
}

function OpenItem(p: OpenItemProps) {
  const { issue } = p;
  const hasObject = issue.element_ids.length > 0;
  return (
    <div
      id={domId(p.rowKey)}
      role="listitem"
      tabIndex={p.tabStop ? 0 : -1}
      data-triage={p.rowKey}
      aria-current={p.active ? "true" : undefined}
      className={cx(s.item, s[`sev_${issue.severity}`], p.active && s.itemActive)}
      onFocus={(e) => {
        if (e.target === e.currentTarget) p.onFocus();
      }}
      onClick={(e) => p.onClick(e.target)}
    >
      <Icon name={SEVERITY_ICON[issue.severity]} size={15} className={s.icon} />
      <div className={s.body}>
        <span className={s.message}>{issue.message}</span>
        <Reveal open={p.active}>
          {p.editing ? (
            <AsideForm issue={issue} editing={p.editing} busy={p.busy} objectName={hasObject ? p.objectName : null} onEdit={p.onEdit} onCommit={p.onCommit} onCancel={p.onCancel} />
          ) : (
            <div className={s.actions}>
              <Button size="sm" icon="fit" onClick={p.onShow} disabled={!p.canShow}>
                Show
              </Button>
              {issue.location ? (
                <Button size="sm" icon="walk" onClick={p.onWalk} disabled={!p.canShow}>
                  Walk to it
                </Button>
              ) : null}
              <Button size="sm" icon="aside" onClick={p.onStartAside}>
                Set aside
              </Button>
            </div>
          )}
        </Reveal>
      </div>
    </div>
  );
}

function AsideForm({
  issue,
  editing,
  busy,
  objectName,
  onEdit,
  onCommit,
  onCancel,
}: {
  issue: Issue;
  editing: Editing;
  busy: boolean;
  objectName: string | null;
  onEdit: (e: Editing) => void;
  onCommit: (e: Editing) => void;
  onCancel: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  // S or the button opens the form: the note field takes focus before the
  // next key arrives, so fast typing never reaches the global shortcuts.
  useLayoutEffect(() => {
    input.current?.focus();
    if (document.activeElement === input.current) return;
    const id = requestAnimationFrame(() => input.current?.focus());
    return () => cancelAnimationFrame(id);
  }, []);
  // An empty note shakes the field once (MOTION.md, Inspector row) and keeps focus.
  useEffect(() => {
    if (editing.invalid === 0) return;
    input.current?.focus();
    play(input.current, [0, -4, 3, -2, 1, 0].map((x) => ({ transform: `translateX(${x}px)` })), "panel");
  }, [editing.invalid]);
  const scopes = SCOPES.filter((o) => o.value !== "element" || objectName !== null);
  const check = checkLabel(issue.code);
  const hint =
    editing.scope === "issue" ? "Only this item." : editing.scope === "check" ? `Every item of "${check}", now and later.` : `"${check}" on ${objectName ?? "this object"} only.`;
  return (
    <div className={s.form}>
      <label className={s.formLabel} htmlFor={`note-${domId(issue.id)}`}>
        Why set it aside?
      </label>
      <input
        ref={input}
        id={`note-${domId(issue.id)}`}
        className={cx(s.noteInput, editing.invalid > 0 && editing.note.trim() === "" && s.noteInvalid)}
        type="text"
        value={editing.note}
        placeholder="For example: the owner accepts it"
        spellCheck={false}
        autoComplete="off"
        onChange={(e) => onEdit({ ...editing, note: e.target.value })}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") {
            e.preventDefault();
            onCommit(editing);
          } else if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          }
        }}
      />
      <div className={s.scopes} role="radiogroup" aria-label="Set aside">
        {scopes.map((o) => (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={editing.scope === o.value}
            className={cx(s.scope, editing.scope === o.value && s.scopeOn)}
            onClick={() => onEdit({ ...editing, scope: o.value })}
          >
            {o.label}
          </button>
        ))}
      </div>
      <p className={s.scopeHint}>{hint}</p>
      <div className={s.actions}>
        <Button size="sm" variant="primary" icon="aside" disabled={busy || editing.note.trim() === ""} onClick={() => onCommit(editing)}>
          Set aside
        </Button>
        <Button size="sm" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </div>
  );
}
