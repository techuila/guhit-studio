// Shared form and chrome primitives used by the hub and the editor shell.
import {
  useEffect,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import type { DisplayUnit } from "../contract/bindings";
import { Icon, type IconName } from "./icons";
import { useFlash, useSlidingIndicator } from "./motionDom";
import { lengthToInput, numberToInput, parseLength, parseNumber } from "./units";
import s from "./controls.module.css";

export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

// ---------------------------------------------------------------- buttons

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "default" | "primary" | "ghost" | "danger" | "chrome";
  size?: "sm" | "md";
  icon?: IconName;
}

export function Button({ variant = "default", size = "md", icon, children, className, ...rest }: ButtonProps) {
  return (
    <button type="button" {...rest} className={cx(s.button, s[`v_${variant}`], size === "sm" && s.sm, className)}>
      {icon ? <Icon name={icon} size={size === "sm" ? 14 : 16} /> : null}
      {children ? <span>{children}</span> : null}
    </button>
  );
}

interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  icon: IconName;
  label: string;
  /** Tooltip text. Defaults to the label. */
  tip?: string;
  tipSide?: "bottom" | "right" | "top" | "top-start" | "top-end" | "bottom-start" | "bottom-end";
  active?: boolean;
  tone?: "light" | "chrome";
  size?: number;
}

export function IconButton({
  icon,
  label,
  tip,
  tipSide = "bottom",
  active,
  tone = "light",
  size = 18,
  className,
  ...rest
}: IconButtonProps) {
  return (
    <button
      type="button"
      {...rest}
      aria-label={label}
      aria-pressed={active}
      data-tip={tip ?? label}
      data-tip-side={tipSide}
      className={cx(s.iconButton, tone === "chrome" && s.iconChrome, active && s.iconActive, className)}
    >
      <Icon name={icon} size={size} />
    </button>
  );
}

// ---------------------------------------------------------------- segmented

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon?: IconName;
  tip?: string;
}

interface SegmentedProps<T extends string> {
  options: Array<SegmentedOption<T>>;
  value: T;
  onChange: (value: T) => void;
  tone?: "light" | "chrome";
  label: string;
  iconOnly?: boolean;
  stretch?: boolean;
}

export function Segmented<T extends string>({
  options,
  value,
  onChange,
  tone = "light",
  label,
  iconOnly,
  stretch,
}: SegmentedProps<T>) {
  const containerRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef<HTMLButtonElement>(null);
  const thumb = useSlidingIndicator(containerRef, activeRef, [value, options.length]);
  return (
    <div ref={containerRef} role="radiogroup" aria-label={label} className={cx(s.segmented, tone === "chrome" && s.segChrome, stretch && s.segStretch)}>
      {thumb.visible ? <span aria-hidden className={cx(s.segThumb, thumb.instant && s.segThumbInstant)} style={thumb.style} /> : null}
      {options.map((o) => (
        <button
          key={o.value}
          ref={o.value === value ? activeRef : undefined}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          aria-label={iconOnly ? o.label : undefined}
          data-tip={o.tip}
          className={cx(s.segment, o.value === value && s.segmentOn)}
          onClick={() => onChange(o.value)}
        >
          {o.icon ? <Icon name={o.icon} size={15} /> : null}
          {iconOnly ? null : <span>{o.label}</span>}
        </button>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------- switch

export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={cx(s.switch, checked && s.switchOn)}
      onClick={() => onChange(!checked)}
    >
      <span className={s.switchKnob} />
    </button>
  );
}

export function CheckRow({
  checked,
  onChange,
  children,
  hint,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  children: ReactNode;
  hint?: string;
}) {
  return (
    <label className={s.checkRow}>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span className={s.checkBox} aria-hidden>
        <Icon name="check" size={12} />
      </span>
      <span className={s.checkText}>
        {children}
        {hint ? <small>{hint}</small> : null}
      </span>
    </label>
  );
}

// ---------------------------------------------------------------- select

export function Select<T extends string>({
  value,
  options,
  onChange,
  label,
  disabled,
}: {
  value: T;
  options: Array<{ value: T; label: string }>;
  onChange: (value: T) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <span className={s.selectWrap}>
      <select
        aria-label={label}
        className={s.select}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value as T)}
        onKeyDown={(e) => e.stopPropagation()}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <Icon name="chevronDown" size={13} className={s.selectChevron} />
    </span>
  );
}

// ---------------------------------------------------------------- text and number fields

/** Shared commit-on-Enter-or-blur, revert-on-Escape behaviour. */
function useDraft(external: string, commit: (text: string) => boolean) {
  const [draft, setDraft] = useState<string | null>(null);
  const [invalid, setInvalid] = useState(false);
  const cancelled = useRef(false);

  useEffect(() => {
    if (!invalid) return;
    const t = window.setTimeout(() => setInvalid(false), 600);
    return () => window.clearTimeout(t);
  }, [invalid]);

  const finish = (text: string | null) => {
    if (text !== null && text !== external) {
      if (!commit(text)) setInvalid(true);
    }
    setDraft(null);
  };

  return {
    value: draft ?? external,
    invalid,
    setDraft,
    onFocus: (e: React.FocusEvent<HTMLInputElement | HTMLTextAreaElement>) => e.currentTarget.select(),
    onChange: (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => setDraft(e.target.value),
    onBlur: () => {
      if (cancelled.current) {
        cancelled.current = false;
        setDraft(null);
        return;
      }
      finish(draft);
    },
    cancel: (el: HTMLElement) => {
      cancelled.current = true;
      el.blur();
    },
    commitNow: (el: HTMLElement) => {
      el.blur();
    },
  };
}

interface TextFieldProps {
  value: string;
  onCommit: (value: string) => void;
  label: string;
  placeholder?: string;
  /** When true an empty value is rejected and the field reverts. */
  required?: boolean;
  multiline?: boolean;
  className?: string;
  autoFocus?: boolean;
  onCancel?: () => void;
}

export function TextField({
  value,
  onCommit,
  label,
  placeholder,
  required,
  multiline,
  className,
  autoFocus,
  onCancel,
}: TextFieldProps) {
  const d = useDraft(value, (text) => {
    const next = multiline ? text : text.trim();
    if (required && next === "") return false;
    onCommit(next);
    return true;
  });
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) => {
    e.stopPropagation();
    if (e.key === "Enter" && (!multiline || e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      d.commitNow(e.currentTarget);
    } else if (e.key === "Escape") {
      e.preventDefault();
      d.cancel(e.currentTarget);
      onCancel?.();
    }
  };
  const shared = {
    "aria-label": label,
    placeholder,
    value: d.value,
    onFocus: d.onFocus,
    onChange: d.onChange,
    onBlur: () => {
      d.onBlur();
    },
    onKeyDown,
    autoFocus,
    spellCheck: false,
    className: cx(s.input, d.invalid && s.inputInvalid, className),
  };
  return multiline ? <textarea rows={3} {...shared} /> : <input type="text" {...shared} />;
}

interface NumberFieldProps {
  /** Millimeters when kind is "length", a plain number otherwise. */
  value: number;
  onCommit: (value: number) => void;
  label: string;
  kind?: "length" | "number";
  unit?: DisplayUnit;
  /** Suffix shown for plain numbers, for example "deg". Lengths show the unit. */
  suffix?: string;
  min?: number;
  max?: number;
  /** Nudge step in stored units (mm for lengths). Shift multiplies by 10. */
  step?: number;
  decimals?: number;
  integer?: boolean;
  disabled?: boolean;
}

export function NumberField({
  value,
  onCommit,
  label,
  kind = "number",
  unit = "mm",
  suffix,
  min,
  max,
  step,
  decimals = 2,
  integer,
  disabled,
}: NumberFieldProps) {
  const isLength = kind === "length";
  const toText = (n: number) => (isLength ? lengthToInput(n, unit) : numberToInput(n, integer ? 0 : decimals));
  const parse = (text: string) => (isLength ? parseLength(text, unit) : parseNumber(text));
  const clamp = (n: number) => {
    let v = integer ? Math.round(n) : n;
    if (min !== undefined) v = Math.max(min, v);
    if (max !== undefined) v = Math.min(max, v);
    return v;
  };
  // A committed value flashes teal briefly (MOTION.md, Inspector row).
  const flash = useFlash(500);
  const send = (n: number) => {
    const v = clamp(n);
    if (Math.abs(v - value) > 1e-6) {
      onCommit(v);
      flash.fire();
    }
  };
  const d = useDraft(toText(value), (text) => {
    const n = parse(text);
    if (n === null) return false;
    send(n);
    return true;
  });
  const nudgeStep = step ?? (isLength ? (unit === "m" ? 50 : 10) : 1);

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      d.commitNow(e.currentTarget);
    } else if (e.key === "Escape") {
      e.preventDefault();
      d.cancel(e.currentTarget);
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const base = parse(d.value) ?? value;
      const delta = (e.key === "ArrowUp" ? 1 : -1) * nudgeStep * (e.shiftKey ? 10 : 1);
      d.setDraft(null);
      send(base + delta);
    }
  };

  return (
    <span className={cx(s.numberWrap, d.invalid && s.inputInvalid, flash.active && s.numberFlash, disabled && s.numberDisabled)}>
      <input
        type="text"
        inputMode="decimal"
        aria-label={label}
        className={s.numberInput}
        value={d.value}
        disabled={disabled}
        onFocus={d.onFocus}
        onChange={d.onChange}
        onBlur={d.onBlur}
        onKeyDown={onKeyDown}
        spellCheck={false}
      />
      <span className={s.numberSuffix}>{isLength ? unit : suffix}</span>
    </span>
  );
}

// ---------------------------------------------------------------- layout helpers

export function Field({ label, children, hint }: { label: string; children: ReactNode; hint?: string }) {
  return (
    <div className={s.field}>
      <span className={s.fieldLabel} title={hint}>
        {label}
      </span>
      <div className={s.fieldControl}>{children}</div>
    </div>
  );
}

export function ReadOnly({ children }: { children: ReactNode }) {
  return <span className={s.readOnly}>{children}</span>;
}

export function Section({
  title,
  icon,
  count,
  open,
  onToggle,
  children,
  aside,
}: {
  title: string;
  icon?: IconName;
  count?: number;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
  aside?: ReactNode;
}) {
  return (
    <section className={s.section}>
      <header className={s.sectionHead}>
        <button type="button" className={s.sectionToggle} aria-expanded={open} onClick={onToggle}>
          <Icon name="chevronRight" size={12} className={cx(s.sectionChevron, open && s.sectionChevronOpen)} />
          {icon ? <Icon name={icon} size={15} /> : null}
          <span>{title}</span>
          {count !== undefined && count > 0 ? <span className={s.sectionCount}>{count}</span> : null}
        </button>
        {aside}
      </header>
      {/* Height reveal via grid-template-rows 0fr -> 1fr (MOTION.md rule 5), not a
          layout-property animation loop: it only runs once per toggle. */}
      <div className={cx(s.sectionReveal, open && s.sectionRevealOpen)} inert={!open}>
        {/* The clip takes the 0fr row; the padded body inside it collapses fully. */}
        <div className={s.sectionClip}>
          <div className={s.sectionBody}>{children}</div>
        </div>
      </div>
    </section>
  );
}

export function Spinner({ size = 18, className }: { size?: number; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 20 20" fill="none" className={cx(s.spinner, className)} aria-hidden>
      <circle cx="10" cy="10" r="7" stroke="currentColor" strokeOpacity={0.25} strokeWidth={1.5} />
      <path d="M10 3a7 7 0 0 1 7 7" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round" />
    </svg>
  );
}
