// The stair tool's flyout on the tool rail (lane L2): the shape the next
// stair takes (straight, L or U), each with a small plan of it, and for an L
// or U the way it turns. The same choices as L and T over the plan. The
// turn row opens by grid rows when a turning shape is picked; the plans
// mirror when the turn flips.
import type { StairShape, Turn } from "../contract/bindings";
import { cx } from "../ui/controls";
import c from "../shell/chrome.module.css";
import { STAIR_SHAPES, setStairShape, setStairTurn, useStairTool } from "./stairTool";
import css from "./StairToolOptions.module.css";

const TURNS: Array<{ value: Turn; label: string; hint: string }> = [
  { value: "left", label: "Left", hint: "going up" },
  { value: "right", label: "Right", hint: "going up" },
];

/** A stair in plan, climbing up the glyph, turning left (mirrored for right). */
const GLYPH: Record<StairShape, string> = {
  straight: "M9 3h6v18H9zM9 7h6M9 11h6M9 15h6",
  l: "M13 10h6v11h-6zM13 4h6v6h-6zM4 4h9v6H4zM13 14h6M13 18h6M7 4v6M10 4v6",
  u: "M14 10h6v11h-6zM4 4h16v6H4zM4 10h6v11H4zM14 14h6M14 18h6M4 14h6M4 18h6",
};

export function StairGlyph({ shape, turn }: { shape: StairShape; turn: Turn }) {
  return (
    <svg className={cx(css.glyph, turn === "right" && shape !== "straight" && css.glyphRight)} viewBox="0 0 24 24" width={22} height={22} aria-hidden="true">
      <path d={GLYPH[shape]} />
    </svg>
  );
}

/** The flyout's body. `onPicked` closes it once the choice is complete. */
export function StairToolOptions({ onPicked }: { onPicked?: () => void }) {
  const shape = useStairTool((st) => st.shape);
  const turn = useStairTool((st) => st.turn);
  const turning = shape !== "straight";
  return (
    <div className={css.root}>
      <div className={c.flyoutTitle}>Stair shape</div>
      {STAIR_SHAPES.map((o) => {
        const on = shape === o.value;
        return (
          <button
            key={o.value}
            type="button"
            aria-pressed={on}
            className={cx(c.flyoutItem, css.item, on && c.flyoutItemOn)}
            onClick={() => {
              setStairShape(o.value);
              // A straight stair needs nothing more; an L or U still picks its turn.
              if (o.value === "straight") onPicked?.();
            }}
          >
            <StairGlyph shape={o.value} turn={turn} />
            <span className={css.label}>{o.label}</span>
            <small>{o.hint}</small>
          </button>
        );
      })}
      <div className={cx(css.reveal, turning && css.revealOpen)} inert={!turning} aria-hidden={!turning}>
        <div className={css.clip}>
          <div className={c.flyoutTitle}>Turns</div>
          {TURNS.map((o) => {
            const on = turn === o.value;
            return (
              <button
                key={o.value}
                type="button"
                aria-pressed={on}
                className={cx(c.flyoutItem, css.item, on && c.flyoutItemOn)}
                onClick={() => {
                  setStairTurn(o.value);
                  onPicked?.();
                }}
              >
                <StairGlyph shape={shape} turn={o.value} />
                <span className={css.label}>{o.label}</span>
                <small>{o.hint}</small>
              </button>
            );
          })}
        </div>
      </div>
      <p className={css.keys}>Over the plan: L changes the shape, T the turn, R rotates</p>
    </div>
  );
}
