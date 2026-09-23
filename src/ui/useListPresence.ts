// Enter and exit state for the rows of a list that changes under the user,
// such as the review list after an edit (docs/MOTION.md rule 6: list rows
// never pop out of existence). A row that disappears from `items` stays for
// one exit duration marked `leaving`, at its old place. A row that arrives
// after the first render is marked `entering`; rows present on mount are not,
// so opening a panel does not replay every row.
import { useEffect, useRef, useState } from "react";
import { dur, type DurKey } from "./motion";

export interface PresentRow<T> {
  key: string;
  item: T;
  entering: boolean;
  leaving: boolean;
}

/** Current rows in their order, with each leaving row kept after the row it followed before. */
export function mergeRows<T>(prev: Array<PresentRow<T>>, items: T[], keyOf: (item: T) => string, mounted: boolean): Array<PresentRow<T>> {
  const prevKeys = new Set(prev.map((r) => r.key));
  const nextKeys = new Set(items.map(keyOf));
  const out: Array<PresentRow<T>> = items.map((item) => {
    const key = keyOf(item);
    const before = prev.find((r) => r.key === key);
    return { key, item, entering: mounted && !prevKeys.has(key) ? true : (before?.entering ?? false) && !before?.leaving, leaving: false };
  });
  prev.forEach((row, i) => {
    if (nextKeys.has(row.key)) return;
    const leaving = { ...row, entering: false, leaving: true };
    // After the nearest earlier row that is still shown, else at the top.
    for (let j = i - 1; j >= 0; j--) {
      const at = out.findIndex((r) => r.key === prev[j].key);
      if (at >= 0) {
        out.splice(at + 1, 0, leaving);
        return;
      }
    }
    out.unshift(leaving);
  });
  return out;
}

export function useListPresence<T>(items: T[], keyOf: (item: T) => string, exit: DurKey = "base"): Array<PresentRow<T>> {
  const [rows, setRows] = useState<Array<PresentRow<T>>>(() => mergeRows([], items, keyOf, false));
  const mounted = useRef(false);
  const timers = useRef(new Map<string, number>());
  const keyRef = useRef(keyOf);
  keyRef.current = keyOf;

  useEffect(() => {
    if (!mounted.current) {
      mounted.current = true;
      return;
    }
    setRows((prev) => {
      // Nothing moved: keep the same array so React skips the render.
      if (prev.length === items.length && prev.every((r, i) => !r.leaving && r.item === items[i])) return prev;
      const next = mergeRows(prev, items, keyRef.current, true);
      for (const row of next) {
        if (!row.leaving) {
          // Came back before its exit finished.
          const t = timers.current.get(row.key);
          if (t !== undefined) {
            window.clearTimeout(t);
            timers.current.delete(row.key);
          }
          continue;
        }
        if (timers.current.has(row.key)) continue;
        const t = window.setTimeout(() => {
          timers.current.delete(row.key);
          setRows((cur) => cur.filter((r) => !(r.key === row.key && r.leaving)));
        }, Math.round(dur(exit) * 0.7) + 30);
        timers.current.set(row.key, t);
      }
      return next;
    });
  }, [items, exit]);

  useEffect(() => {
    const map = timers.current;
    return () => map.forEach((t) => window.clearTimeout(t));
  }, []);

  return rows;
}
