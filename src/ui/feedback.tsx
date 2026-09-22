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

function ToastItem({ toast, leaving }: { toast: Toast; leaving: boolean }) {
  const dismiss = useApp((st) => st.dismissToast);
  useEffect(() => {
    const t = window.setTimeout(() => dismiss(toast.id), LIFETIME_MS[toast.kind]);
    return () => window.clearTimeout(t);
  }, [toast.id, toast.kind, dismiss]);
  return (
    <div
      className={cx(s.toast, s[`toast_${toast.kind}`], leaving && s.toastLeaving)}
      role={toast.kind === "error" ? "alert" : "status"}
    >
      <Icon name={toast.kind === "success" ? "check" : toast.kind === "error" ? "warning" : "info"} size={16} className={s.toastIcon} />
      <span className={s.toastText}>{toast.message}</span>
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

export function Toasts() {
  const toasts = useApp((st) => st.toasts);
  const onHub = useApp((st) => st.screen === "hub");
  const queue = useToastQueue(toasts);
  // Show the newest few, and one copy of a repeated message. Hidden ones still time out on their own.
  const shown = queue.filter((t, i) => !queue.slice(i + 1).some((later) => later.kind === t.kind && later.message === t.message && !later.leaving)).slice(-4);
  return (
    <div className={cx(s.toasts, onHub && s.toastsHub)} aria-live="polite">
      {shown.map((t) => (
        <ToastItem key={t.id} toast={t} leaving={t.leaving} />
      ))}
    </div>
  );
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
