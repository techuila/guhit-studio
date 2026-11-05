// The sheet list in set order (lane L4). A row drags to a new place: it
// tracks the pointer 1:1 while the rows between slide aside (`--dur-base`),
// and on drop it settles from where it was let go into its new slot (FLIP).
// Arrow keys with Alt move the selected sheet one place.
import { useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import type { Sheet } from "../../contract/bindings";
import { cx } from "../../ui/controls";
import { Icon } from "../../ui/icons";
import { play } from "../../ui/motionWaapi";
import { dropIndex, shiftFor } from "./sheetModel";
import s from "./sheets.module.css";

const ROW_STEP = 44; // row height plus its gap, px

interface Drag {
  id: string;
  from: number;
  startY: number;
  dy: number;
  moved: boolean;
}

export function SheetList({
  sheets,
  selected,
  onSelect,
  onMove,
}: {
  sheets: Sheet[];
  selected: string | null;
  onSelect: (id: string) => void;
  onMove: (id: string, index: number) => void;
}) {
  const [drag, setDragState] = useState<Drag | null>(null);
  // The live drag, read by the pointer handlers: a fast drag can move and
  // drop before React renders the state.
  const dragRef = useRef<Drag | null>(null);
  const setDrag = (d: Drag | null) => {
    dragRef.current = d;
    setDragState(d);
  };
  const settle = useRef<{ id: string; offset: number } | null>(null);
  const rows = useRef(new Map<string, HTMLDivElement>());

  // After a drop lands in the set, the moved row slides from where it was
  // let go into its new slot.
  useLayoutEffect(() => {
    const pending = settle.current;
    if (!pending) return;
    settle.current = null;
    const el = rows.current.get(pending.id);
    if (el && Math.abs(pending.offset) > 0.5) {
      play(el, [{ transform: `translateY(${pending.offset}px)` }, { transform: "none" }], "base", "out");
    }
  }, [sheets]);

  const target = drag && drag.moved ? dropIndex(drag.from, drag.dy, ROW_STEP, sheets.length) : null;

  const down = (e: ReactPointerEvent<HTMLDivElement>, id: string, index: number) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    setDrag({ id, from: index, startY: e.clientY, dy: 0, moved: false });
  };
  const move = (e: ReactPointerEvent<HTMLDivElement>) => {
    const cur = dragRef.current;
    if (!cur) return;
    const dy = e.clientY - cur.startY;
    setDrag({ ...cur, dy, moved: cur.moved || Math.abs(dy) > 3 });
  };
  const up = (e: ReactPointerEvent<HTMLDivElement>) => {
    const cur = dragRef.current;
    if (!cur) return;
    const d = { ...cur, dy: e.clientY - cur.startY, moved: cur.moved || Math.abs(e.clientY - cur.startY) > 3 };
    setDrag(null);
    if (!d.moved) {
      onSelect(d.id);
      return;
    }
    const to = dropIndex(d.from, d.dy, ROW_STEP, sheets.length);
    // Where it was let go, relative to its new slot.
    const offset = d.from * ROW_STEP + d.dy - to * ROW_STEP;
    if (to === d.from) {
      const el = rows.current.get(d.id);
      if (el) play(el, [{ transform: `translateY(${d.dy}px)` }, { transform: "none" }], "base", "out");
      return;
    }
    settle.current = { id: d.id, offset };
    onSelect(d.id);
    onMove(d.id, to);
  };

  return (
    <div role="listbox" aria-label="Sheets in set order">
      {sheets.map((sheet, i) => {
        const dragging = drag?.moved && drag.id === sheet.id;
        const shift = drag && target !== null && !dragging ? shiftFor(i, drag.from, target) * ROW_STEP : 0;
        const ty = dragging ? drag.dy : shift;
        return (
          <div
            key={sheet.id}
            ref={(el) => {
              if (el) rows.current.set(sheet.id, el);
              else rows.current.delete(sheet.id);
            }}
            role="option"
            aria-selected={sheet.id === selected}
            tabIndex={0}
            className={cx(s.row, sheet.id === selected && s.rowOn, dragging && s.rowDragging)}
            style={{ marginBottom: 4, transform: ty ? `translateY(${ty}px)` : undefined }}
            onPointerDown={(e) => down(e, sheet.id, i)}
            onPointerMove={move}
            onPointerUp={up}
            onPointerCancel={() => setDrag(null)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelect(sheet.id);
              } else if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
                e.preventDefault();
                const to = Math.max(0, Math.min(sheets.length - 1, i + (e.key === "ArrowUp" ? -1 : 1)));
                if (to !== i) {
                  settle.current = { id: sheet.id, offset: (i - to) * ROW_STEP };
                  onMove(sheet.id, to);
                }
              }
            }}
          >
            <span className={s.grip} aria-hidden>
              <Icon name="more" size={14} />
            </span>
            <span className={s.num}>{sheet.number || "-"}</span>
            <span className={s.rowTitle} title={sheet.title}>
              {sheet.title || "Untitled sheet"}
            </span>
            <span className={s.paperChip}>{sheet.paper.toUpperCase()}</span>
          </div>
        );
      })}
    </div>
  );
}
