// Modal dialog, confirm dialog and a small anchored menu.
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { PresenceStage } from "./motion";
import { Button, IconButton, cx } from "./controls";
import { Icon, type IconName } from "./icons";
import s from "./Dialog.module.css";

const FOCUSABLE =
  'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

/** Number of open modals. Global shortcuts stay quiet while this is above zero. */
let openModals = 0;
export function isModalOpen(): boolean {
  return openModals > 0;
}

interface DialogProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
  /** "center" is a classic dialog, "side" is a sheet on the right edge, "top" sits high like a palette. */
  placement?: "center" | "side" | "top";
  bare?: boolean;
  labelledBy?: string;
  /**
   * Presence stage from a `Presence`/`usePresence` wrapper at the call site.
   * Drives the open/close CSS via `data-stage`. Defaults to "idle" so Dialog
   * still renders sensibly when a caller does not wrap it (no exit).
   */
  stage?: PresenceStage;
}

export function Dialog({ title, onClose, children, footer, width = 440, placement = "center", bare, stage = "idle" }: DialogProps) {
  const panel = useRef<HTMLDivElement>(null);

  // Just the modal counter: symmetric and side-effect-free, so it is safe
  // under React StrictMode's dev-only double-invoke of mount effects.
  useEffect(() => {
    openModals++;
    return () => {
      openModals--;
    };
  }, []);

  // Focus capture/restore is tied to `stage`, not mount/unmount: a caller
  // wrapped in Presence keeps Dialog mounted through the exit animation, so
  // "unmount" happens well after the dialog has visually closed. Driving
  // this off `stage` instead of a mount-effect cleanup also sidesteps
  // StrictMode's double-invoked cleanup stealing focus back mid-open (it
  // would otherwise fire between this grabbing focus and the real close).
  const previousFocus = useRef<HTMLElement | null>(null);
  const grabbed = useRef(false);
  useEffect(() => {
    if (stage === "idle" && !grabbed.current) {
      grabbed.current = true;
      previousFocus.current = document.activeElement as HTMLElement | null;
      const grabFocus = () => {
        const el = panel.current;
        const first = el?.querySelector<HTMLElement>("[data-autofocus]") ?? el?.querySelector<HTMLElement>(FOCUSABLE);
        (first ?? el)?.focus();
      };
      // Deferred two frames (matching usePresence's own enter->idle delay),
      // with one re-assertion shortly after. Deliberately not cancelled on
      // cleanup: React StrictMode's dev-only double-render can otherwise
      // steal focus back in the same window (harmless past a real unmount,
      // since `panel.current` is null by then and this becomes a no-op).
      requestAnimationFrame(() => requestAnimationFrame(grabFocus));
      window.setTimeout(() => {
        if (panel.current && !panel.current.contains(document.activeElement)) grabFocus();
      }, 120);
    } else if (stage === "exit" && grabbed.current) {
      grabbed.current = false;
      previousFocus.current?.focus?.();
    }
  }, [stage]);

  // Fallback for a Dialog unmounted without ever seeing "exit" (used outside
  // a Presence wrapper, or force-removed): restore focus on the real unmount.
  useEffect(
    () => () => {
      if (grabbed.current) previousFocus.current?.focus?.();
    },
    [],
  );

  const onKeyDown = (e: React.KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
      return;
    }
    if (e.key !== "Tab" || !panel.current) return;
    const items = [...panel.current.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((n) => n.offsetParent !== null);
    if (items.length === 0) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      className={cx(s.backdrop, s[`place_${placement}`])}
      data-stage={stage}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      onKeyDown={onKeyDown}
    >
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        data-stage={stage}
        className={cx(s.panel, bare && s.bare)}
        style={{ width }}
      >
        {bare ? null : (
          <header className={s.head}>
            <h2>{title}</h2>
            <IconButton icon="close" label="Close" tip="Close (Esc)" tipSide="bottom-end" onClick={onClose} />
          </header>
        )}
        <div className={bare ? s.bareBody : s.body}>{children}</div>
        {footer ? <footer className={s.foot}>{footer}</footer> : null}
      </div>
    </div>
  );
}

interface ConfirmProps {
  title: string;
  message: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  stage?: PresenceStage;
}

export function ConfirmDialog({ title, message, confirmLabel, danger, onConfirm, onCancel, stage }: ConfirmProps) {
  return (
    <Dialog
      title={title}
      onClose={onCancel}
      width={380}
      stage={stage}
      footer={
        <>
          <Button onClick={onCancel}>Cancel</Button>
          <Button variant={danger ? "danger" : "primary"} onClick={onConfirm} data-autofocus>
            {confirmLabel}
          </Button>
        </>
      }
    >
      <p className={s.message}>{message}</p>
    </Dialog>
  );
}

export interface MenuItem {
  key: string;
  label: string;
  icon?: IconName;
  danger?: boolean;
  onSelect: () => void;
}

/** A small native-feeling menu anchored under its trigger. */
export function Menu({
  items,
  onClose,
  align = "end",
  stage = "idle",
}: {
  items: MenuItem[];
  onClose: () => void;
  align?: "start" | "end";
  stage?: PresenceStage;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(0);

  useEffect(() => {
    ref.current?.focus();
    const down = (e: MouseEvent) => {
      // The trigger lives in the same anchor element and toggles the menu itself.
      const anchor = ref.current?.parentElement ?? ref.current;
      if (anchor && !anchor.contains(e.target as Node)) onClose();
    };
    window.addEventListener("mousedown", down, true);
    return () => window.removeEventListener("mousedown", down, true);
  }, [onClose]);

  return (
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      data-stage={stage}
      className={cx(s.menu, align === "start" && s.menuStart)}
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") onClose();
        else if (e.key === "ArrowDown") setActive((i) => (i + 1) % items.length);
        else if (e.key === "ArrowUp") setActive((i) => (i - 1 + items.length) % items.length);
        else if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          items[active]?.onSelect();
          onClose();
        }
      }}
    >
      {items.map((item, i) => (
        <button
          key={item.key}
          type="button"
          role="menuitem"
          tabIndex={-1}
          className={cx(s.menuItem, i === active && s.menuItemActive, item.danger && s.menuItemDanger)}
          onMouseEnter={() => setActive(i)}
          onClick={() => {
            item.onSelect();
            onClose();
          }}
        >
          {item.icon ? <Icon name={item.icon} size={15} /> : null}
          <span>{item.label}</span>
        </button>
      ))}
    </div>
  );
}
