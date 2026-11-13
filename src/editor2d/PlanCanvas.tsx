// The 2D drafting surface. No props, fills its parent. All interaction lives
// in PlanController; this component mounts the canvas and the small HTML
// overlays (type-to-precise box, inline text editor, the level below switch,
// and the live session layer with other people's cursors and cursor chat).

import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { LiveLayer } from "../live/LiveLayer";
import { useApp } from "../state/store";
import { dur, motionOK, usePresence, type PresenceStage } from "../ui/motion";
import { cssEase } from "../ui/motionWaapi";
import { PlanController, type UiState } from "./controller";
import { levelBelow } from "./slab";
import { useUnderlay } from "./underlay";
import styles from "./PlanCanvas.module.css";

const EMPTY_UI: UiState = { typed: null, editor: null, cursor: "default", hint: null };
const noopSubscribe = (): (() => void) => () => {};

export function PlanCanvas() {
  const rootRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [controller, setController] = useState<PlanController | null>(null);
  const hasDoc = useApp((s) => s.doc !== null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const root = rootRef.current;
    if (!canvas || !root) return;
    const c = new PlanController(canvas);
    setController(c);
    const measure = (): void => {
      const r = root.getBoundingClientRect();
      c.resize(Math.floor(r.width), Math.floor(r.height), window.devicePixelRatio || 1);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(root);
    // Moving the window to a screen with another pixel ratio.
    window.addEventListener("resize", measure);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", measure);
      c.dispose();
      setController(null);
    };
  }, []);

  const ui = useSyncExternalStore(controller ? controller.subscribe : noopSubscribe, controller ? controller.getUi : () => EMPTY_UI);

  // These three are DOM, not canvas: they scale and fade with the CSS tokens
  // and stay mounted long enough for their exit to play (docs/MOTION.md rule 6).
  const typedP = usePresence(!!ui.typed, "base");
  const editorP = usePresence(!!ui.editor, "base");
  const hintP = usePresence(!!ui.hint && !ui.editor, "base");
  const lastTyped = useRef(ui.typed);
  const lastEditor = useRef(ui.editor);
  const lastHint = useRef(ui.hint);
  const editorValue = useRef("");
  // Stable, so the editor's focus effect never re-runs while someone is typing.
  const setEditorValue = useCallback((v: string) => {
    editorValue.current = v;
  }, []);
  if (ui.typed) lastTyped.current = ui.typed;
  if (ui.editor) lastEditor.current = ui.editor;
  if (ui.hint) lastHint.current = ui.hint;

  return (
    <div ref={rootRef} className={styles.root} data-testid="plan-canvas">
      <canvas ref={canvasRef} className={styles.canvas} style={{ cursor: ui.cursor }} />
      {controller && hasDoc ? <LiveLayer controller={controller} rootRef={rootRef} /> : null}
      {!hasDoc && <div className={styles.empty}>No project open</div>}
      {typedP.mounted && lastTyped.current && <TypedBox typed={lastTyped.current} stage={typedP.stage} />}
      {ui.editor && controller && (
        <InlineEditor
          key={`${ui.editor.mode}:${ui.editor.id ?? "new"}`}
          editor={ui.editor}
          controller={controller}
          stage={editorP.stage}
          onValue={setEditorValue}
        />
      )}
      {/* The input is gone the moment it closes; a plain copy plays the exit. */}
      {!ui.editor && editorP.mounted && lastEditor.current && (
        <div className={styles.editor} data-stage="exit" style={editorStyle(lastEditor.current, editorValue.current)} aria-hidden>
          {editorValue.current}
        </div>
      )}
      {hasDoc ? <UnderlayToggle /> : null}
      {hintP.mounted && lastHint.current && (
        <div className={styles.hint} data-stage={hintP.stage}>
          {lastHint.current}
        </div>
      )}
    </div>
  );
}

/**
 * The level below as an underlay: a pill in the bottom left corner with a
 * small switch, shown on any level with a level below. On by default,
 * remembered per project.
 */
function UnderlayToggle() {
  const on = useUnderlay((u) => u.on);
  const toggle = useUnderlay((u) => u.toggle);
  const below = useApp((s) => (s.doc ? (levelBelow(s.doc.project.levels, s.activeLevelId)?.name ?? null) : null));
  const p = usePresence(below !== null, "base");
  const name = useRef(below);
  if (below !== null) name.current = below;
  if (!p.mounted) return null;
  return (
    <button
      type="button"
      className={styles.underlay}
      data-stage={p.stage}
      data-on={on ? "true" : "false"}
      role="switch"
      aria-checked={on}
      /* One name, the visible label: the checked state says on or off. */
      data-testid="underlay-toggle"
      onClick={toggle}
    >
      <span className={styles.underlayTrack} aria-hidden>
        <span className={styles.underlayKnob} />
      </span>
      <span className={styles.underlayLabel}>{name.current} below</span>
    </button>
  );
}

function TypedBox({ typed, stage }: { typed: NonNullable<UiState["typed"]>; stage: PresenceStage }) {
  const { state, labels } = typed;
  // A pipe height (typed after h) is a single field.
  const height = state.mode === "height";
  return (
    <div className={styles.typed} style={{ left: typed.x + 18, top: typed.y + 14 }} data-stage={stage} data-testid="typed-box">
      {(height ? [0] : [0, 1]).map((i) => (
        <div key={i} className={`${styles.field} ${state.active === i ? styles.fieldActive : ""}`}>
          <span className={styles.fieldLabel}>{labels[i]}</span>
          <span className={styles.fieldValue}>
            {state.fields[i] !== "" ? state.fields[i] : state.active === i ? "" : "auto"}
            {state.active === i && <span className={styles.caret} />}
          </span>
        </div>
      ))}
      <span className={styles.typedHelp}>{height ? "Enter sets the height" : "Tab switches, Enter applies"}</span>
    </div>
  );
}

type EditorUi = NonNullable<UiState["editor"]>;

function pipeEditorLabel(editor: EditorUi): string {
  switch (editor.pipe?.field) {
    case "height":
      return "Height above the floor";
    case "fall":
      return "Fall in percent";
    default:
      return "Segment length";
  }
}

/** Rooms: centered on the label. Text: the point is the left end of the baseline. A pipe tag value: centered on the tag. */
function editorStyle(editor: EditorUi, value: string): React.CSSProperties {
  if (editor.mode === "pipe") {
    const width = Math.max(64, (value.length + 3) * 7.5);
    return { left: editor.x - width / 2, top: editor.y - 11, width, minWidth: 0, fontSize: 11, fontFamily: "var(--font-mono)", textAlign: "center" };
  }
  const fontPx = Math.min(Math.max(editor.fontPx, 11), 40);
  const width = Math.max(90, (value.length + 2) * fontPx * 0.62);
  return editor.mode === "room"
    ? { left: editor.x - width / 2, top: editor.y - fontPx * 1.4, width, fontSize: fontPx, fontWeight: 600, textAlign: "center" }
    : { left: editor.x - 7, top: editor.y - fontPx * 1.1 - 3, width, fontSize: fontPx };
}

function InlineEditor({
  editor,
  controller,
  stage,
  onValue,
}: {
  editor: EditorUi;
  controller: PlanController;
  stage: PresenceStage;
  onValue: (v: string) => void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  const [value, setValue] = useState(editor.value);
  const done = useRef(false);

  useLayoutEffect(() => {
    ref.current?.focus();
    ref.current?.select();
    onValue(editor.value);
  }, [editor.value, onValue]);

  const finish = (v: string | null, fromBlur = false): void => {
    if (done.current) return;
    done.current = true;
    // A refused value keeps the box open (a pipe tag value that is not a number).
    if (!controller.closeEditor(v, fromBlur)) done.current = false;
  };

  // Each refusal shakes the box once (MOTION.md: an invalid entry shakes 4 px).
  useEffect(() => {
    const el = ref.current;
    if (!editor.errorSeq || !el || !motionOK()) return;
    el.animate([{ translate: "0" }, { translate: "-4px" }, { translate: "3px" }, { translate: "-2px" }, { translate: "0" }], { duration: dur("panel"), easing: cssEase("out") });
  }, [editor.errorSeq]);

  const box = editorStyle(editor, value);
  return (
    <>
      <input
        ref={ref}
        className={styles.editor}
        style={box}
        data-stage={stage}
        value={value}
        placeholder={editor.mode === "new_text" ? "Type, then Enter" : undefined}
        aria-label={editor.mode === "pipe" ? pipeEditorLabel(editor) : undefined}
        data-testid="inline-editor"
        onChange={(e) => {
          setValue(e.target.value);
          onValue(e.target.value);
        }}
        onBlur={() => finish(value, true)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") finish(value);
          else if (e.key === "Escape") finish(null);
        }}
      />
      {editor.error ? (
        <div key={editor.errorSeq} className={styles.editorError} style={{ left: box.left, top: Number(box.top ?? 0) + 26 }} role="alert" data-testid="inline-editor-error">
          {editor.error}
        </div>
      ) : null}
    </>
  );
}
