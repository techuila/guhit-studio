// Toasts, busy overlay and the per-panel error boundary.
import { Component, useEffect, useRef, useState, type ErrorInfo, type ReactNode } from "react";
import { create } from "zustand";
import { useApp, type Toast } from "../state/store";
import { Button, Spinner, cx } from "./controls";
import { Icon } from "./icons";
import { dur } from "./motion";
import { Presence } from "./motionDom";
import s from "./feedback.module.css";

// What `useApp` state.busy is doing, for BusyOverlay's copy. Set it right
// before starting a busy action (see ProjectHub's open/create calls).
const useBusyLabel = create<{ label: string }>(() => ({ label: "Working" }));
export function setBusyLabel(label: string): void {
  useBusyLabel.setState({ label });
}

const LIFETIME_MS: Record<Toast["kind"], number> = { info: 4000, success: 4500, error: 8000 };
/** A toast with a button stays up long enough to reach it. */
const ACTION_LIFETIME_MS = 8000;
/** After the pointer or focus leaves a paused toast, it stays at least this long. */
const RESUME_MIN_MS = 1500;

export function toastLifetime(toast: Pick<Toast, "kind" | "action">): number {
  const base = LIFETIME_MS[toast.kind];
  return toast.action ? Math.max(base, ACTION_LIFETIME_MS) : base;
}

export interface PausableTimer {
  pause(): void;
  resume(): void;
  cancel(): void;
  /** Time left in ms. */
  remaining(): number;
}

/**
 * A timeout that can pause and resume with the time it had left. A resume
 * gives at least `minResume` ms, so a toast does not vanish the moment the
 * pointer leaves it.
 */
export function createPausableTimer(ms: number, onDone: () => void, minResume = RESUME_MIN_MS): PausableTimer {
  let left = ms;
  let started = Date.now();
  let running = true;
  let done = false;
  const fire = () => {
    done = true;
    running = false;
    onDone();
  };
  let id = setTimeout(fire, left);
  return {
    pause() {
      if (!running) return;
      clearTimeout(id);
      left = Math.max(0, left - (Date.now() - started));
      running = false;
    },
    resume() {
      if (running || done) return;
      left = Math.max(left, minResume);
      started = Date.now();
      running = true;
      id = setTimeout(fire, left);
    },
    cancel() {
      clearTimeout(id);
      running = false;
      done = true;
    },
    remaining: () => (running ? Math.max(0, left - (Date.now() - started)) : left),
  };
}

/** What the toast's button does: its action, then the toast goes. */
export function runToastAction(toast: Toast, dismiss: (id: number) => void): void {
  toast.action?.run();
  dismiss(toast.id);
}

export function ToastItem({ toast, leaving }: { toast: Toast; leaving: boolean }) {
  const dismiss = useApp((st) => st.dismissToast);
  // The timer pauses while the pointer is over the toast or focus is inside
  // it (WCAG 2.2.1), and resumes with the time it had left.
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const paused = hovered || focused;
  const timer = useRef<PausableTimer | null>(null);
  useEffect(() => {
    const t = createPausableTimer(toastLifetime(toast), () => dismiss(toast.id));
    timer.current = t;
    return () => t.cancel();
    // The lifetime is fixed when the toast first shows.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [toast.id, dismiss]);
  useEffect(() => {
    if (paused || leaving) timer.current?.pause();
    else timer.current?.resume();
  }, [paused, leaving]);
  const action = toast.action;
  // No role here: the persistent live regions in ToastsView announce the
  // text, so a toast is never read twice.
  return (
    <div
      className={cx(s.toast, s[`toast_${toast.kind}`], leaving && s.toastLeaving)}
      data-kind={toast.kind}
      data-paused={paused || undefined}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onFocus={() => setFocused(true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFocused(false);
      }}
    >
      <Icon name={toast.kind === "success" ? "check" : toast.kind === "error" ? "warning" : "info"} size={16} className={s.toastIcon} />
      <span className={s.toastText}>{toast.message}</span>
      {action ? (
        <button type="button" className={s.toastAction} onClick={() => runToastAction(toast, dismiss)}>
          {action.label}
        </button>
      ) : null}
      <button type="button" className={s.toastClose} aria-label="Dismiss" onClick={() => dismiss(toast.id)}>
        <Icon name="close" size={13} />
      </button>
    </div>
  );
}

/**
 * The store just removes a dismissed toast from its array. To animate the
 * exit (MOTION.md: "leave with fade + slide", siblings move up smoothly),
 * this keeps a locally-owned mirror: a toast the store dropped is marked
 * `leaving` and kept around for one more exit duration before it is pruned.
 */
function useToastQueue(toasts: Toast[]): Array<Toast & { leaving: boolean }> {
  const [queue, setQueue] = useState<Array<Toast & { leaving: boolean }>>(() => toasts.map((t) => ({ ...t, leaving: false })));
  const timers = useRef(new Map<number, number>());

  useEffect(() => {
    const liveIds = new Set(toasts.map((t) => t.id));
    setQueue((prev) => {
      const next = prev.map((q) => {
        if (liveIds.has(q.id) || q.leaving) return q;
        const ms = Math.round(dur("base") * 0.7) + 30;
        const timerId = window.setTimeout(() => {
          timers.current.delete(q.id);
          setQueue((cur) => cur.filter((x) => x.id !== q.id));
        }, ms);
        timers.current.set(q.id, timerId);
        return { ...q, leaving: true };
      });
      const known = new Set(next.map((q) => q.id));
      const added = toasts.filter((t) => !known.has(t.id)).map((t) => ({ ...t, leaving: false }));
      return [...next, ...added];
    });
  }, [toasts]);

  useEffect(() => {
    const map = timers.current;
    return () => map.forEach((id) => window.clearTimeout(id));
  }, []);

  return queue;
}

export interface ToastAnnouncements {
  polite: Toast | null;
  alert: Toast | null;
}

export const NO_ANNOUNCEMENTS: ToastAnnouncements = { polite: null, alert: null };

/**
 * What the live regions say: the newest info or success toast that has
 * arrived (polite) and the newest error (assertive). Each is keyed by the
 * toast id, so a new toast adds a new node to a region that is already in the
 * DOM, which screen readers announce reliably (a region mounted together with
 * its text is often skipped).
 *
 * A region only ever moves on to a toast newer than the one it said (ids only
 * grow), so each message is announced once, when it arrives. A newer toast
 * that leaves first does not bring back an older one: the region keeps the
 * last thing it said. Returns `prev` itself when nothing new arrived.
 */
export function nextAnnouncements(prev: ToastAnnouncements, toasts: Toast[]): ToastAnnouncements {
  let polite = prev.polite;
  let alert = prev.alert;
  for (const t of toasts) {
    if (t.kind === "error") {
      if (!alert || t.id > alert.id) alert = t;
    } else if (!polite || t.id > polite.id) polite = t;
  }
  return polite === prev.polite && alert === prev.alert ? prev : { polite, alert };
}

export function ToastsView({
  queue,
  said,
  onHub,
}: {
  queue: Array<Toast & { leaving: boolean }>;
  /** What the live regions hold (see nextAnnouncements). */
  said: ToastAnnouncements;
  onHub: boolean;
}) {
  // Show the newest few, and one copy of a repeated message. Hidden ones still time out on their own.
  const shown = queue.filter((t, i) => !queue.slice(i + 1).some((later) => later.kind === t.kind && later.message === t.message && !later.leaving)).slice(-4);
  return (
    <>
      {/* Always in the DOM, empty until a toast arrives. */}
      <div className={s.srOnly} role="status" aria-live="polite" aria-atomic="true">
        {said.polite ? <span key={said.polite.id}>{said.polite.message}</span> : null}
      </div>
      <div className={s.srOnly} role="alert" aria-live="assertive" aria-atomic="true">
        {said.alert ? <span key={said.alert.id}>{said.alert.message}</span> : null}
      </div>
      <div className={cx(s.toasts, onHub && s.toastsHub)}>
        {shown.map((t) => (
          <ToastItem key={t.id} toast={t} leaving={t.leaving} />
        ))}
      </div>
    </>
  );
}

export function Toasts() {
  const toasts = useApp((st) => st.toasts);
  const onHub = useApp((st) => st.screen === "hub");
  const queue = useToastQueue(toasts);
  // Derived during render (React's "adjust state when props change"): the
  // regions move on only when a newer toast arrives.
  const [said, setSaid] = useState<ToastAnnouncements>(NO_ANNOUNCEMENTS);
  const next = nextAnnouncements(said, toasts);
  if (next !== said) setSaid(next);
  return <ToastsView queue={queue} said={next} onHub={onHub} />;
}

export function BusyOverlay() {
  const busy = useApp((st) => st.busy);
  const label = useBusyLabel((st) => st.label);
  return (
    <Presence open={busy} exit="hover">
      {(stage) => (
        <div className={s.busy} data-stage={stage} role="progressbar" aria-label={label}>
          <div className={s.busyCard}>
            <Spinner size={18} />
            <span>{label}</span>
          </div>
        </div>
      )}
    </Presence>
  );
}

interface BoundaryProps {
  /** Shown in the fallback, for example "3D view". */
  name: string;
  children: ReactNode;
  tone?: "light" | "dark";
}

interface BoundaryState {
  error: Error | null;
  attempt: number;
}

/** Keeps one crashing panel from blanking the whole app. */
export class PanelBoundary extends Component<BoundaryProps, BoundaryState> {
  state: BoundaryState = { error: null, attempt: 0 };

  static getDerivedStateFromError(error: Error): Partial<BoundaryState> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(`[${this.props.name}] panel crashed`, error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className={cx(s.crash, this.props.tone === "dark" && s.crashDark)} role="alert">
          <Icon name="warning" size={20} />
          <strong>{this.props.name} stopped working</strong>
          <span className={s.crashDetail}>{this.state.error.message}</span>
          <span className={s.crashHint}>Your project is saved. The rest of the app still works.</span>
          <Button size="sm" onClick={() => this.setState((st) => ({ error: null, attempt: st.attempt + 1 }))}>
            Reload this panel
          </Button>
        </div>
      );
    }
    return <BoundaryKey key={this.state.attempt}>{this.props.children}</BoundaryKey>;
  }
}

function BoundaryKey({ children }: { children: ReactNode }) {
  return <>{children}</>;
}
