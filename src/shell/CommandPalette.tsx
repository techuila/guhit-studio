import { useEffect, useMemo, useRef, useState } from "react";
import { Dialog } from "../ui/Dialog";
import { cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import type { PresenceStage } from "../ui/motion";
import { fuzzyScore, paletteActions, type PaletteAction } from "./actions";
import s from "./overlays.module.css";

export function CommandPalette({ onClose, stage }: { onClose: () => void; stage?: PresenceStage }) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  // Snapshot the actions once per opening: titles depend on the current state.
  const actions = useMemo(() => paletteActions(), []);

  const results = useMemo(() => {
    const q = query.trim();
    if (q === "") return actions;
    const scored: Array<{ action: PaletteAction; score: number }> = [];
    for (const action of actions) {
      const title = fuzzyScore(q, action.title);
      const extra = fuzzyScore(q, `${action.keywords ?? ""} ${action.group}`);
      const score = Math.max(title ?? -Infinity, extra === null ? -Infinity : extra - 200);
      if (score > -Infinity) scored.push({ action, score });
    }
    return scored.sort((a, b) => b.score - a.score).map((r) => r.action);
  }, [actions, query]);

  useEffect(() => setActive(0), [query]);

  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const run = (action: PaletteAction | undefined) => {
    if (!action || action.disabled) return;
    onClose();
    // Let the dialog unmount and focus return before the action opens something else.
    window.setTimeout(action.run, 0);
  };

  const grouped = query.trim() === "";

  return (
    <Dialog title="Command palette" onClose={onClose} placement="top" width={560} bare stage={stage}>
      <label className={s.paletteInput}>
        <Icon name="search" size={17} />
        <input
          data-autofocus
          type="text"
          value={query}
          placeholder="What do you want to do?"
          aria-label="Search commands"
          spellCheck={false}
          autoComplete="off"
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
              e.preventDefault();
              onClose();
            } else if (e.key === "ArrowDown") {
              e.preventDefault();
              setActive((i) => Math.min(results.length - 1, i + 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((i) => Math.max(0, i - 1));
            } else if (e.key === "Enter") {
              e.preventDefault();
              run(results[active]);
            }
          }}
        />
        <kbd>Esc</kbd>
      </label>
      <div ref={list} className={s.paletteList} role="listbox" aria-label="Commands">
        {results.length === 0 ? (
          <div className={s.paletteEmpty}>No command matches "{query}". Try "wall", "export" or "3D".</div>
        ) : (
          results.map((action, i) => (
            <div key={action.id}>
              {grouped && (i === 0 || results[i - 1].group !== action.group) ? <div className={s.paletteGroup}>{action.group}</div> : null}
              <button
                type="button"
                role="option"
                aria-selected={i === active}
                aria-disabled={action.disabled}
                data-index={i}
                tabIndex={-1}
                className={cx(s.paletteItem, i === active && s.paletteItemActive, action.disabled && s.paletteItemDisabled)}
                onMouseMove={() => setActive(i)}
                onClick={() => run(action)}
              >
                <Icon name={action.icon} size={17} />
                <span>{action.title}</span>
                {action.shortcut ? <kbd>{action.shortcut}</kbd> : null}
              </button>
            </div>
          ))
        )}
      </div>
    </Dialog>
  );
}
