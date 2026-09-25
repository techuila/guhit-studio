// Walk and fly overlay over the 3D view: the minimap of the level the walker
// is on (click a spot to glide there), the crosshair, the key hint, the walk
// settings (eye height and speed), a short speed readout when the wheel sets
// the pace, arrows on touch screens, and a mouse lock button where the
// browser supports pointer lock. Motion: docs/MOTION.md, "3D view, walk and fly".

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode, type RefObject } from "react";
import { useApp } from "../../state/store";
import { usePresence } from "../../ui/motion";
import type { ViewerEngine } from "../engine/ViewerEngine";
import { EYE_MAX_MM, EYE_MIN_MM, SPEED_MAX_MM_S, SPEED_MIN_MM_S, type TouchMove } from "../engine/walker";
import { useViewer, type NavMode } from "../viewerStore";
import v3 from "../Viewer3D.module.css";
import { useWalkUi } from "./useWalkSync";
import s from "./walk.module.css";

/** How long the key hint stays before it fades, and how long after the pointer leaves it. */
const HINT_SHOW_MS = 4200;
const HINT_LINGER_MS = 2200;
/** The speed readout stays this long after the last wheel step. */
const SPEED_SHOW_MS = 1100;

/** Eye height presets (walk settings). */
export const EYE_PRESETS: { label: string; mm: number }[] = [
  { label: "Child", mm: 1100 },
  { label: "Seated", mm: 1200 },
  { label: "Standing", mm: 1600 },
];

const stroke = { fill: "none", stroke: "currentColor", strokeWidth: 1.4, strokeLinecap: "round", strokeLinejoin: "round" } as const;

export function WalkOverlay(props: {
  open: boolean;
  nav: NavMode;
  engineRef: RefObject<ViewerEngine | null>;
  locked: boolean;
  lockable: boolean;
}) {
  const presence = usePresence(props.open, "base");
  const [hintOn, setHintOn] = useState(true);
  const settingsOpen = useViewer((st) => st.walkSettingsOpen);
  const setSettingsOpen = useViewer.getState().setWalkSettingsOpen;
  const timer = useRef<number | undefined>(undefined);
  const settingsBtn = useRef<HTMLButtonElement>(null);
  const coarse = useCoarsePointer();
  const { engineRef } = props;

  const fadeLater = useCallback((ms: number) => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setHintOn(false), ms);
  }, []);

  // The hint comes back on every entry and on every walk and fly switch.
  useEffect(() => {
    if (!props.open) return;
    setHintOn(true);
    fadeLater(HINT_SHOW_MS);
    return () => window.clearTimeout(timer.current);
  }, [props.open, props.nav, fadeLater]);

  // The settings close with the overlay.
  useEffect(() => {
    if (!props.open) setSettingsOpen(false);
  }, [props.open, setSettingsOpen]);

  const closeSettings = useCallback(() => setSettingsOpen(false), [setSettingsOpen]);

  const minimapRef = useCallback(
    (el: HTMLCanvasElement | null) => {
      engineRef.current?.setMinimap(el);
    },
    [engineRef],
  );

  if (!presence.mounted) return null;
  const fly = props.nav === "fly";
  return (
    <div className={v3.walkOverlay} data-stage={presence.stage} data-testid="walk-overlay" aria-hidden={!props.open}>
      <div className={v3.crosshair} aria-hidden="true" />
      <WalkMap canvasRef={minimapRef} engineRef={engineRef} />
      <SpeedReadout quiet={settingsOpen} />
      <div className={s.hintRow}>
        <div
          className={v3.walkHint}
          data-on={hintOn}
          data-testid="walk-hint"
          onPointerEnter={() => {
            window.clearTimeout(timer.current);
            setHintOn(true);
          }}
          onPointerLeave={() => fadeLater(HINT_LINGER_MS)}
        >
          {fly ? (
            <>
              <kbd>W</kbd>
              <kbd>A</kbd>
              <kbd>S</kbd>
              <kbd>D</kbd> fly · <kbd>E</kbd> up · <kbd>Q</kbd> down · drag to look · scroll to move · double-click to go there · <kbd>Shift</kbd>{" "}
              faster · <kbd>F</kbd> walk · <kbd>Esc</kbd> orbit
            </>
          ) : (
            <>
              <kbd>W</kbd>
              <kbd>A</kbd>
              <kbd>S</kbd>
              <kbd>D</kbd> or scroll to walk · drag to look · double-click to go there · <kbd>Shift</kbd> run · <kbd>F</kbd> fly · <kbd>X</kbd> shell ·{" "}
              <kbd>Esc</kbd> orbit
            </>
          )}
        </div>
      </div>
      {coarse && <TouchPad engineRef={engineRef} />}
      <div className={`${v3.group} ${s.controls}`}>
        <button
          ref={settingsBtn}
          type="button"
          className={v3.btn}
          data-active={settingsOpen}
          data-testid="walk-settings-button"
          aria-haspopup="dialog"
          aria-expanded={settingsOpen}
          title="Walk settings: eye height and speed"
          onClick={() => setSettingsOpen(!settingsOpen)}
        >
          <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
            <path {...stroke} d="M2.5 4.5h6M11.5 4.5h2M2.5 11.5h2M7.5 11.5h6" />
            <circle {...stroke} cx="10" cy="4.5" r="1.6" />
            <circle {...stroke} cx="6" cy="11.5" r="1.6" />
          </svg>
          <span className={v3.label}>Settings</span>
        </button>
        {props.lockable && (
          <button
            type="button"
            className={v3.btn}
            data-active={props.locked}
            data-testid="walk-lock"
            title={props.locked ? "Press Esc to free the mouse" : "Lock the mouse to look without dragging"}
            onClick={() => (props.locked ? engineRef.current?.exitPointerLock() : engineRef.current?.requestPointerLock())}
          >
            <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
              <path {...stroke} d="M4.5 7.2h7v6.3h-7zM6 7.2V5.3a2 2 0 0 1 4 0v1.9" />
            </svg>
            <span className={v3.labelAlways}>{props.locked ? "Esc to unlock" : "Lock mouse"}</span>
          </button>
        )}
      </div>
      <WalkSettings open={settingsOpen} onClose={closeSettings} anchor={settingsBtn} />
    </div>
  );
}

/** The minimap, a ring where a click sent the walker, and the level name when there are several. */
function WalkMap(props: { canvasRef: (el: HTMLCanvasElement | null) => void; engineRef: RefObject<ViewerEngine | null> }) {
  const levels = useApp((st) => st.doc?.project.levels ?? null);
  const levelId = useWalkUi((st) => st.levelId);
  const [ping, setPing] = useState<{ id: number; x: number; y: number } | null>(null);
  const down = useRef<{ id: number; x: number; y: number } | null>(null);
  const seq = useRef(0);
  const name = levels && levels.length > 1 ? (levels.find((l) => l.id === levelId)?.name ?? null) : null;
  return (
    <div className={s.map} data-testid="walk-map">
      <canvas
        ref={props.canvasRef}
        className={s.mapCanvas}
        data-testid="walk-minimap"
        role="img"
        aria-label="Minimap of this level. Click a spot to go there."
        title="Click a spot to go there"
        onPointerDown={(e) => {
          if (e.button === 0) down.current = { id: e.pointerId, x: e.clientX, y: e.clientY };
        }}
        onPointerUp={(e) => {
          const d = down.current;
          down.current = null;
          if (!d || d.id !== e.pointerId || Math.hypot(e.clientX - d.x, e.clientY - d.y) > 4) return;
          if (!props.engineRef.current?.minimapGlide(e.clientX, e.clientY)) return;
          const rect = e.currentTarget.getBoundingClientRect();
          setPing({ id: ++seq.current, x: e.clientX - rect.left, y: e.clientY - rect.top });
        }}
      />
      {ping && <span key={ping.id} className={s.ping} style={{ left: ping.x, top: ping.y }} onAnimationEnd={() => setPing(null)} aria-hidden="true" />}
      {name && (
        <span key={levelId} className={s.level} data-testid="walk-level">
          {name}
        </span>
      )}
    </div>
  );
}

/** "1.8 m/s", shown briefly whenever the speed changes outside the settings. */
function SpeedReadout(props: { quiet: boolean }) {
  const speed = useViewer((st) => st.walk.speed);
  const [on, setOn] = useState(false);
  const last = useRef(speed);
  const timer = useRef<number | undefined>(undefined);
  const quiet = useRef(props.quiet);
  quiet.current = props.quiet;

  useEffect(() => {
    if (last.current === speed) return;
    last.current = speed;
    if (quiet.current) return;
    setOn(true);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOn(false), SPEED_SHOW_MS);
  }, [speed]);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  return (
    <div className={s.speed} data-on={on} data-testid="walk-speed" role="status" aria-live="polite">
      Speed {speed.toFixed(1)} m/s
    </div>
  );
}

/** Eye height (presets and a slider) and walking speed. Remembered on this computer. */
function WalkSettings(props: { open: boolean; onClose: () => void; anchor: RefObject<HTMLButtonElement | null> }) {
  const presence = usePresence(props.open, "base");
  const walk = useViewer((st) => st.walk);
  const ref = useRef<HTMLDivElement>(null);
  const { open, onClose, anchor } = props;

  // A press outside or Escape closes it. The walker leaves Escape to an open
  // walk popover (engine/walker.ts), so Escape does not also end the walk.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      const t = e.target as Node | null;
      if (t && (ref.current?.contains(t) || anchor.current?.contains(t))) return;
      onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      onClose();
      anchor.current?.focus();
    };
    document.addEventListener("pointerdown", onDown, true);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, onClose, anchor]);

  if (!presence.mounted) return null;
  const set = useViewer.getState().setWalk;
  const eye = Math.round(walk.eyeHeightMm);
  const eyeMin = EYE_MIN_MM;
  const eyeMax = EYE_MAX_MM;
  const speedMin = SPEED_MIN_MM_S / 1000;
  const speedMax = SPEED_MAX_MM_S / 1000;
  const pct = (v: number, lo: number, hi: number) => `${(((v - lo) / (hi - lo)) * 100).toFixed(1)}%`;
  return (
    <div
      ref={ref}
      className={s.popover}
      data-stage={presence.stage}
      data-walk-popover={open ? "open" : "closing"}
      data-testid="walk-settings"
      role="dialog"
      aria-label="Walk settings"
    >
      <div className={s.popTitle}>Walk settings</div>
      <div className={s.row}>
        <label htmlFor="walk-eye">Eye height</label>
        <output htmlFor="walk-eye">{eye} mm</output>
      </div>
      <EyePresets value={eye} onPick={(mm) => set({ eyeHeightMm: mm })} />
      <input
        id="walk-eye"
        className={s.slider}
        type="range"
        min={eyeMin}
        max={eyeMax}
        step={10}
        value={eye}
        style={{ "--fill": pct(eye, eyeMin, eyeMax) } as CSSProperties}
        data-testid="walk-eye"
        onChange={(e) => set({ eyeHeightMm: Math.min(Math.max(Number(e.currentTarget.value), eyeMin), eyeMax) })}
      />
      <div className={s.row}>
        <label htmlFor="walk-speed-range">Speed</label>
        <output htmlFor="walk-speed-range">{walk.speed.toFixed(1)} m/s</output>
      </div>
      <input
        id="walk-speed-range"
        className={s.slider}
        type="range"
        min={speedMin}
        max={speedMax}
        step={0.1}
        value={walk.speed}
        style={{ "--fill": pct(walk.speed, speedMin, speedMax) } as CSSProperties}
        data-testid="walk-speed-range"
        onChange={(e) => set({ speed: Math.min(Math.max(Number(e.currentTarget.value), speedMin), speedMax) })}
      />
      <p className={s.note}>Hold W, A, S or D and turn the wheel to change the speed, or use Alt and the wheel. Shift runs.</p>
    </div>
  );
}

/** Child, seated and standing eye heights; one pill slides to the chosen one. */
function EyePresets(props: { value: number; onPick: (mm: number) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pill, setPill] = useState<{ x: number; w: number } | null>(null);
  const active = EYE_PRESETS.findIndex((p) => p.mm === props.value);
  useLayoutEffect(() => {
    const group = ref.current;
    if (!group) return;
    const el = group.querySelector<HTMLElement>('[data-active="true"]');
    // Off every preset the pill fades where it was, so the next pick slides from there.
    if (el) setPill({ x: el.offsetLeft, w: el.offsetWidth });
  }, [active]);
  return (
    <div className={s.chips} ref={ref} role="radiogroup" aria-label="Eye height presets">
      {pill && (
        <span
          className={s.chipPill}
          data-shown={active >= 0}
          aria-hidden="true"
          style={{ transform: `translateX(${pill.x}px)`, width: `${pill.w}px` }}
        />
      )}
      {EYE_PRESETS.map((p, i): ReactNode => (
        <button
          key={p.mm}
          type="button"
          role="radio"
          aria-checked={i === active}
          className={s.chip}
          data-active={i === active}
          data-testid={`walk-eye-${p.label.toLowerCase()}`}
          onClick={() => props.onPick(p.mm)}
        >
          <b>{p.label}</b>
          <span>{(p.mm / 1000).toFixed(1)} m</span>
        </button>
      ))}
    </div>
  );
}

const PAD: { move: TouchMove; label: string; area: string; d: string }[] = [
  { move: "forward", label: "Walk forward", area: "f", d: "M8 3.5 13 9.5H3z" },
  { move: "left", label: "Step left", area: "l", d: "M3.5 8 9.5 3v10z" },
  { move: "back", label: "Walk back", area: "b", d: "M8 12.5 3 6.5h10z" },
  { move: "right", label: "Step right", area: "r", d: "M12.5 8 6.5 13V3z" },
];

/** Hold an arrow to walk. Only on screens with a touch pointer. */
function TouchPad(props: { engineRef: RefObject<ViewerEngine | null> }) {
  const press = (move: TouchMove, down: boolean) => props.engineRef.current?.walkPress(move, down);
  const release = (move: TouchMove, el: HTMLElement) => {
    if (el.dataset.down !== "true") return;
    el.dataset.down = "false";
    press(move, false);
  };
  return (
    <div className={s.pad} data-testid="walk-pad" role="group" aria-label="Move">
      {PAD.map((b) => (
        <button
          key={b.move}
          type="button"
          className={s.padBtn}
          style={{ gridArea: b.area }}
          aria-label={b.label}
          data-testid={`walk-pad-${b.move}`}
          onPointerDown={(e) => {
            try {
              e.currentTarget.setPointerCapture(e.pointerId);
            } catch {
              // Synthetic pointers cannot be captured; the release still ends the move.
            }
            e.currentTarget.dataset.down = "true";
            press(b.move, true);
          }}
          onPointerUp={(e) => release(b.move, e.currentTarget)}
          onPointerCancel={(e) => release(b.move, e.currentTarget)}
          onLostPointerCapture={(e) => release(b.move, e.currentTarget)}
          onContextMenu={(e) => e.preventDefault()}
        >
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
            <path d={b.d} fill="currentColor" />
          </svg>
        </button>
      ))}
    </div>
  );
}

/** True when the device has a touch pointer, so the on-screen arrows help. */
function useCoarsePointer(): boolean {
  const query = "(any-pointer: coarse)";
  const [coarse, setCoarse] = useState(() => typeof matchMedia === "function" && matchMedia(query).matches);
  useEffect(() => {
    if (typeof matchMedia !== "function") return;
    const mq = matchMedia(query);
    const onChange = () => setCoarse(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
  return coarse;
}
