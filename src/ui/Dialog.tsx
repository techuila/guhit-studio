// Modal dialog, confirm dialog and a small anchored menu.
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { PresenceStage } from "./motion";
import { Button, IconButton, cx, rovingNext } from "./controls";
import { Icon, type IconName } from "./icons";
import s from "./Dialog.module.css";

const FOCUSABLE =
  'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

/** Number of open modals. Global shortcuts stay quiet while this is above zero. */
let openModals = 0;
export function isModalOpen(): boolean {
  return openModals > 0;
}

/**
 * Where toasts go while a modal is open: the top modal's area, above its
 * footer (or its bottom edge), as distances from the window's edges. Offsets
 * ignore the enter transform, so the place is right from the first frame.
 */
export function modalToastArea(
  panel: { offsetLeft: number; offsetTop: number; offsetWidth: number; offsetHeight: number },
  footerTop: number | null,
  view: { width: number; height: number },
  gap = 12,
): { left: number; right: number; bottom: number } {
  const edge = panel.offsetTop + (footerTop ?? panel.offsetHeight);
  return {
    left: Math.max(0, panel.offsetLeft),
    right: Math.max(0, view.width - (panel.offsetLeft + panel.offsetWidth)),
    bottom: Math.max(0, view.height - edge + gap),
  };
}

// Open modal panels, newest last. The newest one holds the toasts.
const modalStack: HTMLElement[] = [];
let modalObserver: ResizeObserver | null = null;

function placeToastsInModal(): void {
  const root = document.documentElement;
  const top = modalStack[modalStack.length - 1];
  if (!top) {
    root.removeAttribute("data-modal-open");
    for (const p of ["--modal-toast-left", "--modal-toast-right", "--modal-toast-bottom"]) root.style.removeProperty(p);
    return;
  }
  const footer = [...top.children].find((c): c is HTMLElement => c.tagName === "FOOTER") ?? null;
  const area = modalToastArea(top, footer ? footer.offsetTop : null, { width: root.clientWidth, height: window.innerHeight });
  root.style.setProperty("--modal-toast-left", `${area.left}px`);
  root.style.setProperty("--modal-toast-right", `${area.right}px`);
  root.style.setProperty("--modal-toast-bottom", `${area.bottom}px`);
  root.setAttribute("data-modal-open", "");
}

function watchTopModal(): void {
  modalObserver?.disconnect();
  modalObserver = null;
  const top = modalStack[modalStack.length - 1];
  if (top && typeof ResizeObserver !== "undefined") {
    modalObserver = new ResizeObserver(placeToastsInModal);
    modalObserver.observe(top);
  }
  if (top) window.addEventListener("resize", placeToastsInModal);
  else window.removeEventListener("resize", placeToastsInModal);
  placeToastsInModal();
}

function registerModal(panel: HTMLElement): void {
  if (modalStack.includes(panel)) return;
  modalStack.push(panel);
  watchTopModal();
}

function unregisterModal(panel: HTMLElement): void {
  const i = modalStack.indexOf(panel);
  if (i < 0) return;
  modalStack.splice(i, 1);
  watchTopModal();
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

  // Toasts move into this modal's area while it is open, and back as it leaves.
  useEffect(() => {
    const el = panel.current;
    if (!el) return;
    if (stage === "exit") {
      unregisterModal(el);
      return;
    }
    registerModal(el);
    return () => unregisterModal(el);
  }, [stage]);

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
  // A destructive confirm (end the session for everyone, move to trash)
  // starts on Cancel, so Enter alone never does it.
  return (
    <Dialog
      title={title}
      onClose={onCancel}
      width={380}
      stage={stage}
      footer={
        <>
          <Button onClick={onCancel} data-autofocus={danger ? true : undefined}>
            Cancel
          </Button>
          <Button variant={danger ? "danger" : "primary"} onClick={onConfirm} data-autofocus={danger ? undefined : true}>
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

/** The trigger a menu belongs to: the first button in its anchor that is not one of its items. */
function menuTrigger(menu: HTMLElement | null): HTMLElement | null {
  const anchor = menu?.parentElement;
  if (!anchor || !menu) return null;
  return [...anchor.querySelectorAll<HTMLElement>("button")].find((b) => !menu.contains(b)) ?? null;
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
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [active, setActive] = useState(0);

  // Roving focus: the highlighted item holds real focus, so a screen reader
  // reads it as the arrows move.
  useEffect(() => {
    if (stage !== "exit") itemRefs.current[active]?.focus();
  }, [active, stage]);

  useEffect(() => {
    const down = (e: MouseEvent) => {
      // The trigger lives in the same anchor element and toggles the menu itself.
      const anchor = ref.current?.parentElement ?? ref.current;
      if (anchor && !anchor.contains(e.target as Node)) onClose();
    };
    window.addEventListener("mousedown", down, true);
    return () => window.removeEventListener("mousedown", down, true);
  }, [onClose]);

  // Closing from the keyboard hands focus back to the trigger, not to <body>.
  const closeToTrigger = () => {
    menuTrigger(ref.current)?.focus();
    onClose();
  };

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
        if (e.key === "Escape" || e.key === "Tab") {
          if (e.key === "Escape") e.preventDefault();
          closeToTrigger();
          return;
        }
        const next = rovingNext(e.key, active, items.length, "vertical");
        if (next !== null) {
          e.preventDefault();
          setActive(next);
        } else if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          items[active]?.onSelect();
          closeToTrigger();
        }
      }}
    >
      {items.map((item, i) => (
        <button
          key={item.key}
          ref={(el) => {
            itemRefs.current[i] = el;
          }}
          type="button"
          role="menuitem"
          tabIndex={i === active ? 0 : -1}
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
