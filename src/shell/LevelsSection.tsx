// Levels in the project inspector: pick the level to work on, rename a level
// in place, add a level above, delete a level after an inline confirmation
// that says what goes with it (never window.confirm), and edit the active
// level's height and elevation. Every change is one undo step.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { Command, DocState, Level } from "../contract/bindings";
import { useApp } from "../state/store";
import { Button, Field, IconButton, NumberField, Section, cx } from "../ui/controls";
import { useSlidingIndicator } from "../ui/motionDom";
import { useListPresence } from "../ui/useListPresence";
import { addLevelAbove, deleteLevel } from "./actions";
import { deleteLevelQuestion, elementsOnLevel, elevationLabel, levelsTopDown } from "./levels";
import { useReveal } from "./PipeSections";
import { useSection, useShell } from "./shellStore";
import s from "./Inspector.module.css";

const dispatch = (command: Command) => void useApp.getState().dispatch(command);
const levelKey = (l: Level) => l.id;

/** Height reveal (MOTION.md rule 5); collapsed content is inert. */
function Reveal({ open, children }: { open: boolean; children: ReactNode }) {
  return (
    <div className={cx(s.levelReveal, open && s.levelRevealOpen)} inert={!open}>
      <div className={s.levelRevealInner}>{children}</div>
    </div>
  );
}

function RenameField({ level, onDone }: { level: Level; onDone: () => void }) {
  const [value, setValue] = useState(level.name);
  const input = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  useLayoutEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);
  const commit = () => {
    if (done.current) return;
    done.current = true;
    const name = value.trim();
    if (name && name !== level.name) dispatch({ type: "update_level", level: { ...level, name } });
    onDone();
  };
  return (
    <input
      ref={input}
      className={s.levelRename}
      value={value}
      aria-label={`New name for ${level.name}`}
      spellCheck={false}
      onChange={(e) => setValue(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") commit();
        else if (e.key === "Escape") {
          done.current = true;
          onDone();
        }
      }}
    />
  );
}

export function LevelsSection({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("level", true);
  const sectionRef = useRef<HTMLDivElement>(null);
  const flash = useReveal("level", sectionRef);
  const activeLevelId = useApp((st) => st.activeLevelId);
  const levels = doc.project.levels;
  const active = levels.find((l) => l.id === activeLevelId) ?? levels[0];
  const ordered = useMemo(() => levelsTopDown(levels), [levels]);
  const rows = useListPresence(ordered, levelKey);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const last = levels.length <= 1;
  const unit = doc.project.settings.display_unit;

  // The palette's "Delete this level" opens the confirmation here.
  const deleteRequest = useShell((st) => st.levelDeleteRequest);
  const seenRequest = useRef(deleteRequest?.token ?? 0);
  useEffect(() => {
    if (!deleteRequest || deleteRequest.token === seenRequest.current) return;
    seenRequest.current = deleteRequest.token;
    if (!last) setConfirming(deleteRequest.levelId);
  }, [deleteRequest, last]);

  // A level that went away (undo, the MCP server) closes its editors.
  useEffect(() => {
    if (confirming && !levels.some((l) => l.id === confirming)) setConfirming(null);
    if (renaming && !levels.some((l) => l.id === renaming)) setRenaming(null);
  }, [levels, confirming, renaming]);

  // One highlight slides to the level being worked on.
  const listRef = useRef<HTMLUListElement>(null);
  const activeRef = useRef<HTMLLIElement>(null);
  const pill = useSlidingIndicator(listRef, activeRef, [active?.id, ordered.length, confirming, renaming]);

  // The confirmation takes focus on Cancel, the safe answer.
  useEffect(() => {
    if (!confirming) return;
    const id = requestAnimationFrame(() => sectionRef.current?.querySelector<HTMLButtonElement>(`[data-confirm-cancel="${confirming}"]`)?.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(id);
  }, [confirming]);

  if (!active) return null;
  const setLevel = (patch: Partial<Level>) => dispatch({ type: "update_level", level: { ...active, ...patch } });

  return (
    <div ref={sectionRef} className={cx(s.reveal, flash && s.revealFlash)}>
      <Section title="Levels" icon="level" count={levels.length > 1 ? levels.length : undefined} open={open} onToggle={toggle}>
        <ul ref={listRef} className={s.levelList} aria-label="Levels, top floor first">
          {pill.visible ? <span aria-hidden className={cx(s.levelPill, pill.instant && s.instant)} style={pill.style} /> : null}
          {rows.map(({ key, item: level, entering, leaving }) => {
            const isActive = level.id === active.id;
            // Counted for every row, so a closing confirmation keeps its words.
            const count = elementsOnLevel(doc.project.elements, level.id);
            return (
              <li
                key={key}
                ref={isActive && !leaving ? activeRef : undefined}
                className={cx(s.levelRow, entering && s.levelRowIn, leaving && s.levelRowOut)}
                inert={leaving}
              >
                <div className={s.levelRowInner}>
                  <div className={s.levelLine}>
                    {renaming === level.id ? (
                      <RenameField level={level} onDone={() => setRenaming(null)} />
                    ) : (
                      <button
                        type="button"
                        className={cx(s.levelPick, isActive && s.levelPickOn)}
                        aria-pressed={isActive}
                        data-tip={isActive ? "Working on this level. Double-click to rename" : "Work on this level"}
                        data-tip-side="top-start"
                        onClick={() => useApp.getState().setActiveLevel(level.id)}
                        onDoubleClick={() => setRenaming(level.id)}
                      >
                        <span className={s.levelName}>{level.name}</span>
                        <span className={s.levelElev}>{elevationLabel(level.elevation_mm)}</span>
                      </button>
                    )}
                    <IconButton icon="pencil" label={`Rename ${level.name}`} tip="Rename" tipSide="top-end" size={14} className={s.levelButton} onClick={() => setRenaming(level.id)} />
                    <IconButton
                      icon="trash"
                      label={`Delete ${level.name}`}
                      tip={last ? "A project keeps at least one level" : "Delete"}
                      tipSide="top-end"
                      size={14}
                      disabled={last}
                      className={s.levelButton}
                      onClick={() => setConfirming((c) => (c === level.id ? null : level.id))}
                    />
                  </div>
                  <Reveal open={confirming === level.id}>
                    <div className={s.levelConfirm} role="alertdialog" aria-label={`Delete ${level.name}`}>
                      <p>{deleteLevelQuestion(level, count)} Undo brings it back.</p>
                      <div className={s.actionsRow}>
                        <Button
                          size="sm"
                          variant="danger"
                          icon="trash"
                          disabled={busy}
                          onClick={async () => {
                            setBusy(true);
                            await deleteLevel(level.id);
                            setBusy(false);
                            setConfirming(null);
                          }}
                        >
                          Delete level
                        </Button>
                        <Button
                          data-confirm-cancel={level.id}
                          size="sm"
                          variant="ghost"
                          onClick={() => setConfirming(null)}
                          onKeyDown={(e) => {
                            if (e.key === "Escape") {
                              e.stopPropagation();
                              setConfirming(null);
                            }
                          }}
                        >
                          Cancel
                        </Button>
                      </div>
                    </div>
                  </Reveal>
                </div>
              </li>
            );
          })}
        </ul>
        <div className={s.actionsRow}>
          <Button size="sm" icon="plus" onClick={() => void addLevelAbove()} data-tip="Stacks a new level on the top floor and works on it" data-tip-side="top-start">
            Add level above
          </Button>
        </div>
        <div className={s.levelFields}>
          <Field label="Floor height" hint="Floor to floor. Walls without their own height use this.">
            <NumberField label={`Floor to floor height of ${active.name}`} kind="length" unit={unit} min={1800} max={12000} step={50} value={active.height_mm} onCommit={(v) => setLevel({ height_mm: v })} />
          </Field>
          <Field label="Elevation" hint="Height of this floor above the ground reference">
            <NumberField label={`Elevation of ${active.name}`} kind="length" unit={unit} step={50} value={active.elevation_mm} onCommit={(v) => setLevel({ elevation_mm: v })} />
          </Field>
        </div>
      </Section>
    </div>
  );
}
