// The History flyout beside undo and redo (DECISIONS D38): every step of the
// shared undo history with who made it and when, newest first, the undone
// steps greyed below them. Hovering a row tints its elements, clicking shows
// them, "Undo to here" takes back several steps as one revision. Pure parts
// in history.ts.
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type RefObject,
} from "react";
import type { HistoryList } from "../contract/bindings";
import { ipc, toIpcError } from "../contract/ipc";
import { initials, peerVar } from "../live/format";
import { useLive } from "../live/liveStore";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { IconButton, cx } from "../ui/controls";
import { isModalOpen } from "../ui/Dialog";
import { Icon } from "../ui/icons";
import type { PresenceStage } from "../ui/motion";
import { Presence } from "../ui/motionDom";
import { flip } from "../ui/motionWaapi";
import { useListPresence } from "../ui/useListPresence";
import {
  EVERYONE,
  RELATIVE_TICK_MS,
  filterRows,
  historyPeople,
  historyRows,
  liveIds,
  relativeTime,
  stepDetail,
  type HistoryRow,
} from "./history";
import { levelOfElement } from "./levels";
import { useShell } from "./shellStore";
import s from "./HistoryPanel.module.css";

/** Rows that stagger in when the panel opens (docs/MOTION.md rule 8). */
const STAGGERED = 8;

/** The History button for the top bar, with its flyout. */
export function HistoryButton() {
  const [open, setOpen] = useState(false);
  const hasDoc = useApp((st) => st.doc !== null);
  const anchor = useRef<HTMLSpanElement>(null);
  const close = useCallback((refocus: boolean) => {
    setOpen(false);
    if (refocus) anchor.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, []);
  // Closing the project closes the panel.
  useEffect(() => {
    if (!hasDoc) setOpen(false);
  }, [hasDoc]);
  return (
    <span className={s.anchor} ref={anchor}>
      <IconButton
        icon="clock"
        tone="chrome"
        label="History"
        tip="History: who changed what, and when"
        className={cx(s.button, open && s.buttonOn)}
        aria-expanded={open}
        aria-haspopup="dialog"
        disabled={!hasDoc}
        onClick={() => setOpen((v) => !v)}
      />
      <Presence open={open} exit="base">
        {(stage) => <HistoryPanel stage={stage} anchor={anchor} onClose={close} />}
      </Presence>
    </span>
  );
}

function HistoryPanel({
  stage,
  anchor,
  onClose,
}: {
  stage: PresenceStage;
  anchor: RefObject<HTMLSpanElement | null>;
  onClose: (refocus: boolean) => void;
}) {
  const revision = useApp((st) => st.doc?.revision ?? null);
  const elements = useApp((st) => st.doc?.project.elements);
  const status = useLive((st) => st.status);
  const me = useLive((st) => st.profile?.name ?? "");
  const people = status.participants.length;
  const [list, setList] = useState<HistoryList | null>(null);
  // The revision the list was read at: "Undo to here" counts its steps there.
  const [listRevision, setListRevision] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [person, setPerson] = useState(EVERYONE);
  const [now, setNow] = useState(() => new Date());
  const [active, setActive] = useState<string | null>(null);
  const panel = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const exiting = stage === "exit";

  // The list follows the document: every commit, undo and redo, and a new
  // name in the session.
  useEffect(() => {
    let alive = true;
    ipc
      .historyList()
      .then((next) => {
        if (!alive) return;
        setList(next);
        setListRevision(revision);
        setError(null);
      })
      .catch((e) => alive && setError(toIpcError(e).message));
    return () => {
      alive = false;
    };
  }, [revision, people]);

  // "2 min ago" moves on while the panel is open.
  useEffect(() => {
    const t = window.setInterval(() => setNow(new Date()), RELATIVE_TICK_MS);
    return () => window.clearInterval(t);
  }, []);

  // Focus moves in, so arrow keys work at once; the tint never outlives the panel.
  useEffect(() => {
    panel.current?.focus({ preventScroll: true });
    return () => useApp.getState().setHistoryTint(null);
  }, []);

  // Esc and a press outside close it, unless a dialog (Undo anyway?) has them.
  useEffect(() => {
    if (exiting) return;
    const down = (e: PointerEvent) => {
      if (isModalOpen()) return;
      const root = anchor.current;
      if (root && !root.contains(e.target as Node)) onClose(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || isModalOpen()) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      onClose(true);
    };
    window.addEventListener("pointerdown", down, true);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("pointerdown", down, true);
      window.removeEventListener("keydown", key, true);
    };
  }, [anchor, onClose, exiting]);

  const rows = useMemo(() => (list ? historyRows(list, status) : []), [list, status]);
  const chips = useMemo(() => historyPeople(rows), [rows]);
  // A filter on someone who no longer has a step falls back to everyone.
  const filter = person !== EVERYONE && !chips.some((c) => c.key === person) ? EVERYONE : person;
  const shown = useMemo(() => filterRows(rows, filter), [rows, filter]);
  const present = useListPresence(shown, (r) => r.key, "base");
  const done = shown.filter((r) => r.stack === "undo").length;
  const undone = shown.length - done;
  const firstUndone = present.findIndex((r) => r.item.stack === "redo" && !r.leaving);
  // The rows of the first load stagger in; a row that comes later rises alone.
  const intro = useRef<Set<string> | null>(null);
  if (intro.current === null && list !== null) intro.current = new Set(shown.slice(0, STAGGERED).map((r) => r.key));

  // Rows that move (an undo sends them below the divider) slide there.
  const rects = useRef(new Map<string, DOMRect>());
  const measure = useCallback(() => {
    const el = listRef.current;
    if (!el) return;
    rects.current = new Map([...el.querySelectorAll<HTMLElement>("[data-flip]")].map((n) => [n.dataset.flip ?? "", n.getBoundingClientRect()]));
  }, []);
  useLayoutEffect(() => {
    const el = listRef.current;
    if (!el) return;
    rects.current = flip(el.querySelectorAll<HTMLElement>("[data-flip]"), rects.current, (n) => n.dataset.flip, "base");
  }, [present, firstUndone]);

  const tabStop = active && present.some((r) => r.key === active && !r.leaving) ? active : (present.find((r) => !r.leaving)?.key ?? null);

  const tint = (row: HistoryRow | null) => {
    const app = useApp.getState();
    const ids = row && elements ? liveIds(row.step, elements) : [];
    app.setHistoryTint(ids.length > 0 && row ? { ids, color: row.color } : null);
  };

  const show = (row: HistoryRow) => {
    const app = useApp.getState();
    const all = app.doc?.project.elements ?? [];
    const ids = liveIds(row.step, all);
    if (ids.length === 0) {
      app.toast("info", row.stack === "redo" ? "This step is undone: what it made is not on the plan now." : "Nothing this step touched is on the plan now.");
      return;
    }
    const byId = new Map(all.map((e) => [e.id, e]));
    const levels = ids.map((id) => byId.get(id)).map((e) => (e ? levelOfElement(e, byId) : null));
    if (!levels.includes(app.activeLevelId)) {
      const level = levels.find((l) => l !== null);
      if (level) app.setActiveLevel(level);
    }
    useApp.getState().select(ids);
    bus.emit("focus_elements", ids);
  };

  // In a live session the engine asks first when any of the steps is
  // someone else's (`other_author`, through `undoConfirm`).
  const stepTo = (row: HistoryRow) => {
    const app = useApp.getState();
    const at = listRevision ?? undefined;
    void (row.stack === "undo" ? app.undoSteps(row.steps, at) : app.redoSteps(row.steps, at));
  };

  const focusRow = (key: string | null) => {
    if (!key) return;
    setActive(key);
    requestAnimationFrame(() => {
      const el = listRef.current?.querySelector<HTMLElement>(`[data-row="${CSS.escape(key)}"]`);
      el?.focus({ preventScroll: true });
      el?.scrollIntoView({ block: "nearest" });
    });
  };

  const onListKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const keys = present.filter((r) => !r.leaving).map((r) => r.key);
    if (keys.length === 0) return;
    const at = tabStop ? keys.indexOf(tabStop) : -1;
    const focusedRow = (e.target as HTMLElement).closest<HTMLElement>("[data-row-item]") !== null;
    let next: string | null = null;
    if (e.key === "ArrowDown") next = keys[focusedRow ? Math.min(at + 1, keys.length - 1) : Math.max(at, 0)];
    else if (e.key === "ArrowUp") next = keys[focusedRow ? Math.max(at - 1, 0) : Math.max(at, 0)];
    else if (e.key === "Home") next = keys[0];
    else if (e.key === "End") next = keys[keys.length - 1];
    if (next === null) return;
    e.preventDefault();
    focusRow(next);
  };

  const openVersions = () => {
    onClose(false);
    useShell.getState().open("versions");
  };

  return (
    <div
      ref={panel}
      className={s.panel}
      data-stage={stage}
      role="dialog"
      aria-label="History"
      tabIndex={-1}
      onKeyDown={onListKey}
      onPointerLeave={() => tint(null)}
    >
      <header className={s.head}>
        <h2 className={s.title}>History</h2>
        <span className={s.count}>
          {list === null && !error ? "Loading" : `${done} ${done === 1 ? "step" : "steps"}${undone > 0 ? `, ${undone} undone` : ""}`}
        </span>
        <IconButton icon="close" label="Close history" size={15} className={s.close} onClick={() => onClose(true)} />
      </header>

      {chips.length > 1 ? (
        <div className={s.chips} role="group" aria-label="Show the steps of">
          <button type="button" className={cx(s.chip, filter === EVERYONE && s.chipOn)} aria-pressed={filter === EVERYONE} onClick={() => setPerson(EVERYONE)}>
            Everyone
          </button>
          {chips.map((c) => (
            <button
              key={c.key}
              type="button"
              className={cx(s.chip, filter === c.key && s.chipOn)}
              aria-pressed={filter === c.key}
              onClick={() => setPerson(c.key)}
              style={dotStyle(c.color)}
            >
              <i className={s.chipDot} aria-hidden />
              {c.name}
              <span className={s.chipCount}>{c.count}</span>
            </button>
          ))}
        </div>
      ) : null}

      <div ref={listRef} className={s.list} role="list" aria-label="Steps, newest first" onScroll={measure}>
        {error ? <p className={s.empty}>History is not available: {error}</p> : null}
        {!error && list !== null && rows.length === 0 ? (
          <p className={s.empty}>No steps yet. Each change to the plan shows here, with who made it.</p>
        ) : null}
        {present.flatMap((p, i) => {
          const row = p.item;
          const redo = row.stack === "redo";
          const time = relativeTime(row.step.at, now);
          const detail = stepDetail(row.step);
          const delay = intro.current?.has(p.key) ? Math.min(i, STAGGERED - 1) : 0;
          const isStop = p.key === tabStop;
          // One divider that slides to the first undone step, and folds
          // away under the last row when nothing is undone.
          const divider =
            i === (firstUndone >= 0 ? firstUndone : present.length - 1) ? (
              <div
                key="divider"
                data-flip={firstUndone >= 0 ? "divider" : undefined}
                className={cx(s.divider, firstUndone < 0 && s.dividerAway)}
                role="presentation"
              >
                <span>Undone</span>
              </div>
            ) : null;
          const item = (
            <div key={p.key} className={s.slot} data-flip={p.key}>
              <div
                role="listitem"
                data-row-item
                className={cx(s.row, redo && s.rowUndone, p.leaving && s.rowLeave)}
                style={{ "--i": delay, ...dotStyle(row.color) } as CSSProperties}
              >
                <div className={s.rowInner}>
                  <button
                    type="button"
                    className={s.main}
                    data-row={p.key}
                    tabIndex={isStop ? 0 : -1}
                    disabled={p.leaving}
                    aria-label={`${row.step.label}, by ${row.name}${time ? `, ${time}` : ""}${detail ? `. ${detail}` : ""}${redo ? ", undone" : ""}. Show on the plan`}
                    onPointerEnter={() => tint(row)}
                    onPointerLeave={() => tint(null)}
                    onFocus={(e) => {
                      setActive(p.key);
                      if (e.currentTarget.matches(":focus-visible")) tint(row);
                    }}
                    onBlur={() => tint(null)}
                    onClick={() => show(row)}
                  >
                    <span className={s.dot} aria-hidden>
                      {initials(row.mine && me ? me : row.name)}
                    </span>
                    <span className={s.text}>
                      <span className={s.label}>{row.step.label || "Change"}</span>
                      <span className={s.meta}>
                        <span className={s.who}>{row.name}</span>
                        {time ? <span className={s.when}>{time}</span> : null}
                        {row.step.origin === "ai" ? (
                          <span className={s.ai} data-tip="Made by the copilot or an MCP client" data-tip-side="right">
                            AI
                          </span>
                        ) : null}
                        {detail ? <span className={s.detail}>{detail}</span> : null}
                      </span>
                    </span>
                  </button>
                  <button
                    type="button"
                    className={s.to}
                    tabIndex={isStop ? 0 : -1}
                    disabled={p.leaving}
                    data-tip={toTip(row)}
                    data-tip-side="left"
                    onFocus={() => setActive(p.key)}
                    onClick={() => stepTo(row)}
                  >
                    <Icon name={redo ? "redo" : "undo"} size={13} />
                    {redo ? "Redo to here" : "Undo to here"}
                  </button>
                </div>
              </div>
            </div>
          );
          if (!divider) return [item];
          return firstUndone >= 0 ? [divider, item] : [item, divider];
        })}
      </div>

      <footer className={s.foot}>
        The last 200 steps, kept while the project is open.{" "}
        <button type="button" className={s.link} onClick={openVersions}>
          Versions
        </button>{" "}
        keep the lasting record.
      </footer>
    </div>
  );
}

function dotStyle(color: number | null): CSSProperties {
  return { "--who": color === null ? "var(--accent)" : peerVar(color) } as CSSProperties;
}

function toTip(row: HistoryRow): string {
  const n = row.steps;
  if (row.stack === "undo") return n === 1 ? "Undo this step" : `Undo the last ${n} steps, back to before this one`;
  return n === 1 ? "Redo this step" : `Redo the next ${n} steps, up to this one`;
}
